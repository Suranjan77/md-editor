use iced::Theme;

use crate::app_shell::{
    AppShellInputs, AppShellPane, AppShellPersistence, AppShellState, AppShellStatus,
    AppShellStatusInputs, WorkflowSidebarTab,
};
use std::sync::Arc;
use std::time::Duration;

use crate::features::editor::EditorFeatureState;
use crate::features::overlays::OverlayState;
use crate::features::pdf::state::PdfFeatureState;
use crate::features::search::SearchState;
pub(crate) use crate::features::shell::ActivePanel;
use crate::features::shell::ShellState;
use crate::features::tracker::TrackerState;
use crate::features::workspace::WorkspaceState;
use crate::messages::Shortcut;
use crate::search::DocumentMatch;
use crate::theme as app_theme;
use crate::views;
use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};

pub const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";
pub const EDITOR_SCROLLABLE_ID: &str = "editor_scrollable";
pub const PDF_RENDER_SUPERSAMPLE: f32 = 2.0;
pub const PDF_RENDER_PRELOAD_PAGES: u16 = 3;
pub const PDF_RENDER_MAX_SCHEDULED_PAGES: u16 = 64;
pub const PDF_TEXT_PAGE_CACHE_LIMIT: usize = 50;
pub(crate) const GLOBAL_PDF_TEXT_SEARCH_MAX_DOCUMENTS: usize = 32;
pub(crate) const GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS: usize = 200;
pub(crate) const PDF_TEXT_INDEX_MAX_DOCUMENTS: usize = 16;
pub(crate) const PDF_TEXT_INDEX_MAX_PAGES_PER_DOCUMENT: u16 = 3;
pub(crate) const LARGE_DOC_LINE_THRESHOLD: usize = 1_000;
pub(crate) const HUGE_DOC_LINE_THRESHOLD: usize = 5_000;
pub(crate) const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(80);
pub(crate) const EDITOR_AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
pub(crate) const APP_SHELL_PERSISTENCE_CONFIG_KEY: &str = "app_shell_persistence";

pub(crate) fn is_supported_image_path(path: &str) -> bool {
    path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".gif")
        || path.ends_with(".bmp")
        || path.ends_with(".webp")
}

#[allow(dead_code)]
pub(crate) fn pdf_slot_offset(page: u16, slot_height: f32) -> f32 {
    PDF_PAGE_LIST_PADDING + f32::from(page) * (slot_height + PDF_PAGE_SPACING)
}

#[allow(dead_code)]
pub(crate) fn pdf_slot_total_height(total_pages: u16, slot_height: f32) -> f32 {
    PDF_PAGE_LIST_PADDING + f32::from(total_pages) * (slot_height + PDF_PAGE_SPACING)
}

pub(crate) fn pdf_search_match_scroll_y_from(
    page_offset: f32,
    rect_y: Option<f32>,
    rect_height: f32,
    page_height: f32,
    zoom: f32,
    max_y: f32,
) -> f32 {
    let match_top = rect_y
        .map(|y| (page_height - y - rect_height).max(0.0) * zoom)
        .unwrap_or(0.0);
    (page_offset + match_top - 96.0).clamp(0.0, max_y.max(0.0))
}

#[allow(dead_code)]
pub(crate) fn pdf_slot_page_at_scroll(scroll_y: f32, total_pages: u16, slot_height: f32) -> u16 {
    if total_pages == 0 {
        return 0;
    }

    let slot_stride = slot_height + PDF_PAGE_SPACING;
    if slot_stride <= 0.0 {
        return 0;
    }

    let page = ((scroll_y - PDF_PAGE_LIST_PADDING).max(0.0) / slot_stride).floor() as u16;
    page.min(total_pages.saturating_sub(1))
}

pub(crate) fn pdf_placeholder_display_size_from(
    placeholder_page_size: Option<(f32, f32)>,
    first_page_size: Option<(f32, f32)>,
    first_dimensions: Option<(u32, u32)>,
    zoom: f32,
) -> (f32, f32) {
    placeholder_page_size
        .or(first_page_size)
        .or_else(|| first_dimensions.map(|(w, h)| (w as f32 / zoom, h as f32 / zoom)))
        .map(|(w, h)| (w * zoom, h * zoom))
        .unwrap_or((612.0 * zoom, 792.0 * zoom))
}

pub fn text_by_char_range(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    text.chars().skip(start).take(end - start).collect()
}

pub(crate) use crate::features::pdf::navigation::NavigationTarget;

