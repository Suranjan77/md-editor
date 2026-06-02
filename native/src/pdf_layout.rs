//! Prefix-sum based PDF page layout.
//!
//! The previous PDF viewer iterated every page on every scroll event to find
//! the current page or compute total document height. For documents with
//! hundreds of pages that O(N) work was happening dozens of times per frame.
//!
//! [`PdfLayout`] caches per-page heights and a prefix-sum so:
//!   * `page_offset(page)` is O(1)
//!   * `total_height()` is O(1)
//!   * `page_at_scroll(scroll_y)` is O(log N) via binary search
//!   * `visible_range(...)` is O(log N)
//!
//! The layout is stored in display-space pixels (already multiplied by zoom).
//! The owner is responsible for calling [`PdfLayout::rebuild`] whenever any
//! input changes (PDF opened, page sizes loaded, zoom changed, fit-to-width).

#![allow(dead_code)]

use std::ops::Range;

use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};

#[derive(Debug, Clone)]
pub struct PdfLayout {
    /// Display-space height for each page (already scaled by zoom).
    page_heights: Vec<f32>,
    /// Cumulative offset to the *top* of each page in display-space.
    /// `page_offsets[i]` = top of page `i`, `page_offsets[i + 1]` = bottom of page
    /// `i` plus spacing (i.e. top of page `i + 1`).
    /// Length is `page_heights.len() + 1`.
    page_offsets: Vec<f32>,
    /// Total document height including the trailing padding.
    total_height: f32,
    spacing: f32,
    padding: f32,
}

impl Default for PdfLayout {
    fn default() -> Self {
        Self {
            page_heights: Vec::new(),
            page_offsets: vec![PDF_PAGE_LIST_PADDING],
            total_height: PDF_PAGE_LIST_PADDING,
            spacing: PDF_PAGE_SPACING,
            padding: PDF_PAGE_LIST_PADDING,
        }
    }
}

impl PdfLayout {
    /// Rebuild the layout from per-page sizes.
    ///
    /// `page_sizes`: raw (width, height) per page in PDF user-space units.
    ///   `None` entries fall back to `fallback_size` which should be the
    ///   placeholder page size used by the viewer.
    /// `zoom`: current display zoom factor.
    /// `spacing`: vertical pixels between consecutive pages.
    /// `padding`: top/bottom padding on the page list.
    pub fn rebuild(
        page_sizes: &[Option<(f32, f32)>],
        zoom: f32,
        fallback_size: (f32, f32),
        spacing: f32,
        padding: f32,
        rotation: u16,
    ) -> Self {
        let zoom = zoom.max(0.01);
        let n = page_sizes.len();
        let mut page_heights = Vec::with_capacity(n);
        let mut page_offsets = Vec::with_capacity(n + 1);

        // First entry is the top padding above page 0.
        let mut offset = padding;
        page_offsets.push(offset);

        for size in page_sizes {
            let (w_units, h_units) = size.unwrap_or(fallback_size);
            let h = if rotation == 90 || rotation == 270 {
                w_units * zoom
            } else {
                h_units * zoom
            };
            page_heights.push(h);
            offset += h + spacing;
            page_offsets.push(offset);
        }

        // total_height: padding at top + N pages each adding (h + spacing)
        // results in `offset` = padding + sum(h + spacing). The trailing
        // spacing past the last page is counted, but that matches the
        // pre-existing pdf_total_height() behaviour exactly so scroll math
        // (especially the active search jump clamp) stays identical.
        let total_height = if n == 0 { padding } else { offset };

        Self {
            page_heights,
            page_offsets,
            total_height,
            spacing,
            padding,
        }
    }

    pub fn page_count(&self) -> usize {
        self.page_heights.len()
    }

    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    pub fn padding(&self) -> f32 {
        self.padding
    }

    /// Display-space offset to the top of `page`.
    /// If `page` is past the last page, returns the total height (cursor at end).
    pub fn page_offset(&self, page: u16) -> f32 {
        let idx = (page as usize).min(self.page_heights.len());
        self.page_offsets[idx]
    }

    /// Display-space height of the given page (excluding spacing).
    /// Returns 0.0 for an out-of-range page.
    pub fn page_height(&self, page: u16) -> f32 {
        self.page_heights.get(page as usize).copied().unwrap_or(0.0)
    }

