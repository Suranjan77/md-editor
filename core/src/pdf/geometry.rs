//! Page space → view space: the transform PDFium renders with, so text, search and link rects line up with the page bitmap whatever the page's rotation or crop box.

use super::PdfRect;
use pdfium_render::prelude::{PdfPage, PdfPoints, PdfRenderConfig};

/// Sub-point precision for sampling PDFium's page-to-device transform, which
/// only reports whole device pixels.
const SAMPLE_SCALE: f32 = 64.0;

/// Affine map from a page's user space (PDF points, y up, origin wherever the
/// MediaBox puts it) to the *displayed* page box: the box `page.width()` ×
/// `page.height()` describes, after `/Rotate` and the crop box are applied.
///
/// Every [`PdfRect`] this crate hands out is in that displayed box, y up from
/// its bottom-left corner. Char boxes, search hits and link rects used to be
/// raw user space, which only coincides with the displayed box on an
/// unrotated page whose crop box starts at the origin. Anywhere else,
/// highlights, search matches and the selection were drawn beside the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageToView {
    // top_left_x = a·x + c·y + e,  top_left_y = b·x + d·y + f
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
    height: f32,
}

impl PageToView {
    /// The plain y-flip: correct for an unrotated page cropped at the origin.
    pub(crate) fn flip(height: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 0.0,
            f: height,
            height,
        }
    }

    /// Sample PDFium's own transform for `page`, with the same render config
    /// shape the worker rasterizes with (target width and height from the
    /// page's displayed size), so the result agrees with the bitmap.
    pub(crate) fn for_page(page: &PdfPage) -> Self {
        let width = page.width().value;
        let height = page.height().value;
        let config = PdfRenderConfig::new()
            .set_target_width((width * SAMPLE_SCALE).round().max(1.0) as i32)
            .set_target_height((height * SAMPLE_SCALE).round().max(1.0) as i32);
        let sample = |x: f32, y: f32| {
            page.points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &config)
                .ok()
                .map(|(px, py)| (px as f32 / SAMPLE_SCALE, py as f32 / SAMPLE_SCALE))
        };
        // Long baselines keep the whole-pixel rounding negligible.
        const SPAN: f32 = 1000.0;
        match (sample(0.0, 0.0), sample(SPAN, 0.0), sample(0.0, SPAN)) {
            (Some(origin), Some(along_x), Some(along_y)) => Self {
                a: (along_x.0 - origin.0) / SPAN,
                b: (along_x.1 - origin.1) / SPAN,
                c: (along_y.0 - origin.0) / SPAN,
                d: (along_y.1 - origin.1) / SPAN,
                e: origin.0,
                f: origin.1,
                height,
            },
            _ => Self::flip(height),
        }
    }

    /// Map a user-space point to the displayed box, y up from bottom-left.
    pub(crate) fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let top_left_x = self.a * x + self.c * y + self.e;
        let top_left_y = self.b * x + self.d * y + self.f;
        (top_left_x, self.height - top_left_y)
    }

    /// Map a user-space rectangle (`left`, `bottom`, `right`, `top`) to the
    /// displayed box. A quarter turn swaps the axes, so the result is the box
    /// around all four mapped corners.
    pub(crate) fn rect(&self, left: f32, bottom: f32, right: f32, top: f32) -> PdfRect {
        let corners = [
            self.point(left, bottom),
            self.point(right, bottom),
            self.point(left, top),
            self.point(right, top),
        ];
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for (x, y) in corners {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        PdfRect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }
}
