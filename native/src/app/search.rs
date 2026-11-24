//! In-note and vault-wide search, match navigation and replace.

use super::*;
use crate::messages::{PdfMessage, SearchMessage};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_search(&mut self, message: SearchMessage) -> Task<Message> {
        match message {
            SearchMessage::GlobalSearchOpen => {
                self.search.visible = true;
                if self.pdf.active_path.is_some() && !self.search.query.trim().is_empty() {
                    Task::batch(vec![self.search_pdf(), focus_global_search_input()])
                } else {
                    focus_global_search_input()
                }
            }
            SearchMessage::GlobalSearchSubmit => {
                if let Some(result) = self.search.pdf_results.first() {
                    Task::done(Message::Pdf(PdfMessage::SearchResultClicked(
                        result.page_index,
                    )))
                } else if let Some(result) = self.search.results.first() {
                    Task::done(Message::Search(SearchMessage::ResultClicked(
                        result.path.clone(),
                    )))
                } else {
                    Task::none()
                }
            }
            SearchMessage::Close => {
                self.search.visible = false;
                self.search.file_visible = false;
                self.restore_scroll_positions()
            }
            SearchMessage::QueryChanged(q) => {
                self.search.query = q;
                self.search.match_index = None;
                self.search.pdf_error = None;
                // Stale results are cleared immediately, but the actual vault
                // FTS query and full-document PDF scan run debounced (see
                // SearchDebounceElapsed) so they fire once per pause in
                // typing, not once per keystroke.
                if self.search.query.len() <= 2 || self.search.regex {
                    self.search.results.clear();
                    self.search.results_truncated = false;
                }
                if !((self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && self.search.query.len() > 1)
                {
                    self.search.pdf_results.clear();
                    self.search.pdf_indices_by_page.clear();
                }
                self.search.pending_query_at = Some(std::time::Instant::now());
                Task::none()
            }
            SearchMessage::DebounceElapsed => {
                if self
                    .search
                    .pending_query_at
                    .is_some_and(|at| at.elapsed() < crate::search_state::SEARCH_DEBOUNCE)
                {
                    return Task::none();
                }
                if self.search.pending_query_at.take().is_none() {
                    return Task::none();
                }
                let q = self.search.query.clone();
                if q.len() > 2
                    && !self.search.regex
                    && let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q)
                {
                    self.search.results = res.items;
                    self.search.results_truncated = res.truncated;
                }
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && q.len() > 1
                {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            // Search arms that mutate only `self.search` are routed to
            // `SearchState::update`; see search_state.rs.
            m @ SearchMessage::ReplaceChanged(_) => self.search.update(Message::Search(m)),
            SearchMessage::RegexToggled(value) => {
                self.search.regex = value;
                self.search.match_index = None;
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && self.search.query.len() > 1
                {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            SearchMessage::MatchCaseToggled(value) => {
                self.search.match_case = value;
                self.search.match_index = None;
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && self.search.query.len() > 1
                {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            SearchMessage::Previous => {
                if self.pdf_search_is_active() {
                    self.navigate_pdf_search(false)
                } else if self.editor_search_is_active() {
                    self.navigate_file_search(false)
                } else {
                    Task::none()
                }
            }
            SearchMessage::Next => {
                if self.pdf_search_is_active() {
                    self.navigate_pdf_search(true)
                } else if self.editor_search_is_active() {
                    self.navigate_file_search(true)
                } else {
                    Task::none()
                }
            }
            SearchMessage::ReplaceAll => match self.replace_all_in_current_document() {
                Ok((count, task)) => {
                    self.ui.toast = Some(format!("Replaced {} matches", count));
                    task
                }
                Err(err) => {
                    self.ui.toast = Some(err);
                    Task::none()
                }
            },
            SearchMessage::ResultClicked(path) => {
                if !search_result_should_reload(self.active_path.as_deref(), &path) {
                    // The open buffer is authoritative while dirty; a global
                    // search hit for this same file must not reload disk state.
                    self.search.visible = false;
                    self.vault.selected_path = Some(path);
                    return Task::none();
                }
                self.search.visible = false;
                self.vault.selected_path = Some(path.clone());
                if path.to_lowercase().ends_with(".pdf") {
                    self.pdf.active_path = Some(path.clone());
                    self.showing_pdf = true;
                    self.open_pdf(&path)
                } else {
                    self.open_file(&path)
                }
            }
        }
    }

    pub(super) fn navigate_file_search(&mut self, forward: bool) -> Task<Message> {
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        let matches = self.search.matches().to_vec();
        if matches.is_empty() {
            self.search.match_index = None;
            return Task::none();
        }

        let next_index = match self.search.match_index {
            Some(index) if forward => (index + 1) % matches.len(),
            Some(0) if !forward => matches.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => matches.len() - 1,
        };
        self.search.match_index = Some(next_index);
        let item = matches[next_index];
        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: item.line,
            anchor_col: item.start_col,
            focus_line: item.line,
            focus_col: item.end_col,
            affinity: Affinity::Downstream,
        });
        self.editor.request_reveal(Reveal::Center);
        Task::none()
    }

    pub(super) fn navigate_pdf_search(&mut self, forward: bool) -> Task<Message> {
        if self.search.pdf_results.is_empty() {
            self.search.match_index = None;
            return Task::none();
        }

        let next_index = match self.search.match_index {
            Some(index) if forward => (index + 1) % self.search.pdf_results.len(),
            Some(0) if !forward => self.search.pdf_results.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => self.search.pdf_results.len() - 1,
        };
        self.navigate_pdf_search_to_index(next_index)
    }

    pub(super) fn navigate_pdf_search_to_index(&mut self, index: usize) -> Task<Message> {
        let Some(result) = self.search.pdf_results.get(index).cloned() else {
            self.search.match_index = None;
            return Task::none();
        };

        self.search.match_index = Some(index);
        let target_page = result
            .page_index
            .min(self.pdf.total_pages.saturating_sub(1));
        self.pdf.current_page = target_page;
        self.pdf.programmatic_scroll = true;
        self.pdf.toc_target_page = None;

        let scroll_y = self.pdf.search_match_scroll_y(&result);
        if let Some(path) = &self.pdf.active_path
            && let Some(abs_path) = self.resolve_active_path(path)
        {
            let path_str = abs_path.to_string_lossy().to_string();
            if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                renderer.set_visible_range(
                    target_page.saturating_sub(1),
                    (target_page + 1).min(self.pdf.total_pages.saturating_sub(1)),
                    &path_str,
                );
            }
        }

        let mut tasks = vec![self.render_pdf_page_direct(target_page)];
        tasks.push(self.render_pdf_page_range(
            target_page.saturating_sub(2),
            (target_page + 2).min(self.pdf.total_pages.saturating_sub(1)),
        ));
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: scroll_y,
            },
        ));
        Task::batch(tasks)
    }

    pub(super) fn pdf_search_is_active(&self) -> bool {
        self.search.file_visible
            && self.pdf.active_path.is_some()
            && (self.showing_pdf
                || (self.ui.split_view_active
                    && self.active_path.is_some()
                    && self.active_panel == ActivePanel::Pdf))
    }

    pub(super) fn editor_search_is_active(&self) -> bool {
        self.search.file_visible
            && self.active_path.is_some()
            && !self.pdf_search_is_active()
            && (!self.ui.split_view_active || self.active_panel == ActivePanel::Markdown)
    }

    pub(super) fn replace_all_in_current_document(
        &mut self,
    ) -> Result<(usize, Task<Message>), String> {
        if self.active_path.is_none() {
            return Err("Open a markdown file before replacing text".to_string());
        }
        if self.search.query.is_empty() {
            return Err("Search query is empty".to_string());
        }

        let text = self.editor.buffer.text();
        let (new_text, count) = if self.search.regex {
            let re = regex::RegexBuilder::new(&self.search.query)
                .case_insensitive(!self.search.match_case)
                .build()
                .map_err(|err| format!("Invalid regex: {err}"))?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search.replace.as_str())
                    .to_string(),
                count,
            )
        } else if self.search.match_case {
            let count = text.match_indices(&self.search.query).count();
            (
                text.replace(&self.search.query, &self.search.replace),
                count,
            )
        } else {
            let re = regex::RegexBuilder::new(&regex::escape(&self.search.query))
                .case_insensitive(true)
                .build()
                .map_err(|err| err.to_string())?;
            let count = re.find_iter(&text).count();
            // Plain-text mode: `$` in the replacement is text, not a
            // capture-group reference.
            (
                re.replace_all(&text, regex::NoExpand(&self.search.replace))
                    .to_string(),
                count,
            )
        };

        if count > 0 {
            self.editor.buffer.replace_all_text(&new_text);
            self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
            // Replace-all is an edit like any other: it arms autosave, so the
            // result reaches disk without waiting for an explicit save.
            self.editor.mark_dirty_now();
            self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            let task = self.highlight_all();
            return Ok((count, task));
        }
        Ok((count, Task::none()))
    }

    pub(super) fn search_pdf(&self) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        // Loose whitespace: build a regex that allows any whitespace run
        // (including PDF line breaks) between the query's tokens, so a phrase
        // wrapping across lines still matches. Reuses the existing regex path.
        let (query, regex) = if self.search.loose {
            let pattern = self
                .search
                .query
                .split_whitespace()
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join(r"\s+");
            (pattern, true)
        } else {
            (self.search.query.clone(), self.search.regex)
        };
        let match_case = self.search.match_case;
        let _state = self.state.clone();
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        Task::perform(
            async move {
                let Some(renderer) = _state.pdf_renderer.as_ref() else {
                    return Err(
                        "PDF search is unavailable because PDFium is not loaded".to_string()
                    );
                };
                renderer.search_text(&path_str, &query, regex, match_case)
            },
            move |result| Message::Pdf(PdfMessage::SearchResult(generation, result)),
        )
    }
}

pub(super) fn focus_file_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::search::FILE_SEARCH_INPUT_ID,
    ))
}

pub(super) fn focus_global_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::search::GLOBAL_SEARCH_INPUT_ID,
    ))
}

pub(super) fn focus_pdf_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::pdf_viewer::PDF_SEARCH_INPUT_ID,
    ))
}
