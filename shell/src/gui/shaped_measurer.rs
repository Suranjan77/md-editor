use std::sync::{Arc, Mutex, RwLock};

use crate::gui::editor_canvas::VisualMetrics;
use cosmic_text::{
    Attrs, Buffer, Cursor, Family, FontSystem, LayoutRun, Metrics, Shaping, Style, Weight,
};
use md3_editor::layout::{LineMeasure, Measurer, StyledLine};
use md3_editor::style::SpanKind;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone)]
pub struct ShapedMeasurer {
    font_system: Arc<Mutex<FontSystem>>,
    metrics: Arc<RwLock<VisualMetrics>>,
    default_font_size: f32,
    default_line_height: f32,
}

impl ShapedMeasurer {
    pub fn new(font_system: Arc<Mutex<FontSystem>>) -> Self {
        Self {
            font_system,
            metrics: Arc::new(RwLock::new(VisualMetrics::default())),
            default_font_size: 17.0,
            default_line_height: 27.0,
        }
    }

    pub fn line_metrics(&self, line: &StyledLine) -> (Metrics, f32, f32) {
        use md3_editor::parse::LineKind;
        let base_size = self.default_font_size;
        let base_lh = self.default_line_height; // 24.0

        // Returns (Metrics, padding_top, padding_bottom)
        match &line.kind {
            LineKind::Heading { level } => match level {
                1 => (Metrics::new(base_size * 2.0, base_lh * 2.0), 24.0, 16.0),
                2 => (Metrics::new(base_size * 1.5, base_lh * 1.5), 20.0, 14.0),
                3 => (Metrics::new(base_size * 1.25, base_lh * 1.25), 16.0, 12.0),
                4 => (Metrics::new(base_size * 1.1, base_lh * 1.1), 12.0, 10.0),
                5 => (Metrics::new(base_size, base_lh), 8.0, 8.0),
                _ => (Metrics::new(base_size, base_lh), 8.0, 8.0),
            },
            LineKind::Blank => (Metrics::new(base_size, base_lh), 0.0, 0.0),
            LineKind::Rule => (Metrics::new(base_size, base_lh), 16.0, 16.0),
            LineKind::Quote => (Metrics::new(base_size, base_lh), 5.0, 7.0),
            LineKind::Bullet { .. } | LineKind::Ordered => {
                (Metrics::new(base_size, base_lh), 2.0, 2.0)
            }
            LineKind::CodeContent => (Metrics::new(base_size * 0.9, base_lh * 0.9), 0.0, 0.0),
            _ => (Metrics::new(base_size, base_lh), 5.0, 10.0), // paragraph spacing
        }
    }

    pub fn create_buffer(&self, line: &StyledLine, wrap_width: f32) -> Buffer {
        // List markers live in a left gutter; the item text is inset by the
        // same amount it is painted (`line_plan`). Folding the indent into the
        // wrap width here means measure, paint, caret and hit-test all wrap at
        // the identical column — a wrapped item can't paint past its measured
        // height (the "checkbox line drawn over the line below" bug).
        let wrap_width = (wrap_width - line_indent(line)).max(1.0);
        let mut fs = self.font_system.lock().unwrap_or_else(|e| e.into_inner());
        let (metrics, _, _) = self.line_metrics(line);
        let default_attrs = Attrs::new().family(Family::SansSerif);
        // Headings paint their prose in bold (`span_style` → `SansBold`); shape
        // it bold here too, or the measured glyph advances (and the end-of-line
        // caret) fall short of the wider painted glyphs — visible as the caret
        // sitting before the last heading character.
        let heading_text = matches!(line.kind, md3_editor::parse::LineKind::Heading { .. });
        let chars = line.display.chars().collect::<Vec<_>>();
        let mut rich = line
            .spans
            .iter()
            .map(|span| {
                let text = chars[span.range.clone()].iter().collect::<String>();
                let attrs = match span.kind {
                    SpanKind::Bold => default_attrs.clone().weight(Weight::BOLD),
                    SpanKind::Italic | SpanKind::QuoteText | SpanKind::Image { .. } => {
                        default_attrs.clone().style(Style::Italic)
                    }
                    // CodeToken shapes identically to CodeContent — same mono
                    // family, no weight/style change. Syntax color is applied
                    // only in paint, so highlighting never moves a glyph.
                    SpanKind::Code
                    | SpanKind::CodeContent
                    | SpanKind::CodeToken(_)
                    | SpanKind::Math
                    | SpanKind::MathContent
                    | SpanKind::Marker
                    | SpanKind::FrontMatter => default_attrs.clone().family(Family::Monospace),
                    SpanKind::Text if heading_text => default_attrs.clone().weight(Weight::BOLD),
                    _ => default_attrs.clone(),
                };
                (text, attrs)
            })
            .collect::<Vec<_>>();
        let mut buffer = build_buffer(
            &mut fs,
            metrics,
            &line.display,
            &rich,
            &default_attrs,
            wrap_width,
        );

        if !matches!(line.conceal, md3_editor::layout::ConcealMode::Revealed) {
            let spacings = self.inline_math_spacings(line, &buffer, wrap_width);
            let mut changed = false;
            for ((_, attrs), spacing) in rich.iter_mut().zip(spacings) {
                if let Some(spacing) = spacing {
                    *attrs = attrs.clone().letter_spacing(spacing);
                    changed = true;
                }
            }
            if changed {
                buffer = build_buffer(
                    &mut fs,
                    metrics,
                    &line.display,
                    &rich,
                    &default_attrs,
                    wrap_width,
                );
            }
        }

        buffer
    }

