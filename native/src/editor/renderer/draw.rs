use super::*;
use crate::app::resolve_relative_link_path;
use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::renderer::geometry::{clip_viewport, normalized_selection};
use crate::pdf_links::parse_pdf_link;
use crate::{search, theme};
use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

pub(crate) fn draw_text_chunk<R>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    renderer.fill_text(
        iced::advanced::text::Text {
            content: content.to_string(),
            bounds: Size::new(max_width.max(1.0), visual_line_step(font_size)),
            size: font_size.into(),
            line_height: iced::advanced::text::LineHeight::default(),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(x, y + (BASE_LINE_HEIGHT - font_size) / 2.0),
        color,
        *viewport,
    );
}

pub(crate) fn draw_wrapped_text<R>(
    renderer: &mut R,
    text: &str,
    x: &mut f32,
    y: &mut f32,
    line_start_x: f32,
    line_right_x: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    let step = visual_line_step(font_size);
    let mut token = String::new();
    let mut flush = |token: &mut String, x: &mut f32, y: &mut f32| {
        if token.is_empty() {
            return;
        }
        let width = measure_width::<R>(token, font_size, font);
        if *x > line_start_x && *x + width > line_right_x {
            *y += step;
            *x = line_start_x;
        }
        if width <= (line_right_x - line_start_x).max(1.0) {
            draw_text_chunk(
                renderer,
                token,
                *x,
                *y,
                line_right_x - *x,
                font_size,
                font,
                color,
                viewport,
            );
            *x += width;
        } else {
            for ch in token.chars() {
                let ch_text = ch.to_string();
                let ch_w = measure_char_width::<R>(ch, font_size, font);
                if *x > line_start_x && *x + ch_w > line_right_x {
                    *y += step;
                    *x = line_start_x;
                }
                draw_text_chunk(
                    renderer,
                    &ch_text,
                    *x,
                    *y,
                    line_right_x - *x,
                    font_size,
                    font,
                    color,
                    viewport,
                );
                *x += ch_w;
            }
        }
        token.clear();
    };

    for ch in text.chars() {
        token.push(ch);
        if ch.is_whitespace() {
            flush(&mut token, x, y);
        }
    }
    flush(&mut token, x, y);
}

pub(crate) fn draw_nowrap_text<R>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    if content.is_empty() {
        return;
    }
    renderer.fill_text(
        iced::advanced::text::Text {
            content: content.to_string(),
            bounds: Size::new(max_width.max(1.0), visual_line_step(font_size)),
            size: font_size.into(),
            line_height: iced::advanced::text::LineHeight::default(),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(x, y),
        color,
        *viewport,
    );
}

pub(crate) fn is_link_target_broken(
    target: &str,
    vault_root: Option<&str>,
    active_path: Option<&str>,
    existing_files: &HashSet<String>,
) -> bool {
    let is_url = target.starts_with("http://")
        || target.starts_with("https://")
        || target.contains("://") && !target.starts_with("pdf://");
    if is_url || target.starts_with('#') {
        return false;
    }

    // Strip hash fragment if any
    let file_target = target.split('#').next().unwrap_or(target);

    if let Some(pdf_target) = parse_pdf_link(file_target) {
        let resolved = resolve_relative_link_path(vault_root, active_path, &pdf_target.path);
        !existing_files.contains(&resolved)
    } else {
        let resolved = resolve_relative_link_path(vault_root, active_path, file_target);
        let mut exists = existing_files.contains(&resolved);
        if !exists {
            exists = existing_files.contains(&format!("{}.md", resolved))
                || existing_files.contains(&format!("{}.markdown", resolved));
        }
        !exists
    }
}

impl<'a, Message> Editor<'a, Message> {
    pub(crate) fn draw_standard_cursor<R>(
        &self,
        renderer: &mut R,
        focused: bool,
        line_idx: usize,
        bounds: Rectangle,
        y: f32,
        _line_height: f32,
    ) where
        R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
    {
        if !focused || line_idx != self.buffer.cursor_line {
            return;
        }

        let mut font_size = 17.0;
        if let Some(styled_line) = self.lines.get(line_idx) {
            let mut char_count = 0;
            let mut found = false;
            for span in &styled_line.spans {
                let span_text = span.visible_text(true);
                let next_char_count = char_count + span_text.chars().count();
                if self.buffer.cursor_col >= char_count && self.buffer.cursor_col <= next_char_count
                {
                    font_size = span.font_size;
                    found = true;
                    break;
                }
                char_count = next_char_count;
            }
            if !found {
                if let Some(first_span) = styled_line.spans.first() {
                    font_size = first_span.font_size;
                }
            }
        }

        let cursor_h = font_size + 2.0;
        if cursor_h <= 0.0 {
            return;
        }

        let (cx, cy) = self.cursor_position::<R>(line_idx, bounds.width);
        // Center cursor within the visual line step, matching text centering
        let line_step = visual_line_step(font_size);
        let centering_offset = (line_step - cursor_h) / 2.0;

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x + TEXT_X_OFFSET + cx,
                    y: y + cy + centering_offset,
                    width: 2.0,
                    height: cursor_h,
                },
                border: iced::Border {
                    radius: 1.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            theme::accent(),
        );
    }
}
