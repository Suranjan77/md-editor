//! Mouse and keyboard handling.
//!
//! The widget never edits the buffer itself: every edit and caret move is
//! published as an [`EditorCommand`] for the app to apply. What it does own is
//! pointer state — focus, the in-progress drag selection, block scrollbars.

#[cfg(test)]
mod tests;

use iced::advanced::graphics::core::event::Event;
use iced::advanced::{Clipboard, Shell, clipboard};
use iced::keyboard::{self, key::Named};
use iced::{Point, Rectangle, mouse};

use super::measure::{measure_width, span_font};
use super::metrics::{TABLE_CELL_TEXT_INSET, text_left};
use super::scroll::table_columns;
use super::spans::span_is_editing;
use super::{Editor, Measure, State};
use crate::editor::buffer::{EditorCommand, Movement};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::editor::renderer::metrics::scaled;

/// The parsed cell span (link or checkbox) whose source text covers `col` in
/// a raw table row. Cells are found in the row's source in order, so a cell
/// whose text also appears earlier in the row still maps to its own range.
pub(super) fn raw_table_span_at_col(line: &StyledLine, col: usize) -> Option<&StyledSpan> {
    let source: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    let mut search_from = 0;
    for span in line.table_cells.iter().flatten() {
        if span.text.is_empty() {
            continue;
        }
        let Some(relative) = source[search_from..].find(&span.text) else {
            continue;
        };
        let byte_start = search_from + relative;
        let byte_end = byte_start + span.text.len();
        let start = source[..byte_start].chars().count();
        let end = start + span.text.chars().count();
        if (start..end).contains(&col) && (span.is_checkbox || span.is_link) {
            return Some(span);
        }
        search_from = byte_end;
    }
    None
}

