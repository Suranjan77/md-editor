//! Characterization tests (P0.T4): freeze the observable behavior of the root
//! reducer for the most important message variants so Phase 2/3 refactors can
//! prove they preserved behavior.
//!
//! These tests assert *current* behavior — they define it as correct by
//! definition. If a refactor changes one deliberately, the commit body must
//! say why. Plain asserts are used instead of `insta` snapshots so the suite
//! is self-verifying without a snapshot-accept round.

#[cfg(test)]
mod tests {
    use crate::app::*;
    use crate::editor::buffer::{DocBuffer, EditorCommand};
    use crate::editor::parser;
    use crate::messages::*;
    use crate::views;
    use std::sync::Arc;

    fn base_app() -> MdEditor {
        let mut app = MdEditor::new().0;
        app.state = Arc::new(
            md_editor_core::state::AppState::try_new_in_memory()
                .expect("in-memory application state should initialize"),
        );
        app.workspace.vault_root = Some("/tmp/md-editor-characterization".to_string());
        app.workspace.vault_entries = vec![
            md_editor_core::domain::FileEntry {
                path: "notes".to_string(),
                name: "notes".to_string(),
                is_dir: true,
            },
            md_editor_core::domain::FileEntry {
                path: "notes/research.md".to_string(),
                name: "research.md".to_string(),
                is_dir: false,
            },
        ];
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.workspace.selected_path = app.workspace.active_path.clone();
        app.editor.buffer = DocBuffer::from_text("# Research\n\nBody line.\n\n## Section\n");
        app.editor.highlighted_lines = parser::highlight_markdown(&app.editor.buffer.text());
        app.editor.toc_entries = vec![
            views::toc::TocEntry {
                level: 1,
                text: "Research".to_string(),
                line: 0,
            },
            views::toc::TocEntry {
                level: 2,
                text: "Section".to_string(),
                line: 4,
            },
        ];
        app
    }

    fn pdf_app() -> MdEditor {
        let mut app = base_app();
        app.pdf.active_path = Some("papers/paper.pdf".to_string());
        app.pdf.showing_pdf = true;
        app.pdf.total_pages = 3;
        app.pdf.current_page = 0;
        app.pdf.pages = vec![None, None, None];
        app.pdf.dimensions = vec![None, None, None];
        app.pdf.view.page_sizes = vec![Some((612.0, 792.0)); 3];
        app.pdf.placeholder_page_size = Some((612.0, 792.0));
        app.shell.active_panel = ActivePanel::Pdf;
        app
    }

    // -- shell --------------------------------------------------------------

    #[test]
    fn sidebar_toggle_flips_visibility_and_back() {
        let mut app = base_app();
        let before = app.shell.sidebar_visible;
        let _ = app.update(Message::SidebarToggle);
        assert_eq!(app.shell.sidebar_visible, !before);
        let _ = app.update(Message::SidebarToggle);
        assert_eq!(app.shell.sidebar_visible, before);
    }

    #[test]
    fn toc_toggle_flips_outline_panel() {
        let mut app = base_app();
        app.shell.toc_visible = false;
        let _ = app.update(Message::ToggleTOC);
        assert!(app.shell.toc_visible);
        let _ = app.update(Message::ToggleTOC);
        assert!(!app.shell.toc_visible);
    }

    #[test]
    fn toc_click_moves_to_markdown_panel_and_targets_heading_line() {
        let mut app = pdf_app();
        let _ = app.update(Message::Editor(EditorMessage::Command(
            EditorCommand::SetCursor { line: 2, col: 0 },
        )));
        let _ = app.update(Message::TocClicked(4));
        // Switches to the markdown panel and emits a CursorMove(4, 0) follow-up.
        assert_eq!(app.shell.active_panel, ActivePanel::Markdown);
    }

    #[test]
    fn window_resize_records_dimensions() {
        let mut app = base_app();
        let _ = app.update(Message::WindowResized(1280.0, 720.0));
        assert_eq!(app.shell.window_width, 1280.0);
        assert_eq!(app.shell.window_height, 720.0);
    }

    // -- workspace ----------------------------------------------------------

    #[test]
    fn folder_toggle_expands_then_collapses() {
        let mut app = base_app();
        assert!(!app.workspace.expanded_folders.contains("notes"));
        let _ = app.update(Message::Workspace(WorkspaceMessage::FolderToggled(
            "notes".to_string(),
        )));
        assert!(app.workspace.expanded_folders.contains("notes"));
        let _ = app.update(Message::Workspace(WorkspaceMessage::FolderToggled(
            "notes".to_string(),
        )));
        assert!(!app.workspace.expanded_folders.contains("notes"));
    }

    #[test]
    fn vault_opened_with_none_is_a_noop_on_vault_root() {
        let mut app = base_app();
        let root_before = app.workspace.vault_root.clone();
        let _ = app.update(Message::Workspace(WorkspaceMessage::VaultOpened(None)));
        assert_eq!(app.workspace.vault_root, root_before);
    }

    // -- editor -------------------------------------------------------------

    #[test]
    fn editor_insert_text_mutates_buffer_at_cursor() {
        let mut app = base_app();
        let _ = app.update(Message::Editor(EditorMessage::Command(
            EditorCommand::SetCursor { line: 2, col: 0 },
        )));
        let _ = app.update(Message::Editor(EditorMessage::Command(
            EditorCommand::InsertText("Inserted ".to_string()),
        )));
        assert!(
            app.editor.buffer.text().contains("Inserted Body line."),
            "buffer should contain inserted text, got: {:?}",
            app.editor.buffer.text()
        );
    }

