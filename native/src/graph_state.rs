//! Cached state and graph-derived research insights for the knowledge graph.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use md_editor_core::types::{GraphNode, GraphNodeKind, GraphSnapshot};

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

/// Force-directed physics simulation for the knowledge graph.
///
/// Uses Euler integration with three forces:
/// - **Coulomb repulsion** between every node pair (inversely proportional to
///   distance squared).
/// - **Hooke spring attraction** along edges (proportional to distance beyond a
///   rest length).
/// - **Center gravity** pulling all nodes gently toward the origin.
///
/// Internally all computation runs on flat, index-aligned `Vec<[f32; 2]>`
/// arrays. String-keyed positions are loaded/stored once per tick via the
/// cached `index` ordering, so the O(n²) repulsion inner loop never touches
/// a `BTreeMap` or allocates.
///
/// Pinned nodes are excluded from integration but still exert forces.
#[derive(Debug, Clone)]
pub struct PhysicsState {
    /// Whether the simulation is currently stepping.
    pub running: bool,
    // ── Cached structural data (rebuilt via `prepare`) ───────────
    /// Ordered node paths — position in this vec *is* the array index used
    /// by `vel`, `forces`, `pos`, and `edge_pairs`.
    index: Vec<String>,
    /// Edge endpoint pairs expressed as indices into `index`.
    edge_pairs: Vec<[usize; 2]>,
    // ── Per-node state (parallel to `index`) ────────────────────
    vel: Vec<[f32; 2]>,
    // ── Reusable computation buffers ────────────────────────────
    forces: Vec<[f32; 2]>,
    pos: Vec<[f32; 2]>,
    // ── Tuning parameters ───────────────────────────────────────
    repulsion: f32,
    attraction: f32,
    damping: f32,
    center_gravity: f32,
    rest_length: f32,
    stability_threshold: f32,
}

impl Default for PhysicsState {
    fn default() -> Self {
        Self {
            running: false,
            index: Vec::new(),
            edge_pairs: Vec::new(),
            vel: Vec::new(),
            forces: Vec::new(),
            pos: Vec::new(),
            repulsion: 8_000.0,
            attraction: 0.012,
            damping: 0.72,
            center_gravity: 0.0008,
            rest_length: 100.0,
            stability_threshold: 0.15,
        }
    }
}

impl PhysicsState {
    /// Rebuild cached structural data from the current positions and edges.
    ///
    /// Must be called whenever the set of nodes or edges changes (snapshot
    /// load, layout reset). Clears velocities so the simulation re-settles
    /// from scratch.
    pub fn prepare(
        &mut self,
        positions: &BTreeMap<String, GraphPoint>,
        edges: &[(String, String)],
    ) {
        let n = positions.len();
        // `BTreeMap::keys()` yields sorted order — same order every time.
        self.index.clear();
        self.index.extend(positions.keys().cloned());

        // Build edge index pairs via binary search on sorted `index`.
        self.edge_pairs.clear();
        for (source, target) in edges {
            let si = self.index.binary_search(source);
            let ti = self.index.binary_search(target);
            if let (Ok(si), Ok(ti)) = (si, ti) {
                self.edge_pairs.push([si, ti]);
            }
        }

        // (Re)initialize per-node arrays.
        self.vel.clear();
        self.vel.resize(n, [0.0; 2]);
        self.forces.resize(n, [0.0; 2]);
        self.pos.resize(n, [0.0; 2]);

        self.running = n > 1;
    }

