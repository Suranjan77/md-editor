//! The Markdown editor: commands, saving, caret reveal, math and the table of contents.

use super::*;
use crate::messages::EditorMessage;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_editor(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::Command(command) => self.run_editor_command(command),
            EditorMessage::CommandNoScroll(command) => {
                self.run_editor_command_with_scroll(command, false)
            }
            // Editor arms that mutate only `self.editor` are routed to
            // `EditorPane::update`; see editor_state.rs.
            m @ EditorMessage::MathRendered(..) => self.editor.update(Message::Editor(m)),
            EditorMessage::Save => {
                match self.save_active_document() {
                    SaveOutcome::Saved => self.ui.toast = Some("File saved".to_string()),
                    // An explicit save always answers, even when autosave got
                    // there first — pressing Ctrl+S and seeing nothing happen
                    // reads as the keystroke having been swallowed.
                    SaveOutcome::NothingToDo if self.active_path.is_some() => {
                        self.ui.toast = Some("No unsaved changes".to_string())
                    }
                    SaveOutcome::NothingToDo => {}
                    SaveOutcome::Failed(err) => self.ui.toast = Some(format!("Save failed: {err}")),
                }
                Task::none()
            }
            EditorMessage::AnimationTick(now) => {
                self.motion.now = now;
                self.step_editor_scroll(now)
            }
            EditorMessage::AutosaveElapsed => {
                if self.editor.autosave_due() {
                    // Autosave is silent on success — a toast on every pause in
                    // typing would be noise. Failures still surface, because a
                    // failed autosave is exactly when the user needs to know.
                    // Only the first failure in a run speaks up, though: the
                    // retry keeps going in the background, and a toast every
                    // few hundred milliseconds would bury the rest of the UI.
                    if let SaveOutcome::Failed(err) = self.save_active_document()
                        && !self.autosave_failure_reported
                    {
                        self.autosave_failure_reported = true;
                        self.ui.toast = Some(format!("Autosave failed: {err}"));
                    }
                }
                Task::none()
            }
            EditorMessage::CheckboxToggle(line_idx) => {
                self.run_editor_command(EditorCommand::ToggleCheckbox { line: line_idx })
            }
            EditorMessage::Scrolled {
                y,
                viewport_width,
                viewport_height,
            } => {
                self.active_panel = ActivePanel::Markdown;
                self.editor.scroll_y = y;
                self.editor.viewport_width = viewport_width;
                self.editor.viewport_height = viewport_height;
                Task::none()
            }
            EditorMessage::CaretView(view) => self.reveal_caret(view),
            EditorMessage::ScrollInterrupted => {
                self.motion.editor_scroll = None;
                Task::none()
            }
            EditorMessage::TocClicked(index) => {
                if self.pdf.active_path.is_some() {
                    let target_page = (index as u16).min(self.pdf.total_pages.saturating_sub(1));
                    self.navigate_pdf_page(target_page)
                } else {
                    let task = self.run_editor_command(EditorCommand::SetCursor {
                        line: index,
                        col: 0,
                        affinity: Affinity::Downstream,
                    });
                    self.editor.request_reveal(Reveal::Center);
                    task
                }
            }
            EditorMessage::ToggleTOC => {
                if self.pdf.active_path.is_some()
                    && (self.showing_pdf
                        || (self.ui.split_view_active && self.active_path.is_some()))
                {
                    self.editor.toc_visible = !self.editor.toc_visible;
                    // The TOC panel changes the PDF pane width — re-fit so pages
                    // don't render stretched at the old width.
                    return self.refit_pdf_if_needed();
                }
                Task::none()
            }
        }
    }

    /// Re-highlight the buffer and load the images and math it references.
    pub(super) fn highlight_all(&mut self) -> Task<Message> {
        self.editor.refresh_highlighting();
        self.load_editor_resources()
    }

    /// Load images and math for the freshly highlighted lines. Images resolve
    /// relative to the active document, so the vault root + active path are
    /// supplied from the shell.
    pub(super) fn load_editor_resources(&mut self) -> Task<Message> {
        if let (Some(root), Some(path)) = (self.vault.root.clone(), self.active_path.clone()) {
            self.editor.load_images(&root, &path);
        }
        self.editor.load_math(self.ui.scale_factor)
    }

    /// Scroll the editor the way the answered reveal request asked, now that
    /// the widget has said exactly where the caret is.
    pub(super) fn reveal_caret(&mut self, view: CaretView) -> Task<Message> {
        let Some(reveal) = self.editor.take_reveal(view.request) else {
            return Task::none();
        };
        let Some(top) = view.viewport_top_for(reveal) else {
            return Task::none();
        };
        // The widget sits inside the scrollable's padding: a widget-relative y
        // plus the padding is a scroll offset.
        let current = view.viewport_top + EDITOR_PADDING;
        let max = (view.height + 2.0 * EDITOR_PADDING - view.viewport_height).max(0.0);
        let target = (top + EDITOR_PADDING).clamp(0.0, max);
        if (target - current).abs() < 0.5 {
            return Task::none();
        }

        let now = std::time::Instant::now();
        let mut spring = match self.motion.editor_scroll {
            // Still gliding from where the page really is: keep the momentum.
            Some(glide) if (glide.applied - current).abs() < 1.0 => glide.spring,
            _ => Spring::at_rest(current, now, SCROLL_STIFFNESS),
        };
        // A far jump cuts to a screen short of the target and glides the rest.
        let reach = view.viewport_height.max(1.0);
        let mut applied = current;
        let mut task = Task::none();
        if (target - spring.value(now)).abs() > 2.0 * reach {
            applied = target - reach.copysign(target - current);
            spring = Spring::at_rest(applied, now, SCROLL_STIFFNESS);
            task = scroll_editor_to(applied);
        }
        spring.retarget(target, now);
        self.motion.editor_scroll = Some(ScrollGlide { spring, applied });
        self.motion.now = now;
        task
    }

    /// Move an animated editor scroll on to the frame drawn at `now`.
    pub(super) fn step_editor_scroll(&mut self, now: std::time::Instant) -> Task<Message> {
        let Some(glide) = self.motion.editor_scroll else {
            return Task::none();
        };
        let y = if glide.spring.is_settled(now) {
            self.motion.editor_scroll = None;
            glide.spring.target()
        } else {
            let y = glide.spring.value(now);
            self.motion.editor_scroll = Some(ScrollGlide {
                applied: y,
                ..glide
            });
            y
        };
        scroll_editor_to(y)
    }

    pub(super) fn run_editor_command(&mut self, command: EditorCommand) -> Task<Message> {
        let keep_cursor_visible = editor_command_keeps_cursor_visible(&command);
        self.run_editor_command_with_scroll(command, keep_cursor_visible)
    }

    pub(super) fn run_editor_command_with_scroll(
        &mut self,
        command: EditorCommand,
        keep_cursor_visible: bool,
    ) -> Task<Message> {
        self.active_panel = ActivePanel::Markdown;
        let result = self.editor.buffer.execute(command);
        if result.text_changed {
            self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
            // Restart the autosave countdown on every edit, so a write happens
            // once typing pauses rather than on each keystroke.
            self.editor.mark_dirty_now();
        }
        let content_task = if result.projection_changed {
            if result.text_changed {
                self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            }
            self.highlight_all()
        } else if result.text_changed {
            self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            Task::none()
        } else {
            Task::none()
        };

        if keep_cursor_visible {
            self.editor.request_reveal(Reveal::Nearest);
        }
        content_task
    }
}

pub(super) fn scroll_editor_to(y: f32) -> Task<Message> {
    operation::scroll_to(
        iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
        AbsoluteOffset { x: 0.0, y },
    )
}

pub(super) fn editor_command_keeps_cursor_visible(command: &EditorCommand) -> bool {
    matches!(
        command,
        EditorCommand::InsertText(_)
            | EditorCommand::DeleteSelection
            | EditorCommand::DeleteBackward
            | EditorCommand::DeleteForward
            | EditorCommand::MoveCursor { .. }
            | EditorCommand::SetCursor { .. }
            | EditorCommand::SetSelection { .. }
            | EditorCommand::SelectAll
            | EditorCommand::ToggleCheckbox { .. }
            | EditorCommand::FormatBold
            | EditorCommand::FormatItalic
            | EditorCommand::FormatInlineCode
            | EditorCommand::InsertLink
            | EditorCommand::Undo
            | EditorCommand::Redo
    )
}