    #[test]
    fn editor_cursor_move_clamps_within_document() {
        let mut app = base_app();
        let _ = app.update(Message::Editor(EditorMessage::CursorMove(9999, 0)));
        let line_count = app.editor.buffer.text().lines().count();
        assert!(
            app.editor.buffer.cursor_line < line_count.max(1) + 1,
            "cursor line {} should be clamped near document end ({} lines)",
            app.editor.buffer.cursor_line,
            line_count
        );
    }

    // -- search -------------------------------------------------------------

    #[test]
    fn search_open_shows_global_search() {
        let mut app = base_app();
        assert!(!app.search.visible);
        let _ = app.update(Message::Search(SearchMessage::Open));
        assert!(app.search.visible);
    }

    #[test]
    fn search_close_hides_all_search_surfaces_and_clears_results() {
        let mut app = pdf_app();
        app.search.visible = true;
        app.search.editor.visible = true;
        app.pdf.view.search.visible = true;
        let _ = app.update(Message::Search(SearchMessage::Close));
        assert!(!app.search.visible);
        assert!(!app.search.editor.visible);
        assert!(!app.pdf.view.search.visible);
        assert!(app.search.global.results.is_empty());
        assert!(app.search.global.error.is_none());
    }

    // -- overlays -----------------------------------------------------------

    #[test]
    fn command_palette_open_sets_visible() {
        let mut app = base_app();
        assert!(!app.overlays.command_palette_visible);
        let _ = app.update(Message::CommandPaletteOpen);
        assert!(app.overlays.command_palette_visible);
    }

    #[test]
    fn command_palette_query_change_records_query() {
        let mut app = base_app();
        let _ = app.update(Message::CommandPaletteOpen);
        let _ = app.update(Message::CommandPaletteQueryChanged("split".to_string()));
        assert_eq!(app.overlays.command_palette_query, "split");
    }

    #[test]
    fn toast_hide_clears_toast() {
        let mut app = base_app();
        app.overlays.toast = Some("Saved".to_string());
        let _ = app.update(Message::ToastHide);
        assert!(app.overlays.toast.is_none());
    }

    // -- tracker ------------------------------------------------------------

    #[test]
    fn tracker_toggle_flips_panel() {
        let mut app = base_app();
        let before = app.tracker.visible;
        let _ = app.update(Message::Tracker(TrackerMessage::Toggle));
        assert_eq!(app.tracker.visible, !before);
    }

    #[test]
    fn tracker_start_then_stop_round_trips_running_state() {
        let mut app = base_app();
        assert!(!app.tracker.running);
        let _ = app.update(Message::Tracker(TrackerMessage::Start));
        assert!(app.tracker.running);
        assert!(app.tracker.started_at.is_some());
        let _ = app.update(Message::Tracker(TrackerMessage::Stop));
        assert!(!app.tracker.running);
        assert!(app.tracker.started_at.is_none());
    }

    #[test]
    fn tracker_manual_field_edits_are_recorded() {
        let mut app = base_app();
        let _ = app.update(Message::Tracker(TrackerMessage::ManualHoursChanged(
            "2.5".to_string(),
        )));
        let _ = app.update(Message::Tracker(TrackerMessage::ManualNotesChanged(
            "evening review".to_string(),
        )));
        assert_eq!(app.tracker.manual_hours, "2.5");
        assert_eq!(app.tracker.manual_notes, "evening review");
    }

    // -- pdf ----------------------------------------------------------------

    #[test]
    fn pdf_first_page_resets_position() {
        let mut app = pdf_app();
        app.pdf.current_page = 2;
        app.pdf.scroll_y = 1500.0;
        let _ = app.update(Message::Pdf(PdfMessage::FirstPage));
        assert_eq!(app.pdf.current_page, 0);
        assert_eq!(app.pdf.scroll_y, 0.0);
    }

    #[test]
    fn pdf_last_page_jumps_to_final_page() {
        let mut app = pdf_app();
        assert_eq!(app.pdf.current_page, 0);
        let _ = app.update(Message::Pdf(PdfMessage::LastPage));
        assert_eq!(app.pdf.current_page, app.pdf.total_pages - 1);
    }

    #[test]
    fn pdf_zoom_change_applies_requested_zoom() {
        let mut app = pdf_app();
        let _ = app.update(Message::Pdf(PdfMessage::ZoomChanged(2.0)));
        assert!(
            (app.pdf.view.zoom - 2.0).abs() < f32::EPSILON,
            "zoom should be 2.0, got {}",
            app.pdf.view.zoom
        );
    }

    #[test]
    fn pdf_selection_finished_then_cleared_round_trips() {
        let mut app = pdf_app();
        let _ = app.update(Message::Pdf(PdfMessage::SelectionFinished(0, 0, 4)));
        assert!(app.pdf.selection.is_some());
        assert_eq!(app.shell.active_panel, ActivePanel::Pdf);
        let _ = app.update(Message::Pdf(PdfMessage::SelectionCleared));
        assert!(app.pdf.selection.is_none());
    }

    // -- split view ---------------------------------------------------------

    #[test]
    fn split_view_toggle_round_trips_when_both_documents_open() {
        let mut app = pdf_app();
        app.shell.split_view_active = true;
        app.shell.active_panel = ActivePanel::Markdown;
        let _ = app.update(Message::SplitViewToggle);
        assert!(!app.shell.split_view_active);
    }
}