    /// Run one simulation step. Reads positions from the `BTreeMap`, computes
    /// forces on flat arrays, and writes updated positions back. Returns
    /// `true` if the simulation is still active (has not auto-stabilized).
    pub fn tick(
        &mut self,
        positions: &mut BTreeMap<String, GraphPoint>,
        pinned: &BTreeSet<String>,
    ) -> bool {
        let n = self.index.len();
        if !self.running || n == 0 {
            return false;
        }

        // ── Load positions into flat buffer ─────────────────────
        for (i, path) in self.index.iter().enumerate() {
            if let Some(p) = positions.get(path) {
                self.pos[i] = [p.x, p.y];
            }
        }

        // ── Zero force accumulator ──────────────────────────────
        for f in self.forces[..n].iter_mut() {
            *f = [0.0; 2];
        }

        // ── Build pinned mask (reuses stack / tiny allocation) ───
        // For typical graph sizes (< few thousand) a Vec<bool> rebuilt
        // each tick is cheaper than hashing or BTreeSet lookups inside
        // the O(n²) inner loop.
        let pinned_mask: Vec<bool> = self
            .index
            .iter()
            .map(|p| pinned.contains(p))
            .collect();

        // ── 1. Coulomb repulsion (O(n²), tight array loop) ──────
        let repulsion = self.repulsion;
        for i in 0..n {
            let [ax, ay] = self.pos[i];
            for j in (i + 1)..n {
                let [bx, by] = self.pos[j];
                let dx = ax - bx;
                let dy = ay - by;
                let dist_sq = (dx * dx + dy * dy).max(1.0);
                let inv_dist = dist_sq.sqrt().recip();
                let force = repulsion / dist_sq;
                let fx = force * dx * inv_dist;
                let fy = force * dy * inv_dist;

                if !pinned_mask[i] {
                    self.forces[i][0] += fx;
                    self.forces[i][1] += fy;
                }
                if !pinned_mask[j] {
                    self.forces[j][0] -= fx;
                    self.forces[j][1] -= fy;
                }
            }
        }

        // ── 2. Spring attraction along edges ────────────────────
        let attraction = self.attraction;
        let rest = self.rest_length;
        for &[si, ti] in &self.edge_pairs {
            let [ax, ay] = self.pos[si];
            let [bx, by] = self.pos[ti];
            let dx = bx - ax;
            let dy = by - ay;
            let dist = (dx * dx + dy * dy).sqrt().max(0.1);
            let displacement = dist - rest;
            let force = attraction * displacement;
            let inv = dist.recip();
            let fx = force * dx * inv;
            let fy = force * dy * inv;

            if !pinned_mask[si] {
                self.forces[si][0] += fx;
                self.forces[si][1] += fy;
            }
            if !pinned_mask[ti] {
                self.forces[ti][0] -= fx;
                self.forces[ti][1] -= fy;
            }
        }

        // ── 3. Center gravity ───────────────────────────────────
        let grav = self.center_gravity;
        for i in 0..n {
            if pinned_mask[i] {
                continue;
            }
            self.forces[i][0] -= grav * self.pos[i][0];
            self.forces[i][1] -= grav * self.pos[i][1];
        }

        // ── 4. Integrate velocities and positions ───────────────
        const MAX_FORCE: f32 = 50.0;
        let damping = self.damping;
        let mut total_energy = 0.0_f32;
        for i in 0..n {
            if pinned_mask[i] {
                self.vel[i] = [0.0; 2];
                continue;
            }
            let [mut fx, mut fy] = self.forces[i];
            let mag = (fx * fx + fy * fy).sqrt();
            if mag > MAX_FORCE {
                let s = MAX_FORCE / mag;
                fx *= s;
                fy *= s;
            }

            let vx = (self.vel[i][0] + fx) * damping;
            let vy = (self.vel[i][1] + fy) * damping;
            self.vel[i] = [vx, vy];
            self.pos[i] = [self.pos[i][0] + vx, self.pos[i][1] + vy];

            total_energy += vx * vx + vy * vy;
        }

        // ── Write positions back to BTreeMap ────────────────────
        for (i, path) in self.index.iter().enumerate() {
            if pinned_mask[i] {
                continue;
            }
            if let Some(p) = positions.get_mut(path) {
                p.x = self.pos[i][0];
                p.y = self.pos[i][1];
            }
        }

        // ── Auto-stabilize ──────────────────────────────────────
        if total_energy < self.stability_threshold {
            self.running = false;
            return false;
        }

        true
    }

    /// Signal that something changed and the simulation should wake up
    /// (e.g. a node was dragged). Does NOT rebuild the index — use
    /// [`prepare`] when the node set or edge set changes.
    pub fn nudge(&mut self) {
        if !self.index.is_empty() {
            self.running = true;
        }
    }

