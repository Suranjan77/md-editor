//! Cached state and graph-derived research insights for the knowledge graph.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use md_editor_core::types::{GraphNode, GraphNodeKind, GraphSnapshot};

mod layout;
mod physics;

use layout::*;
pub use physics::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphScope {
    Global,
    Local { depth: u8 },
}

impl GraphScope {
    fn normalized(self) -> Self {
        match self {
            Self::Global => Self::Global,
            Self::Local { depth } => Self::Local {
                depth: depth.clamp(1, 2),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GraphPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Default)]
pub struct GraphLayout {
    pub positions: BTreeMap<String, GraphPoint>,
}

impl GraphLayout {
    pub fn get(&self, path: &str) -> Option<GraphPoint> {
        self.positions.get(path).copied()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphStats {
    pub markdown: usize,
    pub pdfs: usize,
    pub missing: usize,
    pub connections: usize,
    pub unconnected: usize,
    pub components: usize,
}

impl GraphStats {
    pub fn documents(&self) -> usize {
        self.markdown + self.pdfs
    }
}

#[derive(Debug, Clone)]
pub struct RelatedNode {
    pub node: GraphNode,
    pub score: f32,
    pub shared_connections: usize,
}

/// Widest zoom-out / closest zoom-in the shared view transform is clamped to.
pub const MIN_ZOOM: f32 = 0.002;
pub const MAX_ZOOM: f32 = 6.0;

/// The scope/filter/query-reduced graph, plus the index-based lookups both
/// render layers need.
///
/// Everything here is keyed by position in `snapshot.nodes` rather than by
/// path: the draw and hit-test loops run per node per frame, and at vault
/// scale string hashing in those loops is the entire frame budget.
#[derive(Debug, Default)]
pub struct FilteredGraph {
    pub snapshot: GraphSnapshot,
    /// Path → index into `snapshot.nodes`.
    pub index: BTreeMap<String, usize>,
    /// Endpoint indices for each entry of `snapshot.edges`, in the same order.
    pub edge_pairs: Vec<(usize, usize)>,
    /// Neighbour indices per node, for focus highlighting.
    pub neighbors: Vec<Vec<u32>>,
}

impl FilteredGraph {
    fn build(snapshot: GraphSnapshot) -> Self {
        let index: BTreeMap<String, usize> = snapshot
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.path.clone(), i))
            .collect();

        let mut edge_pairs = Vec::with_capacity(snapshot.edges.len());
        let mut neighbors = vec![Vec::new(); snapshot.nodes.len()];
        for edge in &snapshot.edges {
            let (Some(&source), Some(&target)) = (index.get(&edge.source), index.get(&edge.target))
            else {
                // Keep `edge_pairs` aligned with `snapshot.edges` even if an
                // endpoint was filtered out, so callers can zip the two.
                edge_pairs.push((usize::MAX, usize::MAX));
                continue;
            };
            edge_pairs.push((source, target));
            neighbors[source].push(target as u32);
            neighbors[target].push(source as u32);
        }

        Self {
            snapshot,
            index,
            edge_pairs,
            neighbors,
        }
    }

    pub fn position_of(&self, path: &str) -> Option<usize> {
        self.index.get(path).copied()
    }
}

/// Why the visible graph has nothing to draw. Each variant maps to a distinct
/// empty state with its own copy and (where one exists) a one-click remedy, so
/// "nothing here" is never a dead end the user has to guess their way out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyReason {
    /// The first snapshot for this vault is still being built.
    Loading,
    /// The vault genuinely contains no indexed documents.
    NoDocuments,
    /// Local scope is selected but no document is open to center it on.
    NeedsActiveDocument,
    /// The search query matched nothing.
    NoSearchMatch,
    /// Every node was removed by the type filters.
    FiltersHideEverything,
}

pub struct GraphState {
    pub visible: bool,
    pub scope: GraphScope,
    pub query: String,
    pub show_pdfs: bool,
    pub show_missing: bool,
    pub show_orphans: bool,
    pub selected_path: Option<String>,
    pub active_path: Option<String>,
    /// Node currently under the cursor (drives hover highlight + tooltip).
    pub hovered_path: Option<String>,
    /// Whether the right-hand inspector panel is expanded.
    pub inspector_visible: bool,
    /// Shared world→screen pan (pixels) applied by both render layers.
    pub pan: (f32, f32),
    /// Shared world→screen zoom applied by both render layers.
    pub zoom: f32,
    pub fit_revision: u64,
    /// Bumped whenever the *app* (rather than a pointer gesture) sets the
    /// transform — zoom buttons, "reveal in graph". The render layer owns
    /// pan/zoom during interaction, so it needs this nudge to adopt ours.
    pub view_revision: u64,
    pub physics: PhysicsState,
    snapshot: GraphSnapshot,
    layout: GraphLayout,
    // ── Derived caches ──────────────────────────────────────────
    // `view` runs on every message — every physics tick, every mouse move —
    // so nothing below may be recomputed there. Each cache is rebuilt only by
    // the mutators that can actually invalidate it.
    /// Path → index into `snapshot.nodes`, so lookups are not linear scans.
    node_index: BTreeMap<String, usize>,
    /// Whole-vault adjacency, shared by insights and local-scope traversal.
    adjacency: BTreeMap<String, BTreeSet<String>>,
    /// Endpoint indices for `snapshot.edges`, aligned with it.
    snapshot_edge_pairs: Vec<(usize, usize)>,
    /// Lowercased "label\npath" per node, so search never re-allocates one
    /// string per node per keystroke.
    search_keys: Vec<String>,
    stats_cache: GraphStats,
    /// The scope/filter/query-reduced graph both render layers draw.
    filtered: Arc<FilteredGraph>,
    /// World positions aligned to `filtered.snapshot.nodes`, refreshed
    /// whenever the layout moves.
    positions: Arc<Vec<GraphPoint>>,
    pinned: BTreeSet<String>,
    dirty: bool,
    dirty_generation: u64,
    refresh_in_flight: Option<u64>,
    last_attempted_generation: Option<u64>,
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            visible: false,
            scope: GraphScope::Global,
            query: String::new(),
            show_pdfs: true,
            show_missing: true,
            show_orphans: true,
            selected_path: None,
            active_path: None,
            hovered_path: None,
            inspector_visible: true,
            pan: (0.0, 0.0),
            zoom: 1.0,
            fit_revision: 0,
            view_revision: 0,
            physics: PhysicsState::default(),
            snapshot: GraphSnapshot::default(),
            layout: GraphLayout::default(),
            node_index: BTreeMap::new(),
            adjacency: BTreeMap::new(),
            snapshot_edge_pairs: Vec::new(),
            search_keys: Vec::new(),
            stats_cache: GraphStats::default(),
            filtered: Arc::new(FilteredGraph::default()),
            positions: Arc::new(Vec::new()),
            pinned: BTreeSet::new(),
            dirty: true,
            dirty_generation: 0,
            refresh_in_flight: None,
            last_attempted_generation: None,
        }
    }

    pub fn set_snapshot(&mut self, snapshot: GraphSnapshot) {
        self.refresh_in_flight = None;
        self.last_attempted_generation = None;
        if self.snapshot == snapshot {
            self.dirty = false;
            return;
        }
        let previous = std::mem::take(&mut self.layout.positions);
        let mut layout = build_layout(&snapshot);
        self.pinned
            .retain(|path| layout.positions.contains_key(path));
        // Preserve explicit user placement while allowing unpinned components
        // to react when new links change the topology.
        for (path, position) in previous {
            if self.pinned.contains(&path) {
                layout.positions.insert(path, position);
            }
        }
        self.snapshot = snapshot;
        self.layout = layout;
        self.dirty = false;
        self.rebuild_snapshot_caches();
        let edge_pairs: Vec<(String, String)> = self
            .snapshot
            .edges
            .iter()
            .map(|e| (e.source.clone(), e.target.clone()))
            .collect();
        self.physics.prepare(&self.layout.positions, &edge_pairs);
        if self
            .selected_path
            .as_ref()
            .is_some_and(|path| !self.snapshot.nodes.iter().any(|node| &node.path == path))
        {
            self.selected_path = None;
        }
        if self
            .hovered_path
            .as_ref()
            .is_some_and(|path| !self.snapshot.nodes.iter().any(|node| &node.path == path))
        {
            self.hovered_path = None;
        }
        self.fit_revision = self.fit_revision.wrapping_add(1);
    }

    /// Drop all vault-specific graph data before another vault is opened.
    /// Filters/scope remain user preferences, while nodes, pins, selection, and
    /// any in-flight result are invalidated so old-vault content cannot flash.
    pub fn clear_for_vault_change(&mut self) {
        self.snapshot = GraphSnapshot::default();
        self.layout = GraphLayout::default();
        self.rebuild_snapshot_caches();
        self.pinned.clear();
        self.selected_path = None;
        self.active_path = None;
        self.hovered_path = None;
        self.refresh_in_flight = None;
        self.last_attempted_generation = None;
        self.dirty = true;
        self.dirty_generation = self.dirty_generation.wrapping_add(1);
        self.fit_revision = self.fit_revision.wrapping_add(1);
        self.physics.stop();
    }

    pub fn set_active_path(&mut self, path: Option<String>) {
        if self.active_path != path {
            self.active_path = path;
            self.rebuild_filtered();
            self.fit_revision = self.fit_revision.wrapping_add(1);
        }
    }

    pub fn set_scope(&mut self, scope: GraphScope) {
        let scope = scope.normalized();
        if self.scope != scope {
            self.scope = scope;
            self.rebuild_filtered();
            self.fit_revision = self.fit_revision.wrapping_add(1);
        }
    }

    pub fn set_query(&mut self, query: String) {
        if self.query != query {
            self.query = query;
            self.rebuild_filtered();
            self.fit_revision = self.fit_revision.wrapping_add(1);
        }
    }

    pub fn set_show_pdfs(&mut self, show: bool) {
        if self.show_pdfs != show {
            self.show_pdfs = show;
            self.rebuild_filtered();
            self.request_fit();
        }
    }

    pub fn set_show_missing(&mut self, show: bool) {
        if self.show_missing != show {
            self.show_missing = show;
            self.rebuild_filtered();
            self.request_fit();
        }
    }

    pub fn set_show_orphans(&mut self, show: bool) {
        if self.show_orphans != show {
            self.show_orphans = show;
            self.rebuild_filtered();
            self.request_fit();
        }
    }

    fn request_fit(&mut self) {
        self.fit_revision = self.fit_revision.wrapping_add(1);
    }

    pub fn select(&mut self, path: Option<String>) {
        self.selected_path = path.filter(|candidate| {
            self.snapshot
                .nodes
                .iter()
                .any(|node| node.path == *candidate)
        });
    }

    /// Store the pan/zoom the render layer committed. Non-finite values are
    /// ignored and the zoom is clamped so the transform can never degenerate.
    pub fn set_view(&mut self, pan_x: f32, pan_y: f32, zoom: f32) {
        if pan_x.is_finite() && pan_y.is_finite() && zoom.is_finite() {
            self.pan = (pan_x, pan_y);
            self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    /// Record the hovered node, dropping stale paths that are no longer present.
    pub fn set_hovered(&mut self, path: Option<String>) {
        self.hovered_path = path.filter(|candidate| self.node(candidate).is_some());
    }

    pub fn move_node(&mut self, path: &str, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() && self.layout.positions.contains_key(path) {
            self.layout
                .positions
                .insert(path.to_string(), GraphPoint { x, y });
            self.pinned.insert(path.to_string());
            self.refresh_positions();
            // Wake the simulation so neighbours settle around the new position.
            self.physics.nudge();
        }
    }

    /// Push a transform the render layer must adopt on its next update, even
    /// though that layer is otherwise the authority on pan/zoom.
    fn publish_view(&mut self, pan: (f32, f32), zoom: f32) {
        if pan.0.is_finite() && pan.1.is_finite() && zoom.is_finite() {
            self.pan = pan;
            self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
            self.view_revision = self.view_revision.wrapping_add(1);
        }
    }

    /// Scale the zoom about the canvas center, so the note the user is looking
    /// at stays put instead of sliding off while they zoom.
    pub fn zoom_by(&mut self, factor: f32) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let ratio = zoom / self.zoom.max(f32::EPSILON);
        self.publish_view((self.pan.0 * ratio, self.pan.1 * ratio), zoom);
    }

    /// Select a node and pan it to the middle of the canvas. Zooming out far
    /// enough to lose a node and then clicking it in a list is the common way
    /// to reach one, so this also zooms back in to a readable level.
    pub fn focus_on(&mut self, path: &str) {
        self.select(Some(path.to_string()));
        let Some(point) = self.layout.get(path) else {
            return;
        };
        let zoom = self.zoom.clamp(0.9, 1.8);
        self.publish_view((-point.x * zoom, -point.y * zoom), zoom);
    }

    pub fn is_pinned(&self, path: &str) -> bool {
        self.pinned.contains(path)
    }

    pub fn pinned_count(&self) -> usize {
        self.pinned.len()
    }

    pub fn pinned_paths(&self) -> &BTreeSet<String> {
        &self.pinned
    }

    /// Pin a node where it stands, or release it back to the simulation.
    pub fn toggle_pin(&mut self, path: &str) {
        if !self.layout.positions.contains_key(path) {
            return;
        }
        if !self.pinned.remove(path) {
            self.pinned.insert(path.to_string());
        }
        self.physics.nudge();
    }

    pub fn unpin_all(&mut self) {
        if self.pinned.is_empty() {
            return;
        }
        self.pinned.clear();
        self.physics.nudge();
    }

    /// Pause or resume the layout simulation. Pausing is what lets a user
    /// arrange a graph by hand without the springs undoing their work.
    pub fn toggle_physics(&mut self) {
        if self.physics.running {
            self.physics.stop();
        } else {
            self.physics.nudge();
        }
    }

    pub fn is_loading(&self) -> bool {
        self.refresh_in_flight.is_some()
    }

    /// Whether unlinked notes so dominate the vault that drawing them all
    /// produces a featureless cloud rather than a graph.
    ///
    /// A catalogue-style vault can hold thousands of notes and almost no
    /// links; showing every one of them at 2% zoom communicates nothing, so
    /// the view offers to hide them instead of silently rendering a blob.
    pub fn orphans_dominate(&self) -> bool {
        const MIN_NODES: usize = 200;
        const SHARE: f32 = 0.7;
        let total = self.snapshot.nodes.len();
        total >= MIN_NODES
            && self.show_orphans
            && self.stats_cache.unconnected as f32 / total as f32 >= SHARE
    }

    /// Whether any node/type filter is currently hiding something.
    pub fn filters_active(&self) -> bool {
        !self.show_pdfs || !self.show_missing || !self.show_orphans
    }

    pub fn reset_filters(&mut self) {
        self.show_pdfs = true;
        self.show_missing = true;
        self.show_orphans = true;
        self.query.clear();
        self.rebuild_filtered();
        self.request_fit();
    }

    /// Nodes whose label or path matches the query directly (excluding the
    /// neighbours `filtered_graph` keeps around them for context).
    pub fn query_match_count(&self) -> usize {
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return 0;
        }
        self.search_keys
            .iter()
            .filter(|key| key.contains(&query))
            .count()
    }

    /// Diagnose an empty canvas so the view can say what actually happened.
    pub fn empty_reason(&self) -> EmptyReason {
        if self.snapshot.nodes.is_empty() {
            return if self.is_loading() || self.dirty {
                EmptyReason::Loading
            } else {
                EmptyReason::NoDocuments
            };
        }
        if matches!(self.scope, GraphScope::Local { .. })
            && self
                .active_path
                .as_ref()
                .is_none_or(|path| self.node(path).is_none())
        {
            return EmptyReason::NeedsActiveDocument;
        }
        if !self.query.trim().is_empty() && self.query_match_count() == 0 {
            return EmptyReason::NoSearchMatch;
        }
        EmptyReason::FiltersHideEverything
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.dirty_generation = self.dirty_generation.wrapping_add(1);
    }

    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether the visible graph should start a debounced snapshot rebuild.
    /// A failed generation is held until another invalidation (or an explicit
    /// retry) so a persistent database error cannot create a tight retry loop.
    pub fn refresh_due(&self) -> bool {
        self.dirty
            && self.refresh_in_flight.is_none()
            && self.last_attempted_generation != Some(self.dirty_generation)
    }

    /// Reserve the current dirty generation for one asynchronous rebuild.
    pub fn begin_refresh(&mut self) -> Option<u64> {
        if !self.refresh_due() {
            return None;
        }
        let generation = self.dirty_generation;
        self.refresh_in_flight = Some(generation);
        self.last_attempted_generation = Some(generation);
        Some(generation)
    }

    /// Apply a completed rebuild only if no newer invalidation occurred while
    /// it was running. Returns whether the snapshot was accepted.
    pub fn complete_refresh(&mut self, generation: u64, snapshot: GraphSnapshot) -> bool {
        if self.refresh_in_flight != Some(generation) {
            return false;
        }
        self.refresh_in_flight = None;
        if self.dirty_generation != generation {
            return false;
        }
        self.set_snapshot(snapshot);
        true
    }

    /// Finish a failed rebuild. Current-generation failures stay dirty for UI
    /// accuracy but are not automatically retried until the graph changes.
    pub fn fail_refresh(&mut self, generation: u64) -> bool {
        if self.refresh_in_flight != Some(generation) {
            return false;
        }
        self.refresh_in_flight = None;
        self.dirty_generation == generation
    }

    /// Allow a previously failed dirty generation to be retried, for example
    /// when the user closes and reopens the graph workspace.
    pub fn retry_refresh(&mut self) {
        if self.dirty && self.refresh_in_flight.is_none() {
            self.last_attempted_generation = None;
        }
    }

    #[cfg(test)]
    pub fn graph_layout(&self) -> &GraphLayout {
        &self.layout
    }

    pub fn reset_layout(&mut self) {
        self.pinned.clear();
        self.layout = build_layout(&self.snapshot);
        let edge_pairs: Vec<(String, String)> = self
            .snapshot
            .edges
            .iter()
            .map(|e| (e.source.clone(), e.target.clone()))
            .collect();
        self.physics.prepare(&self.layout.positions, &edge_pairs);
        self.refresh_positions();
        self.fit_revision = self.fit_revision.wrapping_add(1);
    }

    /// Advance the physics simulation by one time step. Returns `true` if the
    /// simulation is still running and the view should be redrawn.
    pub fn physics_tick(&mut self) -> bool {
        let running = self.physics.tick(&mut self.layout.positions, &self.pinned);
        self.refresh_positions();
        running
    }

    pub fn node(&self, path: &str) -> Option<&GraphNode> {
        self.node_index
            .get(path)
            .and_then(|index| self.snapshot.nodes.get(*index))
    }

    /// The cached scope/filter/query-reduced graph. Cheap to clone — both
    /// render layers share one allocation per rebuild, not one per frame.
    pub fn filtered(&self) -> Arc<FilteredGraph> {
        Arc::clone(&self.filtered)
    }

    /// World positions aligned to `filtered().snapshot.nodes`.
    pub fn frame_positions(&self) -> Arc<Vec<GraphPoint>> {
        Arc::clone(&self.positions)
    }

    /// Rebuild everything derived from the raw snapshot alone.
    fn rebuild_snapshot_caches(&mut self) {
        self.node_index = self
            .snapshot
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.path.clone(), i))
            .collect();
        self.adjacency = adjacency(&self.snapshot);
        self.snapshot_edge_pairs = self
            .snapshot
            .edges
            .iter()
            .map(|edge| {
                (
                    self.node_index
                        .get(&edge.source)
                        .copied()
                        .unwrap_or(usize::MAX),
                    self.node_index
                        .get(&edge.target)
                        .copied()
                        .unwrap_or(usize::MAX),
                )
            })
            .collect();
        self.search_keys = self
            .snapshot
            .nodes
            .iter()
            .map(|node| format!("{}\n{}", node.label, node.path).to_lowercase())
            .collect();
        self.stats_cache = self.compute_stats();
        self.rebuild_filtered();
    }

    /// Rebuild the filtered graph. Called by every mutator that can change
    /// what is on canvas: snapshot, scope, active document, filters, query.
    fn rebuild_filtered(&mut self) {
        self.filtered = Arc::new(FilteredGraph::build(self.compute_filtered()));
        self.refresh_positions();
    }

    /// Re-project layout positions into the filtered node order. Called after
    /// anything moves a node — a physics tick, a drag, a relayout.
    fn refresh_positions(&mut self) {
        let positions = self
            .filtered
            .snapshot
            .nodes
            .iter()
            .map(|node| self.layout.get(&node.path).unwrap_or_default())
            .collect();
        self.positions = Arc::new(positions);
    }

    /// Reduce the vault graph to what the current scope, filters, and query
    /// admit.
    ///
    /// Runs entirely on an index-keyed `Vec<bool>` mask. The original
    /// path-keyed `BTreeSet` version did an O(n) linear node lookup per node
    /// and re-lowercased every label on every keystroke, which cost ~145 ms on
    /// a 10k-note vault — enough to make typing in the search box unusable.
    fn compute_filtered(&self) -> GraphSnapshot {
        let count = self.snapshot.nodes.len();
        let mut allowed = vec![false; count];

        match self.scope {
            GraphScope::Global => allowed.fill(true),
            GraphScope::Local { depth } => {
                for path in self.local_paths(depth) {
                    if let Some(&index) = self.node_index.get(&path) {
                        allowed[index] = true;
                    }
                }
            }
        }

        for (index, node) in self.snapshot.nodes.iter().enumerate() {
            if !allowed[index] {
                continue;
            }
            allowed[index] = (self.show_pdfs || node.kind != GraphNodeKind::Pdf)
                && (self.show_missing || node.kind != GraphNodeKind::Missing)
                && (self.show_orphans || node.incoming + node.outgoing > 0);
        }

        let query = self.query.trim().to_lowercase();
        if !query.is_empty() {
            let mut matched = vec![false; count];
            for (index, key) in self.search_keys.iter().enumerate() {
                matched[index] = allowed[index] && key.contains(&query);
            }
            // Keep immediate context around matches; a graph search is more
            // useful when the reason a result matters stays visible.
            let mut contextual = matched.clone();
            for &(source, target) in &self.snapshot_edge_pairs {
                if source == usize::MAX || target == usize::MAX {
                    continue;
                }
                if matched[source] && allowed[target] {
                    contextual[target] = true;
                }
                if matched[target] && allowed[source] {
                    contextual[source] = true;
                }
            }
            allowed = contextual;
        }

        let nodes = self
            .snapshot
            .nodes
            .iter()
            .enumerate()
            .filter(|(index, _)| allowed[*index])
            .map(|(_, node)| node.clone())
            .collect();
        let edges = self
            .snapshot
            .edges
            .iter()
            .zip(&self.snapshot_edge_pairs)
            .filter(|(_, pair)| {
                let (source, target) = **pair;
                source != usize::MAX && target != usize::MAX && allowed[source] && allowed[target]
            })
            .map(|(edge, _)| edge.clone())
            .collect();
        GraphSnapshot { nodes, edges }
    }

    fn local_paths(&self, depth: u8) -> BTreeSet<String> {
        let Some(active) = self
            .active_path
            .as_ref()
            .filter(|path| self.node(path).is_some())
        else {
            return BTreeSet::new();
        };
        let adjacency = &self.adjacency;
        let mut found = BTreeSet::from([active.clone()]);
        let mut queue = VecDeque::from([(active.clone(), 0_u8)]);
        while let Some((path, distance)) = queue.pop_front() {
            if distance >= depth.clamp(1, 2) {
                continue;
            }
            if let Some(neighbors) = adjacency.get(&path) {
                for neighbor in neighbors {
                    if found.insert(neighbor.clone()) {
                        queue.push_back((neighbor.clone(), distance + 1));
                    }
                }
            }
        }
        found
    }

    pub fn stats(&self) -> GraphStats {
        self.stats_cache.clone()
    }

    fn compute_stats(&self) -> GraphStats {
        let mut stats = GraphStats::default();
        for node in &self.snapshot.nodes {
            match node.kind {
                GraphNodeKind::Markdown => stats.markdown += 1,
                GraphNodeKind::Pdf => stats.pdfs += 1,
                GraphNodeKind::Missing => stats.missing += 1,
            }
            if node.kind == GraphNodeKind::Markdown && node.incoming + node.outgoing == 0 {
                stats.unconnected += 1;
            }
        }
        stats.connections = self.snapshot.edges.iter().map(|edge| edge.weight).sum();
        stats.components = components_from(&self.adjacency).len();
        stats
    }

    pub fn incoming(&self, path: &str) -> Vec<GraphNode> {
        let sources: BTreeSet<&str> = self
            .snapshot
            .edges
            .iter()
            .filter(|edge| edge.target == path)
            .map(|edge| edge.source.as_str())
            .collect();
        self.snapshot
            .nodes
            .iter()
            .filter(|node| sources.contains(node.path.as_str()))
            .cloned()
            .collect()
    }

    pub fn outgoing(&self, path: &str) -> Vec<GraphNode> {
        let targets: BTreeSet<&str> = self
            .snapshot
            .edges
            .iter()
            .filter(|edge| edge.source == path)
            .map(|edge| edge.target.as_str())
            .collect();
        self.snapshot
            .nodes
            .iter()
            .filter(|node| targets.contains(node.path.as_str()))
            .cloned()
            .collect()
    }

    pub fn related(&self, path: &str, limit: usize) -> Vec<RelatedNode> {
        let adjacency = &self.adjacency;
        let Some(origin_neighbors) = adjacency.get(path) else {
            return Vec::new();
        };
        let directly_connected = origin_neighbors;
        let mut related = Vec::new();
        for node in &self.snapshot.nodes {
            if node.path == path
                || node.kind != GraphNodeKind::Markdown
                || directly_connected.contains(&node.path)
            {
                continue;
            }
            let Some(candidate_neighbors) = adjacency.get(&node.path) else {
                continue;
            };
            let shared: Vec<&String> = origin_neighbors.intersection(candidate_neighbors).collect();
            if shared.is_empty() {
                continue;
            }
            let score = shared.iter().fold(0.0, |score, neighbor| {
                let degree = adjacency.get(*neighbor).map_or(1, BTreeSet::len);
                score + 1.0 / (1.0 + degree as f32).ln().max(0.1)
            });
            related.push(RelatedNode {
                node: node.clone(),
                score,
                shared_connections: shared.len(),
            });
        }
        related.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.node.path.cmp(&b.node.path))
        });
        related.truncate(limit);
        related
    }

    pub fn hubs(&self, limit: usize) -> Vec<GraphNode> {
        let mut nodes: Vec<_> = self
            .snapshot
            .nodes
            .iter()
            .filter(|node| node.kind != GraphNodeKind::Missing)
            .cloned()
            .collect();
        nodes.sort_by(|a, b| {
            (b.incoming + b.outgoing)
                .cmp(&(a.incoming + a.outgoing))
                .then_with(|| a.path.cmp(&b.path))
        });
        nodes.truncate(limit);
        nodes
    }

    pub fn unconnected(&self, limit: usize) -> Vec<GraphNode> {
        self.snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.kind == GraphNodeKind::Markdown && node.incoming + node.outgoing == 0
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn missing(&self, limit: usize) -> Vec<GraphNode> {
        self.snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == GraphNodeKind::Missing)
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for GraphState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod scale_probe {
    use super::*;
    use md_editor_core::types::{GraphEdge, GraphEdgeKind};
    use std::time::Instant;

    fn big(n: usize) -> GraphSnapshot {
        let mut nodes = Vec::with_capacity(n);
        let mut edges = Vec::new();
        for i in 0..n {
            nodes.push(GraphNode {
                path: format!("notes/n{i:05}.md"),
                label: format!("Note {i}"),
                kind: GraphNodeKind::Markdown,
                exists: true,
                incoming: 0,
                outgoing: 0,
            });
        }
        for i in 0..n {
            for k in 1..=3 {
                let t = (i * 7 + k * 13) % n;
                if t != i {
                    edges.push(GraphEdge {
                        source: format!("notes/n{i:05}.md"),
                        target: format!("notes/n{t:05}.md"),
                        kind: GraphEdgeKind::WikiLink,
                        weight: 1,
                    });
                }
            }
        }
        GraphSnapshot { nodes, edges }
    }

    /// Manual scale check against a vault-sized graph:
    /// `cargo test --release -p md-editor-native scale_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe() {
        let n = 9901;
        let snap = big(n);
        println!("nodes={} edges={}", snap.nodes.len(), snap.edges.len());

        let mut s = GraphState::new();
        let t = Instant::now();
        s.set_snapshot(snap);
        println!("set_snapshot (once)     {:?}", t.elapsed());

        // Everything below runs on the UI thread. The per-frame block has to
        // stay far under the 33 ms tick interval.
        let t = Instant::now();
        for _ in 0..100 {
            let f = s.filtered();
            let p = s.frame_positions();
            let st = s.stats();
            std::hint::black_box((f, p, st));
        }
        println!("per frame (x100)        {:?}", t.elapsed());

        let t = Instant::now();
        for _ in 0..30 {
            s.physics_tick();
        }
        println!("physics tick (x30)      {:?}", t.elapsed());

        let t = Instant::now();
        s.set_query("note 1".to_string());
        println!("keystroke (filter)      {:?}", t.elapsed());

        let t = Instant::now();
        let _ = s.related("notes/n00001.md", 6);
        println!("related (on select)     {:?}", t.elapsed());

        let t = Instant::now();
        let _ = s.hubs(6);
        let _ = s.unconnected(6);
        let _ = s.missing(6);
        println!("insight lists           {:?}", t.elapsed());
    }
}
