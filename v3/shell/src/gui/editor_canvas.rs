//! Markdown editor view. Paint, caret, selection, and hit testing consume
//! geometry from the same shaped measurer.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::paint::{AssetKind, FontRole, PaintOp, PaintRole};
use super::{Message, session::MdSession};

pub mod palette;
use iced::widget::canvas;
use iced::{Color, Font, Point, Rectangle, Size, mouse};
use md3_editor::layout::{LineMeasure, Measurer, StyledLine};
use md3_editor::parse::LineKind;
use md3_editor::style::SpanKind;
use md3_kernel::pane::TabId;

pub(crate) const LINE_HEIGHT: f32 = 24.0;
/// Advance of one column in the imposed grid. Self-consistent (caret, click,
/// paint all use it); CJK/emoji double-width handling is an M3 concern.
pub(crate) const CHAR_WIDTH: f32 = 10.0;
pub(crate) const PAD: f32 = 16.0;

#[derive(Default, Clone)]
pub struct VisualMetrics {
    pub images: HashMap<String, (f32, f32)>,
    pub math: HashMap<String, (f32, f32)>,
    pub math_blocks: HashMap<String, (f32, f32)>,
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

    fn hit_test(&self, line: &StyledLine, wrap_width: f64, x: f64, y: f64) -> usize {
        let cols = wrap_columns(wrap_width as f32);
        let row = (y / f64::from(LINE_HEIGHT)).floor().max(0.0) as usize;
        let col = (x / f64::from(CHAR_WIDTH)).round().max(0.0) as usize;
        let char_idx = row * cols + col;
        char_idx.min(line.display.chars().count())
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

pub(crate) fn block_asset_size(
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

pub(crate) fn inline_math_size(asset_w: f32, asset_h: f32, max_width: f32) -> (f32, f32) {
    let scale = ((LINE_HEIGHT - 2.0) / asset_h).min(max_width / asset_w);
    (asset_w * scale, asset_h * scale)
}

pub(crate) fn span_text(chars: &[char], range: std::ops::Range<usize>) -> String {
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
    modifiers: iced::keyboard::Modifiers,
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
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
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

                let styled = self.session.doc.styled_line(line).unwrap_or_else(|| {
                    md3_editor::layout::StyledLine::plain(
                        "",
                        md3_editor::layout::ConcealMode::Concealed,
                    )
                });
                let local_x = pos.x - PAD;
                let local_y = y - line_top;
                let col = self
                    .session
                    .table_hit_test(line, local_x, local_y)
                    .unwrap_or_else(|| {
                        let display_col = self.session.measurer.hit_test(
                            &styled,
                            f64::from(content_width(bounds.width)),
                            f64::from(local_x),
                            f64::from(local_y),
                        );
                        self.session.doc.display_col_to_source(line, display_col)
                    });

                Some(canvas::Action::publish(Message::EditorClicked {
                    tab: self.tab,
                    line,
                    col,
                    viewport_h: bounds.height,
                    checkbox: matches!(styled.kind, LineKind::Bullet { checkbox: Some(_) })
                        && local_x <= 16.0,
                    ctrl: state.modifiers.control(),
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
        state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
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
        let hovered_link = if state.modifiers.control() {
            cursor.position_in(bounds).and_then(|pos| {
                let absolute_y = pos.y + self.session.scroll;
                let line = doc.layout().line_at(f64::from(absolute_y))?;
                let styled = doc.styled_line(line)?;
                let top = doc.layout().offset_of(line).ok()? as f32;
                let display_col = self.session.measurer.hit_test(
                    &styled,
                    f64::from(content_width(bounds.width)),
                    f64::from(pos.x - PAD),
                    f64::from(absolute_y - top),
                );
                let span = styled.spans.iter().find(|span| {
                    span.range.contains(&display_col)
                        && matches!(span.kind, SpanKind::LinkText { .. } | SpanKind::WikiLink)
                })?;
                Some((
                    line,
                    self.session.measurer.selection_rects(
                        &styled,
                        content_width(bounds.width),
                        span.range.start,
                        span.range.end,
                    ),
                ))
            })
        } else {
            None
        };

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
                    let (_, source_c0) = doc.buffer().offset_to_line_col(lo);
                    let (_, source_c1) = doc.buffer().offset_to_line_col(hi);
                    let c0 = doc.source_col_to_display(index, source_c0);
                    let c1 = doc.source_col_to_display(index, source_c1);
                    paint_selection(
                        &mut frame,
                        &self.session.measurer,
                        &styled,
                        c0,
                        c1,
                        y,
                        bounds.width,
                    );
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
            if let Some((hovered_line, rects)) = &hovered_link
                && *hovered_line == index
            {
                for (x, top, width, height) in rects {
                    frame.fill_rectangle(
                        Point::new(PAD + *x, y + *top + *height - 1.0),
                        Size::new(*width, 1.0),
                        palette::link(),
                    );
                }
            }

            // Caret on its line (focused pane only).
            if self.focused && index == caret_line {
                let display_col = doc.source_col_to_display(index, caret_col);
                let (x, caret_y, caret_h) = self.session.measurer.caret_rect(
                    &styled,
                    content_width(bounds.width),
                    display_col,
                );
                frame.fill_rectangle(
                    Point::new(PAD + x, y + caret_y + 2.0),
                    Size::new(1.5, (caret_h - 4.0).max(1.0)),
                    palette::caret(),
                );
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(pos) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let y = pos.y + self.session.scroll;
        let Some(line) = self.session.doc.layout().line_at(f64::from(y)) else {
            return mouse::Interaction::Text;
        };
        let Some(styled) = self.session.doc.styled_line(line) else {
            return mouse::Interaction::Text;
        };
        if matches!(styled.kind, LineKind::Bullet { checkbox: Some(_) }) && pos.x - PAD <= 16.0 {
            return mouse::Interaction::Pointer;
        }
        if state.modifiers.control() {
            let line_top = self.session.doc.layout().offset_of(line).unwrap_or(0.0) as f32;
            let display_col = self.session.measurer.hit_test(
                &styled,
                f64::from(content_width(bounds.width)),
                f64::from(pos.x - PAD),
                f64::from(y - line_top),
            );
            if styled.spans.iter().any(|span| {
                span.range.contains(&display_col)
                    && matches!(span.kind, SpanKind::LinkText { .. } | SpanKind::WikiLink)
            }) {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::Text
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
    let ops = super::paint::line_plan(index, styled, y, line_height, width, session);
    for op in ops {
        match op {
            PaintOp::Text {
                content,
                x,
                y,
                size,
                role,
                font,
            } => {
                let color = paint_role_color(&role);
                let font = font_role_font(&font);
                frame.fill_text(canvas::Text {
                    content,
                    position: Point::new(x, y),
                    color,
                    size: size.into(),
                    font,
                    shaping: iced::widget::text::Shaping::Advanced,
                    ..canvas::Text::default()
                });
            }
            PaintOp::FillRect { rect, role } => {
                frame.fill_rectangle(
                    Point::new(rect.x, rect.y),
                    Size::new(rect.w, rect.h),
                    paint_role_color(&role),
                );
            }
            PaintOp::StrokeRect {
                rect,
                role,
                thickness,
            } => {
                frame.stroke_rectangle(
                    Point::new(rect.x, rect.y),
                    Size::new(rect.w, rect.h),
                    canvas::Stroke::default()
                        .with_color(paint_role_color(&role))
                        .with_width(thickness),
                );
            }
            PaintOp::Asset { kind, rect } => {
                let handle = match kind {
                    AssetKind::Image(url) => {
                        session.image_cache.get(&url).map(|(h, _, _)| h.clone())
                    }
                    AssetKind::Math(tex) => session.math_cache.get(&tex).map(|(h, _, _)| h.clone()),
                };
                if let Some(handle) = handle {
                    frame.draw_image(
                        Rectangle::new(Point::new(rect.x, rect.y), Size::new(rect.w, rect.h)),
                        canvas::Image::new(handle),
                    );
                }
            }
        }
    }
}

fn paint_role_color(role: &PaintRole) -> Color {
    match role {
        PaintRole::Text => palette::text(),
        PaintRole::Marker => palette::marker(),
        PaintRole::Heading => palette::heading(),
        PaintRole::Code => palette::code(),
        PaintRole::Math => palette::math(),
        PaintRole::Link => palette::link(),
        PaintRole::WikiLink => palette::wikilink(),
        PaintRole::Quote => palette::quote(),
        PaintRole::Caret => palette::caret(),
        PaintRole::CodeBg => palette::code_bg(),
        PaintRole::Syntax(syntax_role) => palette::syntax(*syntax_role),
    }
}

fn font_role_font(role: &FontRole) -> Font {
    match role {
        FontRole::Sans => Font::DEFAULT,
        FontRole::SansBold => Font {
            weight: iced::font::Weight::Bold,
            ..Font::DEFAULT
        },
        FontRole::SansItalic => Font {
            style: iced::font::Style::Italic,
            ..Font::DEFAULT
        },
        FontRole::Mono => Font::MONOSPACE,
    }
}
fn paint_selection(
    frame: &mut canvas::Frame,
    measurer: &super::shaped_measurer::ShapedMeasurer,
    styled: &StyledLine,
    start: usize,
    end: usize,
    y: f32,
    width: f32,
) {
    for (x, top, rect_width, height) in
        measurer.selection_rects(styled, content_width(width), start, end)
    {
        frame.fill_rectangle(
            Point::new(PAD + x, y + top),
            Size::new(rect_width, height),
            palette::selection(),
        );
    }
}

pub(crate) fn content_width(viewport_width: f32) -> f32 {
    (viewport_width - PAD * 2.0).max(CHAR_WIDTH)
}

pub(crate) fn wrap_columns(wrap_width: f32) -> usize {
    (wrap_width / CHAR_WIDTH).floor().max(1.0) as usize
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
