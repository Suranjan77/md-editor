//! Markdown editor view: paints the engine's [`StyledLine`]s onto an iced
//! canvas. The shell imposes a wrapped monospace grid (the same grid the
//! [`MonoMeasurer`] reports to the layout engine), so caret position, click
//! hit-testing, and span painting all agree by construction.
//!
//! Conceal honors the engine's reserved-width contract: concealed `Marker`
//! spans are *not painted* but their columns still advance — geometry never
//! shifts when the caret enters a line (BUG-B discipline, end to end).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use iced::widget::canvas;
use iced::{Color, Font, Point, Rectangle, Size, mouse};
use md3_editor::layout::{ConcealMode, LineMeasure, Measurer, StyledLine};
use md3_editor::parse::LineKind;
use md3_editor::style::SpanKind;
use md3_kernel::pane::TabId;

use super::{Message, session::MdSession};

pub const FONT_SIZE: f32 = 16.0;
pub const LINE_HEIGHT: f32 = 28.0;
/// Advance of one column in the imposed grid. Self-consistent (caret, click,
/// paint all use it); CJK/emoji double-width handling is an M3 concern.
pub const CHAR_WIDTH: f32 = 10.0;
const PAD: f32 = 16.0;

#[derive(Default)]
struct VisualMetrics {
    images: HashMap<String, (f32, f32)>,
    math: HashMap<String, (f32, f32)>,
    math_blocks: HashMap<String, (f32, f32)>,
}

/// Layout-engine measurer for source text and rendered markdown assets.
#[derive(Clone, Default)]
pub struct MonoMeasurer {
    metrics: Arc<RwLock<VisualMetrics>>,
}

impl MonoMeasurer {
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

impl Measurer for MonoMeasurer {
    fn measure(&self, line: &StyledLine, wrap_width: f64) -> LineMeasure {
        let width = wrap_width as f32;
        let columns = wrap_columns(width);
        let text_rows = line.display.chars().count().max(1).div_ceil(columns) as u32;
        let preview_height = self
            .metrics
            .read()
            .ok()
            .map(|metrics| preview_height(line, width, &metrics))
            .unwrap_or(LINE_HEIGHT);
        let text_height = text_rows as f32 * LINE_HEIGHT;
        let height = text_height.max(preview_height) + line_gap(&line.kind);
        let rows = text_rows.max((preview_height / LINE_HEIGHT).ceil().max(1.0) as u32);
        LineMeasure {
            height: f64::from(height),
            rows,
        }
    }
}

fn preview_height(line: &StyledLine, width: f32, metrics: &VisualMetrics) -> f32 {
    if let Some(&(asset_w, asset_h)) = metrics.math_blocks.get(&line.display) {
        let (_, draw_h) = block_asset_size(asset_w, asset_h, width, 320.0, 1.0);
        return draw_h + LINE_HEIGHT;
    }
    let chars = line.display.chars().collect::<Vec<_>>();
    for span in &line.spans {
        match &span.kind {
            SpanKind::Image { url } => {
                if let Some(&(asset_w, asset_h)) = metrics.images.get(url) {
                    let (_, draw_h) = block_asset_size(asset_w, asset_h, width, 420.0, 1.5);
                    return draw_h + LINE_HEIGHT;
                }
            }
            SpanKind::MathContent => {
                let tex = span_text(&chars, span.range.clone());
                if let Some(&(asset_w, asset_h)) = metrics.math.get(&tex) {
                    let (_, draw_h) = block_asset_size(asset_w, asset_h, width, 220.0, 1.0);
                    return draw_h + LINE_HEIGHT;
                }
            }
            _ => {}
        }
    }

    if line
        .spans
        .iter()
        .any(|span| matches!(span.kind, SpanKind::Math))
    {
        return inline_preview_rows(line, width, metrics) as f32 * LINE_HEIGHT;
    }
    LINE_HEIGHT
}

fn line_gap(kind: &LineKind) -> f32 {
    match kind {
        LineKind::Heading { .. } => 10.0,
        LineKind::Paragraph | LineKind::Quote => 6.0,
        LineKind::Bullet { .. } | LineKind::Ordered => 3.0,
        LineKind::Rule => 8.0,
        _ => 0.0,
    }
}

fn inline_preview_rows(line: &StyledLine, width: f32, metrics: &VisualMetrics) -> usize {
    let chars = line.display.chars().collect::<Vec<_>>();
    let mut x = 0.0_f32;
    let mut rows = 1_usize;
    for span in &line.spans {
        if matches!(span.kind, SpanKind::Marker) {
            continue;
        }
        let span_width = match span.kind {
            SpanKind::Math => {
                let tex = span_text(&chars, span.range.clone());
                metrics
                    .math
                    .get(&tex)
                    .map(|&(w, h)| inline_math_size(w, h, width).0)
                    .unwrap_or_else(|| span.range.len() as f32 * CHAR_WIDTH)
            }
            _ => span.range.len() as f32 * CHAR_WIDTH,
        };
        if x > 0.0 && x + span_width > width {
            rows += 1;
            x = 0.0;
        }
        if span_width > width {
            let extra = (span_width / width).ceil() as usize;
            rows += extra.saturating_sub(1);
            x = span_width % width;
        } else {
            x += span_width;
        }
    }
    rows
}

fn block_asset_size(
    asset_w: f32,
    asset_h: f32,
    available_w: f32,
    max_h: f32,
    max_upscale: f32,
) -> (f32, f32) {
    let scale = (available_w / asset_w)
        .min(max_h / asset_h)
        .min(max_upscale);
    (asset_w * scale, asset_h * scale)
}

fn inline_math_size(asset_w: f32, asset_h: f32, max_width: f32) -> (f32, f32) {
    let scale = ((LINE_HEIGHT - 2.0) / asset_h).min(max_width / asset_w);
    (asset_w * scale, asset_h * scale)
}

fn span_text(chars: &[char], range: std::ops::Range<usize>) -> String {
    chars[range]
        .iter()
        .collect::<String>()
        .trim_matches('$')
        .trim()
        .to_string()
}

#[derive(Default)]
pub struct CanvasState {
    viewport: Option<Size>,
}

pub struct EditorCanvas<'a> {
    pub tab: TabId,
    pub session: &'a MdSession,
    pub focused: bool,
}

