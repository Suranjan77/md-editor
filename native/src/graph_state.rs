//! Cached state and graph-derived research insights for the knowledge graph.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

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

/// Barnes-Hut quadtree used to approximate all-pairs repulsion.
///
/// Every node carries unit mass. A subtree whose extent is small relative to
/// its distance from the body being evaluated is collapsed into a single point
/// mass at its center of gravity, which turns the O(n²) repulsion pass into
/// O(n log n) at a bounded, tunable error.
#[derive(Debug, Clone, Default)]
struct Quadtree {
    cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy)]
struct Cell {
    /// Indices of the four child cells, or [`Quadtree::EMPTY`].
    children: [u32; 4],
    /// Center of mass of everything under this cell.
    com: [f32; 2],
    mass: f32,
    /// Geometric center and half-extent of the square this cell covers.
    center: [f32; 2],
    half: f32,
    /// Body index when this cell holds exactly one, else [`Quadtree::EMPTY`].
    body: u32,
}

impl Quadtree {
    const EMPTY: u32 = u32::MAX;
    /// Opening angle. Smaller is more accurate and slower; 0.0 degenerates to
    /// exact all-pairs, which the tests use as the reference.
    const THETA: f32 = 0.75;
    /// Coincident or near-coincident points would otherwise subdivide forever.
    const MAX_DEPTH: u32 = 24;

    fn cell(center: [f32; 2], half: f32) -> Cell {
        Cell {
            children: [Self::EMPTY; 4],
            com: [0.0, 0.0],
            mass: 0.0,
            center,
            half,
            body: Self::EMPTY,
        }
    }