    /// Stop the simulation and clear velocities.
    pub fn stop(&mut self) {
        self.running = false;
        self.vel.iter_mut().for_each(|v| *v = [0.0; 2]);
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
    /// Shared world→screen pan (pixels) applied by both render layers.
    pub pan: (f32, f32),
    /// Shared world→screen zoom applied by both render layers.
    pub zoom: f32,
    pub fit_revision: u64,
    pub physics: PhysicsState,
    snapshot: GraphSnapshot,
    layout: GraphLayout,
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
            pan: (0.0, 0.0),
            zoom: 1.0,
            fit_revision: 0,
            physics: PhysicsState::default(),
            snapshot: GraphSnapshot::default(),
            layout: GraphLayout::default(),
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
            self.fit_revision = self.fit_revision.wrapping_add(1);
        }
    }

    pub fn set_scope(&mut self, scope: GraphScope) {
        let scope = scope.normalized();
        if self.scope != scope {
            self.scope = scope;
            self.fit_revision = self.fit_revision.wrapping_add(1);
        }
    }

    pub fn set_query(&mut self, query: String) {
        if self.query != query {
            self.query = query;
            self.fit_revision = self.fit_revision.wrapping_add(1);
        }
    }

    pub fn set_show_pdfs(&mut self, show: bool) {
        if self.show_pdfs != show {
            self.show_pdfs = show;
            self.request_fit();
        }
    }

    pub fn set_show_missing(&mut self, show: bool) {
        if self.show_missing != show {
            self.show_missing = show;
            self.request_fit();
        }
    }

    pub fn set_show_orphans(&mut self, show: bool) {
        if self.show_orphans != show {
            self.show_orphans = show;
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
            // Wake the simulation so neighbours settle around the new position.
            self.physics.nudge();
        }
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
        self.fit_revision = self.fit_revision.wrapping_add(1);
    }

    /// Advance the physics simulation by one time step. Returns `true` if the
    /// simulation is still running and the view should be redrawn.
    pub fn physics_tick(&mut self) -> bool {
        self.physics.tick(&mut self.layout.positions, &self.pinned)
    }

    pub fn node(&self, path: &str) -> Option<&GraphNode> {
        self.snapshot.nodes.iter().find(|node| node.path == path)
    }

    pub fn filtered_graph(&self) -> GraphSnapshot {
        let mut allowed: BTreeSet<String> = match self.scope {
            GraphScope::Global => self
                .snapshot
                .nodes
                .iter()
                .map(|node| node.path.clone())
                .collect(),
            GraphScope::Local { depth } => self.local_paths(depth),
        };

        allowed.retain(|path| {
            self.node(path).is_some_and(|node| {
                (self.show_pdfs || node.kind != GraphNodeKind::Pdf)
                    && (self.show_missing || node.kind != GraphNodeKind::Missing)
                    && (self.show_orphans || node.incoming + node.outgoing > 0)
            })
        });

        let query = self.query.trim().to_lowercase();
        if !query.is_empty() {
            let matches: BTreeSet<String> = allowed
                .iter()
                .filter(|path| {
                    self.node(path).is_some_and(|node| {
                        node.path.to_lowercase().contains(&query)
                            || node.label.to_lowercase().contains(&query)
                    })
                })
                .cloned()
                .collect();
            // Keep immediate context around matches; a graph search is more
            // useful when the reason a result matters stays visible.
            let mut contextual = matches.clone();
            for edge in &self.snapshot.edges {
                if matches.contains(&edge.source) && allowed.contains(&edge.target) {
                    contextual.insert(edge.target.clone());
                }
                if matches.contains(&edge.target) && allowed.contains(&edge.source) {
                    contextual.insert(edge.source.clone());
                }
            }
            allowed = contextual;
        }

        let nodes = self
            .snapshot
            .nodes
            .iter()
            .filter(|node| allowed.contains(&node.path))
            .cloned()
            .collect();
        let edges = self
            .snapshot
            .edges
            .iter()
            .filter(|edge| allowed.contains(&edge.source) && allowed.contains(&edge.target))
            .cloned()
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
        let adjacency = adjacency(&self.snapshot);
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
        stats.components = connected_components(&self.snapshot).len();
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
        let adjacency = adjacency(&self.snapshot);
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

fn adjacency(snapshot: &GraphSnapshot) -> BTreeMap<String, BTreeSet<String>> {
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = snapshot
        .nodes
        .iter()
        .map(|node| (node.path.clone(), BTreeSet::new()))
        .collect();
    for edge in &snapshot.edges {
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.target.clone());
        adjacency
            .entry(edge.target.clone())
            .or_default()
            .insert(edge.source.clone());
    }
    adjacency
}