impl canvas::Program<Message> for EditorCanvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let viewport = bounds.size();
        if state.viewport != Some(viewport) {
            state.viewport = Some(viewport);
            return Some(canvas::Action::publish(Message::EditorViewportChanged {
                tab: self.tab,
                width: bounds.width,
                height: bounds.height,
            }));
        }
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                let y = pos.y + self.session.scroll;
                let line = self
                    .session
                    .doc
                    .layout()
                    .line_at(f64::from(y))
                    .unwrap_or_else(|| self.session.doc.line_count().saturating_sub(1));
                let line_top = self.session.doc.layout().offset_of(line).unwrap_or(0.0) as f32;
                let visual_row = ((y - line_top) / LINE_HEIGHT).floor().max(0.0) as usize;
                let columns = wrap_columns(content_width(bounds.width));
                let visual_col = (((pos.x - PAD) / CHAR_WIDTH).round().max(0.0)) as usize;
                let col = visual_row
                    .saturating_mul(columns)
                    .saturating_add(visual_col.min(columns));
                Some(canvas::Action::publish(Message::EditorClicked {
                    tab: self.tab,
                    line,
                    col,
                    viewport_h: bounds.height,
                }))
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * LINE_HEIGHT * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                Some(canvas::Action::publish(Message::EditorScrolled {
                    tab: self.tab,
                    dy,
                    viewport_h: bounds.height,
                }))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette::bg());

        let doc = &self.session.doc;
        let scroll = f64::from(self.session.scroll);
        let visible = doc.layout().visible_lines(scroll, f64::from(bounds.height));

        // Primary selection, as (line, col) endpoints.
        let primary = doc.buffer().primary();
        let (sel_min, sel_max) = primary.range();
        let (caret_line, caret_col) = doc.buffer().offset_to_line_col(primary.head);

        for index in visible {
            let Some(styled) = doc.styled_line(index) else {
                continue;
            };
            let Ok(top) = doc.layout().offset_of(index) else {
                continue;
            };
            let y = (top - scroll) as f32;
            let line_height = doc
                .layout()
                .height_of(index)
                .unwrap_or(f64::from(LINE_HEIGHT)) as f32;

            // Selection highlight (behind text), clipped to this line.
            if sel_min != sel_max {
                let line_start = doc.buffer().line_col_to_offset(index, 0);
                let line_len = styled.display.chars().count();
                let line_end = doc.buffer().line_col_to_offset(index, line_len);
                let lo = sel_min.max(line_start);
                let hi = sel_max.min(line_end);
                if lo < hi {
                    let (_, c0) = doc.buffer().offset_to_line_col(lo);
                    let (_, c1) = doc.buffer().offset_to_line_col(hi);
                    paint_selection(&mut frame, c0, c1, y, bounds.width);
                }
            }

            paint_line(
                &mut frame,
                index,
                &styled,
                y,
                line_height,
                bounds.width,
                self.session,
            );

            // Caret on its line (focused pane only).
            if self.focused && index == caret_line {
                let columns = wrap_columns(content_width(bounds.width));
                let row = caret_col / columns;
                let col = caret_col % columns;
                frame.fill_rectangle(
                    Point::new(
                        PAD + col as f32 * CHAR_WIDTH,
                        y + row as f32 * LINE_HEIGHT + 2.0,
                    ),
                    Size::new(1.5, LINE_HEIGHT - 4.0),
                    palette::caret(),
                );
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Paint one styled line at vertical offset `y`. Span ranges are char
/// offsets into `display`; x advances column-by-column so concealed markers
/// keep their reserved width without being drawn.
fn paint_line(
    frame: &mut canvas::Frame,
    index: usize,
    styled: &StyledLine,
    y: f32,
    line_height: f32,
    width: f32,
    session: &MdSession,
) {
    paint_block_decoration(frame, styled, y, line_height, width);
    let chars: Vec<char> = styled.display.chars().collect();
    if styled.conceal == ConcealMode::Concealed {
        if let Some(tex) = session.math_block_at(index) {
            paint_cached_block_math(frame, tex, y, width, session);
            return;
        }
        if session.is_math_block_continuation(index) {
            return;
        }
    }
    if styled.conceal == ConcealMode::Concealed
        && paint_block_asset(frame, styled, &chars, y, width, session)
    {
        return;
    }
    if styled.conceal == ConcealMode::Concealed
        && styled
            .spans
            .iter()
            .any(|span| matches!(span.kind, SpanKind::Math))
    {
        paint_inline_preview(frame, styled, &chars, y, width, session);
        return;
    }
    let columns = wrap_columns(content_width(width));
    for span in &styled.spans {
        let start = span.range.start.min(chars.len());
        let end = span.range.end.min(chars.len());
        if start >= end {
            continue;
        }
        if marker_is_concealed(&span.kind, styled) {
            continue; // reserved width: columns advance, pixels don't
        }
        let (color, font) = span_style(&span.kind, styled);
        let mut offset = start;
        while offset < end {
            let row = offset / columns;
            let col = offset % columns;
            let chunk_end = end.min(offset + columns - col);
            let content: String = chars[offset..chunk_end].iter().collect();
            frame.fill_text(canvas::Text {
                content,
                position: Point::new(
                    PAD + col as f32 * CHAR_WIDTH,
                    y + row as f32 * LINE_HEIGHT + 2.0,
                ),
                color,
                size: FONT_SIZE.into(),
                font,
                shaping: iced::widget::text::Shaping::Advanced,
                ..canvas::Text::default()
            });
            offset = chunk_end;
        }
    }
}

fn paint_cached_block_math(
    frame: &mut canvas::Frame,
    tex: &str,
    y: f32,
    width: f32,
    session: &MdSession,
) {
    let Some((handle, asset_w, asset_h)) = session.math_cache.get(tex) else {
        return;
    };
    let available_w = content_width(width);
    let (draw_w, draw_h) = block_asset_size(*asset_w, *asset_h, available_w, 320.0, 1.0);
    frame.draw_image(
        Rectangle::new(
            Point::new(
                PAD + (available_w - draw_w).max(0.0) / 2.0,
                y + LINE_HEIGHT / 2.0,
            ),
            Size::new(draw_w, draw_h),
        ),
        canvas::Image::new(handle.clone()),
    );
}

fn paint_block_asset(
    frame: &mut canvas::Frame,
    styled: &StyledLine,
    chars: &[char],
    y: f32,
    width: f32,
    session: &MdSession,
) -> bool {
    for span in &styled.spans {
        let (asset, max_h, max_upscale) = match &span.kind {
            SpanKind::Image { url } => (session.image_cache.get(url), 420.0, 1.5),
            SpanKind::MathContent => {
                let tex = span_text(chars, span.range.clone());
                (session.math_cache.get(&tex), 220.0, 1.0)
            }
            _ => continue,
        };
        let Some((handle, asset_w, asset_h)) = asset else {
            continue;
        };
        let available_w = content_width(width);
        let (draw_w, draw_h) =
            block_asset_size(*asset_w, *asset_h, available_w, max_h, max_upscale);
        frame.draw_image(
            Rectangle::new(
                Point::new(
                    PAD + (available_w - draw_w).max(0.0) / 2.0,
                    y + LINE_HEIGHT / 2.0,
                ),
                Size::new(draw_w, draw_h),
            ),
            canvas::Image::new(handle.clone()),
        );
        return true;
    }
    false
}

fn paint_inline_preview(
    frame: &mut canvas::Frame,
    styled: &StyledLine,
    chars: &[char],
    y: f32,
    width: f32,
    session: &MdSession,
) {
    let available_w = content_width(width);
    let mut x = 0.0_f32;
    let mut row = 0_usize;
    for span in &styled.spans {
        if matches!(span.kind, SpanKind::Marker) {
            continue;
        }
        if matches!(span.kind, SpanKind::Math) {
            let tex = span_text(chars, span.range.clone());
            if let Some((handle, asset_w, asset_h)) = session.math_cache.get(&tex) {
                let (draw_w, draw_h) = inline_math_size(*asset_w, *asset_h, available_w);
                if x > 0.0 && x + draw_w > available_w {
                    row += 1;
                    x = 0.0;
                }
                frame.draw_image(
                    Rectangle::new(
                        Point::new(
                            PAD + x,
                            y + row as f32 * LINE_HEIGHT + (LINE_HEIGHT - draw_h) / 2.0,
                        ),
                        Size::new(draw_w, draw_h),
                    ),
                    canvas::Image::new(handle.clone()),
                );
                x += draw_w;
                continue;
            }
        }

        let (color, font) = span_style(&span.kind, styled);
        let mut offset = span.range.start.min(chars.len());
        let end = span.range.end.min(chars.len());
        while offset < end {
            let remaining = ((available_w - x) / CHAR_WIDTH).floor().max(0.0) as usize;
            if remaining == 0 {
                row += 1;
                x = 0.0;
                continue;
            }
            let chunk_end = end.min(offset + remaining);
            let content = chars[offset..chunk_end].iter().collect::<String>();
            frame.fill_text(canvas::Text {
                content,
                position: Point::new(PAD + x, y + row as f32 * LINE_HEIGHT + 2.0),
                color,
                size: FONT_SIZE.into(),
                font,
                shaping: iced::widget::text::Shaping::Advanced,
                ..canvas::Text::default()
            });
            x += (chunk_end - offset) as f32 * CHAR_WIDTH;
            offset = chunk_end;
        }
    }
}

fn marker_is_concealed(kind: &SpanKind, styled: &StyledLine) -> bool {
    matches!(kind, SpanKind::Marker)
        && styled.conceal == ConcealMode::Concealed
        && !matches!(
            styled.kind,
            md3_editor::parse::LineKind::TableRow | md3_editor::parse::LineKind::TableSep
        )
}

fn paint_selection(frame: &mut canvas::Frame, start: usize, end: usize, y: f32, width: f32) {
    let columns = wrap_columns(content_width(width));
    let mut offset = start;
    while offset < end {
        let row = offset / columns;
        let col = offset % columns;
        let row_end = end.min((row + 1) * columns);
        frame.fill_rectangle(
            Point::new(PAD + col as f32 * CHAR_WIDTH, y + row as f32 * LINE_HEIGHT),
            Size::new((row_end - offset) as f32 * CHAR_WIDTH, LINE_HEIGHT),
            palette::selection(),
        );
        offset = row_end;
    }
}

fn paint_block_decoration(
    frame: &mut canvas::Frame,
    styled: &StyledLine,
    y: f32,
    height: f32,
    width: f32,
) {
    match styled.kind {
        LineKind::Rule if styled.conceal == ConcealMode::Concealed => {
            frame.fill_rectangle(
                Point::new(PAD, y + LINE_HEIGHT / 2.0),
                Size::new(content_width(width), 1.0),
                palette::marker(),
            );
        }
        LineKind::Bullet {
            checkbox: Some(checked),
        } if styled.conceal == ConcealMode::Concealed => {
            frame.stroke_rectangle(
                Point::new(PAD, y + 5.0),
                Size::new(12.0, 12.0),
                canvas::Stroke::default().with_color(palette::marker()),
            );
            if checked {
                frame.fill_text(canvas::Text {
                    content: "✓".to_string(),
                    position: Point::new(PAD + 1.0, y + 1.0),
                    color: palette::caret(),
                    size: 14.0.into(),
                    ..canvas::Text::default()
                });
            }
        }
        LineKind::CodeContent => {
            frame.fill_rectangle(
                Point::new(4.0, y),
                Size::new((width - 8.0).max(0.0), height),
                palette::code_bg(),
            );
        }
        _ => {}
    }
}

fn content_width(viewport_width: f32) -> f32 {
    (viewport_width - PAD * 2.0).max(CHAR_WIDTH)
}

fn wrap_columns(wrap_width: f32) -> usize {
    (wrap_width / CHAR_WIDTH).floor().max(1.0) as usize
}

fn span_style(kind: &SpanKind, styled: &StyledLine) -> (Color, Font) {
    let mono = Font::MONOSPACE;
    let bold = Font {
        weight: iced::font::Weight::Bold,
        ..Font::MONOSPACE
    };
    let italic = Font {
        style: iced::font::Style::Italic,
        ..Font::MONOSPACE
    };
    match kind {
        SpanKind::Marker => (palette::marker(), mono),
        SpanKind::Bold => (palette::text(), bold),
        SpanKind::Italic => (palette::text(), italic),
        SpanKind::Code | SpanKind::CodeContent => (palette::code(), mono),
        SpanKind::Math | SpanKind::MathContent => (palette::math(), mono),
        SpanKind::LinkText { .. } => (palette::link(), mono),
        SpanKind::Image { .. } => (palette::link(), italic),
        SpanKind::WikiLink => (palette::wikilink(), mono),
        SpanKind::FrontMatter => (palette::marker(), mono),
        SpanKind::QuoteText => (palette::quote(), italic),
        SpanKind::Text => {
            if matches!(styled.kind, md3_editor::parse::LineKind::Heading { .. }) {
                (palette::heading(), bold)
            } else {
                (palette::text(), mono)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use md3_editor::layout::{ConcealMode, Measurer, StyledLine};
    use md3_editor::parse::LineKind;
    use md3_editor::style::{Span, SpanKind};

    use super::{LINE_HEIGHT, MonoMeasurer};

    #[test]
    fn image_height_comes_from_rendered_asset() {
        let measurer = MonoMeasurer::default();
        measurer.set_image_size("plot.png".to_string(), 400.0, 300.0);
        let line = StyledLine {
            display: "![plot](plot.png)".to_string(),
            conceal: ConcealMode::Concealed,
            kind: LineKind::Paragraph,
            spans: vec![Span {
                range: 0..17,
                kind: SpanKind::Image {
                    url: "plot.png".to_string(),
                },
            }],
        };

        let measured = measurer.measure(&line, 800.0);
        assert!(measured.height > f64::from(LINE_HEIGHT * 10.0));
    }

    #[test]
    fn inline_math_stays_on_text_row_when_it_fits() {
        let measurer = MonoMeasurer::default();
        measurer.set_math_size("x^2".to_string(), 40.0, 24.0);
        let line = StyledLine {
            display: "value $x^2$ here".to_string(),
            conceal: ConcealMode::Concealed,
            kind: LineKind::Paragraph,
            spans: vec![
                Span {
                    range: 0..6,
                    kind: SpanKind::Text,
                },
                Span {
                    range: 6..11,
                    kind: SpanKind::Math,
                },
                Span {
                    range: 11..16,
                    kind: SpanKind::Text,
                },
            ],
        };

        let measured = measurer.measure(&line, 800.0);
        assert_eq!(measured.rows, 1);
        assert_eq!(measured.height, f64::from(LINE_HEIGHT + 6.0));
    }
}

pub mod palette {
    use crate::gui::tokens;
    use iced::Color;

    pub fn bg() -> Color {
        tokens::dark().bg_secondary
    }
    pub fn text() -> Color {
        tokens::dark().text_primary
    }
    pub fn marker() -> Color {
        tokens::dark().text_muted
    }
    pub fn heading() -> Color {
        tokens::dark().danger
    }
    pub fn code() -> Color {
        tokens::dark().success
    }
    pub fn math() -> Color {
        tokens::dark().warning
    }
    pub fn link() -> Color {
        tokens::dark().accent
    }
    pub fn wikilink() -> Color {
        tokens::dark().accent_secondary
    }
    pub fn quote() -> Color {
        tokens::dark().accent
    }
    pub fn caret() -> Color {
        tokens::dark().accent
    }
    pub fn selection() -> Color {
        tokens::dark().sel_tint
    }
    pub fn code_bg() -> Color {
        let mut color = tokens::dark().bg_tertiary;
        color.a = 0.72;
        color
    }
}
