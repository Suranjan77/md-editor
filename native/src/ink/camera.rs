//! Pan and zoom for the ink surface.
//!
//! Strokes are stored in *page* coordinates, which never change. The camera
//! maps between those and the *screen* coordinates the surface is drawn and
//! clicked in, so zooming or panning never rewrites the document.

use super::stroke::Vec2;

/// Multiplier applied per notch of the scroll wheel.
const WHEEL_ZOOM_STEP: f32 = 1.12;

/// Position and scale of the view onto the page.
///
/// `screen = page * zoom + pan`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera {
    /// Below this the strokes are too fine to be useful; above it a page of
    /// handwriting fills the screen many times over.
    pub const MIN_ZOOM: f32 = 0.2;
    pub const MAX_ZOOM: f32 = 8.0;

    pub const fn new() -> Self {
        Self {
            pan: Vec2::new(0.0, 0.0),
            zoom: 1.0,
        }
    }

    pub fn is_identity(&self) -> bool {
        (self.zoom - 1.0).abs() < f32::EPSILON
            && self.pan.x.abs() < f32::EPSILON
            && self.pan.y.abs() < f32::EPSILON
    }

    pub fn to_page(&self, screen: Vec2) -> Vec2 {
        screen.sub(self.pan).scale(1.0 / self.zoom)
    }

    pub fn to_screen(&self, page: Vec2) -> Vec2 {
        page.scale(self.zoom).add(self.pan)
    }

    /// Shift the view by a screen-space delta.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan = self.pan.add(delta);
    }

    /// Scale about a fixed screen point, so whatever is under the cursor stays
    /// under the cursor. Returns whether the zoom actually changed — at the
    /// limits it does not, and the caller can skip invalidating its cache.
    pub fn zoom_about(&mut self, anchor: Vec2, factor: f32) -> bool {
        let target = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        if (target - self.zoom).abs() < f32::EPSILON {
            return false;
        }

        // Solve `anchor = page * target + pan` for the pan that keeps the page
        // point under the anchor fixed.
        let page = self.to_page(anchor);
        self.zoom = target;
        self.pan = anchor.sub(page.scale(target));
        true
    }

    /// Zoom by `notches` of scroll wheel about `anchor`.
    pub fn zoom_by_wheel(&mut self, anchor: Vec2, notches: f32) -> bool {
        if notches.abs() < f32::EPSILON {
            return false;
        }
        self.zoom_about(anchor, WHEEL_ZOOM_STEP.powf(notches))
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// The page-space box currently visible in a surface of `size` screen
    /// pixels, as `(min, max)`.
    pub fn visible_page_box(&self, size: Vec2) -> (Vec2, Vec2) {
        (
            self.to_page(Vec2::new(0.0, 0.0)),
            self.to_page(Vec2::new(size.x, size.y)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vec2, b: Vec2) -> bool {
        (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001
    }

    #[test]
    fn identity_camera_maps_one_to_one() {
        let camera = Camera::new();
        let point = Vec2::new(37.0, 91.0);
        assert!(approx(camera.to_screen(point), point));
        assert!(approx(camera.to_page(point), point));
    }

    #[test]
    fn page_and_screen_are_inverses() {
        let camera = Camera {
            pan: Vec2::new(-120.0, 64.0),
            zoom: 2.5,
        };
        let page = Vec2::new(19.0, -7.5);
        assert!(approx(camera.to_page(camera.to_screen(page)), page));
    }

    #[test]
    fn zoom_keeps_the_anchor_point_fixed() {
        let mut camera = Camera::new();
        let anchor = Vec2::new(400.0, 300.0);
        let page_under_anchor = camera.to_page(anchor);

        assert!(camera.zoom_about(anchor, 2.0));

        assert!(
            approx(camera.to_screen(page_under_anchor), anchor),
            "the point under the cursor must not drift while zooming"
        );
    }

    #[test]
    fn zoom_is_clamped_and_reports_no_change_at_the_limit() {
        let mut camera = Camera::new();
        camera.zoom_about(Vec2::new(0.0, 0.0), 1000.0);
        assert!((camera.zoom - Camera::MAX_ZOOM).abs() < 0.001);
        assert!(
            !camera.zoom_about(Vec2::new(0.0, 0.0), 2.0),
            "already at the ceiling, so nothing changed"
        );

        camera.zoom_about(Vec2::new(0.0, 0.0), 0.0001);
        assert!((camera.zoom - Camera::MIN_ZOOM).abs() < 0.001);
        assert!(!camera.zoom_about(Vec2::new(0.0, 0.0), 0.5));
    }

    #[test]
    fn wheel_notches_zoom_in_and_out_symmetrically() {
        let mut camera = Camera::new();
        let anchor = Vec2::new(100.0, 100.0);

        camera.zoom_by_wheel(anchor, 3.0);
        let zoomed_in = camera.zoom;
        assert!(zoomed_in > 1.0);

        camera.zoom_by_wheel(anchor, -3.0);
        assert!(
            (camera.zoom - 1.0).abs() < 0.001,
            "equal and opposite scrolling should return to the start, got {}",
            camera.zoom
        );
    }

    #[test]
    fn panning_shifts_the_view_in_screen_space() {
        let mut camera = Camera::new();
        camera.pan_by(Vec2::new(25.0, -10.0));
        assert!(approx(camera.to_screen(Vec2::new(0.0, 0.0)), Vec2::new(25.0, -10.0)));
    }

    #[test]
    fn visible_box_shrinks_as_you_zoom_in() {
        let size = Vec2::new(800.0, 600.0);

        let wide = Camera::new().visible_page_box(size);
        let mut close = Camera::new();
        close.zoom_about(Vec2::new(400.0, 300.0), 4.0);
        let narrow = close.visible_page_box(size);

        assert!(
            (narrow.1.x - narrow.0.x) < (wide.1.x - wide.0.x),
            "zooming in should show less of the page"
        );
    }

    #[test]
    fn reset_returns_to_the_identity_view() {
        let mut camera = Camera::new();
        camera.pan_by(Vec2::new(300.0, -80.0));
        camera.zoom_about(Vec2::new(10.0, 10.0), 3.0);
        assert!(!camera.is_identity());

        camera.reset();
        assert!(camera.is_identity());
    }
}