impl<Message> Editor<'_, Message> {
    /// Handle one event. `bounds` is the content column.
    pub(super) fn on_event<R: Measure>(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                match cursor.position_in(bounds) {
                    Some(pos) => self.on_press::<R>(state, pos, bounds.width, shell),
                    None => {
                        state.is_focused = false;
                        state.selection_anchor = None;
                        state.selection_focus = None;
                    }
                }
            }
            // A drag keeps tracking the pointer after it leaves the widget.
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.is_dragging => {
                if let Some(pos) = relative_position(cursor, bounds) {
                    self.extend_pointer_selection::<R>(state, pos, bounds.width, shell);
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
                if state.horizontal_scroll_drag.is_some() =>
            {
                if let (Some(pos), Some(drag)) = (
                    relative_position(cursor, bounds),
                    state.horizontal_scroll_drag,
                ) {
                    state
                        .block_scroll_x
                        .insert(drag.block_id(), drag.scroll_for_pointer(pos.x));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_dragging = false;
                state.horizontal_scroll_drag = None;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    self.scroll_block_with_wheel::<R>(state, pos, bounds.width, delta);
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if state.is_focused => {
                self.on_key_press::<R>(
                    state,
                    key.as_ref(),
                    *modifiers,
                    text.as_deref(),
                    bounds.width,
                    clipboard,
                    shell,
                );
            }
            _ => {}
        }
    }

    /// Mouse cursor to show over the editor.
    pub(super) fn interaction<R: Measure>(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(pos) = cursor.position_in(bounds) else {
            return mouse::Interaction::Idle;
        };
        if self
            .horizontal_scrollbar_hit::<R>(pos, bounds.width, state)
            .is_some()
        {
            return mouse::Interaction::Pointer;
        }

        let span = self.span_at_point::<R>(pos, bounds.width, state);
        let ctrl = state.modifiers.control() || state.modifiers.command();
        match span {
            Some(span) if span.is_checkbox || (ctrl && clickable_link(span).is_some()) => {
                mouse::Interaction::Pointer
            }
            _ => mouse::Interaction::Text,
        }
    }

    fn on_press<R: Measure>(
        &self,
        state: &mut State,
        pos: Point,
        available_width: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        if let Some(drag) = self.horizontal_scrollbar_hit::<R>(pos, available_width, state) {
            state.horizontal_scroll_drag = Some(drag);
            state.is_dragging = false;
            shell.capture_event();
            return;
        }

        // Resolve what was clicked against the layout that was on screen,
        // before focusing can change which spans show their source.
        let hit = self.hit_test::<R>(pos, available_width, state.is_focused, state);
        let (line_idx, col) = (hit.line, hit.col);
        let span = self.span_at_point::<R>(pos, available_width, state);
        state.is_focused = true;
        state.selection_anchor = Some((line_idx, col));
        state.selection_focus = Some((line_idx, col));
        state.desired_visual_x = None;
        shell.publish((self.on_pointer_command)(EditorCommand::SetCursor {
            line: line_idx,
            col,
            affinity: hit.affinity,
        }));
        state.is_dragging = true;

        let clicks = count_click(state, pos);
        if clicks > 1 {
            // The buffer owns a word or line selection now; a drag would
            // replace it with a character selection.
            state.is_dragging = false;
            state.selection_anchor = None;
            state.selection_focus = None;
            let command = if clicks == 2 {
                EditorCommand::SelectWord {
                    line: line_idx,
                    col,
                }
            } else {
                EditorCommand::SelectLine { line: line_idx }
            };
            shell.publish((self.on_pointer_command)(command));
            return;
        }

        let Some(span) = span else {
            return;
        };
        if span.is_checkbox {
            shell.publish((self.on_checkbox_toggle)(line_idx));
        } else if let Some(target) = clickable_link(span)
            && (state.modifiers.control() || state.modifiers.command())
        {
            shell.publish((self.on_link_click)(target.to_string()));
        }
    }

    fn extend_pointer_selection<R: Measure>(
        &self,
        state: &mut State,
        pos: Point,
        available_width: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        let hit = self.hit_test::<R>(pos, available_width, state.is_focused, state);
        let (line_idx, col) = (hit.line, hit.col);
        state.selection_focus = Some((line_idx, col));
        if let Some((anchor_line, anchor_col)) = state.selection_anchor {
            shell.publish((self.on_pointer_command)(EditorCommand::SetSelection {
                anchor_line,
                anchor_col,
                focus_line: line_idx,
                focus_col: col,
                affinity: hit.affinity,
            }));
        }
    }

    /// The inline span under a point relative to the content bounds. Code,
    /// rendered tables and rendered block math have none.
    fn span_at_point<R: Measure>(
        &self,
        pos: Point,
        available_width: f32,
        state: &State,
    ) -> Option<&StyledSpan> {
        let focused = state.is_focused;
        let line_idx = self.line_at_widget_y(pos.y, state)?;
        let line = self.lines.get(line_idx)?;
        let is_editing = self.is_block_editing(line, focused);
        if line.is_table_row && !is_editing {
            return self.rendered_table_span_at::<R>(line_idx, pos.x, available_width, state);
        }
        if line.is_code_block || (line.is_math_block && !is_editing) {
            return None;
        }
        if line.is_table_row {
            // A table row being edited shows its raw source as one span; its
            // links and checkboxes only exist in the parsed cells.
            let col = self.hit_test::<R>(pos, available_width, focused, state).col;
            return raw_table_span_at_col(line, col);
        }

        // A rendered image is drawn over its line, not laid out in the flow.
        let active_col = self.active_col(line_idx, focused);
        let body_top = self.line_body_top(line_idx, state);
        for (span_idx, span) in line.spans.iter().enumerate() {
            if !span.is_image || span_is_editing(line, span_idx, is_editing, active_col) {
                continue;
            }
            if let Some((_, image)) = self.image_box(span, available_width)
                && image.contains(Point::new(pos.x, pos.y - body_top))
            {
                return Some(span);
            }
        }

        let flow = self.flow::<R>(
            line_idx,
            available_width,
            is_editing,
            self.active_col(line_idx, focused),
        );
        let span_idx = flow.span_at(
            pos.x - text_left(available_width),
            pos.y - self.line_body_top(line_idx, state),
        )?;
        line.spans.get(span_idx)
    }

    /// The cell span under `x` in a rendered (not edited) table row, using
    /// the same column widths, scroll offset and cell inset the row is
    /// painted with.
    fn rendered_table_span_at<R: Measure>(
        &self,
        line_idx: usize,
        x: f32,
        available_width: f32,
        state: &State,
    ) -> Option<&StyledSpan> {
        let line = self.lines.get(line_idx)?;
        let &(start, end) = state.block_ranges.get(&line.block_id)?;
        let columns = table_columns::<R>(self.lines.get(start..=end)?);
        let scroll_x = self
            .scroll_extent::<R>(state, line.block_id, available_width, state.is_focused)
            .map_or(0.0, |extent| state.block_scroll(line.block_id, &extent));
        let table_x = x - text_left(available_width) + scroll_x;
        if table_x < 0.0 {
            return None;
        }

        let mut column_x = 0.0;
        for (cell, &width) in line.table_cells.iter().zip(&columns) {
            if table_x < column_x + width {
                let mut span_x = column_x + TABLE_CELL_TEXT_INSET;
                for span in cell {
                    let text = span.visible_text(false);
                    if text.is_empty() {
                        continue;
                    }
                    let span_w =
                        measure_width::<R>(text, scaled(span.font_size), span_font(span, line));
                    if table_x >= span_x && table_x < span_x + span_w {
                        return Some(span);
                    }
                    span_x += span_w;
                }
                return None;
            }
            column_x += width;
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn on_key_press<R: Measure>(
        &self,
        state: &mut State,
        key: keyboard::Key<&str>,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        available_width: f32,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        state.modifiers = modifiers;

        if !matches!(key, keyboard::Key::Named(Named::ArrowUp | Named::ArrowDown)) {
            state.desired_visual_x = None;
        }

        // Editing and navigation keys first — they must never fall through to
        // character input.
        if let keyboard::Key::Named(named) = key
            && self.on_named_key::<R>(state, named, modifiers, available_width, shell)
        {
            return;
        }

        if modifiers.command() || modifiers.control() {
            if let keyboard::Key::Character(c) = key {
                self.on_shortcut(state, c, modifiers.shift(), clipboard, shell);
            }
            // Unbound shortcuts are swallowed rather than typed.
            return;
        }

        if let Some(t) = text
            && let Some(c) = t.chars().next()
            && !c.is_control()
        {
            // Route bracket/quote characters through auto-pairing
            // (single-char input only); everything else inserts
            // verbatim.
            let command = if t.chars().count() == 1
                && matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`')
            {
                EditorCommand::TypePaired(c)
            } else {
                EditorCommand::InsertText(t.to_string())
            };
            self.send_edit(state, shell, command);
        }
    }

    /// Handle an editing or navigation key. Returns whether it was one.
    fn on_named_key<R: Measure>(
        &self,
        state: &mut State,
        named: Named,
        modifiers: keyboard::Modifiers,
        available_width: f32,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        if self.completion_open
            && matches!(
                named,
                Named::ArrowUp | Named::ArrowDown | Named::Enter | Named::Tab | Named::Escape
            )
        {
            return true;
        }
        let extend = modifiers.shift();
        let ctrl = modifiers.command() || modifiers.control();
        let movement = |movement| EditorCommand::MoveCursor { movement, extend };
        let multi_line = self
            .buffer
            .selection
            .is_some_and(|(start_line, _, end_line, _)| start_line != end_line);
        let command = match named {
            // Ctrl+Tab and Ctrl+PageUp/PageDown switch document tabs (app-wide).
            Named::Tab | Named::PageUp | Named::PageDown if ctrl => return false,
            Named::Backspace if ctrl => EditorCommand::DeleteWordBackward,
            Named::Delete if ctrl => EditorCommand::DeleteWordForward,
            Named::Backspace => EditorCommand::DeleteBackward,
            Named::Delete => EditorCommand::DeleteForward,
            Named::Enter => EditorCommand::InsertText("\n".to_string()),
            Named::Tab if extend => EditorCommand::OutdentLines,
            Named::Tab if multi_line => EditorCommand::IndentLines,
            Named::Tab => EditorCommand::InsertText("    ".to_string()),
            Named::ArrowLeft if ctrl => movement(Movement::WordLeft),
            Named::ArrowRight if ctrl => movement(Movement::WordRight),
            Named::Home if ctrl => movement(Movement::DocStart),
            Named::End if ctrl => movement(Movement::DocEnd),
            Named::ArrowLeft => movement(Movement::Left),
            Named::ArrowRight => movement(Movement::Right),
            Named::Home => movement(Movement::Home),
            Named::End => movement(Movement::End),
            Named::PageUp | Named::PageDown => {
                self.move_by_page(state, named == Named::PageDown, extend, shell);
                return true;
            }
            Named::ArrowUp => {
                self.move_vertically::<R>(state, -1.0, extend, available_width, shell);
                return true;
            }
            Named::ArrowDown => {
                self.move_vertically::<R>(state, 1.0, extend, available_width, shell);
                return true;
            }
            _ => return false,
        };
        self.send_edit(state, shell, command);
        true
    }

    /// A Ctrl/Cmd chord. Chords the editor acts on are captured, so the
    /// app-wide shortcut listener does not also fire for them (Ctrl+B is
    /// bold here, not the sidebar toggle). Unbound chords are left uncaptured
    /// for the app.
    fn on_shortcut(
        &self,
        state: &mut State,
        c: &str,
        shift: bool,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // Shift reports letters in upper case on most layouts.
        let lower = c.to_lowercase();
        let c = lower.as_str();
        if matches!(c, "z" | "y" | "a" | "b" | "i" | "e" | "k" | "c" | "x" | "v") {
            shell.capture_event();
        }
        let command = match c {
            "z" if shift => EditorCommand::Redo,
            "z" => EditorCommand::Undo,
            "y" => EditorCommand::Redo,
            "a" => EditorCommand::SelectAll,
            "b" => EditorCommand::FormatBold,
            "i" => EditorCommand::FormatItalic,
            "e" => EditorCommand::FormatInlineCode,
            "k" => EditorCommand::InsertLink,
            "c" => {
                if let Some(selected) = self.clipboard_selection(state) {
                    clipboard.write(clipboard::Kind::Standard, selected);
                }
                return;
            }
            "x" => {
                if let Some(selected) = self.clipboard_selection(state) {
                    clipboard.write(clipboard::Kind::Standard, selected);
                    self.send_edit(state, shell, EditorCommand::DeleteSelection);
                }
                return;
            }
            "v" => {
                if let Some(on_paste) = &self.on_paste {
                    shell.publish(on_paste());
                    return;
                }
                if let Some(text) = clipboard.read(clipboard::Kind::Standard) {
                    self.send_edit(state, shell, EditorCommand::InsertText(text));
                }
                return;
            }
            _ => return,
        };
        self.send_edit(state, shell, command);
    }

    /// Text a copy or cut takes: the buffer's selection, else the pointer's.
    fn clipboard_selection(&self, state: &State) -> Option<String> {
        self.buffer
            .selected_text()
            .or_else(|| self.selected_text(state))
    }

    fn move_vertically<R: Measure>(
        &self,
        state: &mut State,
        delta_lines: f32,
        extend: bool,
        available_width: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        let target = self.move_visual::<R>(state, delta_lines, available_width);
        let (new_line, new_col) = (target.line, target.col);
        if extend {
            let (anchor_line, anchor_col) = state
                .selection_anchor
                .or_else(|| self.buffer.selection.map(|(sl, sc, _, _)| (sl, sc)))
                .unwrap_or((self.buffer.cursor_line, self.buffer.cursor_col));
            state.selection_anchor = Some((anchor_line, anchor_col));
            state.selection_focus = Some((new_line, new_col));
            shell.publish((self.on_command)(EditorCommand::SetSelection {
                anchor_line,
                anchor_col,
                focus_line: new_line,
                focus_col: new_col,
                affinity: target.affinity,
            }));
        } else {
            self.send_edit(
                state,
                shell,
                EditorCommand::SetCursor {
                    line: new_line,
                    col: new_col,
                    affinity: target.affinity,
                },
            );
        }
    }

    /// PageUp / PageDown: move the caret one screen (less a line) up or down,
    /// keeping its column; at the first or last screen, to the document edge.
    fn move_by_page(
        &self,
        state: &mut State,
        down: bool,
        extend: bool,
        shell: &mut Shell<'_, Message>,
    ) {
        let count = self.lines.len();
        if count == 0 {
            return;
        }
        let (line, col) = (self.buffer.cursor_line, self.buffer.cursor_col);
        let line_height = state.layout_tree.get_height(line.min(count - 1)).max(1.0);
        let page = (state.viewport_height - line_height).max(line_height);
        let y = state.layout_tree.prefix_sum(line);
        let target_y = if down { y + page } else { y - page };
        let edge = if down {
            target_y >= state.layout_tree.prefix_sum(count)
        } else {
            target_y < 0.0
        };
        let command = match (edge, down) {
            (true, false) => EditorCommand::MoveCursor {
                movement: Movement::DocStart,
                extend,
            },
            (true, true) => EditorCommand::MoveCursor {
                movement: Movement::DocEnd,
                extend,
            },
            _ => {
                let target = state.layout_tree.find_line_at_y(target_y).min(count - 1);
                let (anchor_line, anchor_col) = self
                    .buffer
                    .selection
                    .map(|(sl, sc, _, _)| (sl, sc))
                    .unwrap_or((line, col));
                if extend {
                    EditorCommand::SetSelection {
                        anchor_line,
                        anchor_col,
                        focus_line: target,
                        focus_col: col,
                        affinity: Default::default(),
                    }
                } else {
                    EditorCommand::SetCursor {
                        line: target,
                        col,
                        affinity: Default::default(),
                    }
                }
            }
        };
        self.send_edit(state, shell, command);
    }

    /// Publish a keyboard command. The buffer now owns the selection, so the
    /// pointer selection is dropped.
    fn send_edit(&self, state: &mut State, shell: &mut Shell<'_, Message>, command: EditorCommand) {
        shell.publish((self.on_command)(command));
        state.selection_anchor = None;
        state.selection_focus = None;
    }
}

/// Record a press at `pos` and return how many presses in a row (1–3, then
/// back to 1) landed close together in time and place.
fn count_click(state: &mut State, pos: Point) -> u8 {
    let now = std::time::Instant::now();
    let repeated = state.last_click.is_some_and(|(at, point)| {
        now.duration_since(at) <= crate::design::motion::MULTI_CLICK
            && (point.x - pos.x).abs() <= crate::design::metrics::MULTI_CLICK_SLOP
            && (point.y - pos.y).abs() <= crate::design::metrics::MULTI_CLICK_SLOP
    });
    state.click_count = if repeated && state.click_count < 3 {
        state.click_count + 1
    } else {
        1
    };
    state.last_click = Some((now, pos));
    state.click_count
}

/// The cursor position relative to `bounds`, even outside them.
fn relative_position(cursor: mouse::Cursor, bounds: Rectangle) -> Option<Point> {
    cursor
        .position()
        .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))
}

/// Where Ctrl+click on `span` goes: a link's target, or the source of an
/// image drawn from a document the app opens: a PDF crop (`pdf://…&crop=`)
/// opens the PDF there, a handwritten `.ink` page opens in the ink window.
fn clickable_link(span: &StyledSpan) -> Option<&str> {
    if span.is_link {
        return span.link_target.as_deref();
    }
    let path = span.image_path.as_deref().filter(|_| span.is_image)?.trim();
    let path = path
        .strip_prefix('<')
        .and_then(|p| p.strip_suffix('>'))
        .unwrap_or(path);
    let opens = path.starts_with("pdf://") || crate::ink::is_sketch_path(path);
    opens.then_some(path)
}
