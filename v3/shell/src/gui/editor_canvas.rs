//! Markdown editor view: paints the engine's [`StyledLine`]s onto an iced
//! canvas. The shell imposes a monospace character grid (the same grid the
//! [`MonoMeasurer`] reports to the layout engine), so caret position, click
//! hit-testing, and span painting all agree by construction.
//!
//! Conceal honors the engine's reserved-width contract: concealed `Marker`
//! spans are *not painted* but their columns still advance — geometry never
//! shifts when the caret enters a line (BUG-B discipline, end to end).

use iced::widget::canvas;
use iced::{Color, Font, Point, Rectangle, Size, mouse};
use md3_editor::layout::{ConcealMode, LineMeasure, Measurer, StyledLine};
use md3_editor::style::SpanKind;
use md3_kernel::pane::TabId;

use super::{Message, session::MdSession};

pub const FONT_SIZE: f32 = 15.0;
pub const LINE_HEIGHT: f32 = 22.0;
/// Advance of one column in the imposed grid. Self-consistent (caret, click,
/// paint all use it); CJK/emoji double-width handling is an M3 concern.
pub const CHAR_WIDTH: f32 = 9.0;
const PAD: f32 = 12.0;

/// Layout-engine measurer for the canvas grid. Soft wrap is off in the M1
/// shell (rows = 1); the engine's wrap machinery stays exercised by tests.
pub struct MonoMeasurer;

impl Measurer for MonoMeasurer {
    fn measure(&self, _line: &StyledLine, _wrap_width: f64) -> LineMeasure {
        LineMeasure {
            height: f64::from(LINE_HEIGHT),
            rows: 1,
        }
    }
}

pub struct EditorCanvas<'a> {
    pub tab: TabId,
    pub session: &'a MdSession,
    pub focused: bool,
}

impl canvas::Program<Message> for EditorCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
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
                let col = (((pos.x - PAD) / CHAR_WIDTH).round().max(0.0)) as usize;
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
                    frame.fill_rectangle(
                        Point::new(PAD + c0 as f32 * CHAR_WIDTH, y),
                        Size::new((c1.saturating_sub(c0)) as f32 * CHAR_WIDTH, LINE_HEIGHT),
                        palette::selection(),
                    );
                }
            }

            paint_line(&mut frame, &styled, y);

            // Caret on its line (focused pane only).
            if self.focused && index == caret_line {
                frame.fill_rectangle(
                    Point::new(PAD + caret_col as f32 * CHAR_WIDTH, y + 2.0),
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
fn paint_line(frame: &mut canvas::Frame, styled: &StyledLine, y: f32) {
    let chars: Vec<char> = styled.display.chars().collect();
    for span in &styled.spans {
        let start = span.range.start.min(chars.len());
        let end = span.range.end.min(chars.len());
        if start >= end {
            continue;
        }
        if matches!(span.kind, SpanKind::Marker) && styled.conceal == ConcealMode::Concealed {
            continue; // reserved width: columns advance, pixels don't
        }
        let content: String = chars[start..end].iter().collect();
        let (color, font) = span_style(&span.kind, styled);
        frame.fill_text(canvas::Text {
            content,
            position: Point::new(PAD + start as f32 * CHAR_WIDTH, y + 2.0),
            color,
            size: FONT_SIZE.into(),
            font,
            shaping: iced::widget::text::Shaping::Advanced,
            ..canvas::Text::default()
        });
    }
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

pub mod palette {
    use crate::gui::tokens;
    use iced::Color;

    pub fn bg() -> Color {
        tokens::dark().bg_primary
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
}
