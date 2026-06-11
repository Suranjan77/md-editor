//! Continuous-scroll geometry (plan §3.3 "continuous scroll with
//! virtualization"): pure math from page sizes to pixel layout — cumulative
//! page offsets, visibility queries, and the exact set of tiles (with their
//! display rectangles) that cover a viewport. The shell paints what this
//! module returns and nothing else; no pdfium, no toolkit.
//!
//! Coordinate model: one vertical strip of pages, centered horizontally,
//! separated by `gap` pixels, in *display* pixels at the current zoom
//! (1.0 = 72 dpi points). Tiles are addressed in *bucket* space (the
//! zoom-bucket render scale) and mapped back to display space, so the
//! display magnification of any tile stays ≤ 1.4× by construction.

use crate::tile::{TILE_PX, TileKey, zoom_bucket, zoom_bucket_scale};

/// A tile to draw and where to draw it, in viewport coordinates (already
/// shifted by the scroll offset; `(0, 0)` is the viewport's top-left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedTile {
    pub key: TileKey,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A page's slot in the strip, in viewport coordinates — the white sheet
/// the shell paints behind the tiles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedPage {
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Layout of one document at one zoom level. Rebuilt on zoom change (cheap:
/// one pass over page sizes); scroll queries are O(log n).
#[derive(Debug, Clone)]
pub struct DocLayout {
    /// Page sizes in PDF points (zoom-independent).
    pages: Vec<(f32, f32)>,
    zoom: f32,
    gap: f32,
    /// Top of each page in display px; one extra entry = total height.
    tops: Vec<f32>,
    max_width: f32,
}

impl DocLayout {
    pub fn new(pages: Vec<(f32, f32)>, zoom: f32, gap: f32) -> DocLayout {
        let mut layout = DocLayout {
            pages,
            zoom: 1.0,
            gap,
            tops: Vec::new(),
            max_width: 0.0,
        };
        layout.set_zoom(zoom);
        layout
    }

    /// Change zoom and rebuild offsets. The caller re-anchors scroll (e.g.
    /// via [`Self::page_top`] of the page it wants kept in view).
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.05, 64.0);
        self.tops.clear();
        self.tops.reserve(self.pages.len() + 1);
        let mut y = 0.0_f32;
        let mut max_w = 0.0_f32;
        for &(w, h) in &self.pages {
            self.tops.push(y);
            y += h * self.zoom + self.gap;
            max_w = max_w.max(w * self.zoom);
        }
        // Total height: drop the trailing gap.
        self.tops.push((y - self.gap).max(0.0));
        self.max_width = max_w;
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn total_height(&self) -> f32 {
        match self.tops.last() {
            Some(&total) => total,
            None => 0.0,
        }
    }

    pub fn max_width(&self) -> f32 {
        self.max_width
    }

    /// Top of `page` in document px (clamped to the last page).
    pub fn page_top(&self, page: usize) -> f32 {
        let page = page.min(self.pages.len().saturating_sub(1));
        self.tops.get(page).copied().unwrap_or(0.0)
    }

    /// Display size of `page` in px at the current zoom.
    pub fn page_size_px(&self, page: usize) -> (f32, f32) {
        match self.pages.get(page) {
            Some(&(w, h)) => (w * self.zoom, h * self.zoom),
            None => (0.0, 0.0),
        }
    }

    /// The page whose slot (sheet + following gap) contains document-`y` —
    /// what the "p. N/M" pill shows. Clamped at both ends.
    pub fn page_at(&self, y: f32) -> usize {
        if self.pages.is_empty() {
            return 0;
        }
        // partition_point over tops[1..=n]: first page whose *next* top is
        // above y.
        let n = self.pages.len();
        self.tops[1..=n].partition_point(|&top| top <= y).min(n - 1)
    }

    /// Pages intersecting `[scroll, scroll + viewport_h)`, as an inclusive
    /// index range. Empty documents yield an empty range.
    pub fn visible_pages(&self, scroll: f32, viewport_h: f32) -> std::ops::Range<usize> {
        if self.pages.is_empty() || viewport_h <= 0.0 {
            return 0..0;
        }
        let first = self.page_at(scroll.max(0.0));
        let last = self.page_at((scroll + viewport_h).max(0.0));
        first..(last + 1)
    }

    /// The page sheets intersecting the viewport, in viewport coordinates.
    pub fn placed_pages(&self, scroll: f32, viewport: (f32, f32)) -> Vec<PlacedPage> {
        let (viewport_w, viewport_h) = viewport;
        let mut out = Vec::new();
        for page in self.visible_pages(scroll, viewport_h) {
            let (w, h) = self.page_size_px(page);
            out.push(PlacedPage {
                page: page as u32,
                x: page_x(viewport_w, w),
                y: self.page_top(page) - scroll,
                width: w,
                height: h,
            });
        }
        out
    }

    /// Every tile needed to cover the viewport at the current zoom, with
    /// display rectangles. Keys are in the zoom's bucket, so a tile rendered
    /// for a key here is magnified ≤ 1.4× when painted at the returned size.
    pub fn visible_tiles(&self, scroll: f32, viewport: (f32, f32)) -> Vec<PlacedTile> {
        let (viewport_w, viewport_h) = viewport;
        let bucket = zoom_bucket(self.zoom);
        let bucket_scale = zoom_bucket_scale(bucket);
        // Display px per bucket px.
        let display_per_bucket = self.zoom / bucket_scale;
        let tile_display = TILE_PX as f32 * display_per_bucket;

        let mut out = Vec::new();
        for page in self.visible_pages(scroll, viewport_h) {
            let (page_w, page_h) = self.page_size_px(page);
            let top = self.page_top(page) - scroll; // viewport y of page top
            let left = page_x(viewport_w, page_w);

            // Visible slice of this page, in page-local display px.
            let y0 = (-top).max(0.0);
            let y1 = (viewport_h - top).min(page_h);
            if y1 <= y0 {
                continue;
            }
            // Page extent in bucket px (the renderer's grid).
            let (pts_w, pts_h) = self.pages[page];
            let bucket_w = (pts_w * bucket_scale).round().max(1.0);
            let bucket_h = (pts_h * bucket_scale).round().max(1.0);
            let cols = (bucket_w / TILE_PX as f32).ceil() as u16;
            let rows = (bucket_h / TILE_PX as f32).ceil() as u16;

            let row0 = (y0 / tile_display) as u16;
            let row1 = (((y1 / tile_display).ceil() as u16).max(row0 + 1)).min(rows);
            // Horizontal: the whole page width is in view whenever the page
            // is narrower than the viewport; otherwise clip like vertical.
            let x0 = (-left).max(0.0);
            let x1 = (viewport_w - left).min(page_w);
            let col0 = (x0 / tile_display) as u16;
            let col1 = (((x1 / tile_display).ceil() as u16).max(col0 + 1)).min(cols);

            for row in row0..row1 {
                for col in col0..col1 {
                    // Edge tiles are smaller than TILE_PX in bucket space.
                    let w_bucket = (TILE_PX as f32).min(bucket_w - col as f32 * TILE_PX as f32);
                    let h_bucket = (TILE_PX as f32).min(bucket_h - row as f32 * TILE_PX as f32);
                    out.push(PlacedTile {
                        key: TileKey {
                            page: page as u32,
                            bucket,
                            col,
                            row,
                        },
                        x: left + col as f32 * tile_display,
                        y: top + row as f32 * tile_display,
                        width: w_bucket * display_per_bucket,
                        height: h_bucket * display_per_bucket,
                    });
                }
            }
        }
        out
    }

    /// Largest useful scroll offset for a viewport (top of the over-scroll
    /// dead zone). Zero when the document fits.
    pub fn max_scroll(&self, viewport_h: f32) -> f32 {
        (self.total_height() - viewport_h).max(0.0)
    }
}