    pub fn caret_rect(
        &self,
        line: &StyledLine,
        wrap_width: f32,
        char_index: usize,
    ) -> (f32, f32, f32) {
        let buffer = self.create_buffer(line, wrap_width);
        let (_, pad_top, _) = self.line_metrics(line);
        let indent = line_indent(line);
        let byte_index = char_to_byte(&line.display, char_index);
        let mut last = (indent, pad_top, self.default_line_height);
        for run in buffer.layout_runs() {
            last = (indent + run.line_w, pad_top + run.line_top, run.line_height);
            if let Some(x) = run_caret_x(&run, byte_index) {
                return (indent + x, pad_top + run.line_top, run.line_height);
            }
        }
        (last.0, last.1, last.2)
    }

    pub fn selection_rects(
        &self,
        line: &StyledLine,
        wrap_width: f32,
        start: usize,
        end: usize,
    ) -> Vec<(f32, f32, f32, f32)> {
        let buffer = self.create_buffer(line, wrap_width);
        let (_, pad_top, _) = self.line_metrics(line);
        let indent = line_indent(line);
        let start = Cursor::new(0, char_to_byte(&line.display, start));
        let end = Cursor::new(0, char_to_byte(&line.display, end));
        buffer
            .layout_runs()
            .filter_map(|run| {
                run.highlight(start, end)
                    .map(|(x, width)| (indent + x, pad_top + run.line_top, width, run.line_height))
            })
            .collect()
    }

    pub fn set_image_size(&self, key: String, width: f32, height: f32) {
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.images.insert(key, (width, height));
        }
    }

    pub fn set_math_size(&self, key: String, width: f32, height: f32) {
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.math.insert(key, (width, height));
        }
    }

    pub fn set_math_block_size(&self, first_line: String, width: f32, height: f32) {
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.math_blocks.insert(first_line, (width, height));
        }
    }
}

impl Measurer for ShapedMeasurer {
    fn measure(&self, line: &StyledLine, wrap_width: f64) -> LineMeasure {
        let (metrics, pad_top, pad_bot) = self.line_metrics(line);
        let buffer = self.create_buffer(line, wrap_width as f32);

        let mut height = 0.0;
        let mut rows = 0;
        for run in buffer.layout_runs() {
            height += run.line_height;
            rows += 1;
        }

        if rows == 0 {
            height = metrics.line_height;
            rows = 1;
        }

        // Add paragraph spacing rhythm
        height += pad_top + pad_bot;
        if line.conceal == md3_editor::layout::ConcealMode::Concealed {
            height = height.max(self.asset_height(line, wrap_width as f32));
        }

        LineMeasure {
            height: height as f64,
            rows,
        }
    }

    fn hit_test(&self, line: &StyledLine, wrap_width: f64, x: f64, y: f64) -> usize {
        let (_, pad_top, _) = self.line_metrics(line);
        let buffer = self.create_buffer(line, wrap_width as f32);

        if let Some(cursor) = buffer.hit((x as f32) - line_indent(line), (y as f32) - pad_top) {
            let char_indices: Vec<usize> = line.display.char_indices().map(|(b, _)| b).collect();
            let mut byte_to_char = vec![0; line.display.len() + 1];
            for (c_idx, &b_idx) in char_indices.iter().enumerate() {
                byte_to_char[b_idx] = c_idx;
            }
            byte_to_char[line.display.len()] = char_indices.len();

            byte_to_char[cursor.index.min(line.display.len())]
        } else {
            line.display.chars().count()
        }
    }
}

