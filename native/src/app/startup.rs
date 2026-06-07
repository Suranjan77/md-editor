use iced::Task;

use crate::features::pdf::state::PdfPageCache;
use crate::features::pdf::view_model::PdfLayout;
use std::sync::Arc;

use crate::editor::buffer::DocBuffer;
use crate::features::overlays::OverlayState;
use crate::features::pdf::search::PdfSearchState;
use crate::features::pdf::state::PdfViewState;
use crate::features::search::SearchState;
use crate::features::tracker::TrackerState;
use crate::features::workspace::WorkspaceState;
use crate::messages::Message;
use crate::views;
use std::collections::HashSet;

use super::model::*;
use crate::app::*;

impl MdEditor {
    pub(crate) fn new() -> (Self, Task<Message>) {
        let state = Arc::new(md_editor_core::state::AppState::new());
        let last_vault = md_editor_core::config::get_sys_config(&state, "last_vault")
            .ok()
            .flatten();
        let last_file = md_editor_core::config::get_sys_config(&state, "last_file")
            .ok()
            .flatten();
        let tracker_sessions = md_editor_core::tracker::get_sessions(&state).unwrap_or_default();
        let tracker_config_json = md_editor_core::config::get_sys_config(&state, "tracker_config")
            .ok()
            .flatten()
            .filter(|json| views::tracker::parse_config(json).is_ok())
            .unwrap_or_else(views::tracker::default_config_json);

        let mut app = Self {
            state: state.clone(),
            workspace: WorkspaceState::default(),
            sidebar_visible: true,
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            highlight_generation: 0,
            pending_highlight_generation: None,
            pending_highlight_requested_at: None,
            pending_highlight_text: None,
            pdf_current_page: 0,
            pdf_total_pages: 0,
            pdf_state: PdfViewState {
                zoom: 1.5,
                page_sizes: Vec::new(),
                page_cache: PdfPageCache::default(),
                layout: PdfLayout::default(),
                search: PdfSearchState::default(),
            },
            pdf_rotation: 0,
            pdf_pages: Vec::new(),
            pdf_dimensions: Vec::new(),
            pdf_placeholder_page_size: None,
            active_pdf_path: None,
            active_image_path: None,
            active_image: None,
            pdf_scroll_y: 0.0,
            pdf_viewport_height: 0.0,
            pdf_page_links: std::collections::HashMap::new(),
            pdf_link_preview: None,
            showing_pdf: false,
            pdf_fit_to_width: true,
            pdf_fit_to_page: false,
            pdf_document_id: None,
            pdf_page_text: std::collections::HashMap::new(),
            pdf_selection: None,
            pdf_annotations: std::collections::HashMap::new(),
            focused_annotation_id: None,
            pending_editor_save: None,
            pdf_initial_target_page: None,
            pdf_initial_target_annotation: None,
            pdf_pending_text: HashSet::new(),
            pdf_text_lru: std::collections::VecDeque::new(),
            tracker: TrackerState::new(
                tracker_sessions,
                md_editor_core::tracker::get_kv(&state)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|item| (item.key, item.value))
                    .collect(),
                tracker_config_json,
                chrono::Local::now().format("%Y-%m-%d").to_string(),
            ),
            overlays: OverlayState::default(),
            search: SearchState::default(),
            toc_visible: false,
            pdf_annotations_visible: false,
            pdf_annotations_filter_color: None,
            pdf_annotations_filter_page: None,
            pdf_annotations_filter_tag: None,
            pdf_annotations_filter_linked: None,
            pdf_annotations_filter_unresolved: None,
            image_cache: std::collections::HashMap::new(),
            math_cache: std::collections::HashMap::new(),
            image_errors: std::collections::HashMap::new(),
            math_errors: std::collections::HashMap::new(),
            pdf_pending_pages: HashSet::new(),
            pdf_stale_pages: HashSet::new(),
            pdf_pending_links: HashSet::new(),
            pdf_render_generation: 0,
            pdf_programmatic_scroll: false,
            pdf_toc_target_page: None,
            md_toc_entries: Vec::new(),
            pdf_toc_entries_flat: None,
            split_view_active: false,
            split_ratio: 0.5,
            is_resizing_split: false,
            pdf_split_ratio: 0.3,
            active_panel: ActivePanel::Markdown,
            keyboard_modifiers: iced::keyboard::Modifiers::default(),
            window_width: 1200.0,
            window_height: 800.0,
            editor_scroll_y: 0.0,
            editor_viewport_width: 900.0,
            editor_viewport_height: 720.0,
        };

        app.load_shell_persistence();

        let mut task = Task::none();
        if let Some(path) = last_vault {
            app.open_vault(&path);
            if let Some(file_path) = last_file {
                let lower = file_path.to_lowercase();
                if lower.ends_with(".md") || lower.ends_with(".markdown") {
                    task = app.open_file(&file_path);
                } else if lower.ends_with(".pdf") {
                    app.active_pdf_path = Some(file_path.clone());
                    app.showing_pdf = true;
                    task = app.open_pdf(&file_path);
                } else if is_supported_image_path(&lower) {
                    task = app.open_image(&file_path);
                }
            }
        }

        (app, task)
    }

    pub(crate) fn open_vault(&mut self, path: &str) {
        self.workspace.vault_root = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_vault", path);
        self.workspace.vault_entries =
            md_editor_core::vault::set_vault_root(&self.state, path).unwrap_or_default();
        let _ = reindex_vault_with_parser_targets(&self.state, std::path::Path::new(path));

        if let Ok(broken) =
            crate::integrity::check_vault_integrity(&self.state, std::path::Path::new(path))
        {
            if !broken.is_empty() {
                eprintln!(
                    "Vault integrity check: found {} broken references",
                    broken.len()
                );
            }
        }
    }
}
