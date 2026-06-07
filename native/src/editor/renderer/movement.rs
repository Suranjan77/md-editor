use super::*;
use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::{StyledLine, StyledSpan};
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

impl<'a, Message> Editor<'a, Message> {
    pub(crate) fn move_visual<R>(
        &self,
        state: &mut State,
        delta_lines: f32,
        available_width: f32,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        if state.layout_tree.len() != self.lines.len() {
            self.rebuild_layout_tree::<R>(state, available_width);
        }
        let (cur_x, cur_y_in_line) =
            self.cursor_position::<R>(self.buffer.cursor_line, available_width);
        let cur_y_base = self.widget_y_for_line(self.buffer.cursor_line, state);

        let visual_x = *state.desired_visual_x.get_or_insert(cur_x);
        let line = &self.lines[self.buffer.cursor_line];
        let max_font = line
            .spans
            .iter()
            .map(|s| s.font_size)
            .fold(17.0_f32, f32::max);
        let step = visual_line_step(max_font);

        let target_y = cur_y_base + cur_y_in_line + delta_lines * step + step / 2.0;

        let mut target = self.hit_test::<R>(
            Point::new(visual_x + TEXT_X_OFFSET, target_y),
            available_width,
            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
            state.is_focused,
            state,
        );

        let current = (self.buffer.cursor_line, self.buffer.cursor_col);
        if delta_lines > 0.0 && target <= current {
            target = self.fallback_visual_line_move::<R>(visual_x, 1, available_width);
        } else if delta_lines < 0.0 && target >= current {
            target = self.fallback_visual_line_move::<R>(visual_x, -1, available_width);
        }

        target
    }

    pub(crate) fn fallback_visual_line_move<R>(
        &self,
        visual_x: f32,
        delta_lines: isize,
        available_width: f32,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let current_line = self.buffer.cursor_line;
        let target_line = if delta_lines < 0 {
            current_line.saturating_sub(1)
        } else {
            (current_line + 1).min(self.lines.len().saturating_sub(1))
        };

        if target_line == current_line {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        }

        let Some(line) = self.lines.get(target_line) else {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        };
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let is_editing = is_block_editing_line(line, active_block_id, true);
        let col = self.col_for_visual_point::<R>(
            line,
            visual_x,
            BASE_LINE_HEIGHT / 2.0,
            available_width,
            is_editing,
            None,
        );
        (target_line, col)
    }
}