pub(crate) struct MdEditor {
    pub(crate) state: Arc<md_editor_core::state::AppState>,
    pub(crate) shell: ShellState,
    pub(crate) workspace: WorkspaceState,
    pub(crate) editor: EditorFeatureState,
    pub(crate) pdf: PdfFeatureState,
    pub(crate) tracker: TrackerState,
    pub(crate) overlays: OverlayState,
    pub(crate) search: SearchState,
}

impl MdEditor {
    pub(crate) fn title(&self) -> String {
        format!(
            "{}Md-editor — {}",
            if self.editor.buffer.dirty { "● " } else { "" },
            self.workspace
                .active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.editor.active_image_path.as_deref())
                .unwrap_or("New File")
        )
    }

    pub(crate) fn theme(&self) -> Theme {
        app_theme::md_editor_theme()
    }

    pub(crate) fn current_shell_persistence(&self) -> AppShellPersistence {
        let active_workflow_tab = if self.tracker.visible {
            WorkflowSidebarTab::Tracker
        } else if self.shell.toc_visible {
            WorkflowSidebarTab::Outline
        } else if self.shell.pdf_annotations_visible {
            WorkflowSidebarTab::Annotations
        } else if self.workspace.backlinks_visible {
            WorkflowSidebarTab::Backlinks
        } else {
            WorkflowSidebarTab::None
        };
        let last_focused_pane = match self.shell.active_panel {
            ActivePanel::Markdown => AppShellPane::Markdown,
            ActivePanel::Pdf => AppShellPane::Pdf,
        };

        AppShellPersistence {
            reduce_motion: false,
            sidebar_width: 260.0,
            reference_width: self.shell.pdf_split_ratio * self.shell.window_width,
            workflow_width: 280.0,
            split_ratio: self.shell.split_ratio,
            sidebar_collapsed: !self.shell.sidebar_visible,
            reference_collapsed: !self.shell.split_view_active,
            workflow_collapsed: !self.workspace.backlinks_visible
                && !self.shell.toc_visible
                && !self.tracker.visible
                && !self.shell.pdf_annotations_visible,
            active_workflow_tab,
            last_focused_pane,
            theme: app_theme::get_active_theme(),
        }
    }

    pub(crate) fn app_shell_state(&self) -> AppShellState {
        let persistence = self
            .current_shell_persistence()
            .clamp_for_window(self.shell.window_width);

        AppShellState::derive(
            AppShellInputs {
                active_pane: AppShellPane::Markdown,
                vault_open: self.workspace.vault_root.is_some(),
                vault_has_entries: !self.workspace.vault_entries.is_empty(),
                markdown_open: self.workspace.active_path.is_some(),
                pdf_open: self.pdf.active_path.is_some(),
                image_open: self.editor.active_image_path.is_some(),
                split_requested: self.shell.split_view_active,
                search_visible: self.search.visible,
                command_palette_visible: self.overlays.command_palette_visible,
                citation_palette_visible: self.overlays.citation_palette_visible,
            },
            persistence,
        )
    }

    pub(crate) fn app_shell_status(&self, shell_state: AppShellState) -> AppShellStatus {
        AppShellStatus::derive(AppShellStatusInputs {
            background_status: None,
            toast: self.overlays.toast.clone(),
            document_open: self.workspace.active_path.is_some()
                || self.pdf.active_path.is_some()
                || self.editor.active_image_path.is_some(),
            document_dirty: self.workspace.active_path.is_some() && self.editor.buffer.dirty,
            global_search_searching: self.search.global.searching,
            global_search_status: self.search.global.pdf_status.clone(),
            global_search_visible: self.search.visible,
            active_pane: shell_state.active_pane,
            background_error: self
                .search
                .global
                .error
                .clone()
                .or_else(|| self.search.pdf_error.clone()),
        })
    }

    pub(crate) fn load_shell_persistence(&mut self) {
        let Ok(Some(value)) =
            md_editor_core::config::get_sys_config(&self.state, APP_SHELL_PERSISTENCE_CONFIG_KEY)
        else {
            return;
        };
        let Some(saved) = AppShellPersistence::deserialize(&value) else {
            return;
        };
        let saved = saved.clamp_for_window(self.shell.window_width);

        self.shell.sidebar_visible = !saved.sidebar_collapsed;
        self.workspace.backlinks_visible =
            matches!(saved.active_workflow_tab, WorkflowSidebarTab::Backlinks)
                && !saved.workflow_collapsed;
        self.shell.toc_visible = matches!(saved.active_workflow_tab, WorkflowSidebarTab::Outline)
            && !saved.workflow_collapsed;
        self.tracker.visible = matches!(saved.active_workflow_tab, WorkflowSidebarTab::Tracker)
            && !saved.workflow_collapsed;
        self.shell.pdf_annotations_visible =
            matches!(saved.active_workflow_tab, WorkflowSidebarTab::Annotations)
                && !saved.workflow_collapsed;
        self.shell.split_ratio = saved.split_ratio;
        self.shell.pdf_split_ratio =
            (saved.reference_width / self.shell.window_width.max(1.0)).clamp(0.15, 0.75);
        self.shell.active_panel = if matches!(saved.last_focused_pane, AppShellPane::Pdf) {
            ActivePanel::Pdf
        } else {
            ActivePanel::Markdown
        };
        app_theme::set_active_theme(saved.theme);
    }

    pub(crate) fn persist_shell_state(&self) {
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            APP_SHELL_PERSISTENCE_CONFIG_KEY,
            &self.current_shell_persistence().serialize(),
        );
    }

    pub(crate) fn toggle_sidebar_visible(&mut self) {
        self.shell.sidebar_visible = !self.shell.sidebar_visible;
        self.persist_shell_state();
    }

    pub(crate) fn set_active_panel(&mut self, active_panel: ActivePanel) {
        if self.shell.active_panel != active_panel {
            self.shell.active_panel = active_panel;
            self.persist_shell_state();
        } else {
            self.shell.active_panel = active_panel;
        }
    }

    pub(crate) fn new_entry_path(&self, name: &str) -> String {
        let parent = self.workspace.selected_path.as_deref().and_then(|path| {
            if self
                .workspace
                .vault_entries
                .iter()
                .any(|entry| entry.path == path && entry.is_dir)
            {
                Some(path.to_string())
            } else {
                std::path::Path::new(path).parent().and_then(|p| {
                    let parent = p.to_string_lossy().replace('\\', "/");
                    if parent.is_empty() {
                        None
                    } else {
                        Some(parent)
                    }
                })
            }
        });

        parent
            .map(|dir| format!("{}/{}", dir.trim_end_matches('/'), name))
            .unwrap_or_else(|| name.to_string())
    }

    pub(crate) fn current_document_match_count(&self) -> usize {
        self.current_document_matches().len()
    }

    pub(crate) fn active_search_match_position(&self) -> Option<(usize, usize)> {
        let matches = self.current_document_matches();
        let index = self.search.editor.active_index?;
        matches
            .get(index.min(matches.len().saturating_sub(1)))
            .map(|m| (m.line, m.start_col))
    }

    pub(crate) fn current_document_matches(&self) -> Vec<DocumentMatch> {
        if self.search.editor.query.is_empty() || self.workspace.active_path.is_none() {
            return Vec::new();
        }

        (0..self.editor.buffer.line_count())
            .flat_map(|line| {
                let text = self.editor.buffer.line_text(line);
                crate::search::line_matches(
                    &text,
                    &self.search.editor.query,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                )
                .into_iter()
                .map(move |line_match| DocumentMatch {
                    line,
                    start_col: line_match.start_col,
                    end_col: line_match.end_col,
                })
            })
            .collect()
    }

    pub(crate) fn pdf_search_is_active(&self) -> bool {
        self.pdf.view.search.visible
            && self.pdf.active_path.is_some()
            && (self.pdf.showing_pdf
                || (self.shell.split_view_active
                    && self.workspace.active_path.is_some()
                    && self.shell.active_panel == ActivePanel::Pdf))
    }

    pub(crate) fn editor_search_is_active(&self) -> bool {
        self.search.editor.visible
            && self.workspace.active_path.is_some()
            && (!self.shell.split_view_active || self.shell.active_panel == ActivePanel::Markdown)
    }

    pub(crate) fn pdf_copy_shortcut_is_active(&self) -> bool {
        self.pdf.selection.is_some()
            && self.pdf.active_path.is_some()
            && (self.pdf.showing_pdf
                || (self.shell.split_view_active
                    && self.workspace.active_path.is_some()
                    && self.shell.active_panel == ActivePanel::Pdf))
    }

    pub(crate) fn command_palette_commands(&self) -> Vec<views::command_palette::Command> {
        let ctx = crate::command_registry::CommandContext {
            markdown_open: self.workspace.active_path.is_some(),
            pdf_open: self.pdf.active_path.is_some(),
            image_open: self.editor.active_image_path.is_some(),
            active_pane: match self.shell.active_panel {
                ActivePanel::Markdown => crate::app_shell::AppShellPane::Markdown,
                ActivePanel::Pdf => crate::app_shell::AppShellPane::Pdf,
            },
            has_vault: self.workspace.vault_root.is_some(),
            pdf_has_selection: self.workspace.active_path.is_some()
                && self.pdf_selection_quote_link_command().is_some(),
            has_focused_annotation: self.workspace.active_path.is_some()
                && self
                    .pdf
                    .focused_annotation_id
                    .as_deref()
                    .and_then(|id| self.pdf_annotation_link_command(id))
                    .is_some(),
        };

        crate::command_registry::get_command_registry()
            .into_iter()
            .filter(|meta| {
                if matches!(
                    meta.id,
                    Shortcut::InsertPdfQuote | Shortcut::InsertPdfHighlight
                ) {
                    meta.is_enabled(ctx).is_ok()
                } else {
                    true
                }
            })
            .map(|meta| {
                let disabled_reason = meta.is_enabled(ctx).err();
                views::command_palette::Command {
                    name: meta.name.to_string(),
                    shortcut: meta.id,
                    icon: meta.icon.to_string(),
                    group_name: match meta.group {
                        crate::app_shell::CommandGroup::File => "File",
                        crate::app_shell::CommandGroup::Edit => "Edit",
                        crate::app_shell::CommandGroup::Navigation => "Navigation",
                        crate::app_shell::CommandGroup::View => "View",
                        crate::app_shell::CommandGroup::Research => "Research",
                        crate::app_shell::CommandGroup::Annotation => "Annotation",
                        crate::app_shell::CommandGroup::Search => "Search",
                    },
                    shortcut_label: meta.default_shortcut.map(|s| s.to_string()),
                    disabled_reason,
                }
            })
            .collect()
    }

    pub(crate) fn citation_palette_items(&self) -> Vec<crate::messages::CitationItem> {
        let mut items = Vec::new();

        // 1. Current selection
        if let (Some(sel), Some(_path)) = (&self.pdf.selection, &self.pdf.active_path) {
            if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                let start = sel.anchor_idx.min(sel.focus_idx);
                let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                let selected_text = text_by_char_range(&page_text.text, start, end);
                if !selected_text.trim().is_empty() {
                    items.push(crate::messages::CitationItem::Selection {
                        text: selected_text,
                        page_index: sel.page_index,
                    });
                }
            }
        }

        // If query is empty, show selection + active PDF annotations.
        let query_trimmed = self.overlays.citation_palette_query.trim();
        if query_trimmed.is_empty() {
            // Add all annotations from current PDF
            for page_anns in self.pdf.annotations.values() {
                for ann in page_anns {
                    items.push(crate::messages::CitationItem::Annotation {
                        id: ann.id.clone(),
                        text: ann.selected_text.clone(),
                        page_index: ann.page_index,
                    });
                }
            }
        } else {
            // Search active PDF annotations
            for page_anns in self.pdf.annotations.values() {
                for ann in page_anns {
                    let matches_text = ann
                        .selected_text
                        .to_lowercase()
                        .contains(&query_trimmed.to_lowercase());
                    let matches_note = ann
                        .note
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&query_trimmed.to_lowercase()))
                        .unwrap_or(false);
                    if matches_text || matches_note {
                        items.push(crate::messages::CitationItem::Annotation {
                            id: ann.id.clone(),
                            text: ann.selected_text.clone(),
                            page_index: ann.page_index,
                        });
                    }
                }
            }

            // Search database cached PDF FTS content
            if let Ok(hits) = self.state.search_cached_pdf_text(query_trimmed, 20) {
                for hit in hits {
                    items.push(crate::messages::CitationItem::SearchHit {
                        path: hit.vault_path,
                        page_index: hit.page_index,
                        snippet: md_editor_core::vault::search_result_preview(
                            &hit.content,
                            query_trimmed,
                            None,
                        ),
                    });
                }
            }
        }

        items
    }

    pub(crate) fn build_global_search_query(
        &self,
        text: String,
    ) -> md_editor_core::domain::UnifiedSearchQuery {
        let mut query = md_editor_core::domain::UnifiedSearchQuery::all_sources(text)
            .with_active_paths(
                self.workspace.active_path.as_deref(),
                self.pdf.active_path.as_deref(),
            );
        query.sources = self.search.global.sources.clone();
        query
    }

    pub(crate) fn update_global_search_searching(&mut self) {
        self.search.global.update_searching();
    }
}