impl ShapedMeasurer {
    fn inline_math_spacings(
        &self,
        line: &StyledLine,
        buffer: &Buffer,
        wrap_width: f32,
    ) -> Vec<Option<f32>> {
        let Ok(metrics) = self.metrics.read() else {
            return vec![None; line.spans.len()];
        };
        line.spans
            .iter()
            .map(|span| {
                // Only math rendered as an asset needs its source text
                // letter-spaced to the asset width; a revealed inline `$math$`
                // shows its source verbatim, so leave its glyphs alone.
                if !matches!(span.kind, SpanKind::Math) || line.conceal.reveals_at(span.range.start)
                {
                    return None;
                }
                let chars = line.display.chars().collect::<Vec<_>>();
                let tex = super::editor_canvas::span_text(&chars, span.range.clone());
                let &(asset_w, asset_h) = metrics.math.get(&tex)?;
                let desired =
                    super::editor_canvas::inline_math_size(asset_w, asset_h, wrap_width).0;
                let start = char_to_byte(&line.display, span.range.start);
                let end = char_to_byte(&line.display, span.range.end);
                let glyphs = buffer
                    .layout_runs()
                    .flat_map(|run| run.glyphs.iter())
                    .filter(|glyph| {
                        glyph.w > f32::EPSILON && glyph.start >= start && glyph.start < end
                    })
                    .collect::<Vec<_>>();
                if glyphs.is_empty() {
                    return None;
                }
                let natural = glyphs.iter().map(|glyph| glyph.w).sum::<f32>();
                let (line_metrics, _, _) = self.line_metrics(line);
                Some((desired - natural) / glyphs.len() as f32 / line_metrics.font_size)
            })
            .collect()
    }

    fn asset_height(&self, line: &StyledLine, wrap_width: f32) -> f32 {
        let Ok(metrics) = self.metrics.read() else {
            return 0.0;
        };
        if let Some(&(width, height)) = metrics.math_blocks.get(&line.display) {
            return super::editor_canvas::block_asset_size(width, height, wrap_width, 320.0, 1.0).1
                + self.default_line_height;
        }
        let chars = line.display.chars().collect::<Vec<_>>();
        for span in &line.spans {
            let size = match &span.kind {
                SpanKind::Image { url } => metrics.images.get(url).map(|&(width, height)| {
                    super::editor_canvas::block_asset_size(width, height, wrap_width, 420.0, 1.5)
                }),
                SpanKind::MathContent => {
                    let tex = super::editor_canvas::span_text(&chars, span.range.clone());
                    metrics.math.get(&tex).map(|&(width, height)| {
                        super::editor_canvas::block_asset_size(
                            width, height, wrap_width, 220.0, 1.0,
                        )
                    })
                }
                _ => None,
            };
            if let Some((_, height)) = size {
                return height + self.default_line_height;
            }
        }
        0.0
    }
}

fn build_buffer(
    fs: &mut FontSystem,
    metrics: Metrics,
    display: &str,
    rich: &[(String, Attrs<'_>)],
    default_attrs: &Attrs<'_>,
    wrap_width: f32,
) -> Buffer {
    let mut buffer = Buffer::new(fs, metrics);
    if rich.is_empty() {
        buffer.set_text(fs, display, default_attrs, Shaping::Advanced, None);
    } else {
        buffer.set_rich_text(
            fs,
            rich.iter()
                .map(|(text, attrs)| (text.as_str(), attrs.clone())),
            default_attrs,
            Shaping::Advanced,
            None,
        );
    }
    buffer.set_size(fs, Some(wrap_width), None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

/// Width of left decoration gutter. Concealed list markers and blockquote bars
/// live here; text is inset by same amount in measure, paint, caret, selection,
/// and hit-testing.
pub(crate) const LIST_INDENT: f32 = 24.0;
pub(crate) const QUOTE_INDENT: f32 = 22.0;

pub(crate) fn line_indent(line: &StyledLine) -> f32 {
    use md3_editor::layout::ConcealMode;
    use md3_editor::parse::LineKind;
    if !matches!(line.conceal, ConcealMode::Revealed) {
        match line.kind {
            LineKind::Bullet { .. } | LineKind::Ordered => LIST_INDENT,
            LineKind::Quote => QUOTE_INDENT,
            _ => 0.0,
        }
    } else {
        0.0
    }
}

fn char_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

fn run_caret_x(run: &LayoutRun<'_>, byte_index: usize) -> Option<f32> {
    for glyph in run.glyphs.iter().filter(|glyph| glyph.w > f32::EPSILON) {
        if byte_index == glyph.start {
            let x = if glyph.level.is_rtl() {
                glyph.x + glyph.w - 0.01
            } else {
                glyph.x + 0.01
            };
            return Some(x.max(0.0));
        }
        if byte_index > glyph.start && byte_index < glyph.end {
            let cluster = &run.text[glyph.start..glyph.end];
            let total = cluster.grapheme_indices(true).count().max(1);
            let before = cluster
                .grapheme_indices(true)
                .filter(|(offset, _)| glyph.start + offset < byte_index)
                .count();
            let offset = glyph.w * before as f32 / total as f32;
            let x = if glyph.level.is_rtl() {
                glyph.x + glyph.w - offset + 0.01
            } else {
                glyph.x + offset - 0.01
            };
            return Some(x.max(0.0));
        }
    }

    run.glyphs
        .iter()
        .filter(|glyph| glyph.w > f32::EPSILON)
        .find_map(|glyph| {
            (byte_index == glyph.end).then(|| {
                if glyph.level.is_rtl() {
                    glyph.x + 0.01
                } else {
                    (glyph.x + glyph.w - 0.01).max(0.0)
                }
            })
        })
}
