//! Tests for `graph_state`.

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
fn brute_force_repulsion(positions: &[[f32; 2]], body: usize, repulsion: f32) -> [f32; 2] {
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
    assert!(
        force[1].abs() < force[0],
        "force should be mostly horizontal"
    );
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
    let positions: Vec<GraphPoint> = state.graph_layout().positions.values().copied().collect();
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