    fn build(&mut self, positions: &[[f32; 2]]) {
        self.cells.clear();
        if positions.is_empty() {
            return;
        }

        // One square root cell covering every body.
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for p in positions {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
        let half = ((max_x - min_x).max(max_y - min_y) * 0.5).max(1.0) * 1.05;
        let center = [(min_x + max_x) * 0.5, (min_y + max_y) * 0.5];
        self.cells.push(Self::cell(center, half));

        for (index, position) in positions.iter().enumerate() {
            self.insert(index as u32, *position);
        }
    }

    fn quadrant(cell: &Cell, p: [f32; 2]) -> usize {
        usize::from(p[0] >= cell.center[0]) | (usize::from(p[1] >= cell.center[1]) << 1)
    }

    fn child_cell(parent: &Cell, quadrant: usize) -> Cell {
        let half = parent.half * 0.5;
        let dx = if quadrant & 1 == 0 { -half } else { half };
        let dy = if quadrant & 2 == 0 { -half } else { half };
        Self::cell([parent.center[0] + dx, parent.center[1] + dy], half)
    }

    fn insert(&mut self, body: u32, position: [f32; 2]) {
        let mut current = 0_usize;
        let mut depth = 0_u32;

        loop {
            // Accumulate mass/COM on the way down so no second pass is needed.
            let cell = &mut self.cells[current];
            let mass = cell.mass + 1.0;
            cell.com[0] = (cell.com[0] * cell.mass + position[0]) / mass;
            cell.com[1] = (cell.com[1] * cell.mass + position[1]) / mass;
            cell.mass = mass;

            if mass == 1.0 {
                // First body to land here: keep it as a leaf.
                cell.body = body;
                return;
            }

            if depth >= Self::MAX_DEPTH {
                // Bodies are effectively coincident; stop subdividing and let
                // them share this cell as one clump.
                cell.body = Self::EMPTY;
                return;
            }

            // This cell now holds more than one body, so it must branch. Push
            // any resident leaf body down before descending with the new one.
            let resident = std::mem::replace(&mut self.cells[current].body, Self::EMPTY);
            if resident != Self::EMPTY {
                let resident_position = self.cells[current].com;
                // The COM already includes both bodies, so recover the
                // resident's own position from the running average.
                let resident_position = [
                    resident_position[0] * mass - position[0],
                    resident_position[1] * mass - position[1],
                ];
                self.push_down(current, resident, resident_position, depth);
            }

            let quadrant = Self::quadrant(&self.cells[current], position);
            current = self.ensure_child(current, quadrant);
            depth += 1;
        }
    }

    /// Place an already-counted body into the correct child of `parent`,
    /// creating cells as needed without touching `parent`'s own mass.
    fn push_down(&mut self, parent: usize, body: u32, position: [f32; 2], depth: u32) {
        let mut current = parent;
        let mut depth = depth;
        loop {
            let quadrant = Self::quadrant(&self.cells[current], position);
            let child = self.ensure_child(current, quadrant);
            let cell = &mut self.cells[child];
            let mass = cell.mass + 1.0;
            cell.com[0] = (cell.com[0] * cell.mass + position[0]) / mass;
            cell.com[1] = (cell.com[1] * cell.mass + position[1]) / mass;
            cell.mass = mass;
            depth += 1;

            if mass == 1.0 {
                cell.body = body;
                return;
            }
            if depth >= Self::MAX_DEPTH {
                cell.body = Self::EMPTY;
                return;
            }
            let resident = std::mem::replace(&mut self.cells[child].body, Self::EMPTY);
            if resident != Self::EMPTY {
                let com = self.cells[child].com;
                let resident_position =
                    [com[0] * mass - position[0], com[1] * mass - position[1]];
                self.push_down(child, resident, resident_position, depth);
            }
            current = child;
        }
    }

    fn ensure_child(&mut self, parent: usize, quadrant: usize) -> usize {
        let existing = self.cells[parent].children[quadrant];
        if existing != Self::EMPTY {
            return existing as usize;
        }
        let child = Self::child_cell(&self.cells[parent], quadrant);
        let index = self.cells.len();
        self.cells.push(child);
        self.cells[parent].children[quadrant] = index as u32;
        index
    }

    /// Total repulsive force exerted on `body` at `position`.
    fn repulsion_on(&self, body: usize, position: [f32; 2], repulsion: f32) -> [f32; 2] {
        if self.cells.is_empty() {
            return [0.0, 0.0];
        }
        let mut force = [0.0_f32, 0.0];
        // A fixed-size stack keeps the hot loop allocation-free.
        let mut stack = [0_u32; 128];
        let mut depth = 1_usize;

        while depth > 0 {
            depth -= 1;
            let cell = &self.cells[stack[depth] as usize];
            if cell.mass == 0.0 || cell.body == body as u32 {
                continue;
            }

            let dx = position[0] - cell.com[0];
            let dy = position[1] - cell.com[1];
            let dist_sq = dx * dx + dy * dy;
            let leaf = cell.children.iter().all(|c| *c == Self::EMPTY);

            if leaf || (cell.half * 2.0).powi(2) < Self::THETA * Self::THETA * dist_sq {
                let dist_sq = dist_sq.max(1.0);
                let inv_dist = dist_sq.sqrt().recip();
                let magnitude = repulsion * cell.mass / dist_sq;
                force[0] += magnitude * dx * inv_dist;
                force[1] += magnitude * dy * inv_dist;
            } else {
                for child in cell.children {
                    if child != Self::EMPTY && depth < stack.len() {
                        stack[depth] = child;
                        depth += 1;
                    }
                }
            }
        }
        force
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
    /// Nodes with no edges at all. A force layout has nothing to say about
    /// where an unlinked note belongs, so simulating it only lets repulsion
    /// and center gravity compress thousands of them into one uniform disc.
    /// They keep the deterministic scatter `build_layout` gave them, which is
    /// what makes a link-poor vault read as a starfield with the connected
    /// clusters as constellations inside it.
    anchored: Vec<bool>,
    // ── Reusable computation buffers ────────────────────────────
    forces: Vec<[f32; 2]>,
    pos: Vec<[f32; 2]>,
    /// Rebuilt every tick; kept here so its arena allocation is reused.
    tree: Quadtree,
    // ── Tuning parameters ───────────────────────────────────────
    repulsion: f32,
    attraction: f32,
    damping: f32,
    center_gravity: f32,
    rest_length: f32,
    /// Mean kinetic energy per node below which the layout is considered
    /// settled and the simulation parks itself.
    stability_threshold: f32,
    /// Ticks left in this settling run. A force layout on a link-poor vault can
    /// jitter below its own stability threshold indefinitely, so the budget
    /// guarantees the simulation always parks and stops burning a core. The
    /// user can always resume it or relayout.
    budget: u32,
}

impl Default for PhysicsState {
    fn default() -> Self {
        Self {
            running: false,
            index: Vec::new(),
            edge_pairs: Vec::new(),
            vel: Vec::new(),
            anchored: Vec::new(),
            forces: Vec::new(),
            pos: Vec::new(),
            tree: Quadtree::default(),
            repulsion: 8_000.0,
            attraction: 0.012,
            damping: 0.72,
            center_gravity: 0.0008,
            rest_length: 100.0,
            stability_threshold: 0.006,
            budget: 0,
        }
    }
}

impl PhysicsState {
    /// Ticks allowed for a layout to settle from scratch — about 20 s at the
    /// 33 ms tick interval.
    const SETTLE_BUDGET: u32 = 600;
    /// Ticks granted when something disturbs a settled layout, e.g. a drag.
    const NUDGE_BUDGET: u32 = 150;

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

        // Anything the edge list never mentions stays where it was placed.
        self.anchored.clear();
        self.anchored.resize(n, true);
        for [source, target] in &self.edge_pairs {
            self.anchored[*source] = false;
            self.anchored[*target] = false;
        }

        // (Re)initialize per-node arrays.
        self.vel.clear();
        self.vel.resize(n, [0.0; 2]);
        self.forces.resize(n, [0.0; 2]);
        self.pos.resize(n, [0.0; 2]);

        self.running = n > 1;
        self.budget = Self::SETTLE_BUDGET;
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
        if self.budget == 0 {
            self.running = false;
            return false;
        }
        self.budget -= 1;

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
            .enumerate()
            .map(|(i, p)| self.anchored[i] || pinned.contains(p))
            .collect();

        // ── 1. Coulomb repulsion (Barnes-Hut, O(n log n)) ───────
        // A brute-force pass costs ~118 ms at 10k nodes, which is four times
        // the tick interval — the simulation could never keep up and the whole
        // window went unresponsive. The quadtree approximates any group of
        // distant nodes by its center of mass instead.
        self.tree.build(&self.pos[..n]);
        let repulsion = self.repulsion;
        for i in 0..n {
            if pinned_mask[i] {
                continue;
            }
            let [fx, fy] = self.tree.repulsion_on(i, self.pos[i], repulsion);
            self.forces[i][0] += fx;
            self.forces[i][1] += fy;
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
        // Compared per node, not as a total: an absolute energy budget is
        // unreachable once it is summed over thousands of nodes, so a large
        // vault would simulate — and burn a core — forever.
        let movers = pinned_mask.iter().filter(|p| !**p).count().max(1);
        if total_energy / (movers as f32) < self.stability_threshold {
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
            self.budget = self.budget.max(Self::NUDGE_BUDGET);
        }
    }

    /// Stop the simulation and clear velocities.
    pub fn stop(&mut self) {
        self.running = false;
        self.budget = 0;
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
            let (Some(&source), Some(&target)) =
                (index.get(&edge.source), index.get(&edge.target))
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
                    self.node_index.get(&edge.source).copied().unwrap_or(usize::MAX),
                    self.node_index.get(&edge.target).copied().unwrap_or(usize::MAX),
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
    components_from(&adjacency(snapshot))
}

/// Connected components of an already-built adjacency map, so callers holding
/// the cached one do not pay to rebuild it.
fn components_from(adjacency: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
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

/// One connected component, measured and ready to be placed by `build_layout`.
struct Island {
    nodes: BTreeMap<String, GraphPoint>,
    min_x: f32,
    min_y: f32,
    /// Footprint including padding — reserves scatter space.
    width: f32,
    height: f32,
    /// Half-diagonal of the bare node span — drives clearance.
    extent: f32,
    seed: u64,
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
        // Seed the island's scatter from its own contents so the layout stays
        // deterministic across rebuilds.
        let seed = hash_path(local.keys().next().map_or("", String::as_str));
        islands.push(Island {
            nodes: local,
            min_x,
            min_y,
            // Padded footprint drives how much room the scatter reserves.
            width: (max_x - min_x).max(80.0) + ISLAND_PADDING,
            height: (max_y - min_y).max(80.0) + ISLAND_PADDING,
            // Bare span drives clearance. Using the padded box here made a
            // lone note "reach" 163 units, which flung the orphans of a small
            // vault so far out that Fit had to shrink the interesting cluster
            // to a third of its readable size.
            extent: 0.5
                * ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt(),
            seed,
        });
    }

    // Islands are scattered along a phyllotaxis (sunflower) spiral rather than
    // packed into rows. Row packing filled a hard-edged rectangle, and once the
    // simulation rounded it off a link-poor vault looked like one solid disc.
    // The spiral spaces islands evenly but without a grid, so a vault reads as
    // a field of separate points with real gaps between them.
    //
    // Islands are visited largest-first (`connected_components` sorts by size),
    // which puts the substantial clusters near the middle and scatters the
    // single-note dust around them.
    const GOLDEN_ANGLE: f32 = 2.399_963_2;
    // Ring radius grows with the *area* already placed, so each island lands
    // outside everything before it however large those were.
    const SPREAD: f32 = 1.15;
    /// Breathing room between the central cluster and its nearest neighbour.
    const GAP: f32 = 140.0;
    let core = islands.first().map_or(0.0, |island| island.extent);

    let mut positions = BTreeMap::new();
    let mut placed_area = 0.0_f32;

    for (rank, island) in islands.into_iter().enumerate() {
        let Island {
            nodes: local,
            min_x,
            min_y,
            width,
            height,
            extent,
            seed,
        } = island;
        let radius = if rank == 0 {
            0.0
        } else {
            placed_area += width * height;
            // Clear the central island, this island's own span, and the area
            // already laid down further in.
            core + extent + GAP + (placed_area / std::f32::consts::PI).sqrt() * SPREAD
        };

        // A pure phyllotaxis spiral is *too* even: at vault scale its
        // parastichies show up as concentric arcs and the whole thing reads as
        // a sunflower rather than a sky. Deterministic per-island jitter breaks
        // the pattern while preserving density.
        //
        // Radial jitter is outward-only so it can never eat into the clearance
        // computed above, and angular jitter shrinks with rank so the arc it
        // sweeps stays comparable to the local spacing instead of growing with
        // the radius.
        let (radial, angular) = unit_pair(seed);
        let radius = radius + radial * (extent + GAP) * 0.45;
        let angle = rank as f32 * GOLDEN_ANGLE
            + (angular - 0.5) * 1.4 / ((rank as f32) + 1.0).sqrt();
        // Place each island by its center so big clusters are not shoved
        // off-spiral by their own extent.
        let offset_x = radius * angle.cos() - min_x - width * 0.5;
        let offset_y = radius * angle.sin() - min_y - height * 0.5;
        for (path, point) in local {
            positions.insert(
                path,
                GraphPoint {
                    x: point.x + offset_x,
                    y: point.y + offset_y,
                },
            );
        }
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

/// FNV-1a. Stable across runs and platforms, unlike `DefaultHasher`, which
/// matters because the layout must be reproducible for a given vault.
fn hash_path(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Two independent values in `0.0..1.0` from one hash.
fn unit_pair(seed: u64) -> (f32, f32) {
    let first = (seed >> 11) as u32 as f32 / u32::MAX as f32;
    let second = (seed >> 43) as u32 as f32 / u32::MAX as f32;
    (first.clamp(0.0, 1.0), second.clamp(0.0, 1.0))
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
        assert_eq!(state.filtered().snapshot.nodes.len(), 2);
        state.set_scope(GraphScope::Local { depth: 2 });
        assert_eq!(state.filtered().snapshot.nodes.len(), 3);
    }

    #[test]
    fn local_scope_without_an_active_document_is_empty() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_scope(GraphScope::Local { depth: 2 });
        assert!(state.filtered().snapshot.nodes.is_empty());
    }

    #[test]
    fn query_keeps_matching_node_and_immediate_context() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_query("c".to_string());
        let paths: Vec<_> = state
            .filtered()
            .snapshot
            .nodes
            .iter()
            .map(|node| node.path.clone())
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

    /// Reference implementation the quadtree has to agree with.
    fn brute_force_repulsion(
        positions: &[[f32; 2]],
        body: usize,
        repulsion: f32,
    ) -> [f32; 2] {
        let mut force = [0.0_f32, 0.0];
        for (other, p) in positions.iter().enumerate() {
            if other == body {
                continue;
            }
            let dx = positions[body][0] - p[0];
            let dy = positions[body][1] - p[1];
            let dist_sq = (dx * dx + dy * dy).max(1.0);
            let inv_dist = dist_sq.sqrt().recip();
            let magnitude = repulsion / dist_sq;
            force[0] += magnitude * dx * inv_dist;
            force[1] += magnitude * dy * inv_dist;
        }
        force
    }

    fn scattered(count: usize) -> Vec<[f32; 2]> {
        // Deterministic pseudo-random spread — no rand dependency needed.
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1_u64 << 53) as f32
        };
        (0..count)
            .map(|_| [next() * 2_000.0 - 1_000.0, next() * 2_000.0 - 1_000.0])
            .collect()
    }

    #[test]
    fn quadtree_accounts_for_every_body_exactly_once() {
        let positions = scattered(500);
        let mut tree = Quadtree::default();
        tree.build(&positions);
        assert_eq!(tree.cells[0].mass, 500.0, "root must hold every body");

        // Coincident points must terminate rather than subdivide forever.
        let duplicates = vec![[7.0, -3.0]; 64];
        tree.build(&duplicates);
        assert_eq!(tree.cells[0].mass, 64.0);
    }

    #[test]
    fn barnes_hut_approximates_brute_force_repulsion() {
        let positions = scattered(400);
        let mut tree = Quadtree::default();
        tree.build(&positions);

        // Compare on a sample of bodies; the approximation is bounded by THETA,
        // so require the direction and magnitude to stay close, not identical.
        for body in [0_usize, 17, 99, 250, 399] {
            let exact = brute_force_repulsion(&positions, body, 8_000.0);
            let approx = tree.repulsion_on(body, positions[body], 8_000.0);
            let exact_mag = (exact[0] * exact[0] + exact[1] * exact[1]).sqrt();
            let error = ((approx[0] - exact[0]).powi(2) + (approx[1] - exact[1]).powi(2)).sqrt();
            assert!(
                error <= exact_mag * 0.25 + 1.0,
                "body {body}: exact {exact:?} approx {approx:?} (err {error})"
            );
        }
    }

    #[test]
    fn repulsion_pushes_a_body_away_from_a_dense_cluster() {
        // One body to the right of a tight cluster on the left must be pushed
        // further right — a sign error here would collapse every layout.
        let mut positions: Vec<[f32; 2]> = (0..64)
            .map(|i| [-500.0 + (i % 8) as f32, -500.0 + (i / 8) as f32])
            .collect();
        positions.push([500.0, -500.0]);
        let body = positions.len() - 1;

        let mut tree = Quadtree::default();
        tree.build(&positions);
        let force = tree.repulsion_on(body, positions[body], 8_000.0);

        assert!(force[0] > 0.0, "expected outward push, got {force:?}");
        assert!(force[1].abs() < force[0], "force should be mostly horizontal");
    }

    #[test]
    fn simulation_still_separates_nodes_it_used_to_separate() {
        // End-to-end guard that swapping in the quadtree did not break layout:
        // two unconnected nodes starting close together must drift apart.
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.reset_layout();
        for _ in 0..120 {
            state.physics_tick();
        }
        let positions: Vec<GraphPoint> =
            state.graph_layout().positions.values().copied().collect();
        for (i, a) in positions.iter().enumerate() {
            assert!(a.x.is_finite() && a.y.is_finite(), "layout went non-finite");
            for b in &positions[i + 1..] {
                let d = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
                assert!(d > 1.0, "nodes collapsed onto each other: {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn a_large_layout_settles_instead_of_simulating_forever() {
        // The stability test is per node; an absolute energy budget is
        // unreachable once summed over thousands of nodes, which pinned a CPU
        // core for as long as the graph stayed open.
        let mut nodes = Vec::new();
        for i in 0..800 {
            nodes.push(GraphNode {
                path: format!("n{i}.md"),
                label: format!("n{i}"),
                kind: GraphNodeKind::Markdown,
                exists: true,
                incoming: 0,
                outgoing: 0,
            });
        }
        let mut state = GraphState::new();
        state.set_snapshot(GraphSnapshot {
            nodes,
            edges: Vec::new(),
        });
        assert!(state.physics.running);

        let mut ticks = 0;
        while state.physics_tick() && ticks < 5_000 {
            ticks += 1;
        }
        assert!(
            ticks < 5_000,
            "layout never stabilized after {ticks} ticks — it would spin forever"
        );
        assert!(!state.physics.running);
    }

    #[test]
    fn a_vault_of_unlinked_notes_is_flagged_as_a_hairball() {
        let mut nodes = Vec::new();
        for i in 0..300 {
            nodes.push(GraphNode {
                path: format!("n{i}.md"),
                label: format!("n{i}"),
                kind: GraphNodeKind::Markdown,
                exists: true,
                incoming: 0,
                outgoing: 0,
            });
        }
        let mut state = GraphState::new();
        state.set_snapshot(GraphSnapshot {
            nodes,
            edges: Vec::new(),
        });
        assert!(state.orphans_dominate());

        // Acting on the offer clears the condition, so the hint cannot persist
        // after the user has taken it.
        state.set_show_orphans(false);
        assert!(!state.orphans_dominate());

        // A small vault is never nagged, however unlinked it is.
        let mut small = GraphState::new();
        small.set_snapshot(sample());
        assert!(!small.orphans_dominate());
    }

    #[test]
    fn pins_toggle_and_release_without_touching_positions() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        let before = state.graph_layout().get("a.md").unwrap();

        state.toggle_pin("a.md");
        assert!(state.is_pinned("a.md"));
        assert_eq!(state.pinned_count(), 1);
        assert_eq!(state.graph_layout().get("a.md"), Some(before));

        state.toggle_pin("a.md");
        assert!(!state.is_pinned("a.md"));

        state.toggle_pin("b.md");
        state.toggle_pin("c.md");
        state.unpin_all();
        assert_eq!(state.pinned_count(), 0);

        // A path that is not in the layout can never become pinned.
        state.toggle_pin("nowhere.md");
        assert_eq!(state.pinned_count(), 0);
    }

    #[test]
    fn zoom_buttons_keep_the_canvas_center_fixed() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_view(120.0, -60.0, 1.0);
        let revision = state.view_revision;

        state.zoom_by(2.0);

        assert_eq!(state.zoom, 2.0);
        // The world point at the canvas center is -pan/zoom; holding it fixed
        // means pan must scale with zoom.
        assert_eq!(state.pan, (240.0, -120.0));
        assert_ne!(state.view_revision, revision);
    }

    #[test]
    fn zoom_clamps_and_survives_the_floor() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_view(0.0, 0.0, MIN_ZOOM);
        state.zoom_by(0.1);
        assert_eq!(state.zoom, MIN_ZOOM);
        assert!(state.pan.0.is_finite() && state.pan.1.is_finite());

        state.set_view(0.0, 0.0, MAX_ZOOM);
        state.zoom_by(10.0);
        assert_eq!(state.zoom, MAX_ZOOM);
    }

    #[test]
    fn focusing_a_node_selects_it_and_centers_the_view() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        let point = state.graph_layout().get("c.md").unwrap();

        state.focus_on("c.md");

        assert_eq!(state.selected_path.as_deref(), Some("c.md"));
        // Centered means the node lands at the canvas origin: pan = -world*zoom.
        assert!((state.pan.0 - -point.x * state.zoom).abs() < 0.001);
        assert!((state.pan.1 - -point.y * state.zoom).abs() < 0.001);
        assert!(state.zoom >= 0.9);
    }

    #[test]
    fn empty_reasons_distinguish_the_ways_a_graph_goes_blank() {
        let mut state = GraphState::new();
        // Nothing loaded yet, and still dirty from construction.
        assert_eq!(state.empty_reason(), EmptyReason::Loading);

        state.set_snapshot(GraphSnapshot::default());
        assert_eq!(state.empty_reason(), EmptyReason::NoDocuments);

        state.set_snapshot(sample());
        state.set_scope(GraphScope::Local { depth: 1 });
        assert_eq!(state.empty_reason(), EmptyReason::NeedsActiveDocument);

        state.set_scope(GraphScope::Global);
        state.set_query("zzzz-no-such-note".to_string());
        assert_eq!(state.empty_reason(), EmptyReason::NoSearchMatch);

        state.set_query(String::new());
        assert_eq!(state.empty_reason(), EmptyReason::FiltersHideEverything);
    }

    #[test]
    fn resetting_filters_clears_the_query_too() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        state.set_show_pdfs(false);
        state.set_query("a".to_string());
        assert!(state.filters_active());

        state.reset_filters();

        assert!(!state.filters_active());
        assert!(state.query.is_empty());
        assert_eq!(state.query_match_count(), 0);
    }

    #[test]
    fn pausing_and_resuming_the_layout_round_trips() {
        let mut state = GraphState::new();
        state.set_snapshot(sample());
        assert!(state.physics.running);

        state.toggle_physics();
        assert!(!state.physics.running);
        assert!(!state.physics_tick(), "a paused layout must not integrate");

        state.toggle_physics();
        assert!(state.physics.running);
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