fn connected_components(snapshot: &GraphSnapshot) -> Vec<Vec<String>> {
    let adjacency = adjacency(snapshot);
    let mut remaining: BTreeSet<String> = adjacency.keys().cloned().collect();
    let mut components = Vec::new();
    while let Some(start) = remaining.first().cloned() {
        remaining.remove(&start);
        let mut component = Vec::new();
        let mut queue = VecDeque::from([start]);
        while let Some(path) = queue.pop_front() {
            component.push(path.clone());
            if let Some(neighbors) = adjacency.get(&path) {
                for neighbor in neighbors {
                    if remaining.remove(neighbor) {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
        component.sort();
        components.push(component);
    }
    components.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
    components
}

fn build_layout(snapshot: &GraphSnapshot) -> GraphLayout {
    let adjacency = adjacency(snapshot);
    let components = connected_components(snapshot);
    if components.is_empty() {
        return GraphLayout::default();
    }

    // Lay each connected component out around its own origin first. Packing
    // the measured rectangles afterwards prevents large components from
    // overlapping smaller islands, which a fixed grid cannot guarantee.
    let mut islands = Vec::with_capacity(components.len());
    for component in &components {
        let mut local = BTreeMap::new();
        let root = component
            .iter()
            .max_by(|a, b| {
                adjacency
                    .get(*a)
                    .map_or(0, BTreeSet::len)
                    .cmp(&adjacency.get(*b).map_or(0, BTreeSet::len))
                    .then_with(|| b.cmp(a))
            })
            .expect("components are never empty")
            .clone();
        local.insert(root.clone(), GraphPoint::default());

        let mut levels: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        let mut seen = BTreeSet::from([root.clone()]);
        let mut queue = VecDeque::from([(root, 0_usize)]);
        while let Some((path, level)) = queue.pop_front() {
            if level > 0 {
                levels.entry(level).or_default().push(path.clone());
            }
            if let Some(neighbors) = adjacency.get(&path) {
                for neighbor in neighbors {
                    if component.binary_search(neighbor).is_ok() && seen.insert(neighbor.clone()) {
                        queue.push_back((neighbor.clone(), level + 1));
                    }
                }
            }
        }

        for (level, mut paths) in levels {
            paths.sort();
            let count = paths.len();
            let radius = (level as f32 * 105.0).max(count as f32 * 13.0);
            let offset = if level % 2 == 0 { 0.35 } else { 0.0 };
            for (index, path) in paths.into_iter().enumerate() {
                let angle = std::f32::consts::TAU * index as f32 / count.max(1) as f32 + offset;
                local.insert(
                    path,
                    GraphPoint {
                        x: radius * angle.cos(),
                        y: radius * angle.sin(),
                    },
                );
            }
        }

        let (min_x, max_x, min_y, max_y) = point_bounds(local.values().copied());
        const ISLAND_PADDING: f32 = 150.0;
        islands.push((
            local,
            min_x,
            min_y,
            (max_x - min_x).max(80.0) + ISLAND_PADDING,
            (max_y - min_y).max(80.0) + ISLAND_PADDING,
        ));
    }

    let total_area: f32 = islands.iter().map(|island| island.3 * island.4).sum();
    let widest = islands
        .iter()
        .map(|island| island.3)
        .fold(0.0_f32, f32::max);
    let target_row_width = (total_area.sqrt() * 1.25).max(widest);
    let mut cursor_x = 0.0;
    let mut cursor_y = 0.0;
    let mut row_height = 0.0_f32;
    let mut positions = BTreeMap::new();

    for (local, min_x, min_y, width, height) in islands {
        if cursor_x > 0.0 && cursor_x + width > target_row_width {
            cursor_x = 0.0;
            cursor_y += row_height;
            row_height = 0.0;
        }
        let offset_x = cursor_x + 75.0 - min_x;
        let offset_y = cursor_y + 75.0 - min_y;
        for (path, point) in local {
            positions.insert(
                path,
                GraphPoint {
                    x: point.x + offset_x,
                    y: point.y + offset_y,
                },
            );
        }
        cursor_x += width;
        row_height = row_height.max(height);
    }

    // Center the packed map around the canvas origin so Fit and reset produce
    // stable, intuitive pan values regardless of component count.
    let (min_x, max_x, min_y, max_y) = point_bounds(positions.values().copied());
    let center_x = (min_x + max_x) / 2.0;
    let center_y = (min_y + max_y) / 2.0;
    for point in positions.values_mut() {
        point.x -= center_x;
        point.y -= center_y;
    }
    GraphLayout { positions }
}

fn point_bounds(points: impl IntoIterator<Item = GraphPoint>) -> (f32, f32, f32, f32) {
    let mut points = points.into_iter();
    let first = points.next().unwrap_or_default();
    points.fold(
        (first.x, first.x, first.y, first.y),
        |(min_x, max_x, min_y, max_y), point| {
            (
                min_x.min(point.x),
                max_x.max(point.x),
                min_y.min(point.y),
                max_y.max(point.y),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::types::{GraphEdge, GraphEdgeKind};

    fn node(path: &str) -> GraphNode {
        GraphNode {
            path: path.to_string(),
            label: path.trim_end_matches(".md").to_string(),
            kind: GraphNodeKind::Markdown,
            exists: true,
            incoming: 0,
            outgoing: 0,
        }
    }

    fn edge(source: &str, target: &str) -> GraphEdge {
        GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            kind: GraphEdgeKind::WikiLink,
            weight: 1,
        }
    }

    fn sample() -> GraphSnapshot {
        GraphSnapshot {
            nodes: ["a.md", "b.md", "c.md", "d.md", "orphan.md"]
                .into_iter()
                .map(node)
                .collect(),
            edges: vec![
                edge("a.md", "b.md"),
                edge("b.md", "c.md"),
                edge("c.md", "d.md"),
            ],
        }
    }

    #[test]
    fn layout_is_deterministic_and_finite() {
        let first = build_layout(&sample());
        let second = build_layout(&sample());
        assert_eq!(first.positions, second.positions);
        assert!(
            first
                .positions
                .values()
                .all(|point| point.x.is_finite() && point.y.is_finite())
        );
    }

    #[test]
    fn local_scope_is_bounded_by_depth() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_active_path(Some("a.md".to_string()));
        state.set_scope(GraphScope::Local { depth: 1 });
        assert_eq!(state.filtered_graph().nodes.len(), 2);
        state.set_scope(GraphScope::Local { depth: 2 });
        assert_eq!(state.filtered_graph().nodes.len(), 3);
    }

    #[test]
    fn local_scope_without_an_active_document_is_empty() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_scope(GraphScope::Local { depth: 2 });
        assert!(state.filtered_graph().nodes.is_empty());
    }

    #[test]
    fn query_keeps_matching_node_and_immediate_context() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_query("c".to_string());
        let paths: Vec<_> = state
            .filtered_graph()
            .nodes
            .into_iter()
            .map(|node| node.path)
            .collect();
        assert_eq!(paths, vec!["b.md", "c.md", "d.md"]);
    }

    #[test]
    fn related_notes_exclude_direct_neighbors() {
        let snapshot = GraphSnapshot {
            nodes: ["a.md", "b.md", "c.md", "shared.md"]
                .into_iter()
                .map(node)
                .collect(),
            edges: vec![
                edge("a.md", "shared.md"),
                edge("b.md", "shared.md"),
                edge("a.md", "c.md"),
            ],
        };
        let mut state = GraphState::new();
        state.set_snapshot(snapshot);
        let related = state.related("a.md", 5);
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].node.path, "b.md");
        assert_eq!(related[0].shared_connections, 1);
    }

    #[test]
    fn moving_a_node_rejects_non_finite_coordinates() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        let before = state.graph_layout().get("a.md").unwrap();
        state.move_node("a.md", f32::NAN, 12.0);
        assert_eq!(state.graph_layout().get("a.md"), Some(before));
        state.move_node("a.md", 20.0, 30.0);
        assert_eq!(
            state.graph_layout().get("a.md"),
            Some(GraphPoint { x: 20.0, y: 30.0 })
        );
    }

    #[test]
    fn identical_snapshot_keeps_view_and_reset_releases_pins() {
        let mut state = GraphState::new();
        let snapshot = sample();
        state.set_snapshot(snapshot.clone());
        let fit_revision = state.fit_revision;
        state.set_snapshot(snapshot);
        assert_eq!(state.fit_revision, fit_revision);

        state.move_node("a.md", 9_000.0, 9_000.0);
        state.reset_layout();
        assert_ne!(
            state.graph_layout().get("a.md"),
            Some(GraphPoint {
                x: 9_000.0,
                y: 9_000.0,
            })
        );
    }

    #[test]
    fn pinned_node_survives_topology_changes() {
        let mut state = GraphState::new();
        let mut snapshot = sample();
        state.set_snapshot(snapshot.clone());
        state.move_node("a.md", 9_000.0, -4_000.0);

        snapshot.edges.push(edge("d.md", "orphan.md"));
        state.set_snapshot(snapshot);

        assert_eq!(
            state.graph_layout().get("a.md"),
            Some(GraphPoint {
                x: 9_000.0,
                y: -4_000.0,
            })
        );
    }

    #[test]
    fn vault_change_clears_snapshot_pins_and_in_flight_refresh() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.select(Some("a.md".to_string()));
        state.move_node("a.md", 9_000.0, 9_000.0);
        state.mark_dirty();
        let old_generation = state.begin_refresh().unwrap();

        state.clear_for_vault_change();

        assert!(state.node("a.md").is_none());
        assert!(state.selected_path.is_none());
        assert!(state.graph_layout().positions.is_empty());
        assert!(state.refresh_due());
        assert!(!state.complete_refresh(old_generation, sample()));
    }

    #[test]
    fn measured_component_packing_keeps_islands_separate() {
        let mut nodes = vec![node("hub.md"), node("pair-a.md"), node("pair-b.md")];
        let mut edges = vec![edge("pair-a.md", "pair-b.md")];
        for index in 0..48 {
            let path = format!("leaf-{index:02}.md");
            nodes.push(node(&path));
            edges.push(edge("hub.md", &path));
        }
        let snapshot = GraphSnapshot { nodes, edges };
        let layout = build_layout(&snapshot);
        let components = connected_components(&snapshot);
        let bounds: Vec<_> = components
            .iter()
            .map(|component| {
                point_bounds(
                    component
                        .iter()
                        .filter_map(|path| layout.positions.get(path).copied()),
                )
            })
            .collect();

        for (index, first) in bounds.iter().enumerate() {
            for second in &bounds[index + 1..] {
                let separated = first.1 + 100.0 <= second.0
                    || second.1 + 100.0 <= first.0
                    || first.3 + 100.0 <= second.2
                    || second.3 + 100.0 <= first.2;
                assert!(separated, "component bounds overlap: {first:?} {second:?}");
            }
        }
    }

    #[test]
    fn refresh_discards_snapshot_when_graph_changed_in_flight() {
        let mut state = GraphState::new();
        let first_generation = state.begin_refresh().unwrap();
        assert!(!state.refresh_due());

        state.mark_dirty();
        assert!(!state.complete_refresh(first_generation, sample()));
        assert!(state.is_dirty());
        assert!(state.refresh_due());
        assert!(state.node("a.md").is_none());

        let current_generation = state.begin_refresh().unwrap();
        assert!(state.complete_refresh(current_generation, sample()));
        assert!(!state.is_dirty());
        assert!(!state.refresh_due());
        assert!(state.node("a.md").is_some());
    }

    #[test]
    fn failed_generation_waits_for_retry_or_new_invalidation() {
        let mut state = GraphState::new();
        let generation = state.begin_refresh().unwrap();

        assert!(state.fail_refresh(generation));
        assert!(state.is_dirty());
        assert!(!state.refresh_due());

        state.retry_refresh();
        assert!(state.refresh_due());
        let retry_generation = state.begin_refresh().unwrap();
        assert_eq!(retry_generation, generation);
        assert!(state.fail_refresh(retry_generation));

        state.mark_dirty();
        assert!(state.refresh_due());
        assert_ne!(state.begin_refresh(), Some(generation));
    }

    #[test]
    fn stale_failure_does_not_throttle_the_new_generation() {
        let mut state = GraphState::new();
        let stale_generation = state.begin_refresh().unwrap();
        state.mark_dirty();

        assert!(!state.fail_refresh(stale_generation));
        assert!(state.refresh_due());
        assert_ne!(state.begin_refresh(), Some(stale_generation));
    }
}
