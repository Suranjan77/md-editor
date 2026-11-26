//! The Markdown editor: commands, saving, caret reveal, math and the table of contents.

use super::*;
use crate::messages::EditorMessage;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_editor(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::Command(command) => self.run_editor_command(command),
            EditorMessage::Paste => self.paste(),
            EditorMessage::Pasted(payload) => self.pasted(payload),
            EditorMessage::InsertImageDialog => self.insert_image_dialog(),
            EditorMessage::ImagesPicked(files) => {
                if files.is_empty() {
                    Task::none()
                } else {
                    self.insert_files(&files)
                }
            }
            EditorMessage::OpenContextMenu(x, y) => {
                self.ui.context_menu = Some(crate::ui_state::ActiveContextMenu {
                    target: crate::messages::ContextMenuTarget::Editor,
                    x,
                    y,
                });
                Task::none()
            }
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
                    SaveOutcome::Failed(err) => {
                        self.ui.notify(
                            crate::ui::ToastKind::Error,
                            format!("Save failed: {err}"),
                            true,
                        );
                    }
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
            EditorMessage::ImageLoaded(key, abs, result) => {
                let root = self.vault.root.clone();
                let active = self.active_path.clone();
                self.editor
                    .image_loaded(key, abs, result, root.as_deref(), active.as_deref());
                Task::none()
            }
            EditorMessage::PdfCropLoaded(key, result) => {
                self.editor.pdf_crop_loaded(key, result);
                Task::none()
            }
            EditorMessage::CaretView(view) => {
                self.editor.caret_view = Some(view);
                self.reveal_caret(view)
            }
            m @ (EditorMessage::CompletionMove(_)
            | EditorMessage::CompletionAccept
            | EditorMessage::CompletionPick(_)
            | EditorMessage::CompletionClose) => self.update_completion_message(m),
            EditorMessage::CodeBlockFinished {
                expected_path,
                expected_close_line,
                expected_body,
                result,
            } => self.on_code_block_finished(
                expected_path,
                expected_close_line,
                expected_body,
                result,
            ),
            EditorMessage::ScrollInterrupted => {
                self.motion.editor_scroll = None;
                Task::none()
            }
            EditorMessage::TocClicked(index) => {
                if self.pdf.active_path.is_some() && self.showing_pdf {
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
                if self.active_path.is_some() || self.pdf.active_path.is_some() {
                    self.editor.toc_visible = !self.editor.toc_visible;
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
        let images = match (self.vault.root.clone(), self.active_path.clone()) {
            (Some(root), Some(path)) => self.editor.load_images(&root, &path),
            _ => Task::none(),
        };
        Task::batch(vec![
            images,
            self.load_pdf_crops(),
            self.editor.load_math(self.ui.scale_factor),
        ])
    }

    /// Render, off the UI thread, each `pdf://…&crop=` image the note shows
    /// that is not rendered yet. Paths resolve the way the link opens.
    fn load_pdf_crops(&mut self) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        for key in self.editor.take_wanted_pdf_crops() {
            let link = crate::pdf_notes::parse_pdf_link(unwrap_angle_brackets(&key));
            let rel =
                resolve_relative_link_path(Some(&root), self.active_path.as_deref(), &link.path);
            let source = md_editor_core::vault::resolve_vault_path_checked(
                std::path::Path::new(&root),
                &rel,
            );
            let (Some(region), Ok(abs)) = (link.crop, source) else {
                self.editor
                    .pdf_crop_loaded(key, Err("not a crop of a PDF in this vault".into()));
                continue;
            };
            let page = link.page.unwrap_or(1).saturating_sub(1);
            // Shown at the crop's size in CSS pixels (96 per 72 points), rendered
            // at twice the display density so it stays sharp when enlarged.
            let (width, height) = (region.width * 4.0 / 3.0, region.height * 4.0 / 3.0);
            let target_px = (width * self.ui.scale_factor * 2.0).clamp(64.0, 3000.0) as u32;
            let state = self.state.clone();
            tasks.push(Task::perform(
                super::blocking(move || {
                    let renderer = state
                        .pdf_renderer
                        .as_ref()
                        .ok_or_else(|| "PDF renderer is unavailable".to_string())?;
                    let image = renderer
                        .render_region(&abs.to_string_lossy(), page, region, target_px)
                        .map_err(|e| e.to_string())?;
                    let handle = iced::widget::image::Handle::from_rgba(
                        image.width,
                        image.height,
                        image.rgba,
                    );
                    Ok((handle, width, height))
                }),
                move |result| Message::Editor(EditorMessage::PdfCropLoaded(key.clone(), result)),
            ));
        }
        Task::batch(tasks)
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
        self.refresh_completion(result.text_changed);
        if result.text_changed {
            self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
            // Restart the autosave countdown on every edit, so a write happens
            // once typing pauses rather than on each keystroke.
            self.editor.mark_dirty_now();
        }
        let content_task = if result.projection_changed {
            if result.text_changed {
                self.editor.toc_entries = self.editor.buffer.toc();
            }
            self.highlight_all()
        } else if result.text_changed {
            self.editor.toc_entries = self.editor.buffer.toc();
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
        workspace::current_pane().editor_scrollable_id(),
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
            | EditorCommand::InsertTable
            | EditorCommand::InsertMath
            | EditorCommand::InsertCheckbox
            | EditorCommand::InsertBlock(_)
            | EditorCommand::Undo
            | EditorCommand::Redo
    )
}

/// A link destination written `<…>` (CommonMark's form for spaces), unwrapped.
fn unwrap_angle_brackets(link: &str) -> &str {
    let link = link.trim();
    link.strip_prefix('<')
        .and_then(|l| l.strip_suffix('>'))
        .unwrap_or(link)
}
