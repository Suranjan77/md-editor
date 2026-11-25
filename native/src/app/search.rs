//! In-note and vault-wide search, match navigation and replace.

use super::*;
use crate::messages::{PdfMessage, SearchMessage};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_search(&mut self, message: SearchMessage) -> Task<Message> {
        match message {
            SearchMessage::GlobalSearchOpen => {
                self.search.visible = true;
                self.search.global_selected = 0;
                if self.pdf.active_path.is_some() && !self.search.query.trim().is_empty() {
                    Task::batch(vec![self.search_pdf(), focus_global_search_input()])
                } else {
                    focus_global_search_input()
                }
            }
            SearchMessage::GlobalSearchMoveSelection(delta) => {
                let total = self.search.pdf_results.len() + self.search.results.len();
                if total > 0 {
                    let cur = self.search.global_selected as isize;
                    self.search.global_selected =
                        (cur + delta).clamp(0, total as isize - 1) as usize;
                }
                Task::none()
            }
            SearchMessage::GlobalSearchSubmit => self.on_global_search_submit(),
            SearchMessage::Close => {
                self.search.visible = false;
                self.search.file_visible = false;
                self.restore_scroll_positions()
            }
            SearchMessage::QueryChanged(q) => {
                self.on_search_query_changed(q);
                Task::none()
            }
            SearchMessage::DebounceElapsed => self.on_search_debounce_elapsed(),
            // Search arms that mutate only `self.search` are routed to
            // `SearchState::update`; see search_state.rs.
            m @ (SearchMessage::ReplaceChanged(_) | SearchMessage::ToggleReplaceRow) => {
                self.search.update(Message::Search(m))
            }
            SearchMessage::RegexToggled(value) => {
                self.search.regex = value;
                self.search.match_index = None;
                self.rerun_pdf_search()
            }
            SearchMessage::MatchCaseToggled(value) => {
                self.search.match_case = value;
                self.search.match_index = None;
                self.rerun_pdf_search()
            }
            SearchMessage::Previous => self.step_search(false),
            SearchMessage::Next => self.step_search(true),
            SearchMessage::ReplaceOne => self.replace_current_match(),
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
            SearchMessage::ResultClicked(path) => self.on_search_result_clicked(path),
        }
    }

    /// Whether the open PDF should be searched for the current query.
    fn pdf_search_wanted(&self) -> bool {
        (self.search.visible || self.pdf_search_is_active())
            && self.pdf.active_path.is_some()
            && self.search.query.len() > 1
    }

    fn rerun_pdf_search(&mut self) -> Task<Message> {
        if self.pdf_search_wanted() {
            self.search_pdf()
        } else {
            Task::none()
        }
    }

    fn step_search(&mut self, forward: bool) -> Task<Message> {
        if self.pdf_search_is_active() {
            self.navigate_pdf_search(forward)
        } else if self.editor_search_is_active() {
            self.navigate_file_search(forward)
        } else {
            Task::none()
        }
    }

    fn on_global_search_submit(&mut self) -> Task<Message> {
        let idx = self.search.global_selected;
        let pdf_len = self.search.pdf_results.len();
        if idx < pdf_len
            && let Some(result) = self.search.pdf_results.get(idx)
        {
            Task::done(Message::Pdf(PdfMessage::SearchResultClicked(
                result.page_index,
            )))
        } else if let Some(result) = self.search.results.get(idx.saturating_sub(pdf_len)) {
            Task::done(Message::Search(SearchMessage::ResultClicked(
                result.path.clone(),
            )))
        } else {
            Task::none()
        }
    }

    fn on_search_query_changed(&mut self, q: String) {
        self.search.query = q;
        self.search.match_index = None;
        self.search.global_selected = 0;
        self.search.pdf_error = None;
        // Stale results are cleared immediately, but the actual vault
        // FTS query and full-document PDF scan run debounced (see
        // SearchDebounceElapsed) so they fire once per pause in
        // typing, not once per keystroke.
        if !vault_query_ready(&self.search.query) || self.search.regex {
            self.search.results.clear();
            self.search.results_truncated = false;
        }
        if !self.pdf_search_wanted() {
            self.search.pdf_results.clear();
            self.search.pdf_indices_by_page.clear();
        }
        self.search.pending_query_at = Some(std::time::Instant::now());
    }

    fn on_search_debounce_elapsed(&mut self) -> Task<Message> {
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
        if vault_query_ready(&q)
            && !self.search.regex
            && let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q)
        {
            self.search.results = res.items;
            self.search.results_truncated = res.truncated;
        }
        self.rerun_pdf_search()
    }

    /// Open a vault search hit, then show the query in the find bar with the
    /// match the hit points at selected (a note) or found (a PDF).
    fn on_search_result_clicked(&mut self, path: String) -> Task<Message> {
        self.search.visible = false;
        let line = self
            .search
            .results
            .iter()
            .find(|r| r.path == path)
            .map_or(1, |r| r.line.max(1));
        self.vault.selected_path = Some(path.clone());
        let is_pdf = path.to_lowercase().ends_with(".pdf");
        // The open buffer is authoritative while dirty; a hit in the open
        // note must not reload it from disk.
        let open = if !search_result_should_reload(self.active_path.as_deref(), &path) {
            Task::none()
        } else if is_pdf {
            self.pdf.active_path = Some(path.clone());
            self.showing_pdf = true;
            self.open_pdf(&path)
        } else {
            self.open_file(&path)
        };
        if self.search.query.trim().is_empty() || self.search.regex {
            return open;
        }
        let find = self.open_contextual_search();
        let jump = if is_pdf {
            Task::none()
        } else {
            self.select_match_from_line(line.saturating_sub(1))
        };
        Task::batch(vec![open, find, jump])
    }

    /// Select the first in-note match on or after `line` (0-based), wrapping
    /// to the first match.
    fn select_match_from_line(&mut self, line: usize) -> Task<Message> {
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        let matches = self.search.matches();
        let len = matches.len();
        if len == 0 {
            return Task::none();
        }
        let target = matches.iter().position(|m| m.line >= line).unwrap_or(0);
        // `navigate_file_search` steps forward onto the target.
        self.search.match_index = Some((target + len - 1) % len);
        self.navigate_file_search(true)
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
            // No match picked yet: start from the page being read, not from
            // the top of the document.
            None => first_pdf_match_from(&self.search.pdf_results, self.pdf.current_page, forward),
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

    pub(super) fn replace_current_match(&mut self) -> Task<Message> {
        if self.active_path.is_none() || self.search.query.is_empty() {
            return Task::none();
        }
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        let matches = self.search.matches().to_vec();
        if matches.is_empty() {
            return Task::none();
        }
        let idx = self.search.match_index.unwrap_or(0).min(matches.len() - 1);
        let item = matches[idx];
        let replacement = crate::search::replacement_for(
            &self.editor.buffer.line_text(item.line),
            item.start_col,
            &self.search.query,
            &self.search.replace,
            self.search.regex,
            self.search.match_case,
        );
        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: item.line,
            anchor_col: item.start_col,
            focus_line: item.line,
            focus_col: item.end_col,
            affinity: Affinity::Downstream,
        });
        let task = self.run_editor_command(EditorCommand::InsertText(replacement));
        // Go on from the end of what was just inserted: the match list has
        // shifted under the old index, and the replacement may itself match.
        let after = (
            self.editor.buffer.cursor_line,
            self.editor.buffer.cursor_col,
        );
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        let remaining = self.search.matches();
        // `navigate_file_search` steps forward, so park one before the target
        // (the first match at or after the insertion, wrapping to the top).
        let len = remaining.len();
        self.search.match_index = (len > 0).then(|| {
            let next = remaining
                .iter()
                .position(|m| (m.line, m.start_col) >= after)
                .unwrap_or(0);
            (next + len - 1) % len
        });
        let nav = self.navigate_file_search(true);
        Task::batch(vec![task, nav])
    }

    pub(super) fn search_pdf(&mut self) -> Task<Message> {
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
            (loose_pdf_pattern(&self.search.query), true)
        } else {
            (self.search.query.clone(), self.search.regex)
        };
        let match_case = self.search.match_case;
        let _state = self.state.clone();
        let path_str = abs_path.to_string_lossy().to_string();
        self.search.pdf_search_seq = self.search.pdf_search_seq.wrapping_add(1);
        let generation = self.search.pdf_search_seq;
        Task::perform(
            super::blocking(move || {
                let Some(renderer) = _state.pdf_renderer.as_ref() else {
                    return Err(
                        "PDF search is unavailable because PDFium is not loaded".to_string()
                    );
                };
                renderer
                    .search_text(&path_str, &query, regex, match_case)
                    .map_err(String::from)
            }),
            move |result| Message::Pdf(PdfMessage::SearchResult(generation, result)),
        )
    }
}

