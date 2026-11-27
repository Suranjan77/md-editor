//! Initial layout: adjacency, connected components, island packing and seeded positions.

use super::*;

pub(super) fn adjacency(snapshot: &GraphSnapshot) -> BTreeMap<String, BTreeSet<String>> {
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

pub(super) fn connected_components(snapshot: &GraphSnapshot) -> Vec<Vec<String>> {
    components_from(&adjacency(snapshot))
}

/// Connected components of an already-built adjacency map, so callers holding
/// the cached one do not pay to rebuild it.
pub(super) fn components_from(adjacency: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
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
pub(super) struct Island {
    pub(super) nodes: BTreeMap<String, GraphPoint>,
    pub(super) min_x: f32,
    pub(super) min_y: f32,
    /// Footprint including padding — reserves scatter space.
    pub(super) width: f32,
    pub(super) height: f32,
    /// Half-diagonal of the bare node span — drives clearance.
    pub(super) extent: f32,
    pub(super) seed: u64,
}

pub(super) fn build_layout(snapshot: &GraphSnapshot) -> GraphLayout {
    let adjacency = adjacency(snapshot);
    let components = connected_components(snapshot);
    if components.is_empty() {
        return GraphLayout::default();
    }

    // Lay each connected component out around its own origin first. Packing
    // the measured rectangles afterwards prevents large components from
    // overlapping smaller islands, which a fixed grid cannot guarantee.
    let islands = components
        .iter()
        .map(|component| layout_island(component, &adjacency))
        .collect();
    let mut positions = scatter_islands(islands);

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

/// One connected component laid out in rings around its best-connected note,
/// one ring per BFS level.
fn layout_island(component: &[String], adjacency: &BTreeMap<String, BTreeSet<String>>) -> Island {
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
    Island {
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
        extent: 0.5 * ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt(),
        seed,
    }
}

/// Place laid-out islands on the canvas; returns every note's position.
fn scatter_islands(islands: Vec<Island>) -> BTreeMap<String, GraphPoint> {
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
        let angle =
            rank as f32 * GOLDEN_ANGLE + (angular - 0.5) * 1.4 / ((rank as f32) + 1.0).sqrt();
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

    positions
}

/// FNV-1a. Stable across runs and platforms, unlike `DefaultHasher`, which
/// matters because the layout must be reproducible for a given vault.
pub(super) fn hash_path(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Two independent values in `0.0..1.0` from one hash.
pub(super) fn unit_pair(seed: u64) -> (f32, f32) {
    let first = (seed >> 11) as u32 as f32 / u32::MAX as f32;
    let second = (seed >> 43) as u32 as f32 / u32::MAX as f32;
    (first.clamp(0.0, 1.0), second.clamp(0.0, 1.0))
}

pub(super) fn point_bounds(points: impl IntoIterator<Item = GraphPoint>) -> (f32, f32, f32, f32) {
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
