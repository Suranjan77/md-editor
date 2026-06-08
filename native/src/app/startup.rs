use iced::Task;

use crate::features::pdf::state::PdfFeatureState;
use std::sync::Arc;

use crate::editor::buffer::DocBuffer;
use crate::features::editor::EditorFeatureState;
use crate::features::overlays::OverlayState;
use crate::features::search::SearchState;
use crate::features::shell::{ActivePanel, ShellState};
use crate::features::tracker::TrackerState;
use crate::features::workspace::WorkspaceState;
use crate::messages::Message;
use crate::views;

use super::model::*;
use crate::app::*;

impl MdEditor {
    pub(crate) fn new() -> (Self, Task<Message>) {
        let (state, startup_error) = match md_editor_core::state::AppState::try_new() {
            Ok(state) => (Arc::new(state), None),
            Err(error) => {
                let fallback = md_editor_core::state::AppState::try_new_in_memory()
                    .unwrap_or_else(|fallback_error| {
                        panic!(
                            "persistent database failed ({error}); in-memory fallback failed ({fallback_error})"
                        )
                    });
                (
                    Arc::new(fallback),
                    Some(format!(
                        "Settings database unavailable; using temporary session: {error}"
                    )),
                )
            }
        };
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
            shell: ShellState {
                sidebar_visible: true,
                toc_visible: false,
                pdf_annotations_visible: false,
                split_view_active: false,
                split_ratio: 0.5,
                is_resizing_split: false,
                pdf_split_ratio: 0.3,
                active_panel: ActivePanel::Markdown,
                keyboard_modifiers: iced::keyboard::Modifiers::default(),
                window_width: 1200.0,
                window_height: 800.0,
            },
            workspace: WorkspaceState::default(),
            editor: EditorFeatureState {
                buffer: DocBuffer::new(),
                highlighted_lines: Vec::new(),
                highlight_generation: 0,
                pending_highlight_generation: None,
                pending_highlight_requested_at: None,
                pending_highlight_text: None,
                pending_save: None,
                image_cache: std::collections::HashMap::new(),
                math_cache: std::collections::HashMap::new(),
                image_errors: std::collections::HashMap::new(),
                math_errors: std::collections::HashMap::new(),
                toc_entries: Vec::new(),
                scroll_y: 0.0,
                viewport_width: 900.0,
                viewport_height: 720.0,
                active_image_path: None,
                active_image: None,
            },
            pdf: PdfFeatureState::default(),
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
        };

        app.load_shell_persistence();
        app.overlays.toast = startup_error;

        let mut task = Task::none();
        if let Some(path) = last_vault {
            app.open_vault(&path);
            if let Some(file_path) = last_file {
                let lower = file_path.to_lowercase();
                if lower.ends_with(".md") || lower.ends_with(".markdown") {
                    task = app.open_file(&file_path);
                } else if lower.ends_with(".pdf") {
                    app.pdf.active_path = Some(file_path.clone());
                    app.pdf.showing_pdf = true;
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
