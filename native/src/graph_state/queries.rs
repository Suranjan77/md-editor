//! Filtering and read-only queries over the knowledge graph: scope, filters and search, stats, backlinks, related notes, hubs, orphans and missing targets.

use super::*;

impl GraphState {
    /// Reduce the vault graph to what the current scope, filters, and query
    /// admit.
    ///
    /// Runs entirely on an index-keyed `Vec<bool>` mask. The original
    /// path-keyed `BTreeSet` version did an O(n) linear node lookup per node
    /// and re-lowercased every label on every keystroke, which cost ~145 ms on
    /// a 10k-note vault — enough to make typing in the search box unusable.
    pub(super) fn compute_filtered(&self) -> GraphSnapshot {
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
                && (self.show_sketches || node.kind != GraphNodeKind::Sketch)
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

    pub(super) fn compute_stats(&self) -> GraphStats {
        let mut stats = GraphStats::default();
        for node in &self.snapshot.nodes {
            match node.kind {
                GraphNodeKind::Markdown => stats.markdown += 1,
                GraphNodeKind::Pdf => stats.pdfs += 1,
                GraphNodeKind::Sketch => stats.sketches += 1,
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

    /// Incoming connections to `path` with their edge role.
    pub fn incoming_edges(&self, path: &str) -> Vec<(GraphNode, GraphEdgeKind)> {
        self.snapshot
            .edges
            .iter()
            .filter(|edge| edge.target == path)
            .filter_map(|edge| {
                self.node(&edge.source)
                    .map(|node| (node.clone(), edge.kind))
            })
            .collect()
    }

    /// Outgoing connections from `path` with their edge role.
    pub fn outgoing_edges(&self, path: &str) -> Vec<(GraphNode, GraphEdgeKind)> {
        self.snapshot
            .edges
            .iter()
            .filter(|edge| edge.source == path)
            .filter_map(|edge| {
                self.node(&edge.target)
                    .map(|node| (node.clone(), edge.kind))
            })
            .collect()
    }

    /// Step keyboard selection forward or backward across the currently visible nodes.
    pub fn cycle_node(&mut self, delta: isize) {
        let nodes = &self.filtered.snapshot.nodes;
        if nodes.is_empty() {
            self.selected_path = None;
            return;
        }
        let next_idx = match self
            .selected_path
            .as_deref()
            .and_then(|sel| nodes.iter().position(|n| n.path == sel))
        {
            None => {
                if delta >= 0 {
                    0
                } else {
                    nodes.len() - 1
                }
            }
            Some(idx) => (idx as isize + delta).rem_euclid(nodes.len() as isize) as usize,
        };
        let path = nodes[next_idx].path.clone();
        self.focus_on(&path);
    }

    /// Capture the current pins, filter chips, scope, and camera as a named pinboard.
    pub fn capture_pinboard(&self, name: &str) -> super::pinboards::Pinboard {
        let mut pinned_positions = BTreeMap::new();
        for path in &self.pinned {
            if let Some(pt) = self.layout.get(path)
                && pt.x.is_finite()
                && pt.y.is_finite()
            {
                pinned_positions.insert(
                    path.clone(),
                    super::pinboards::PinPosition { x: pt.x, y: pt.y },
                );
            }
        }
        super::pinboards::Pinboard {
            name: name.trim().to_string(),
            pinned_positions,
            scope: self.scope,
            show_pdfs: self.show_pdfs,
            show_sketches: self.show_sketches,
            show_missing: self.show_missing,
            show_orphans: self.show_orphans,
            pan: self.pan,
            zoom: self.zoom,
        }
    }

    /// Restore a saved pinboard: pins existing nodes at saved coords, restores filters/scope/camera.
    pub fn apply_pinboard(&mut self, board: &super::pinboards::Pinboard) {
        self.scope = board.scope.normalized();
        self.show_pdfs = board.show_pdfs;
        self.show_sketches = board.show_sketches;
        self.show_missing = board.show_missing;
        self.show_orphans = board.show_orphans;
        self.pinned.clear();
        for (path, pos) in &board.pinned_positions {
            if pos.x.is_finite() && pos.y.is_finite() && self.layout.positions.contains_key(path) {
                self.layout
                    .positions
                    .insert(path.clone(), GraphPoint { x: pos.x, y: pos.y });
                self.pinned.insert(path.clone());
            }
        }
        self.rebuild_filtered();
        self.publish_view(board.pan, board.zoom);
        self.physics.nudge();
    }
}
