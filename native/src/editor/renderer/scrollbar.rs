use super::*;
use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::parser::{StyledLine, StyledSpan};
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::renderer::geometry::{clip_viewport, normalized_selection};
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

pub(crate) fn draw_horizontal_scrollbar<R>(
    renderer: &mut R,
    block_id: usize,
    state: &State,
    viewport_x: f32,
    viewport_w: f32,
    y: f32,
    content_w: f32,
) where
    R: renderer::Renderer,
{
    if content_w <= viewport_w + 1.0 {
        return;
    }

    let scroll = state
        .block_scroll_x
        .get(&block_id)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, (content_w - viewport_w).max(0.0));
    let track_w = viewport_w.max(1.0);
    let thumb_w = (track_w * (viewport_w / content_w)).clamp(32.0, track_w);
    let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));

    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: viewport_x,
                y,
                width: track_w,
                height: 4.0,
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        theme::border_subtle(),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: thumb_x,
                y,
                width: thumb_w,
                height: 4.0,
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        theme::accent_dim(),
    );
}

impl<'a, Message> Editor<'a, Message> {
    pub(crate) fn horizontal_scrollbar_hit<R>(
        &self,
        pos: Point,
        available_width: f32,
        state: &State,
    ) -> Option<HorizontalScrollDrag>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let line_idx = self.line_at_widget_y(pos.y, state)?;
        let line = self.lines.get(line_idx)?;
        if !(line.is_code_block || line.is_table_row || line.is_math_block) {
            return None;
        }
        let &(start, end) = state.block_ranges.get(&line.block_id)?;
        let block_y = TOP_PAD + state.layout_tree.prefix_sum(start);
        let block_h = state.layout_tree.prefix_sum(end.saturating_add(1))
            - state.layout_tree.prefix_sum(start);
        let available_w = available_width - TEXT_X_OFFSET - MARGIN_RIGHT;
        let viewport_w = if line.is_table_row {
            available_w
        } else if line.is_math_block {
            available_w - 48.0
        } else {
            available_w - 24.0
        }
        .max(80.0);

        self.scrollbar_hit_for_block::<R>(
            pos,
            line.block_id,
            block_y,
            block_h,
            TEXT_X_OFFSET,
            viewport_w,
            available_width,
            state.is_focused,
            Some((line_idx, line_idx)),
            state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scrollbar_hit_for_block<R>(
        &self,
        pos: Point,
        block_id: usize,
        block_y: f32,
        block_h: f32,
        viewport_x: f32,
        viewport_w: f32,
        available_width: f32,
        focused: bool,
        scan_hint: Option<(usize, usize)>,
        state: &State,
    ) -> Option<HorizontalScrollDrag>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let content_w =
            self.block_content_width::<R>(block_id, available_width, focused, scan_hint, state);
        if content_w <= viewport_w + 1.0 {
            return None;
        }

        let scrollbar_y = block_y + block_h - 7.0;
        if pos.x < viewport_x
            || pos.x > viewport_x + viewport_w
            || pos.y < scrollbar_y - 8.0
            || pos.y > scrollbar_y + 10.0
        {
            return None;
        }

        let scroll = state
            .block_scroll_x
            .get(&block_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, (content_w - viewport_w).max(0.0));
        let track_w = viewport_w.max(1.0);
        let thumb_w = (track_w * (viewport_w / content_w)).clamp(32.0, track_w);
        let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));
        let grab_offset = if pos.x >= thumb_x && pos.x <= thumb_x + thumb_w {
            pos.x - thumb_x
        } else {
            thumb_w / 2.0
        };

        Some(HorizontalScrollDrag {
            block_id,
            viewport_x,
            viewport_w,
            content_w,
            grab_offset,
        })
    }
}