/// Horizontal placement: centered, but never pushed off the left edge.
fn page_x(viewport_w: f32, page_w: f32) -> f32 {
    ((viewport_w - page_w) / 2.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three A4-ish pages plus a wide landscape one.
    fn layout(zoom: f32) -> DocLayout {
        DocLayout::new(
            vec![
                (595.0, 842.0),
                (595.0, 842.0),
                (841.0, 595.0),
                (595.0, 842.0),
            ],
            zoom,
            16.0,
        )
    }

    #[test]
    fn offsets_accumulate_pages_and_gaps() {
        let l = layout(1.0);
        assert_eq!(l.page_top(0), 0.0);
        assert_eq!(l.page_top(1), 842.0 + 16.0);
        assert_eq!(l.page_top(2), 2.0 * (842.0 + 16.0));
        assert_eq!(
            l.total_height(),
            3.0 * 842.0 + 595.0 + 3.0 * 16.0,
            "total drops the trailing gap"
        );
        assert_eq!(l.max_width(), 841.0);
    }

    #[test]
    fn zoom_scales_geometry_linearly() {
        let l1 = layout(1.0);
        let l2 = layout(2.0);
        assert!((l2.total_height() - (2.0 * l1.total_height() - 3.0 * 16.0)).abs() < 0.5);
        assert_eq!(l2.page_size_px(0), (1190.0, 1684.0));
        assert_eq!(
            l2.page_top(1),
            2.0 * 842.0 + 16.0,
            "gap is zoom-independent"
        );
    }

    #[test]
    fn page_at_maps_offsets_to_pages_with_clamping() {
        let l = layout(1.0);
        assert_eq!(l.page_at(-50.0), 0);
        assert_eq!(l.page_at(0.0), 0);
        assert_eq!(l.page_at(841.9), 0);
        assert_eq!(
            l.page_at(842.0 + 8.0),
            0,
            "the gap belongs to the page above"
        );
        assert_eq!(l.page_at(842.0 + 16.0), 1);
        assert_eq!(l.page_at(1.0e9), 3, "clamped to the last page");
    }

    #[test]
    fn visible_pages_covers_partial_overlap() {
        let l = layout(1.0);
        // Viewport straddling the page 0 / page 1 boundary sees both.
        assert_eq!(l.visible_pages(800.0, 600.0), 0..2);
        // Fully inside page 0.
        assert_eq!(l.visible_pages(10.0, 100.0), 0..1);
        // Empty document.
        let empty = DocLayout::new(vec![], 1.0, 16.0);
        assert_eq!(empty.visible_pages(0.0, 600.0), 0..0);
        assert_eq!(empty.total_height(), 0.0);
    }

    #[test]
    fn placed_pages_are_centered_and_scroll_shifted() {
        let l = layout(1.0);
        let placed = l.placed_pages(800.0, (1000.0, 600.0));
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].page, 0);
        assert_eq!(placed[0].y, -800.0);
        assert_eq!(placed[0].x, (1000.0 - 595.0) / 2.0);
        assert_eq!(placed[1].page, 1);
        assert_eq!(placed[1].y, 842.0 + 16.0 - 800.0);
    }

    #[test]
    fn visible_tiles_tile_the_viewport_and_stay_in_bucket() {
        let l = layout(1.0);
        let viewport = (1000.0, 600.0);
        let tiles = l.visible_tiles(0.0, viewport);
        assert!(!tiles.is_empty());
        let bucket = zoom_bucket(1.0);
        for t in &tiles {
            assert_eq!(t.key.bucket, bucket);
            // Every tile intersects the viewport (virtualization: nothing
            // fully offscreen is requested).
            assert!(t.x < viewport.0 && t.x + t.width > 0.0, "{t:?}");
            assert!(t.y < viewport.1 && t.y + t.height > 0.0, "{t:?}");
            // Display magnification respects the 1.4× ceiling.
            let mag = t.width / (TILE_PX as f32).min(595.0 - t.key.col as f32 * TILE_PX as f32);
            assert!(mag <= 1.4 * 1.001, "{t:?} magnified {mag}");
        }
        // The visible part of page 0 is fully covered: sample points.
        let (page_w, _) = l.page_size_px(0);
        let left = (viewport.0 - page_w) / 2.0;
        for &(px, py) in &[(0.0_f32, 0.0_f32), (594.0, 599.0), (300.0, 300.0)] {
            let covered = tiles.iter().any(|t| {
                px + left >= t.x && px + left < t.x + t.width && py >= t.y && py < t.y + t.height
            });
            assert!(covered, "page point ({px}, {py}) uncovered");
        }
    }

    #[test]
    fn scrolled_zoomed_viewport_requests_only_intersecting_tiles() {
        let l = layout(2.5);
        let viewport = (900.0, 700.0);
        let scroll = l.page_top(2) + 37.0;
        let tiles = l.visible_tiles(scroll, viewport);
        assert!(!tiles.is_empty());
        for t in &tiles {
            assert!(
                t.x < viewport.0 && t.x + t.width > 0.0 && t.y < viewport.1 && t.y + t.height > 0.0,
                "offscreen tile requested: {t:?}"
            );
        }
        // All requested tiles are for pages the viewport touches.
        let pages = l.visible_pages(scroll, viewport.1);
        for t in &tiles {
            assert!(pages.contains(&(t.key.page as usize)), "{t:?}");
        }
    }

    #[test]
    fn max_scroll_clamps_to_zero_for_short_documents() {
        let l = layout(1.0);
        assert_eq!(l.max_scroll(1.0e9), 0.0);
        assert!((l.max_scroll(600.0) - (l.total_height() - 600.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn set_zoom_keeps_page_identity_for_re_anchoring() {
        let mut l = layout(1.0);
        let anchor = l.page_at(900.0);
        l.set_zoom(3.0);
        // The caller re-anchors: page_top of the same page at the new zoom.
        let new_scroll = l.page_top(anchor);
        assert_eq!(l.page_at(new_scroll), anchor);
    }
}