    pub fn total_height(&self) -> f32 {
        self.total_height
    }

    /// Find the page that contains the given absolute scroll Y.
    /// Returns `0` for empty layouts and the last page for scroll positions
    /// past the end.
    pub fn page_at_scroll(&self, scroll_y: f32) -> u16 {
        self.page_at_scroll_with_probes(scroll_y).0
    }

    fn page_at_scroll_with_probes(&self, scroll_y: f32) -> (u16, usize) {
        let n = self.page_heights.len();
        if n == 0 {
            return (0, 0);
        }

        // page_offsets[i + 1] is the boundary just past page i (start of page
        // i+1). Find the smallest i such that scroll_y < page_offsets[i + 1].
        // This hand-rolled binary search keeps operation counts testable
        // without relying on wall-clock timings.
        let boundaries = &self.page_offsets[1..];
        let mut lo = 0;
        let mut hi = boundaries.len();
        let mut probes = 0;
        while lo < hi {
            probes += 1;
            let mid = lo + (hi - lo) / 2;
            if boundaries[mid] <= scroll_y {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let idx = lo;
        let clamped = idx.min(n - 1);
        (clamped as u16, probes)
    }

    /// Compute a half-open page range that overlaps the viewport, expanded by
    /// `buffer_pages` on each side. The returned range is clamped to
    /// `0..page_count`.
    pub fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        buffer_pages: u16,
    ) -> Range<u16> {
        let n = self.page_heights.len();
        if n == 0 {
            return 0..0;
        }

        let first = self.page_at_scroll(scroll_y);
        let viewport_bottom = scroll_y + viewport_height.max(0.0);
        // Find the page containing the bottom of the viewport. If the bottom
        // is at or past the document end, this naturally clamps to the last
        // page.
        let last_inclusive = self.page_at_scroll(viewport_bottom);

        let buffer = buffer_pages as usize;
        let start = (first as usize).saturating_sub(buffer);
        let end_inclusive = (last_inclusive as usize).saturating_add(buffer).min(n - 1);
        let end_exclusive = end_inclusive + 1;

        (start as u16)..(end_exclusive as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_uniform(n: usize, page_h: f32, zoom: f32) -> PdfLayout {
        let sizes: Vec<Option<(f32, f32)>> = (0..n).map(|_| Some((100.0, page_h))).collect();
        PdfLayout::rebuild(
            &sizes,
            zoom,
            (100.0, page_h),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            0,
        )
    }

    #[test]
    fn empty_layout_has_only_padding() {
        let layout = PdfLayout::rebuild(&[], 1.0, (612.0, 792.0), 20.0, 20.0, 0);
        assert_eq!(layout.page_count(), 0);
        assert_eq!(layout.total_height(), 20.0);
        assert_eq!(layout.page_at_scroll(0.0), 0);
        assert_eq!(layout.page_at_scroll(99999.0), 0);
        assert!(layout.visible_range(0.0, 1000.0, 1).is_empty());
    }

    #[test]
    fn page_offsets_are_cumulative() {
        let layout = build_uniform(5, 100.0, 1.0);
        let pad = PDF_PAGE_LIST_PADDING;
        let stride = 100.0 + PDF_PAGE_SPACING;
        assert_eq!(layout.page_offset(0), pad);
        assert_eq!(layout.page_offset(1), pad + stride);
        assert_eq!(layout.page_offset(4), pad + 4.0 * stride);
        // Past last page returns total height.
        assert_eq!(layout.page_offset(5), layout.total_height());
        assert_eq!(layout.page_offset(99), layout.total_height());
    }

    #[test]
    fn page_at_scroll_uses_binary_search() {
        let layout = build_uniform(10, 100.0, 1.0);
        let pad = PDF_PAGE_LIST_PADDING;
        let stride = 100.0 + PDF_PAGE_SPACING;
        // Inside page 0
        assert_eq!(layout.page_at_scroll(pad), 0);
        assert_eq!(layout.page_at_scroll(pad + 50.0), 0);
        // Inside the spacing region between page 0 and page 1 belongs to page 0
        // (matches the original `pdf_page_at_scroll` semantics where scroll_y
        // < offset + page_h + spacing returns the current page).
        assert_eq!(layout.page_at_scroll(pad + 100.0), 0);
        // Just into page 1
        assert_eq!(layout.page_at_scroll(pad + stride), 1);
        // Page 5
        assert_eq!(layout.page_at_scroll(pad + 5.0 * stride + 10.0), 5);
        // Past the end clamps to last page.
        assert_eq!(layout.page_at_scroll(99_999.0), 9);
    }

    #[test]
    fn page_at_scroll_matches_linear_scan_for_uniform_pages() {
        let layout = build_uniform(50, 80.0, 1.5);
        for y in (0..5000).step_by(37) {
            let y = y as f32;
            let expected = linear_page_at_scroll(&layout, y);
            assert_eq!(
                layout.page_at_scroll(y),
                expected,
                "mismatch at scroll_y = {y}"
            );
        }
    }

    fn linear_page_at_scroll(layout: &PdfLayout, scroll_y: f32) -> u16 {
        let n = layout.page_count();
        if n == 0 {
            return 0;
        }
        let mut offset = layout.padding();
        for i in 0..n {
            let page_h = layout.page_height(i as u16);
            if scroll_y < offset + page_h + layout.spacing() {
                return i as u16;
            }
            offset += page_h + layout.spacing();
        }
        (n - 1) as u16
    }

    #[test]
    fn visible_range_clamps_and_buffers() {
        let layout = build_uniform(20, 100.0, 1.0);
        let pad = PDF_PAGE_LIST_PADDING;
        let stride = 100.0 + PDF_PAGE_SPACING;
        // Viewport showing pages 5..=8 with buffer of 1 -> 4..10
        let scroll = pad + 5.0 * stride + 10.0;
        let viewport = 4.0 * stride - 30.0;
        let range = layout.visible_range(scroll, viewport, 1);
        assert_eq!(range.start, 4);
        assert_eq!(range.end, 10);

        // At top of doc: lower bound clamps to 0.
        let range = layout.visible_range(0.0, 200.0, 3);
        assert_eq!(range.start, 0);
        assert!(range.end >= 1);

        // At very bottom: upper bound clamps to page_count.
        let range = layout.visible_range(layout.total_height(), 200.0, 3);
        assert_eq!(range.end, 20);
    }

    #[test]
    fn large_pdf_layout_page_lookup_stays_logarithmic() {
        let layout = build_uniform(50_000, 100.0, 1.0);
        let target_y = layout.page_offset(40_000);

        let (page, probes) = layout.page_at_scroll_with_probes(target_y);

        assert_eq!(page, 40_000);
        assert!(
            probes <= 32,
            "page lookup should remain logarithmic, got {probes}"
        );
        let range = layout.visible_range(target_y, 240.0, 3);
        assert_eq!(range, 39_997..40_006);
    }

    #[test]
    fn rebuild_handles_missing_page_sizes_with_fallback() {
        let sizes = vec![None, Some((100.0, 200.0)), None];
        let layout = PdfLayout::rebuild(&sizes, 1.0, (100.0, 50.0), 20.0, 20.0, 0);
        assert_eq!(layout.page_count(), 3);
        assert_eq!(layout.page_height(0), 50.0); // fallback
        assert_eq!(layout.page_height(1), 200.0);
        assert_eq!(layout.page_height(2), 50.0);
    }

    #[test]
    fn page_height_zero_for_out_of_range() {
        let layout = build_uniform(3, 100.0, 1.0);
        assert_eq!(layout.page_height(3), 0.0);
        assert_eq!(layout.page_height(99), 0.0);
    }

    #[test]
    fn rebuild_handles_rotation() {
        let sizes = vec![Some((100.0, 200.0))];
        let layout_normal = PdfLayout::rebuild(&sizes, 1.0, (100.0, 200.0), 20.0, 20.0, 0);
        let layout_rotated = PdfLayout::rebuild(&sizes, 1.0, (100.0, 200.0), 20.0, 20.0, 90);
        assert_eq!(layout_normal.page_height(0), 200.0);
        assert_eq!(layout_rotated.page_height(0), 100.0);
    }
}
