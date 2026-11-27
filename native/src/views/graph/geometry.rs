//! Shared view geometry: the pan/zoom transform, node radii, culling and hit tests.

use super::*;

// ─────────────────────────── shared geometry ────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewTransform {
    pub pan: Vector,
    pub zoom: f32,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            pan: Vector::new(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

pub(crate) fn kind_color(kind: GraphNodeKind) -> Color {
    match kind {
        GraphNodeKind::Markdown => crate::design::tokens().accent,
        GraphNodeKind::Pdf => crate::design::tokens().status_info,
        GraphNodeKind::Sketch => crate::design::tokens().status_warning,
        GraphNodeKind::Missing => crate::design::tokens().fg_tertiary,
    }
}

/// Zoom below which the graph is being read as a whole rather than clicked,
/// so nodes are allowed to shrink toward points.
pub(super) const OVERVIEW_ZOOM: f32 = 0.35;

pub(crate) fn node_screen_radius(node: &GraphNode, zoom: f32) -> f32 {
    (base_radius(node) * radius_scale(zoom)).clamp(0.6, 20.0)
}

pub(super) fn base_radius(node: &GraphNode) -> f32 {
    5.5 + (node_degree(node) + 1.0).ln() * 2.2
}

/// Nodes are mostly screen-space so they stay clickable at working zoom, but
/// below [`OVERVIEW_ZOOM`] they shrink with it.
///
/// Without this the radius had a flat 4.5 px floor, so ten thousand nodes at
/// 2% zoom each painted a 4.5 px disc with a wider halo on top — they overlapped
/// into one solid blob instead of reading as a field of separate points.
pub(super) fn radius_scale(zoom: f32) -> f32 {
    let screen_space = 0.75 + zoom.sqrt() * 0.25;
    screen_space * (zoom / OVERVIEW_ZOOM).clamp(0.12, 1.0)
}

pub(super) fn node_degree(node: &GraphNode) -> f32 {
    (node.incoming + node.outgoing) as f32
}

/// How brightly a node burns, 0..1, from its connectedness — hubs read as
/// bright stars and leaves as faint ones, which is what gives a large graph
/// visible structure instead of uniform fill.
pub(crate) fn node_magnitude(node: &GraphNode) -> f32 {
    ((node_degree(node) + 1.0).ln() / 3.0).clamp(0.0, 1.0)
}

/// The hover card. Sized from its contents and flipped away from whichever
/// edge the cursor is near, so it never gets clipped or covers the node it
/// describes.
pub(super) fn draw_hover_card(
    frame: &mut canvas::Frame<Renderer>,
    node: &GraphNode,
    pinned: bool,
    cursor: Point,
    bounds: Rectangle,
) {
    let kind_label = match node.kind {
        GraphNodeKind::Markdown => "Note",
        GraphNodeKind::Pdf => "PDF",
        GraphNodeKind::Sketch => "Sketch",
        GraphNodeKind::Missing => "Unresolved",
    };
    let detail = format!(
        "{} in · {} out{}",
        node.incoming,
        node.outgoing,
        if pinned { " · pinned" } else { "" }
    );

    let width = 236.0;
    let height = 62.0;
    // Flip rather than clamp: a card pinned against the edge sits on top of the
    // cursor, which hides the very node the user is pointing at.
    let x = if cursor.x + 16.0 + width > bounds.width {
        (cursor.x - 16.0 - width).max(6.0)
    } else {
        cursor.x + 16.0
    };
    let y = if cursor.y + 16.0 + height > bounds.height {
        (cursor.y - 16.0 - height).max(6.0)
    } else {
        cursor.y + 16.0
    };

    let card =
        canvas::Path::rounded_rectangle(Point::new(x, y), Size::new(width, height), 7.0.into());
    frame.fill(
        &card,
        crate::design::fade(crate::design::tokens().bg_raised, 0.97),
    );
    frame.stroke(
        &card,
        canvas::Stroke::default()
            .with_color(crate::design::tokens().border_default)
            .with_width(1.0),
    );
    frame.fill(
        &canvas::Path::circle(Point::new(x + 13.0, y + 15.0), 3.0),
        kind_color(node.kind),
    );
    frame.fill_text(canvas::Text {
        content: truncate_label(&node.label, 30),
        position: Point::new(x + 23.0, y + 8.0),
        color: crate::design::tokens().fg_primary,
        size: iced::Pixels(12.0),
        ..canvas::Text::default()
    });
    frame.fill_text(canvas::Text {
        content: format!("{kind_label} · {detail}"),
        position: Point::new(x + 11.0, y + 26.0),
        color: crate::design::tokens().fg_secondary,
        size: iced::Pixels(9.5),
        max_width: width - 22.0,
        ..canvas::Text::default()
    });
    frame.fill_text(canvas::Text {
        content: node.path.clone(),
        position: Point::new(x + 11.0, y + 42.0),
        color: crate::design::tokens().fg_tertiary,
        size: iced::Pixels(9.0),
        max_width: width - 22.0,
        ..canvas::Text::default()
    });
}

pub(crate) fn world_to_screen(world: Point, bounds: Rectangle, transform: ViewTransform) -> Point {
    Point::new(
        bounds.width / 2.0 + transform.pan.x + world.x * transform.zoom,
        bounds.height / 2.0 + transform.pan.y + world.y * transform.zoom,
    )
}

pub(crate) fn screen_to_world(screen: Point, bounds: Rectangle, transform: ViewTransform) -> Point {
    Point::new(
        (screen.x - bounds.width / 2.0 - transform.pan.x) / transform.zoom,
        (screen.y - bounds.height / 2.0 - transform.pan.y) / transform.zoom,
    )
}

pub(crate) fn zoom_about(
    transform: ViewTransform,
    bounds: Rectangle,
    anchor: Point,
    factor: f32,
    min_zoom: f32,
    max_zoom: f32,
) -> ViewTransform {
    let world = screen_to_world(anchor, bounds, transform);
    let zoom = (transform.zoom * factor).clamp(min_zoom, max_zoom);
    ViewTransform {
        zoom,
        pan: Vector::new(
            anchor.x - bounds.width / 2.0 - world.x * zoom,
            anchor.y - bounds.height / 2.0 - world.y * zoom,
        ),
    }
}

pub(crate) fn fit_transform(points: &[Point], bounds: Rectangle) -> ViewTransform {
    if points.is_empty() {
        return ViewTransform::default();
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) =
        (points[0].x, points[0].x, points[0].y, points[0].y);
    for point in &points[1..] {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    let width = (max_x - min_x).max(80.0);
    let height = (max_y - min_y).max(80.0);
    let zoom = ((bounds.width - 110.0).max(80.0) / width)
        .min((bounds.height - 110.0).max(80.0) / height)
        .clamp(MIN_ZOOM, 1.65);
    let center = Point::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    ViewTransform {
        zoom,
        pan: Vector::new(-center.x * zoom, -center.y * zoom),
    }
}

pub(crate) fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

pub(super) fn circle_may_be_visible(center: Point, radius: f32, bounds: Rectangle) -> bool {
    center.x + radius >= 0.0
        && center.y + radius >= 0.0
        && center.x - radius <= bounds.width
        && center.y - radius <= bounds.height
}
