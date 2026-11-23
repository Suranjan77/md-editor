//! The force simulation: a Barnes–Hut quadtree for repulsion and the per-tick physics state.

use super::*;

/// Barnes-Hut quadtree used to approximate all-pairs repulsion.
///
/// Every node carries unit mass. A subtree whose extent is small relative to
/// its distance from the body being evaluated is collapsed into a single point
/// mass at its center of gravity, which turns the O(n²) repulsion pass into
/// O(n log n) at a bounded, tunable error.
#[derive(Debug, Clone, Default)]
pub(super) struct Quadtree {
    pub(super) cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Cell {
    /// Indices of the four child cells, or [`Quadtree::EMPTY`].
    pub(super) children: [u32; 4],
    /// Center of mass of everything under this cell.
    pub(super) com: [f32; 2],
    pub(super) mass: f32,
    /// Geometric center and half-extent of the square this cell covers.
    pub(super) center: [f32; 2],
    pub(super) half: f32,
    /// Body index when this cell holds exactly one, else [`Quadtree::EMPTY`].
    pub(super) body: u32,
}

impl Quadtree {
    pub(super) const EMPTY: u32 = u32::MAX;
    /// Opening angle. Smaller is more accurate and slower; 0.0 degenerates to
    /// exact all-pairs, which the tests use as the reference.
    pub(super) const THETA: f32 = 0.75;
    /// Coincident or near-coincident points would otherwise subdivide forever.
    pub(super) const MAX_DEPTH: u32 = 24;

    pub(super) fn cell(center: [f32; 2], half: f32) -> Cell {
        Cell {
            children: [Self::EMPTY; 4],
            com: [0.0, 0.0],
            mass: 0.0,
            center,
            half,
            body: Self::EMPTY,
        }
    }

    pub(super) fn build(&mut self, positions: &[[f32; 2]]) {
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

    pub(super) fn quadrant(cell: &Cell, p: [f32; 2]) -> usize {
        usize::from(p[0] >= cell.center[0]) | (usize::from(p[1] >= cell.center[1]) << 1)
    }

    pub(super) fn child_cell(parent: &Cell, quadrant: usize) -> Cell {
        let half = parent.half * 0.5;
        let dx = if quadrant & 1 == 0 { -half } else { half };
        let dy = if quadrant & 2 == 0 { -half } else { half };
        Self::cell([parent.center[0] + dx, parent.center[1] + dy], half)
    }

    pub(super) fn insert(&mut self, body: u32, position: [f32; 2]) {
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
    pub(super) fn push_down(&mut self, parent: usize, body: u32, position: [f32; 2], depth: u32) {
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
                let resident_position = [com[0] * mass - position[0], com[1] * mass - position[1]];
                self.push_down(child, resident, resident_position, depth);
            }
            current = child;
        }
    }

    pub(super) fn ensure_child(&mut self, parent: usize, quadrant: usize) -> usize {
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
    pub(super) fn repulsion_on(&self, body: usize, position: [f32; 2], repulsion: f32) -> [f32; 2] {
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
    pub(super) index: Vec<String>,
    /// Edge endpoint pairs expressed as indices into `index`.
    pub(super) edge_pairs: Vec<[usize; 2]>,
    // ── Per-node state (parallel to `index`) ────────────────────
    pub(super) vel: Vec<[f32; 2]>,
    /// Nodes with no edges at all. A force layout has nothing to say about
    /// where an unlinked note belongs, so simulating it only lets repulsion
    /// and center gravity compress thousands of them into one uniform disc.
    /// They keep the deterministic scatter `build_layout` gave them, which is
    /// what makes a link-poor vault read as a starfield with the connected
    /// clusters as constellations inside it.
    pub(super) anchored: Vec<bool>,
    // ── Reusable computation buffers ────────────────────────────
    pub(super) forces: Vec<[f32; 2]>,
    pub(super) pos: Vec<[f32; 2]>,
    /// Rebuilt every tick; kept here so its arena allocation is reused.
    pub(super) tree: Quadtree,
    // ── Tuning parameters ───────────────────────────────────────
    pub(super) repulsion: f32,
    pub(super) attraction: f32,
    pub(super) damping: f32,
    pub(super) center_gravity: f32,
    pub(super) rest_length: f32,
    /// Mean kinetic energy per node below which the layout is considered
    /// settled and the simulation parks itself.
    pub(super) stability_threshold: f32,
    /// Ticks left in this settling run. A force layout on a link-poor vault can
    /// jitter below its own stability threshold indefinitely, so the budget
    /// guarantees the simulation always parks and stops burning a core. The
    /// user can always resume it or relayout.
    pub(super) budget: u32,
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
    pub(super) const SETTLE_BUDGET: u32 = 600;
    /// Ticks granted when something disturbs a settled layout, e.g. a drag.
    pub(super) const NUDGE_BUDGET: u32 = 150;

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
