//! Mouse and keyboard handling.
//!
//! The widget never edits the buffer itself: every edit and caret move is
//! published as an [`EditorCommand`] for the app to apply. What it does own is
//! pointer state — focus, the in-progress drag selection, block scrollbars.

use iced::advanced::graphics::core::event::Event;
use iced::advanced::{Clipboard, Shell, clipboard};
use iced::keyboard::{self, key::Named};
use iced::{Point, Rectangle, mouse};

use super::metrics::TEXT_X_OFFSET;
use super::{Editor, Measure, State};
use crate::editor::buffer::{EditorCommand, Movement};
use crate::editor::highlight::StyledSpan;

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
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.is_dragging => {
                if let Some(pos) = cursor.position_in(bounds) {
                    self.extend_pointer_selection::<R>(state, pos, bounds.width, shell);
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
                if state.horizontal_scroll_drag.is_some() =>
            {
                if let (Some(pos), Some(drag)) =
                    (cursor.position_in(bounds), state.horizontal_scroll_drag)
                {
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
            Some(span) if span.is_checkbox || (span.is_link && ctrl) => mouse::Interaction::Pointer,
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
        let (line_idx, col) = self.hit_test::<R>(pos, available_width, state.is_focused, state);
        let span = self.span_at_point::<R>(pos, available_width, state);
        state.is_focused = true;
        state.selection_anchor = Some((line_idx, col));
        state.selection_focus = Some((line_idx, col));
        state.desired_visual_x = None;
        shell.publish((self.on_pointer_command)(EditorCommand::SetCursor {
            line: line_idx,
            col,
        }));
        state.is_dragging = true;

        let Some(span) = span else {
            return;
        };
        if span.is_checkbox {
            shell.publish((self.on_checkbox_toggle)(line_idx));
        } else if span.is_link
            && let Some(target) = &span.link_target
            && (state.modifiers.control() || state.modifiers.command())
        {
            shell.publish((self.on_link_click)(target.clone()));
        }
    }

    fn extend_pointer_selection<R: Measure>(
        &self,
        state: &mut State,
        pos: Point,
        available_width: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        let (line_idx, col) = self.hit_test::<R>(pos, available_width, state.is_focused, state);
        state.selection_focus = Some((line_idx, col));
        if let Some((anchor_line, anchor_col)) = state.selection_anchor {
            shell.publish((self.on_pointer_command)(EditorCommand::SetSelection {
                anchor_line,
                anchor_col,
                focus_line: line_idx,
                focus_col: col,
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
        if line.is_code_block || ((line.is_table_row || line.is_math_block) && !is_editing) {
            return None;
        }

        let flow = self.flow::<R>(
            line_idx,
            available_width,
            is_editing,
            self.active_col(line_idx, focused),
        );
        let span_idx = flow.span_at(
            pos.x - TEXT_X_OFFSET,
            pos.y - self.line_body_top(line_idx, state, focused),
        )?;
        line.spans.get(span_idx)
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
            && self.on_named_key::<R>(state, named, modifiers.shift(), available_width, shell)
        {
            return;
        }

        if modifiers.command() || modifiers.control() {
            if let keyboard::Key::Character(c) = key {
                self.on_shortcut(state, c, clipboard, shell);
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
        extend: bool,
        available_width: f32,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let movement = |movement| EditorCommand::MoveCursor { movement, extend };
        let command = match named {
            Named::Backspace => EditorCommand::DeleteBackward,
            Named::Delete => EditorCommand::DeleteForward,
            Named::Enter => EditorCommand::InsertText("\n".to_string()),
            Named::Tab => EditorCommand::InsertText("    ".to_string()),
            Named::ArrowLeft => movement(Movement::Left),
            Named::ArrowRight => movement(Movement::Right),
            Named::Home => movement(Movement::Home),
            Named::End => movement(Movement::End),
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

    fn on_shortcut(
        &self,
        state: &mut State,
        c: &str,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let command = match c {
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
        let (new_line, new_col) = self.move_visual::<R>(state, delta_lines, available_width);
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
            }));
        } else {
            self.send_edit(
                state,
                shell,
                EditorCommand::SetCursor {
                    line: new_line,
                    col: new_col,
                },
            );
        }
    }

    /// Publish a keyboard command. The buffer now owns the selection, so the
    /// pointer selection is dropped.
    fn send_edit(&self, state: &mut State, shell: &mut Shell<'_, Message>, command: EditorCommand) {
        shell.publish((self.on_command)(command));
        state.selection_anchor = None;
        state.selection_focus = None;
    }
}