/// The first match on or after `page` (forward), or the last one on or before
/// it (backward), wrapping around the document when there is none.
pub(super) fn first_pdf_match_from(
    results: &[md_editor_core::pdf::PdfSearchMatch],
    page: u16,
    forward: bool,
) -> usize {
    let found = if forward {
        results.iter().position(|m| m.page_index >= page)
    } else {
        results.iter().rposition(|m| m.page_index <= page)
    };
    found.unwrap_or(if forward {
        0
    } else {
        results.len().saturating_sub(1)
    })
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

/// Whether a vault search query is long enough to run: two characters, or a
/// single CJK character (one is already a word there).
pub(super) fn vault_query_ready(query: &str) -> bool {
    let query = query.trim();
    query.chars().count() >= 2 || query.chars().any(is_cjk)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// Loose PDF search: any whitespace run (line breaks included) between the
/// query's words, and inside a word a line break hyphenated by the typesetter
/// (`algo-` / `rithm`, or a soft hyphen), so a phrase or word wrapped across
/// lines still matches.
pub(super) fn loose_pdf_pattern(query: &str) -> String {
    // `-`, soft hyphen (U+00AD), hyphen (U+2010) or PDFium's U+0002 marker,
    // then the line break; or a bare soft hyphen / marker.
    const HYPHEN_BREAK: &str = r"(?:[-\x{AD}\x{2010}\x{02}]\s*[\r\n]\s*|[\x{AD}\x{02}])?";
    query
        .split_whitespace()
        .map(|word| {
            let mut out = String::new();
            let mut prev_alnum = false;
            for c in word.chars() {
                if prev_alnum && c.is_alphanumeric() {
                    out.push_str(HYPHEN_BREAK);
                }
                out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4])));
                prev_alnum = c.is_alphanumeric();
            }
            out
        })
        .collect::<Vec<_>>()
        .join(r"\s+")
}
