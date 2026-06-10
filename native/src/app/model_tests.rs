#[cfg(test)]
use crate::editor::{buffer::DocBuffer, parser};

#[cfg(test)]
pub(crate) use crate::features::pdf::navigation::NavigationHistory;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::*;
    use crate::app_shell::{AppShellMode, AppShellPane, WorkflowSidebarTab};
    use crate::editor::buffer::EditorCommand;
    use crate::features::pdf::view_model::PdfLayout;
    use crate::messages::*;
    use md_editor_core::domain::pdf::{
        PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfAnnotationStatus,
    };
    use md_editor_core::domain::FileEntry;
    use std::sync::Arc;

    use crate::views;
    use crate::views::modals::ModalType;
    use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};
    use iced::Theme;

    fn text_layout_bounds(
        ui: &mut iced_test::Simulator<'_, Message, Theme, iced::Renderer>,
        text: &str,
    ) -> iced::Rectangle {
        ui.find(text)
            .unwrap_or_else(|_| panic!("{text:?} should render"))
            .bounds()
    }

    fn rectangles_overlap(a: iced::Rectangle, b: iced::Rectangle) -> bool {
        let a_right = a.x + a.width;
        let b_right = b.x + b.width;
        let a_bottom = a.y + a.height;
        let b_bottom = b.y + b.height;

        a.x < b_right && b.x < a_right && a.y < b_bottom && b.y < a_bottom
    }

    fn assert_no_text_overlap(
        ui: &mut iced_test::Simulator<'_, Message, Theme, iced::Renderer>,
        first: &str,
        second: &str,
    ) {
        let first_bounds = text_layout_bounds(ui, first);
        let second_bounds = text_layout_bounds(ui, second);

        assert!(
            !rectangles_overlap(first_bounds, second_bounds),
            "{first:?} at {first_bounds:?} should not overlap {second:?} at {second_bounds:?}"
        );
    }

    fn pdf_text_batch(
        results: Vec<md_editor_core::domain::UnifiedSearchResult>,
        searched_documents: usize,
        total_candidates: usize,
        result_cap_reached: bool,
        document_cap_reached: bool,
    ) -> md_editor_core::domain::UnifiedPdfTextSearchResultBatch {
        md_editor_core::domain::UnifiedPdfTextSearchResultBatch {
            results,
            searched_documents,
            total_candidates,
            result_cap_reached,
            document_cap_reached,
        }
    }

    fn app_without_vault() -> MdEditor {
        let mut app = MdEditor::new().0;
        app.state = Arc::new(
            md_editor_core::state::AppState::try_new_in_memory()
                .expect("in-memory application state should initialize"),
        );
        app.workspace.vault_root = None;
        app.workspace.vault_entries.clear();
        app.workspace.selected_path = None;
        app.workspace.active_path = None;
        app.pdf.active_path = None;
        app.editor.active_image_path = None;
        app.editor.active_image = None;
        app.pdf.showing_pdf = false;
        app.shell.split_view_active = false;
        app.workspace.navigation_history = NavigationHistory::default();
        app
    }

    fn app_with_vault() -> MdEditor {
        let mut app = app_without_vault();
        app.shell.sidebar_visible = true;
        app.workspace.backlinks_visible = false;
        app.shell.toc_visible = false;
        app.tracker.visible = false;
        app.shell.pdf_annotations_visible = false;
        app.shell.split_view_active = false;
        app.shell.split_ratio = 0.5;
        app.shell.pdf_split_ratio = 0.3;
        app.shell.active_panel = ActivePanel::Markdown;
        app.workspace.vault_root = Some("/tmp/md-editor-ui-audit".to_string());
        app.workspace.vault_entries = vec![
            FileEntry {
                path: "notes".to_string(),
                name: "notes".to_string(),
                is_dir: true,
            },
            FileEntry {
                path: "notes/research.md".to_string(),
                name: "research.md".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "papers/paper.pdf".to_string(),
                name: "paper.pdf".to_string(),
                is_dir: false,
            },
        ];
        app
    }

    fn app_with_markdown_file() -> MdEditor {
        let mut app = app_with_vault();
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.workspace.selected_path = app.workspace.active_path.clone();
        app.editor.buffer = DocBuffer::from_text("# Research\n\nSee [[related]].\n");
        app.editor.highlighted_lines = parser::highlight_markdown(&app.editor.buffer.text());
        app.editor.toc_entries = vec![views::toc::TocEntry {
            level: 1,
            text: "Research".to_string(),
            line: 0,
        }];
        app
    }

    fn app_with_large_markdown_file() -> MdEditor {
        let mut app = app_with_markdown_file();
        let mut text = String::from("# Large Research\n\n");
        for line in 0..1_500 {
            text.push_str(&format!("- finding {line}\n"));
        }
        app.workspace.active_path = Some("notes/large.md".to_string());
        app.workspace.selected_path = app.workspace.active_path.clone();
        app.editor.buffer = DocBuffer::from_text(&text);
        app.editor.highlighted_lines = parser::highlight_markdown(&app.editor.buffer.text());
        app.editor.toc_entries = views::toc::get_toc(&app.editor.highlighted_lines);
        app
    }

    fn app_with_pdf_file() -> MdEditor {
        let mut app = app_with_vault();
        app.pdf.active_path = Some("papers/paper.pdf".to_string());
        app.workspace.selected_path = app.pdf.active_path.clone();
        app.pdf.showing_pdf = true;
        app.pdf.total_pages = 3;
        app.pdf.current_page = 0;
        app.pdf.pages = vec![None, None, None];
        app.pdf.dimensions = vec![None, None, None];
        app.pdf.view.page_sizes = vec![Some((612.0, 792.0)); 3];
        app.pdf.placeholder_page_size = Some((612.0, 792.0));
        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf.placeholder_page_size.unwrap_or((612.0, 792.0)),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );
        app.shell.active_panel = ActivePanel::Pdf;
        app
    }

    fn app_with_split_research() -> MdEditor {
        let mut app = app_with_pdf_file();
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.editor.buffer =
            DocBuffer::from_text("# Research\n\n[p. 1](pdf://papers/paper.pdf?page=1)\n");
        app.editor.highlighted_lines = parser::highlight_markdown(&app.editor.buffer.text());
        app.shell.split_view_active = true;
        app.shell.active_panel = ActivePanel::Markdown;
        app
    }

    fn app_with_global_search() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.search.visible = true;
        app.search.editor.query = "missing".to_string();
        app.search.global.searching = false;
        app.search.global.results.clear();
        app
    }

    fn app_with_file_search() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.search.editor.visible = true;
        app.search.editor.query = "finding".to_string();
        app.search.editor.matches.clear();
        app.search.editor.active_index = None;
        app
    }

    fn app_with_pdf_search() -> MdEditor {
        let mut app = app_with_pdf_file();
        app.pdf.view.search.visible = true;
        app.pdf.view.search.query = "finding".to_string();
        app
    }

    fn app_with_command_palette() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.overlays.command_palette_visible = true;
        app.overlays.command_palette_query = "navigate".to_string();
        app
    }

    fn app_with_active_modal() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.overlays.active_modal = Some(ModalType::CreateFile);
        app.overlays.modal_input = "new-note.md".to_string();
        app
    }

    fn app_with_annotation_heavy_pdf() -> MdEditor {
        let mut app = app_with_pdf_file();
        app.shell.pdf_annotations_visible = true;
        let annotations = (0..12)
            .map(|index| PdfAnnotation {
                id: format!("ann-{index}"),
                document_id: "doc".to_string(),
                page_index: index % 3,
                kind: PdfAnnotationKind::Highlight,
                color: PdfAnnotationColor::Yellow,
                selected_text: format!("Important quote {index}"),
                ranges: Vec::new(),
                rects: Vec::new(),
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: vec!["review".to_string()],
                status: PdfAnnotationStatus::Unresolved,
                created_at: index as i64,
                updated_at: index as i64,
            })
            .collect::<Vec<_>>();
        for annotation in annotations {
            app.pdf
                .annotations
                .entry(annotation.page_index)
                .or_default()
                .push(annotation);
        }
        app
    }

    #[test]
    fn test_slugify_and_find_heading_line() {
        assert_eq!(slugify("Equation 1"), "equation-1");
        assert_eq!(slugify("Header: Equation 1"), "header-equation-1");
        assert_eq!(slugify("**Bold Heading**"), "bold-heading");

        let text = "# Equation 1\nSome text\n## Header: Equation 1\nMore text\n# **Bold Heading**";
        assert_eq!(find_heading_line(text, "equation-1"), Some(0));
        assert_eq!(find_heading_line(text, "header-equation-1"), Some(2));
        assert_eq!(find_heading_line(text, "bold-heading"), Some(4));
        assert_eq!(find_heading_line(text, "not-existent"), None);
    }

    #[test]
    fn indexable_markdown_link_target_filters_external_links() {
        let wiki = markdown_link("notes/topic", parser::MarkdownLinkKind::Wiki);
        let local_inline = markdown_link("../paper.md#section", parser::MarkdownLinkKind::Inline);
        let external = markdown_link("https://example.com", parser::MarkdownLinkKind::Inline);
        let pdf = markdown_link(
            "pdf://papers/a.pdf?page=2",
            parser::MarkdownLinkKind::Inline,
        );
        let reference = markdown_link("ref-id", parser::MarkdownLinkKind::Reference);
        let anchor = markdown_link("#local", parser::MarkdownLinkKind::Wiki);
        let resolved_reference =
            markdown_link("papers/b.pdf", parser::MarkdownLinkKind::ResolvedReference);

        assert_eq!(
            indexable_markdown_link_target(&wiki).as_deref(),
            Some("notes/topic")
        );
        assert_eq!(
            indexable_markdown_link_target(&local_inline).as_deref(),
            Some("../paper.md#section")
        );
        assert!(indexable_markdown_link_target(&external).is_none());
        assert_eq!(
            indexable_markdown_link_target(&pdf).as_deref(),
            Some("papers/a.pdf")
        );
        assert!(indexable_markdown_link_target(&reference).is_none());
        assert!(indexable_markdown_link_target(&anchor).is_none());
        assert_eq!(
            indexable_markdown_link_target(&resolved_reference).as_deref(),
            Some("papers/b.pdf")
        );
    }

    fn markdown_link(target: &str, kind: parser::MarkdownLinkKind) -> parser::MarkdownLinkEntry {
        parser::MarkdownLinkEntry {
            line: 0,
            target: target.to_string(),
            display_text: target.to_string(),
            source_text: target.to_string(),
            kind,
        }
    }

    #[test]
    fn ui_audit_fixture_no_vault_renders_welcome() {
        let app = app_without_vault();
        let mut ui = iced_test::simulator(app.view());

        ui.find("Open Vault")
            .expect("no-vault fixture should render vault opener");
        ui.find("Ctrl+O")
            .expect("no-vault fixture should expose keyboard path");
    }

    #[test]
    fn ui_audit_fixture_markdown_file_renders_shell_and_editor() {
        let app = app_with_markdown_file();
        let mut ui = iced_test::simulator(app.view());

        // Toolbar now shows basename only (B1: full path removed, "• Saved" removed)
        ui.find("research.md")
            .expect("markdown fixture should render active basename");
    }

    #[test]
    fn ui_audit_fixture_pdf_file_renders_pdf_toolbar() {
        let app = app_with_pdf_file();
        let mut ui = iced_test::simulator(app.view());

        // Toolbar shows basename only (B1). Page still visible in PDF toolbar (B5).
        ui.find("paper.pdf")
            .expect("PDF fixture should render active PDF basename");
        ui.find("1 / 3")
            .expect("PDF fixture should render page status with 1-based label");
    }

    #[test]
    fn ui_audit_fixture_split_research_renders_both_active_paths() {
        let app = app_with_split_research();
        let mut ui = iced_test::simulator(app.view());

        // Toolbar shows active-pane basename; PDF controls always visible when PDF open.
        ui.find("research.md")
            .expect("split fixture should keep markdown basename visible");
        ui.find("1 / 3")
            .expect("split fixture should keep PDF controls visible");
    }

    #[test]
    fn ui_audit_fixture_overlays_and_sidebars_render_stable_states() {
        let search_app = app_with_global_search();
        let mut search_ui = iced_test::simulator(search_app.view());
        search_ui
            .find("No results found")
            .expect("global-search fixture should render empty state");

        let command_app = app_with_command_palette();
        let mut command_ui = iced_test::simulator(command_app.view());
        command_ui
            .find("Navigate Back")
            .expect("command-palette fixture should render filtered command");

        let modal_app = app_with_active_modal();
        let mut modal_ui = iced_test::simulator(modal_app.view());
        modal_ui
            .find("Create New File")
            .expect("modal fixture should render create action");

        let annotation_app = app_with_annotation_heavy_pdf();
        let mut annotation_ui = iced_test::simulator(annotation_app.view());
        annotation_ui
            .find("\"Important quote 0\"")
            .expect("annotation-heavy fixture should render annotation row");
        annotation_ui
            .find("#review")
            .expect("annotation-heavy fixture should render tag metadata");
    }

    #[test]
    fn ui_audit_fixture_large_and_narrow_states_render_stable_shell() {
        let large_app = app_with_large_markdown_file();
        let mut large_ui = iced_test::simulator(large_app.view());
        large_ui
            .find("large.md")
            .expect("large markdown fixture should render active basename");

        let search_app = app_with_file_search();
        let mut search_ui = iced_test::simulator(search_app.view());
        search_ui
            .find("No matches")
            .expect("file-search fixture should render no-result state");

        let narrow_app = app_with_split_research();
        let mut narrow_ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(420.0, 720.0),
            narrow_app.view(),
        );
        narrow_ui
            .find("research.md")
            .expect("narrow split fixture should preserve markdown basename");
        narrow_ui
            .find("1 / 3")
            .expect("narrow split fixture should preserve PDF page status");
    }

    #[test]
    fn ui_audit_keyboard_shortcuts_expose_baseline_accessibility_paths() {
        let mut app = app_with_markdown_file();

        let _ = app.update(Message::KeyboardShortcut(Shortcut::CommandPalette));
        assert!(app.overlays.command_palette_visible);
        assert!(!app.overlays.citation_palette_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::CitationPalette));
        assert!(app.overlays.citation_palette_visible);
        assert!(!app.overlays.command_palette_visible);
        assert!(!app.search.visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(!app.overlays.citation_palette_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Search));
        assert!(app.search.editor.visible);
        assert!(!app.search.visible);
        assert!(!app.pdf.view.search.visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::TableOfContents));
        assert!(app.shell.toc_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::FocusMode));
        assert!(!app.shell.sidebar_visible);
        assert!(!app.workspace.backlinks_visible);
        assert!(!app.shell.toc_visible);
        assert!(!app.tracker.visible);
    }

    #[test]
    fn test_active_pane_shortcut_routing_and_switching() {
        let mut app = app_with_markdown_file();
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.pdf.active_path = Some("vault/paper.pdf".to_string());
        app.shell.split_view_active = true;
        app.shell.active_panel = ActivePanel::Markdown;

        // Switch to PDF pane
        let _ = app.update(Message::KeyboardShortcut(Shortcut::SwitchPane));
        assert_eq!(app.shell.active_panel, ActivePanel::Pdf);

        // Search is routed to PDF search
        let _ = app.update(Message::KeyboardShortcut(Shortcut::Search));
        assert!(app.pdf.view.search.visible);
        assert!(!app.search.editor.visible);

        // Switch back to Markdown pane
        let _ = app.update(Message::KeyboardShortcut(Shortcut::SwitchPane));
        assert_eq!(app.shell.active_panel, ActivePanel::Markdown);

        // Search is routed to Markdown/editor search
        app.pdf.view.search.visible = false;
        let _ = app.update(Message::KeyboardShortcut(Shortcut::Search));
        assert!(app.search.editor.visible);
        assert!(!app.pdf.view.search.visible);
    }

    #[test]
    fn ui_audit_focus_targets_map_to_rendered_input_ids() {
        assert_eq!(
            FocusTarget::FileSearch.widget_id(),
            views::search::FILE_SEARCH_INPUT_ID
        );
        assert_eq!(
            FocusTarget::GlobalSearch.widget_id(),
            views::search::GLOBAL_SEARCH_INPUT_ID
        );
        assert_eq!(
            FocusTarget::PdfSearch.widget_id(),
            views::pdf_viewer::PDF_SEARCH_INPUT_ID
        );
        assert_eq!(
            FocusTarget::CommandPalette.widget_id(),
            views::command_palette::COMMAND_PALETTE_INPUT_ID
        );
        assert_eq!(
            FocusTarget::CitationPalette.widget_id(),
            views::citation_palette::CITATION_PALETTE_INPUT_ID
        );

        let mut command_app = app_with_markdown_file();
        let _ = command_app.update(Message::CommandPaletteOpen);
        let mut command_ui = iced_test::simulator(command_app.view());
        command_ui
            .find(iced_test::selector::id(
                FocusTarget::CommandPalette.widget_id(),
            ))
            .expect("command palette shortcut target should exist when open");

        let mut citation_app = app_with_annotation_heavy_pdf();
        let _ = citation_app.update(Message::CitationPaletteToggle);
        let mut citation_ui = iced_test::simulator(citation_app.view());
        citation_ui
            .find(iced_test::selector::id(
                FocusTarget::CitationPalette.widget_id(),
            ))
            .expect("citation palette shortcut target should exist when open");

        let file_search_app = app_with_file_search();
        let mut file_search_ui = iced_test::simulator(file_search_app.view());
        file_search_ui
            .find(iced_test::selector::id(FocusTarget::FileSearch.widget_id()))
            .expect("file search target should exist when open");

        let global_search_app = app_with_global_search();
        let mut global_search_ui = iced_test::simulator(global_search_app.view());
        global_search_ui
            .find(iced_test::selector::id(
                FocusTarget::GlobalSearch.widget_id(),
            ))
            .expect("global search target should exist when open");

        let pdf_search_app = app_with_pdf_search();
        let mut pdf_search_ui = iced_test::simulator(pdf_search_app.view());
        pdf_search_ui
            .find(iced_test::selector::id(FocusTarget::PdfSearch.widget_id()))
            .expect("PDF search target should exist when open");
    }

    #[test]
    fn ui_audit_escape_closes_modal_before_background_overlays() {
        let mut app = app_with_active_modal();
        app.search.visible = true;
        app.overlays.command_palette_visible = true;

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(app.overlays.active_modal.is_none());
        assert!(app.search.visible);
        assert!(app.overlays.command_palette_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(!app.search.visible);
        assert!(app.overlays.command_palette_visible);
    }

    #[test]
    fn ui_audit_shell_labels_do_not_overlap_in_baseline_layouts() {
        // Toolbar now shows basename + no "• Saved" text (B1).
        // Verify page status and filename do not overlap in PDF and split layouts.
        let pdf_app = app_with_pdf_file();
        let mut pdf_ui = iced_test::simulator(pdf_app.view());
        assert_no_text_overlap(&mut pdf_ui, "paper.pdf", "1 / 3");

        let narrow_app = app_with_split_research();
        let mut narrow_ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(420.0, 720.0),
            narrow_app.view(),
        );
        assert_no_text_overlap(&mut narrow_ui, "research.md", "1 / 3");
    }

    #[test]
    fn app_shell_state_matches_ui_audit_fixtures() {
        let no_vault = app_without_vault().app_shell_state();
        assert_eq!(no_vault.mode, AppShellMode::NoVault);
        assert_eq!(no_vault.active_pane, AppShellPane::None);

        let markdown = app_with_markdown_file().app_shell_state();
        assert_eq!(markdown.mode, AppShellMode::EditorOnly);
        assert_eq!(markdown.active_pane, AppShellPane::Markdown);

        let pdf = app_with_pdf_file().app_shell_state();
        assert_eq!(pdf.mode, AppShellMode::PdfOnly);
        assert_eq!(pdf.active_pane, AppShellPane::Pdf);

        let split = app_with_split_research().app_shell_state();
        assert_eq!(split.mode, AppShellMode::SplitResearch);
        assert_eq!(split.active_pane, AppShellPane::Markdown);
        assert!(split.uses_split_research_layout());
        assert!(
            split
                .command_groups()
                .contains(&crate::app_shell::CommandGroup::Research)
        );
        assert!(
            split
                .command_groups()
                .contains(&crate::app_shell::CommandGroup::Annotation)
        );

        let search = app_with_global_search().app_shell_state();
        assert_eq!(search.mode, AppShellMode::SearchHeavy);
        assert_eq!(search.active_pane, AppShellPane::Markdown);
    }

    #[test]
    fn app_shell_status_matches_document_and_pdf_state() {
        let mut app = app_with_split_research();
        app.editor.buffer.dirty = true;
        app.pdf.current_page = 1;
        app.pdf.total_pages = 3;
        app.pdf.view.zoom = 1.5;
        app.search.global.searching = true;
        app.search.global.pdf_status = Some("Searched 2 PDFs".to_string());

        let shell_state = app.app_shell_state();
        let status = app.app_shell_status(shell_state);

        assert_eq!(status.save_status, crate::app_shell::SaveStatus::Unsaved);
        assert_eq!(status.search_status.as_deref(), Some("Searched 2 PDFs"));
        // pdf_status removed from status bar — page/zoom live in PDF toolbar only.
        assert_eq!(status.active_pane, AppShellPane::Markdown);
    }

    #[test]
    fn app_shell_status_surfaces_toast_before_background_error() {
        let mut app = app_with_pdf_file();
        app.overlays.toast = Some("Linked note created".to_string());
        app.search.pdf_error = Some("PDF search failed".to_string());

        let status = app.app_shell_status(app.app_shell_state());

        assert_eq!(status.save_status, crate::app_shell::SaveStatus::Saved);
        assert_eq!(status.message.as_deref(), Some("Linked note created"));
    }

    #[test]
    fn app_shell_status_bar_active_pane_indicators_render() {
        let app_md = app_with_markdown_file();
        let mut ui_md = iced_test::simulator(app_md.view());
        ui_md
            .find("EDITOR")
            .expect("should render EDITOR active pane indicator");

        let app_pdf = app_with_pdf_file();
        let mut ui_pdf = iced_test::simulator(app_pdf.view());
        ui_pdf
            .find("PDF")
            .expect("should render PDF active pane indicator");
    }

    #[test]
    fn app_shell_persistence_reflects_visible_panels_and_window_width() {
        let mut app = app_with_split_research();
        app.workspace.backlinks_visible = true;
        app.shell.window_width = 900.0;
        let wide = app.app_shell_state();
        assert!(!wide.persistence.sidebar_collapsed);
        assert!(!wide.persistence.reference_collapsed);
        assert!(!wide.persistence.workflow_collapsed);
        assert_eq!(
            wide.persistence.active_workflow_tab,
            WorkflowSidebarTab::Backlinks
        );

        app.shell.window_width = 600.0;
        let narrow = app.app_shell_state();
        assert!(!narrow.persistence.sidebar_collapsed);
        assert!(narrow.persistence.reference_collapsed);
        assert!(narrow.persistence.workflow_collapsed);
    }

    #[test]
    fn app_shell_persistence_round_trips_through_config() {
        let mut app = app_with_split_research();
        app.shell.sidebar_visible = false;
        app.workspace.backlinks_visible = false;
        app.shell.toc_visible = true;
        app.tracker.visible = false;
        app.shell.pdf_annotations_visible = false;
        app.shell.split_ratio = 0.62;
        app.shell.pdf_split_ratio = 0.4;
        app.set_active_panel(ActivePanel::Pdf);
        app.persist_shell_state();

        let saved =
            md_editor_core::config::get_sys_config(&app.state, APP_SHELL_PERSISTENCE_CONFIG_KEY)
                .unwrap()
                .expect("shell persistence should be written");
        assert!(saved.contains("active_workflow_tab=outline"));
        assert!(saved.contains("last_focused_pane=pdf"));

        app.shell.sidebar_visible = true;
        app.shell.toc_visible = false;
        app.shell.split_ratio = 0.5;
        app.shell.pdf_split_ratio = 0.3;
        app.shell.active_panel = ActivePanel::Markdown;
        app.load_shell_persistence();

        assert!(!app.shell.sidebar_visible);
        assert!(app.shell.toc_visible);
        assert_eq!(app.shell.active_panel, ActivePanel::Pdf);
        assert!((app.shell.split_ratio - 0.62).abs() < f32::EPSILON);
        assert!((app.shell.pdf_split_ratio - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn save_markdown_file_with_parser_targets_indexes_local_links() {
        let root = unique_temp_dir("native_parser_save");
        std::fs::create_dir_all(&root).unwrap();
        let state = md_editor_core::state::AppState::try_new_in_memory()
            .expect("in-memory application state should initialize");
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();

        save_markdown_file_with_parser_targets(
            &state,
            "source.md",
            "See [[wiki-target]], [inline](inline-target), [pdf](pdf://papers/a.pdf?page=2), and [web](https://example.com).",
        )
        .unwrap();

        let wiki_backlinks =
            md_editor_core::vault::get_backlinks(&state, "wiki-target.md").unwrap();
        assert!(
            wiki_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "wiki link should be indexed: {wiki_backlinks:?}"
        );
        let inline_backlinks =
            md_editor_core::vault::get_backlinks(&state, "inline-target.md").unwrap();
        assert!(
            inline_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "local inline link should be indexed: {inline_backlinks:?}"
        );
        let pdf_backlinks = md_editor_core::vault::get_backlinks(&state, "papers/a.pdf").unwrap();
        assert!(
            pdf_backlinks.iter().any(|path| path.ends_with("source.md")),
            "pdf link should be indexed against vault PDF path: {pdf_backlinks:?}"
        );
        let external_backlinks =
            md_editor_core::vault::get_backlinks(&state, "https://example.com.md").unwrap();
        assert!(
            external_backlinks.is_empty(),
            "external URL should not be indexed: {external_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_markdown_file_with_reference_links_indexes_resolved_targets() {
        let root = unique_temp_dir("native_reference_save");
        std::fs::create_dir_all(&root).unwrap();
        let state = md_editor_core::state::AppState::try_new_in_memory()
            .expect("in-memory application state should initialize");
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();

        save_markdown_file_with_parser_targets(
            &state,
            "source.md",
            "See [my text][ref1] and [shortcut_ref] and [unresolved_ref].\n\n[ref1]: papers/ref-target.pdf\n[shortcut_ref]: <another_note.md>",
        )
        .unwrap();

        let ref1_backlinks =
            md_editor_core::vault::get_backlinks(&state, "papers/ref-target.pdf").unwrap();
        assert!(
            ref1_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "reference pdf link should be indexed: {ref1_backlinks:?}"
        );

        let shortcut_backlinks =
            md_editor_core::vault::get_backlinks(&state, "another_note.md").unwrap();
        assert!(
            shortcut_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "shortcut reference link should be indexed: {shortcut_backlinks:?}"
        );

        let unresolved_backlinks =
            md_editor_core::vault::get_backlinks(&state, "unresolved_ref.md").unwrap();
        assert!(
            unresolved_backlinks.is_empty(),
            "unresolved reference ID should not be indexed: {unresolved_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reindex_vault_with_parser_targets_replaces_regex_backlinks() {
        let root = unique_temp_dir("native_parser_reindex");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("source.md"),
            "```md\n[[ignored-code-link]]\n```\nSee [inline](inline-target).",
        )
        .unwrap();

        let state = md_editor_core::state::AppState::try_new_in_memory()
            .expect("in-memory application state should initialize");
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();
        let regex_backlinks =
            md_editor_core::vault::get_backlinks(&state, "ignored-code-link.md").unwrap();
        assert!(
            regex_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "core fallback should see raw wiki text before native parser reindex"
        );

        reindex_vault_with_parser_targets(&state, &root).unwrap();

        let ignored_backlinks =
            md_editor_core::vault::get_backlinks(&state, "ignored-code-link.md").unwrap();
        assert!(
            ignored_backlinks.is_empty(),
            "parser reindex should drop links inside code blocks: {ignored_backlinks:?}"
        );
        let inline_backlinks =
            md_editor_core::vault::get_backlinks(&state, "inline-target.md").unwrap();
        assert!(
            inline_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "parser reindex should add local inline links: {inline_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reindex_markdown_file_with_parser_targets_updates_opened_file() {
        let root = unique_temp_dir("native_parser_open_file");
        std::fs::create_dir_all(&root).unwrap();
        let state = md_editor_core::state::AppState::try_new_in_memory()
            .expect("in-memory application state should initialize");
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();
        md_editor_core::vault::save_file(&state, "source.md", "See [[old-target]].").unwrap();

        reindex_markdown_file_with_parser_targets(
            &state,
            "source.md",
            "```md\n[[old-target]]\n```\nSee [new](new-target).",
        )
        .unwrap();

        let old_backlinks = md_editor_core::vault::get_backlinks(&state, "old-target.md").unwrap();
        assert!(
            old_backlinks.is_empty(),
            "parser reindex should remove stale/code-block links: {old_backlinks:?}"
        );
        let new_backlinks = md_editor_core::vault::get_backlinks(&state, "new-target.md").unwrap();
        assert!(
            new_backlinks.iter().any(|path| path.ends_with("source.md")),
            "parser reindex should add current local inline links: {new_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("md_editor_{name}_{nanos}"))
    }

    #[test]
    fn test_resolve_relative_link_path() {
        assert_eq!(
            resolve_relative_link_path(None, Some("notes/math.md"), "../science/chemistry"),
            "science/chemistry"
        );
        assert_eq!(
            resolve_relative_link_path(None, Some("notes/math.md"), "./geometry"),
            "notes/geometry"
        );
        assert_eq!(
            resolve_relative_link_path(None, None, "../science/chemistry"),
            "../science/chemistry"
        );
        assert_eq!(
            resolve_relative_link_path(None, Some("math.md"), "./geometry"),
            "geometry"
        );
    }

    #[test]
    fn test_resolve_relative_link_path_with_vault() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let target_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("test_vault_{}", unique_id));
        let sub_dir = target_dir.join("subdir");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let target_file = sub_dir.join("another_file.md");
        std::fs::write(&target_file, "content").unwrap();

        let vault_root = target_dir.to_str().unwrap();
        let active_path = "subdir/active.md";

        let resolved =
            resolve_relative_link_path(Some(vault_root), Some(active_path), "another_file");
        assert_eq!(resolved, "subdir/another_file");

        let _ = std::fs::remove_dir_all(&target_dir);
    }

    #[test]
    fn test_find_heading_or_widget_line() {
        let text = "Line 0\n$$E = mc^2$$ \\label{equation-1}\nLine 2\n<div id=\"figure-1\">\nLine 4\n$$E = h\\nu$$ { #equation-2 }";
        let highlighted = parser::highlight_markdown(text);
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "equation-1"),
            Some(1)
        );
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "figure-1"),
            Some(3)
        );
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "equation-2"),
            Some(5)
        );
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "not-existent"),
            None
        );

        // Also test the dynamic numbering of figures and math equations
        let dynamic_text = "Here is an image:\n![Alt](image.png)\nAnd a math block:\n$$\nE = mc^2\n$$\nAnother image:\n![Alt2](pic.png)";
        let dyn_highlighted = parser::highlight_markdown(dynamic_text);
        assert_eq!(
            find_heading_or_widget_line(dynamic_text, &dyn_highlighted, "figure-1"),
            Some(1)
        );
        assert_eq!(
            find_heading_or_widget_line(dynamic_text, &dyn_highlighted, "equation-1"),
            Some(3)
        );
        assert_eq!(
            find_heading_or_widget_line(dynamic_text, &dyn_highlighted, "figure-2"),
            Some(7)
        );
    }

    #[test]
    fn insert_text_keeps_cursor_visible_after_enter_at_eof() {
        assert!(EditorCommand::should_keep_cursor_visible(
            &EditorCommand::InsertText("\n".to_string())
        ));
    }

    #[test]
    fn pdf_slot_offsets_use_fixed_placeholder_stride() {
        let slot_height = 792.0;
        let target_page = 250;

        let offset = pdf_slot_offset(target_page, slot_height);

        assert_eq!(
            offset,
            PDF_PAGE_LIST_PADDING + f32::from(target_page) * (slot_height + PDF_PAGE_SPACING)
        );
        assert_eq!(
            pdf_slot_page_at_scroll(offset, 500, slot_height),
            target_page
        );
    }

    #[test]
    fn pdf_slot_page_lookup_does_not_drift_to_later_pages() {
        let slot_height = 792.0;
        let target_page = 250;
        let offset = pdf_slot_offset(target_page, slot_height);

        assert_eq!(pdf_slot_page_at_scroll(offset, 500, slot_height), 250);
        assert_ne!(pdf_slot_page_at_scroll(offset, 500, slot_height), 400);
    }

    #[test]
    fn pdf_total_height_reserves_space_for_every_blank_page() {
        let total_pages = 500;
        let slot_height = 792.0;

        assert_eq!(
            pdf_slot_total_height(total_pages, slot_height),
            PDF_PAGE_LIST_PADDING + f32::from(total_pages) * (slot_height + PDF_PAGE_SPACING)
        );
    }

    #[test]
    fn pdf_search_scroll_targets_match_rect_not_just_page_top() {
        assert_eq!(
            pdf_search_match_scroll_y_from(1000.0, Some(250.0), 20.0, 792.0, 2.0, 5000.0),
            1948.0
        );
        assert_eq!(
            pdf_search_match_scroll_y_from(20.0, Some(780.0), 10.0, 792.0, 1.0, 5000.0),
            0.0
        );
    }

    #[test]
    fn pdf_placeholder_size_scales_with_zoom() {
        assert_eq!(
            pdf_placeholder_display_size_from(Some((612.0, 792.0)), None, None, 2.0),
            (1224.0, 1584.0)
        );
    }

    #[test]
    fn pdf_placeholder_prefers_first_page_size_over_rendered_dimensions() {
        assert_eq!(
            pdf_placeholder_display_size_from(
                Some((612.0, 792.0)),
                Some((300.0, 300.0)),
                Some((5000, 5000)),
                1.5,
            ),
            (918.0, 1188.0)
        );
    }

    #[test]
    fn pdf_text_lru_keeps_fifty_pages() {
        let mut app = MdEditor::new().0;
        app.pdf.render_generation = 7;

        for page in 0..60 {
            let page_text = md_editor_core::domain::pdf::PdfPageText {
                page_index: page,
                page_width: 612.0,
                page_height: 792.0,
                text: format!("page {page}"),
                chars: Vec::new(),
                lines: Vec::new(),
            };
            let _ = app.update(Message::Pdf(PdfMessage::PageTextLoaded(
                7,
                page,
                Ok(page_text),
            )));
        }

        assert_eq!(app.pdf.text_lru.len(), PDF_TEXT_PAGE_CACHE_LIMIT);
        assert_eq!(app.pdf.page_text.len(), PDF_TEXT_PAGE_CACHE_LIMIT);
        assert!(!app.pdf.page_text.contains_key(&0));
        assert!(!app.pdf.page_text.contains_key(&9));
        assert!(app.pdf.page_text.contains_key(&10));
        assert!(app.pdf.page_text.contains_key(&59));
    }

    #[test]
    fn pdf_render_page_range_caps_accidental_large_spans() {
        let mut app = MdEditor::new().0;
        app.pdf.total_pages = 1_000;
        app.pdf.pages = vec![None; 1_000];

        let _ = app.render_pdf_page_range(0, 999);

        assert_eq!(
            app.pdf.pending_pages.len(),
            PDF_RENDER_MAX_SCHEDULED_PAGES as usize
        );
        assert!(app.pdf.pending_pages.contains(&0));
        assert!(
            app.pdf
                .pending_pages
                .contains(&(PDF_RENDER_MAX_SCHEDULED_PAGES - 1))
        );
        assert!(
            !app.pdf
                .pending_pages
                .contains(&PDF_RENDER_MAX_SCHEDULED_PAGES)
        );
    }

    #[test]
    fn pdf_viewport_render_range_uses_visible_pages_plus_small_preload() {
        let mut app = MdEditor::new().0;
        app.pdf.total_pages = 100;
        app.pdf.pages = vec![None; 100];
        app.pdf.view.page_sizes = vec![Some((100.0, 100.0)); 100];
        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        let scroll_y = app.pdf_page_offset(10);
        let _ = app.render_pdf_pages_for_viewport(scroll_y, 220.0);

        let expected = app
            .pdf
            .view
            .layout
            .visible_range(scroll_y, 220.0, PDF_RENDER_PRELOAD_PAGES);
        assert_eq!(expected, 7..15);
        assert_eq!(app.pdf.pending_pages.len(), expected.len());
        for page in expected {
            assert!(app.pdf.pending_pages.contains(&page));
        }
        assert!(!app.pdf.pending_pages.contains(&6));
        assert!(!app.pdf.pending_pages.contains(&15));
    }

    #[test]
    fn pdf_zoom_keeps_existing_pages_as_stale_placeholders() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("dummy.pdf".to_string());
        app.pdf.showing_pdf = true;
        app.pdf.total_pages = 2;
        app.pdf.view.page_sizes = vec![Some((100.0, 200.0)); 2];
        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
        app.pdf.pages = vec![Some(handle.clone()), Some(handle)];
        app.pdf.dimensions = vec![Some((100, 200)), Some((100, 200))];

        let _ = app.update(Message::Pdf(PdfMessage::ZoomChanged(2.0)));

        assert!(app.pdf.pages.iter().all(Option::is_some));
        assert!(app.pdf.stale_pages.contains(&0));
        assert!(app.pdf.stale_pages.contains(&1));
        assert_eq!(app.pdf.view.zoom, 2.0);
    }

    #[test]
    fn closing_pdf_link_preview_clears_hidden_context_menu() {
        let mut app = MdEditor::new().0;
        app.pdf.link_preview = Some(iced::widget::image::Handle::from_rgba(
            1,
            1,
            vec![255, 255, 255, 255],
        ));
        app.overlays.active_modal = Some(views::modals::ModalType::PdfContextMenu(
            views::modals::PdfContextMenuState {
                absolute_pos: iced::Point::ORIGIN,
                items: Vec::new(),
            },
        ));

        let _ = app.update(Message::Pdf(PdfMessage::CloseLinkPreview));

        assert!(app.pdf.link_preview.is_none());
        assert!(app.overlays.active_modal.is_none());
    }

    #[test]
    fn escape_closing_pdf_link_preview_clears_hidden_context_menu() {
        let mut app = MdEditor::new().0;
        app.pdf.link_preview = Some(iced::widget::image::Handle::from_rgba(
            1,
            1,
            vec![255, 255, 255, 255],
        ));
        app.overlays.active_modal = Some(views::modals::ModalType::PdfContextMenu(
            views::modals::PdfContextMenuState {
                absolute_pos: iced::Point::ORIGIN,
                items: Vec::new(),
            },
        ));

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));

        assert!(app.pdf.link_preview.is_none());
        assert!(app.overlays.active_modal.is_none());
    }

    #[test]
    fn split_view_places_pdf_before_markdown() {
        let source = include_str!("view.rs").replace("\r\n", "\n");
        let split_row = source
            .find("if shell_state.uses_split_research_layout()")
            .expect("split view branch should use app shell state");
        let pdf_pos = source[split_row..]
            .find("container(pdf_view)")
            .expect("PDF pane should exist in split row");
        let editor_pos = source[split_row..]
            .find("container(editor_view)")
            .expect("editor pane should exist in split row");

        assert!(
            pdf_pos < editor_pos,
            "split view should render PDF on the left and markdown on the right"
        );
    }

    #[test]
    fn split_view_toggle_works_from_markdown_view_with_loaded_pdf() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("note.md".to_string());
        app.pdf.active_path = Some("paper.pdf".to_string());
        app.pdf.showing_pdf = false;
        app.shell.active_panel = ActivePanel::Markdown;

        let _ = app.update(Message::SplitViewToggle);

        assert!(app.shell.split_view_active);
    }

    #[test]
    fn pdf_ctrl_scroll_zoom_clamps_and_requires_modifier() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("dummy.pdf".to_string());
        app.pdf.showing_pdf = true;
        app.pdf.view.zoom = 1.0;

        let _ = app.update(Message::Pdf(PdfMessage::WheelScrolledForZoom(0.5)));
        assert_eq!(app.pdf.view.zoom, 1.0);

        app.shell.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;
        let _ = app.update(Message::Pdf(PdfMessage::WheelScrolledForZoom(10.0)));
        assert_eq!(app.pdf.view.zoom, 1.0);

        let _ = app.update(Message::Pdf(PdfMessage::ZoomChanged(10.0)));
        assert_eq!(app.pdf.view.zoom, 4.0);
    }

    #[test]
    fn default_pdf_note_path_uses_pdf_name_page_and_annotation_prefix() {
        let ann = md_editor_core::domain::pdf::PdfAnnotation {
            id: "abcdef123456".to_string(),
            document_id: "doc".to_string(),
            page_index: 4,
            kind: md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
            selected_text: "Important field result".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            tags: Vec::new(),
            status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        };

        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("papers/My PDF File.pdf".to_string());
        assert_eq!(
            app.default_pdf_note_path(&ann),
            "pdf-notes/my-pdf-file-p5-abcdef12.md"
        );
    }

    #[test]
    fn pdf_selection_quote_link_command_targets_page() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("papers/paper.pdf".to_string());
        app.pdf.selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 2,
            anchor_idx: 0,
            focus_idx: 9,
        });
        app.pdf.page_text.insert(
            2,
            md_editor_core::domain::pdf::PdfPageText {
                page_index: 2,
                page_width: 612.0,
                page_height: 792.0,
                text: "Quoted PDF text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );

        let Some(EditorCommand::InsertPdfQuoteLink {
            selected_text,
            page_number,
            link,
        }) = app.pdf_selection_quote_link_command()
        else {
            panic!("expected PDF quote link command");
        };
        assert_eq!(selected_text, "Quoted PDF");
        assert_eq!(page_number, 3);
        assert_eq!(link, "pdf://papers/paper.pdf?page=3");
    }

    #[test]
    fn pdf_insert_annotation_link_uses_annotation_target() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("notes/current.md".to_string());
        app.pdf.active_path = Some("papers/My PDF.pdf".to_string());
        app.pdf.annotations.insert(
            4,
            vec![md_editor_core::domain::pdf::PdfAnnotation {
                id: "ann#1".to_string(),
                document_id: "doc".to_string(),
                page_index: 4,
                kind: md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
                color: md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
                selected_text: "Important highlighted text".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: Vec::new(),
                status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            }],
        );

        let _ = app.update(Message::Pdf(PdfMessage::InsertAnnotationLink(
            "ann#1".to_string(),
        )));

        assert_eq!(
            app.editor.buffer.text(),
            "[label](pdf://papers/My%20PDF.pdf?page=5&annotation=ann%231)"
        );
        assert!(app.editor.buffer.undo());
        assert_eq!(app.editor.buffer.text(), "");
    }

    #[test]
    fn command_palette_adds_pdf_insert_actions_only_when_available() {
        let mut app = MdEditor::new().0;
        assert!(!app.command_palette_commands().iter().any(|cmd| matches!(
            cmd.shortcut,
            Shortcut::InsertPdfQuote | Shortcut::InsertPdfHighlight
        )));

        app.workspace.active_path = Some("notes/current.md".to_string());
        app.pdf.active_path = Some("papers/paper.pdf".to_string());
        app.pdf.selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 2,
            anchor_idx: 0,
            focus_idx: 9,
        });
        app.pdf.page_text.insert(
            2,
            md_editor_core::domain::pdf::PdfPageText {
                page_index: 2,
                page_width: 612.0,
                page_height: 792.0,
                text: "Quoted PDF text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );
        app.pdf.focused_annotation_id = Some("ann#1".to_string());
        app.pdf.annotations.insert(
            4,
            vec![md_editor_core::domain::pdf::PdfAnnotation {
                id: "ann#1".to_string(),
                document_id: "doc".to_string(),
                page_index: 4,
                kind: md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
                color: md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
                selected_text: "Important highlighted text".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: Vec::new(),
                status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            }],
        );

        let shortcuts = app
            .command_palette_commands()
            .into_iter()
            .map(|cmd| cmd.shortcut)
            .collect::<Vec<_>>();

        assert!(shortcuts.contains(&Shortcut::InsertPdfQuote));
        assert!(shortcuts.contains(&Shortcut::InsertPdfHighlight));
    }

    #[test]
    fn pdf_quote_insert_requires_markdown_file() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("papers/paper.pdf".to_string());
        app.pdf.selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 2,
            anchor_idx: 0,
            focus_idx: 9,
        });
        app.pdf.page_text.insert(
            2,
            md_editor_core::domain::pdf::PdfPageText {
                page_index: 2,
                page_width: 612.0,
                page_height: 792.0,
                text: "Quoted PDF text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );

        let _ = app.update(Message::Pdf(PdfMessage::InsertQuoteLink));

        assert_eq!(
            app.overlays.toast.as_deref(),
            Some("Open a markdown file before inserting a quote link")
        );
        assert_eq!(app.editor.buffer.text(), "");
    }

    #[test]
    fn pdf_highlight_shortcut_without_selection_shows_toast() {
        let mut app = app_with_pdf_file();

        let _ = app.update(Message::KeyboardShortcut(Shortcut::PdfHighlight));

        assert_eq!(
            app.overlays.toast.as_deref(),
            Some("Select PDF text before highlighting")
        );
        assert!(app.pdf.annotations.values().all(Vec::is_empty));
    }

    #[test]
    fn pdf_create_annotation_uses_selected_pdf_text() {
        let mut app = app_with_pdf_file();
        app.pdf.document_id = Some("doc-1".to_string());
        app.state
            .save_pdf_document("doc-1", "papers/paper.pdf", 100, Some(1))
            .expect("test PDF document should register");
        app.pdf.selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 0,
            anchor_idx: 0,
            focus_idx: 8,
        });
        app.pdf.page_text.insert(
            0,
            md_editor_core::domain::pdf::PdfPageText {
                page_index: 0,
                page_width: 612.0,
                page_height: 792.0,
                text: "Keyboard highlight text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );

        let _ = app.update(Message::Pdf(PdfMessage::CreateAnnotation(
            md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
            md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
        )));

        let page_annotations = app
            .pdf
            .annotations
            .get(&0)
            .expect("highlight shortcut should create page annotation");
        assert_eq!(page_annotations.len(), 1);
        assert_eq!(page_annotations[0].selected_text, "Keyboard ");
        assert_eq!(
            page_annotations[0].kind,
            md_editor_core::domain::pdf::PdfAnnotationKind::Highlight
        );
        assert_eq!(
            page_annotations[0].color,
            md_editor_core::domain::pdf::PdfAnnotationColor::Yellow
        );
        assert!(app.pdf.selection.is_none());
    }

    #[test]
    fn pdf_companion_note_key_is_stable_for_path_separators() {
        assert_eq!(
            pdf_companion_note_key("papers\\paper.pdf"),
            "pdf_companion_note:papers/paper.pdf"
        );
    }

    #[test]
    fn test_pdf_navigation_history() {
        let mut history = NavigationHistory::default();
        let p1 = NavigationTarget::Pdf {
            path: "doc1.pdf".to_string(),
            page: 1,
            scroll_offset: 100.0,
            zoom: 1.0,
        };
        let p2 = NavigationTarget::Pdf {
            path: "doc1.pdf".to_string(),
            page: 2,
            scroll_offset: 200.0,
            zoom: 1.5,
        };
        let p3 = NavigationTarget::Markdown {
            path: "note.md".to_string(),
            line: 5,
            column: 10,
        };

        // Test push
        history.push(p1.clone());
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.current_index, 0);

        // Test duplicate push ignored
        history.push(p1.clone());
        assert_eq!(history.entries.len(), 1);

        // Push more
        history.push(p2.clone());
        history.push(p3.clone());
        assert_eq!(history.entries.len(), 3);
        assert_eq!(history.current_index, 2);

        // Test back
        assert_eq!(history.go_back(), Some(p2.clone()));
        assert_eq!(history.current_index, 1);
        assert_eq!(history.go_back(), Some(p1.clone()));
        assert_eq!(history.current_index, 0);
        assert_eq!(history.go_back(), None);

        // Test forward
        assert_eq!(history.go_forward(), Some(p2.clone()));
        assert_eq!(history.current_index, 1);
        assert_eq!(history.go_forward(), Some(p3.clone()));
        assert_eq!(history.current_index, 2);
        assert_eq!(history.go_forward(), None);

        // Test branch truncation on push
        assert_eq!(history.go_back(), Some(p2.clone())); // current_index = 1
        let p4 = NavigationTarget::Pdf {
            path: "doc2.pdf".to_string(),
            page: 4,
            scroll_offset: 400.0,
            zoom: 1.0,
        };
        history.push(p4.clone()); // truncates forward, adds p4 at index 2
        assert_eq!(history.entries.len(), 3);
        assert_eq!(history.entries[2].target, p4);
        assert_eq!(history.current_index, 2);
        assert_eq!(history.go_forward(), None);
    }

    #[test]
    fn test_pdf_page_rotation() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("dummy.pdf".to_string());
        app.pdf.showing_pdf = true;
        app.pdf.total_pages = 1;
        app.pdf.view.page_sizes = vec![Some((100.0, 200.0))];
        app.pdf.view.zoom = 1.0;
        app.pdf.rotation = 0;

        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        assert_eq!(app.pdf.view.layout.page_height(0), 200.0);
        assert_eq!(app.pdf.rotation, 0);

        let _ = app.update(Message::Pdf(PdfMessage::RotateClockwise));
        assert_eq!(app.pdf.rotation, 90);
        assert_eq!(app.pdf.view.layout.page_height(0), 100.0);

        let _ = app.update(Message::Pdf(PdfMessage::RotateClockwise));
        assert_eq!(app.pdf.rotation, 180);
        assert_eq!(app.pdf.view.layout.page_height(0), 200.0);

        let _ = app.update(Message::Pdf(PdfMessage::RotateClockwise));
        assert_eq!(app.pdf.rotation, 270);
        assert_eq!(app.pdf.view.layout.page_height(0), 100.0);
    }

    #[test]
    fn test_pdf_link_click_in_split_view_navigates_and_preserves_scroll() {
        let mut app = MdEditor::new().0;
        app.shell.split_view_active = true;
        app.pdf.showing_pdf = true;
        app.workspace.active_path = Some("note.md".to_string());
        app.pdf.active_path = Some("paper.pdf".to_string());
        app.pdf.total_pages = 10;
        app.pdf.view.page_sizes = vec![Some((500.0, 700.0)); 10];
        app.pdf.view.zoom = 1.0;

        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        app.editor.scroll_y = 120.0;

        // Click on a relative link with hash delimiter and no schema prefix
        let _ = app.update(Message::Workspace(WorkspaceMessage::FileClicked(
            "paper.pdf#page=5".to_string(),
        )));

        // Assert editor scroll is preserved
        assert_eq!(app.editor.scroll_y, 120.0);
        // Assert PDF page navigated to page 4 (index of page 5)
        assert_eq!(app.pdf.current_page, 4);
    }

    #[test]
    fn test_pdf_open_race_condition_navigation() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("note.md".to_string());
        app.pdf.active_path = Some("paper.pdf".to_string());

        // Initial target page starts at Some(4) when we click a link
        app.pdf.initial_target_page = Some(4);

        // Hashing finishes first before PDF pages count is loaded
        let _ = app.update(Message::Pdf(PdfMessage::DocumentIdComputed(Some((
            "paper.pdf".to_string(),
            "dummyhash".to_string(),
            1000,
            Some(0),
        )))));

        // Verify target page was deferred and not clamped/consumed yet
        assert_eq!(app.pdf.initial_target_page, Some(4));

        // PDF total pages finishes loading
        let generation = app.pdf.render_generation;
        let _ = app.update(Message::Pdf(PdfMessage::Loaded(generation, 10)));
        assert_eq!(app.pdf.total_pages, 10);

        // Page sizes finish loading, which triggers layout rebuild and PdfFitToWidth
        let _ = app.update(Message::Pdf(PdfMessage::PageSizesLoaded(
            generation,
            "paper.pdf".to_string(),
            vec![(500.0, 700.0); 10],
        )));

        // Under the hood, PdfPageSizesLoaded dispatches PdfFitToWidth, which we execute here
        let _ = app.update(Message::Pdf(PdfMessage::FitToWidth));

        // Now it should be consumed and navigated to page 4
        assert_eq!(app.pdf.initial_target_page, None);
        assert_eq!(app.pdf.current_page, 4);
    }

    #[test]
    fn test_manual_scroll_clears_programmatic_scroll_target() {
        let mut app = MdEditor::new().0;
        app.pdf.total_pages = 10;
        app.pdf.pages = vec![None; 10];
        app.pdf.view.page_sizes = vec![Some((500.0, 700.0)); 10];
        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        // 1. Programmatic scroll to page 5 when page 5 is NOT ready (still loading)
        app.pdf.toc_target_page = Some(5);
        app.pdf.programmatic_scroll = true;

        // Scroll event at expected placeholder position arrives
        let target_y = app.pdf_page_offset(5);
        let _ = app.update(Message::Pdf(PdfMessage::Scrolled {
            y: target_y,
            viewport_height: 500.0,
        }));
        // Since page is not ready, programmatic scroll and target page are preserved
        assert!(app.pdf.programmatic_scroll);
        assert_eq!(app.pdf.toc_target_page, Some(5));

        // 2. Now simulate page 5 finishing loading/rendering
        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
        app.pdf.pages[5] = Some(handle);

        // Scroll event arrives now that the page is ready
        let _ = app.update(Message::Pdf(PdfMessage::Scrolled {
            y: target_y,
            viewport_height: 500.0,
        }));
        // It arrives, so both flags are cleared
        assert!(!app.pdf.programmatic_scroll);
        assert_eq!(app.pdf.toc_target_page, None);

        // 3. Manual scroll clears target page (when pdf_programmatic_scroll is false)
        app.pdf.toc_target_page = Some(3);
        let _ = app.update(Message::Pdf(PdfMessage::Scrolled {
            y: 100.0,
            viewport_height: 500.0,
        }));
        assert_eq!(app.pdf.toc_target_page, None);
    }

    #[test]
    fn test_split_view_width_calculations() {
        let mut app = MdEditor::new().0;
        app.shell.window_width = 1200.0;
        app.shell.sidebar_visible = false;
        app.shell.toc_visible = false;
        app.workspace.backlinks_visible = false;
        app.shell.pdf_annotations_visible = false;
        app.editor.viewport_width = 0.0;

        app.workspace.active_path = Some("note.md".to_string());
        app.pdf.active_path = Some("paper.pdf".to_string());
        app.shell.split_view_active = true;
        app.shell.split_ratio = 0.6; // PDF gets 60%, Editor gets 40%

        let pdf_width = app.pdf_available_width();
        let editor_width = app.estimated_editor_viewport_width();

        // 1200.0 * 0.6 = 720.0
        assert!((pdf_width - 720.0).abs() < 1e-3);
        // 1200.0 * 0.4 = 480.0
        assert!((editor_width - 480.0).abs() < 1e-3);
    }

    #[test]
    fn test_reference_link_resolves_and_preserves_scroll() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("note.md".to_string());
        app.editor.scroll_y = 120.0;
        app.editor.buffer =
            DocBuffer::from_text("# Heading 1\n\n[my-ref]\n\n[my-ref]: #heading-1\n");
        app.editor.highlighted_lines = parser::highlight_markdown(&app.editor.buffer.text());

        // Click on the reference "my-ref"
        let _ = app.update(Message::Workspace(WorkspaceMessage::FileClicked(
            "my-ref".to_string(),
        )));

        // Active path should still be note.md
        assert_eq!(app.workspace.active_path.as_deref(), Some("note.md"));
        // Editor cursor should be moved to heading 1 (line 0)
        assert_eq!(app.editor.buffer.cursor_line, 0);
    }

    #[test]
    fn test_ctrl_click_programmatic_scroll_bypasses_cancellation() {
        let mut app = MdEditor::new().0;
        app.pdf.total_pages = 10;
        app.pdf.pages = vec![None; 10];
        app.pdf.view.page_sizes = vec![Some((500.0, 700.0)); 10];
        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        // Simulate Ctrl modifier active
        app.shell.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;

        // 1. Programmatic scroll is triggered
        app.pdf.toc_target_page = Some(5);
        app.pdf.programmatic_scroll = true;

        // Populate page 5 in cache to mark as ready
        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
        app.pdf.pages[5] = Some(handle);

        // Scroll event arrives (with Ctrl held down)
        let target_y = app.pdf_page_offset(5);
        let _ = app.update(Message::Pdf(PdfMessage::Scrolled {
            y: target_y,
            viewport_height: 500.0,
        }));

        // Programmatic scroll bypasses Ctrl key cancellation, sets self.pdf.programmatic_scroll = false, and clears target
        assert!(!app.pdf.programmatic_scroll);
        assert_eq!(app.pdf.toc_target_page, None);
    }

    #[test]
    fn test_large_doc_highlight_debounce_and_reset() {
        let mut app = MdEditor::new().0;

        // Setup a buffer with more than LARGE_DOC_LINE_THRESHOLD (1,000) lines
        let mut text = String::new();
        for i in 0..1005 {
            text.push_str(&format!("Line {}\n", i));
        }
        app.editor.buffer.set_text(&text);

        // 1. Initial edit (opened_file = false)
        let _task = app.refresh_highlighting_for_current_buffer(false);
        assert_eq!(app.editor.highlight_generation, 1);
        assert_eq!(app.editor.pending_highlight_generation, Some(1));
        assert!(app.editor.pending_highlight_requested_at.is_some());
        assert!(app.editor.pending_highlight_text.is_some());

        // 2. Second edit before debounce triggers resets and increments generation
        let _task2 = app.refresh_highlighting_for_current_buffer(false);
        assert_eq!(app.editor.highlight_generation, 2);
        assert_eq!(app.editor.pending_highlight_generation, Some(2));

        // 3. Mock time elapsed to trigger highlight debounce
        app.editor.pending_highlight_requested_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(300));
        let _debounce_task = app.update(Message::Editor(EditorMessage::HighlightDebounceElapsed));

        // Debounce state cleared
        assert_eq!(app.editor.pending_highlight_generation, None);
        assert!(app.editor.pending_highlight_requested_at.is_none());
        assert!(app.editor.pending_highlight_text.is_none());
    }

    #[test]
    fn test_stale_highlight_generation_handling() {
        let mut app = MdEditor::new().0;
        app.editor.highlight_generation = 5;

        let dummy_lines_stale = vec![crate::editor::parser::StyledLine::new()];
        let mut dummy_lines_newer = vec![crate::editor::parser::StyledLine::new()];
        dummy_lines_newer[0]
            .spans
            .push(crate::editor::parser::StyledSpan::plain("newer"));

        // 1. Stale highlight ready (generation 4 < 5) should be ignored
        let _ = app.update(Message::Editor(EditorMessage::HighlightReady(
            4,
            dummy_lines_stale,
        )));
        assert!(app.editor.highlighted_lines.is_empty());

        // 2. Newer highlight ready (generation 5 == 5) should be accepted
        let _ = app.update(Message::Editor(EditorMessage::HighlightReady(
            5,
            dummy_lines_newer,
        )));
        assert_eq!(app.editor.highlighted_lines.len(), 1);
        assert_eq!(app.editor.highlighted_lines[0].spans[0].text, "newer");
    }

    #[test]
    fn test_pdf_open_clears_page_text_cache() {
        let mut app = MdEditor::new().0;

        // 1. Populate the page text cache with some dummy entries
        app.pdf.page_text.insert(
            0,
            md_editor_core::domain::pdf::PdfPageText {
                page_index: 0,
                page_width: 500.0,
                page_height: 700.0,
                text: "Hello".to_string(),
                chars: vec![],
                lines: vec![],
            },
        );
        app.pdf.text_lru.push_back(0);

        // 2. Perform open_pdf (we'll set vault root first so path resolves)
        let root = unique_temp_dir("open_pdf_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();
        app.workspace.vault_root = Some(root_str);

        // Create a dummy pdf file so resolve_active_path works
        let pdf_path = root.join("test.pdf");
        std::fs::write(&pdf_path, "%PDF-1.4 ...").unwrap();

        let _task = app.open_pdf("test.pdf");

        // 3. Verify page text cache is cleared
        assert!(app.pdf.page_text.is_empty());
        assert!(app.pdf.text_lru.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_sync_quick_note_to_linked_note_file() {
        let mut app = MdEditor::new().0;
        let root = unique_temp_dir("sync_quick_note_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // 1. Create a dummy linked note file
        let note_path = "linked-note.md";
        let doc_id = format!("doc-{}", uuid::Uuid::new_v4());
        let ann_id = format!("ann-{}", uuid::Uuid::new_v4());
        let pdf_path = "paper.pdf";

        let ann = md_editor_core::domain::pdf::PdfAnnotation {
            id: ann_id.clone(),
            document_id: doc_id.clone(),
            page_index: 0,
            kind: md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
            selected_text: "Target Highlight Text".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: Some(note_path.to_string()),
            markdown_anchor: None,
            tags: Vec::new(),
            status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        };

        // Create the linked note file with initial empty content
        let initial_content = crate::features::pdf::annotations::new_linked_pdf_note_content(
            note_path, pdf_path, &ann,
        );
        std::fs::write(root.join(note_path), &initial_content).unwrap();

        app.state
            .save_pdf_document(&doc_id, pdf_path, 0, None)
            .unwrap();
        app.state.save_pdf_annotation(&ann).unwrap();

        // Setup app state
        app.pdf.active_path = Some(pdf_path.to_string());
        app.pdf.annotations.insert(0, vec![ann.clone()]);

        // Open the file as active in the editor so we test real-time buffer reload
        app.workspace.active_path = Some(note_path.to_string());
        app.editor.buffer = crate::editor::buffer::DocBuffer::from_text(&initial_content);

        // 2. Fire PdfAddQuickNote
        let _ = app.update(Message::Pdf(PdfMessage::AddQuickNote(
            ann_id.to_string(),
            "New note update from UI".to_string(),
        )));

        // 3. Verifications
        // Check annotation in app memory
        let updated_ann = app
            .pdf
            .annotations
            .get(&0)
            .unwrap()
            .iter()
            .find(|a| a.id == ann_id)
            .unwrap();
        assert_eq!(
            updated_ann.note,
            Some("New note update from UI".to_string())
        );

        // Check persisted annotation
        let db_note = app
            .state
            .get_pdf_annotations(&doc_id, Some(0))
            .unwrap()
            .into_iter()
            .find(|annotation| annotation.id == ann_id)
            .and_then(|annotation| annotation.note);
        assert_eq!(db_note, Some("New note update from UI".to_string()));

        // Check file on disk
        let disk_content = std::fs::read_to_string(root.join(note_path)).unwrap();
        assert!(disk_content.contains("### Notes\n\nNew note update from UI\n\n"));

        // Check active editor buffer reload
        assert!(
            app.editor
                .buffer
                .text()
                .contains("### Notes\n\nNew note update from UI\n\n")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_cross_pane_navigation_history() {
        let mut app = app_without_vault();
        let root = unique_temp_dir("cross_pane_nav_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // Create a markdown file and a dummy PDF file
        let note_path = "document.md";
        let pdf_path = "document.pdf";
        std::fs::write(root.join(note_path), "# Title\nSome content here").unwrap();
        std::fs::write(root.join(pdf_path), "%PDF-1.4 ...").unwrap();

        // 1. Open the markdown file
        let _ = app.open_file(note_path);
        assert_eq!(app.workspace.active_path.as_deref(), Some(note_path));
        assert!(!app.pdf.showing_pdf);

        // 2. Open the PDF (this should trigger history push of markdown path)
        let _ = app.open_pdf(pdf_path);
        assert_eq!(app.pdf.active_path.as_deref(), Some(pdf_path));
        assert!(app.pdf.showing_pdf);

        // 3. Verify history has 1 entry (for Markdown)
        assert_eq!(app.workspace.navigation_history.entries.len(), 1);
        match &app.workspace.navigation_history.entries[0].target {
            NavigationTarget::Markdown { path, .. } => {
                assert_eq!(path, note_path);
            }
            _ => panic!("Expected Markdown target"),
        }

        // 4. Trigger PdfNavBack to return to Markdown
        let _ = app.update(Message::Pdf(PdfMessage::NavBack));
        assert_eq!(app.workspace.active_path.as_deref(), Some(note_path));
        assert!(!app.pdf.showing_pdf);

        // 5. Trigger PdfNavForward to return to PDF
        let _ = app.update(Message::Pdf(PdfMessage::NavForward));
        assert_eq!(app.pdf.active_path.as_deref(), Some(pdf_path));
        assert!(app.pdf.showing_pdf);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_follow_citation() {
        let mut app = MdEditor::new().0;

        let link_span = parser::StyledSpan {
            text: "[citation](pdf://papers/a.pdf)".to_string(),
            display_text: Some("citation".to_string()),
            color: iced::Color::BLACK,
            bold: false,
            italic: false,
            font_size: 12.0,
            is_code: false,
            is_link: true,
            link_target: Some("pdf://papers/a.pdf".to_string()),
            is_heading: false,
            heading_level: 0,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            image_alt: None,
            is_math: false,
            is_syntax: false,
            id: None,
        };
        app.editor.highlighted_lines = vec![parser::StyledLine {
            spans: vec![link_span],
            is_code_block: false,
            is_math_block: false,
            code_block_lang: None,
            is_blockquote: false,
            block_id: 1,
            is_block_fence: false,
            is_table_row: false,
            table_cells: vec![],
        }];

        app.editor.buffer.cursor_line = 0;
        app.editor.buffer.cursor_col = 5;
        let _task = app.follow_citation();

        app.editor.buffer.cursor_col = 50;
        let _task_none = app.follow_citation();

        app.editor.buffer.cursor_line = 10;
        let _task_oob = app.follow_citation();
    }

    #[test]
    fn test_show_usages() {
        let mut app = MdEditor::new().0;
        let root = unique_temp_dir("test_show_usages_dir");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        save_markdown_file_with_parser_targets(
            &app.state,
            "source.md",
            "Refer to [doc](pdf://papers/a.pdf?page=2)",
        )
        .unwrap();

        app.pdf.active_path = Some("papers/a.pdf".to_string());
        app.pdf.showing_pdf = true;

        let _ = app.show_usages();

        assert!(app.workspace.backlinks_visible);
        assert!(!app.workspace.backlinks.is_empty());
        let backlink_labels = app
            .workspace
            .backlinks
            .iter()
            .map(|b| b.label.clone())
            .collect::<Vec<_>>();
        assert!(backlink_labels.contains(&"source.md".to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_combined_outline_toc_navigator() {
        let md_toc = vec![parser::OutlineEntry {
            level: 1,
            text: "Heading 1".to_string(),
            line: 5,
        }];
        let pdf_toc = vec![parser::OutlineEntry {
            level: 2,
            text: "Bookmark 2".to_string(),
            line: 12,
        }];

        let _element = views::toc::view(&md_toc, &pdf_toc, 250.0, None, None);
    }

    #[test]
    fn test_global_unified_search() {
        let mut app = app_without_vault();
        let root = unique_temp_dir("test_global_unified_search_dir");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        app.search.visible = true;
        let _ = app.update(Message::Search(SearchMessage::QueryChanged(
            "vault".to_string(),
        )));

        assert_eq!(app.search.editor.query, "vault");
        assert!(app.search.global.searching);

        let match_item = md_editor_core::domain::UnifiedSearchResult {
            group: md_editor_core::domain::SearchResultGroup::Heading,
            path: "source.md".to_string(),
            line: 1,
            context: "# Welcome to the Vault".to_string(),
            score: 8.0,
            page_index: None,
            annotation_id: None,
        };

        let _ = app.update(Message::Search(SearchMessage::UnifiedMatchesFound(
            app.search.global.id,
            vec![match_item],
        )));

        assert_eq!(app.search.global.results.len(), 1);
        assert_eq!(
            app.search.global.results[0].context,
            "# Welcome to the Vault"
        );
        let _ = app.update(Message::Search(SearchMessage::UnifiedPdfMatchesFound(
            app.search.global.id,
            pdf_text_batch(Vec::new(), 0, 0, false, false),
        )));
        assert!(!app.search.global.searching);

        let _ = app.update(Message::Search(SearchMessage::UnifiedFinished(
            app.search.global.id,
            Ok(()),
        )));
        assert!(!app.search.global.searching);

        let _click_task = app.update(Message::Search(SearchMessage::UnifiedResultClicked(
            app.search.global.results[0].clone(),
        )));
        assert!(!app.search.visible);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_search_registered_pdf_text_results_does_not_deadlock() {
        let mut app = MdEditor::new().0;
        app.state = Arc::new(
            md_editor_core::state::AppState::try_new_in_memory()
                .expect("in-memory application state should initialize"),
        );
        let root = unique_temp_dir("search_deadlock_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();

        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // Register a pdf
        let pdf_path = "doc.pdf";
        let abs_path = root.join(pdf_path);
        std::fs::write(&abs_path, "PDF Dummy content").unwrap();

        let metadata = std::fs::metadata(&abs_path).unwrap();
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        app.state
            .save_pdf_document("doc-1", pdf_path, size, Some(mtime))
            .unwrap();

        let mut query = md_editor_core::domain::UnifiedSearchQuery::all_sources("Dummy".to_string());
        query.sources = vec![md_editor_core::domain::UnifiedSearchSource::PdfContent];

        // This would deadlock if state.vault_root lock guard was held and then validate_and_invalidate_pdf_cache tried to lock it again
        let _batch = search_registered_pdf_text_results(&app.state, &query, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_search_unopened_pdf_discovered_from_disk() {
        let app = MdEditor::new().0;
        let root = unique_temp_dir("unopened_pdf_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();

        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // Write an unopened PDF file to disk (but DO NOT save it in the DB / register it)
        let pdf_path = "unopened.pdf";
        let abs_path = root.join(pdf_path);
        std::fs::write(&abs_path, "PDF Dummy content").unwrap();

        let pdf_paths = md_editor_core::vault::list_all_pdf_files(&root).unwrap();
        assert_eq!(pdf_paths.len(), 1);
        let rel_path = md_editor_core::vault::path_to_relative_string(&pdf_paths[0], &root);
        assert_eq!(rel_path, "unopened.pdf");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_pdf_toc_navigation_completes_if_already_scrolled() {
        let mut app = MdEditor::new().0;
        app.pdf.total_pages = 5;
        app.pdf.pages = vec![None; 5];
        app.pdf.dimensions = vec![Some((600, 800)); 5];

        // Setup state to be programmatically scrolling to page 2
        app.pdf.toc_target_page = Some(2);
        app.pdf.programmatic_scroll = true;

        // Mock scrollable position to be already at page 2 offset
        let scroll_y = app.pdf_page_offset(2);
        app.pdf.scroll_y = scroll_y;

        // Emit PdfRendered for page 2
        let _ = app.update(Message::Pdf(PdfMessage::Rendered(
            app.pdf.render_generation,
            2,
            image::DynamicImage::ImageRgba8(image::ImageBuffer::new(10, 10)),
        )));

        // Programmatic scroll flags should be cleared and page should be marked as current
        assert!(app.pdf.toc_target_page.is_none());
        assert!(!app.pdf.programmatic_scroll);
        assert_eq!(app.pdf.current_page, 2);
    }

    #[test]
    fn stale_pdf_matches_do_not_enter_global_results() {
        let mut app = MdEditor::new().0;
        app.search.visible = true;
        app.pdf.active_path = Some("paper.pdf".to_string());
        app.search.editor.query = "needle".to_string();
        app.search.pdf_active_id = 7;
        app.search.global.pdf_search_id = Some(8);

        let _ = app.update(Message::Pdf(PdfMessage::SearchMatchesFound(
            7,
            vec![md_editor_core::application::pdf_service::PdfSearchMatch {
                page_index: 0,
                context: "needle context".to_string(),
                rects: Vec::new(),
            }],
        )));

        assert!(app.search.global.results.is_empty());
        assert_eq!(app.pdf.view.search.matches.len(), 1);
    }

    #[test]
    fn global_search_query_uses_source_toggles() {
        let mut app = MdEditor::new().0;
        let source = md_editor_core::domain::UnifiedSearchSource::PdfContent;

        let _ = app.update(Message::Search(SearchMessage::SourceToggled(source, false)));
        let query = app.build_global_search_query("needle".to_string());

        assert!(!query.includes(source));
        assert!(query.includes(md_editor_core::domain::UnifiedSearchSource::MarkdownContent));
    }

    #[test]
    fn pdf_content_global_result_activates_matching_search_hit() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("paper.pdf".to_string());
        app.pdf.showing_pdf = true;
        app.pdf.total_pages = 3;
        app.pdf.view.search.matches =
            vec![md_editor_core::application::pdf_service::PdfSearchMatch {
                page_index: 1,
                context: "needle context".to_string(),
                rects: vec![md_editor_core::domain::pdf::PdfRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 10.0,
                }],
            }];
        app.rebuild_pdf_search_page_index();

        let _ = app.update(Message::Search(SearchMessage::UnifiedResultClicked(
            md_editor_core::domain::UnifiedSearchResult {
                group: md_editor_core::domain::SearchResultGroup::PdfContent,
                path: "paper.pdf".to_string(),
                line: 2,
                context: "PDF text (1 areas): needle context".to_string(),
                score: 6.0,
                page_index: Some(1),
                annotation_id: Some("0".to_string()),
            },
        )));

        assert_eq!(app.pdf.view.search.active_index, Some(0));
        assert_eq!(app.pdf.current_page, 1);
        assert!(app.pdf.programmatic_scroll);
    }

    #[test]
    fn pdf_content_global_result_navigates_page_when_already_open_without_annotation_id() {
        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("paper.pdf".to_string());
        app.pdf.showing_pdf = true;
        app.pdf.total_pages = 3;
        app.pdf.pages = vec![None; 3];
        app.pdf.view.page_sizes = vec![Some((500.0, 700.0)); 3];
        app.pdf.view.layout = PdfLayout::rebuild(
            &app.pdf.view.page_sizes,
            app.pdf.view.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf.rotation,
        );

        let _ = app.update(Message::Search(SearchMessage::UnifiedResultClicked(
            md_editor_core::domain::UnifiedSearchResult {
                group: md_editor_core::domain::SearchResultGroup::PdfContent,
                path: "paper.pdf".to_string(),
                line: 2,
                context: "needle context".to_string(),
                score: 6.0,
                page_index: Some(1),
                annotation_id: None,
            },
        )));

        // It should navigate directly, setting current page to index 1 (page 2)
        assert_eq!(app.pdf.current_page, 1);
        assert!(app.pdf.programmatic_scroll);
        // It shouldn't clear pages/total pages since it was already open
        assert_eq!(app.pdf.pages.len(), 3);
        assert_eq!(app.pdf.total_pages, 3);
    }

    #[test]
    fn vault_pdf_text_results_merge_only_for_visible_current_search() {
        let mut app = MdEditor::new().0;
        app.search.visible = true;
        app.search.global.id = 5;
        app.search.global.pending_vault_pdf = true;

        let pdf_result = md_editor_core::domain::UnifiedSearchResult {
            group: md_editor_core::domain::SearchResultGroup::PdfContent,
            path: "other.pdf".to_string(),
            line: 3,
            context: "PDF text (1 areas): needle".to_string(),
            score: 4.0,
            page_index: Some(2),
            annotation_id: None,
        };

        let _ = app.update(Message::Search(SearchMessage::UnifiedPdfMatchesFound(
            4,
            pdf_text_batch(vec![pdf_result.clone()], 1, 2, false, false),
        )));
        assert!(app.search.global.results.is_empty());
        assert!(app.search.global.pending_vault_pdf);

        let _ = app.update(Message::Search(SearchMessage::UnifiedPdfMatchesFound(
            5,
            pdf_text_batch(vec![pdf_result], 1, 2, false, true),
        )));
        assert_eq!(app.search.global.results.len(), 1);
        assert!(!app.search.global.pending_vault_pdf);
        assert_eq!(
            app.search.global.pdf_status.as_deref(),
            Some("PDF text: searched 1 of 2 registered PDFs; document cap reached")
        );

        app.search.visible = false;
        app.search.global.id = 6;
        let _ = app.update(Message::Search(SearchMessage::UnifiedPdfMatchesFound(
            6,
            pdf_text_batch(
                vec![md_editor_core::domain::UnifiedSearchResult {
                    group: md_editor_core::domain::SearchResultGroup::PdfContent,
                    path: "stale.pdf".to_string(),
                    line: 1,
                    context: "stale".to_string(),
                    score: 4.0,
                    page_index: Some(0),
                    annotation_id: None,
                }],
                1,
                1,
                false,
                false,
            ),
        )));
        assert_eq!(app.search.global.results.len(), 1);
    }

    #[test]
    fn registered_pdf_search_targets_skip_active_and_cap_work() {
        let paths = (0..40)
            .map(|idx| format!("paper-{idx}.pdf"))
            .collect::<Vec<_>>();

        let targets = registered_pdf_search_targets(paths, Some("paper-3.pdf"), 5);

        assert_eq!(targets.len(), 5);
        assert!(!targets.iter().any(|path| path == "paper-3.pdf"));
        assert_eq!(targets[0], "paper-0.pdf");
        assert_eq!(targets[4], "paper-5.pdf");
    }

    #[test]
    fn registered_pdf_index_targets_cap_documents() {
        let paths = (0..40)
            .map(|idx| format!("paper-{idx}.pdf"))
            .collect::<Vec<_>>();

        let targets = registered_pdf_index_targets(paths, 3);

        assert_eq!(targets, vec!["paper-0.pdf", "paper-1.pdf", "paper-2.pdf"]);
    }

    #[test]
    fn pdf_search_status_reports_result_cap_first() {
        let batch = pdf_text_batch(Vec::new(), 32, 100, true, true);

        assert_eq!(
            format_pdf_search_status(&batch),
            "PDF text: searched 32 of 100 registered PDFs; result cap reached"
        );
    }

    #[test]
    fn test_excerpt_mode_queue_and_batch_insert() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("test_note.md".to_string());
        app.pdf.active_path = Some("document.pdf".to_string());

        // Toggle excerpt mode
        let _ = app.update(Message::ExcerptModeToggle);
        assert!(app.overlays.excerpt_mode_active);

        // Queue items using CitationPaletteChoose
        let item1 = crate::messages::CitationItem::Selection {
            text: "first queued excerpt".to_string(),
            page_index: 1, // page 2
        };
        let item2 = crate::messages::CitationItem::Annotation {
            id: "ann-456".to_string(),
            text: "second queued excerpt".to_string(),
            page_index: 4, // page 5
        };

        let _ = app.update(Message::CitationPaletteChoose(item1));
        let _ = app.update(Message::CitationPaletteChoose(item2));

        assert_eq!(app.overlays.excerpts_queue.len(), 2);

        // Insert batch
        let _ = app.update(Message::ExcerptQueueInsertBatch);

        // Queue should be cleared
        assert!(app.overlays.excerpts_queue.is_empty());

        // Document buffer should contain the citations
        let content = app.editor.buffer.text();
        assert!(content.contains("> first queued excerpt"));
        assert!(content.contains("[Selection (Page 2)](pdf://document.pdf?page=2)"));
        assert!(content.contains("> second queued excerpt"));
        assert!(
            content.contains("[Highlight (Page 5)](pdf://document.pdf?page=5&annotation=ann-456)")
        );
    }

    #[test]
    fn citation_palette_submit_first_queues_first_item_in_excerpt_mode() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("test_note.md".to_string());
        app.overlays.citation_palette_visible = true;
        app.overlays.excerpt_mode_active = true;
        app.pdf.annotations.insert(
            0,
            vec![md_editor_core::domain::pdf::PdfAnnotation {
                id: "ann-keyboard".to_string(),
                document_id: "doc".to_string(),
                page_index: 0,
                kind: md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
                color: md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
                selected_text: "keyboard citation".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: vec![],
                status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            }],
        );

        let _ = app.update(Message::CitationPaletteSubmitFirst);

        assert!(!app.overlays.citation_palette_visible);
        assert_eq!(app.overlays.excerpts_queue.len(), 1);
        assert!(matches!(
            app.overlays.excerpts_queue.as_slice(),
            [crate::messages::CitationItem::Annotation { id, .. }] if id == "ann-keyboard"
        ));
    }

    #[test]
    fn test_command_registry_is_enabled_context_rules() {
        use crate::command_registry::{CommandContext, get_command_registry};
        use crate::messages::Shortcut;

        let registry = get_command_registry();
        let save_cmd = registry.iter().find(|c| c.id == Shortcut::Save).unwrap();

        // 1. Save disabled when no markdown is open
        let ctx_no_md = CommandContext {
            markdown_open: false,
            pdf_open: false,
            image_open: false,
            active_pane: crate::app_shell::AppShellPane::None,
            has_vault: true,
            pdf_has_selection: false,
            has_focused_annotation: false,
        };
        assert_eq!(
            save_cmd.is_enabled(ctx_no_md),
            Err("No active markdown file to save")
        );

        // 2. Save enabled when markdown is open
        let ctx_md = CommandContext {
            markdown_open: true,
            pdf_open: false,
            image_open: false,
            active_pane: crate::app_shell::AppShellPane::Markdown,
            has_vault: true,
            pdf_has_selection: false,
            has_focused_annotation: false,
        };
        assert_eq!(save_cmd.is_enabled(ctx_md), Ok(()));
    }

    #[test]
    fn test_command_palette_context_aware_ranking_and_grouping() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = None; // No markdown file open

        // Querying commands with no markdown open
        let commands = app.command_palette_commands();

        // Verify Save command has a disabled reason
        let save_cmd = commands
            .iter()
            .find(|c| c.shortcut == crate::messages::Shortcut::Save)
            .unwrap();
        assert_eq!(
            save_cmd.disabled_reason,
            Some("No active markdown file to save")
        );

        // Open a markdown file
        app.workspace.active_path = Some("test.md".to_string());
        let commands_with_md = app.command_palette_commands();
        let save_cmd_enabled = commands_with_md
            .iter()
            .find(|c| c.shortcut == crate::messages::Shortcut::Save)
            .unwrap();
        assert_eq!(save_cmd_enabled.disabled_reason, None);
    }

    #[test]
    fn test_search_wrap_and_replace() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("test.md".to_string());
        app.search.editor.visible = true;
        app.editor.buffer.set_text("banana apple banana");
        app.editor.highlighted_lines =
            crate::editor::parser::highlight_markdown(app.editor.buffer.text().as_str());

        // 1. Check search queries reset wrap_status
        let _ = app.update(Message::Search(SearchMessage::QueryChanged(
            "banana".to_string(),
        )));
        assert_eq!(app.current_document_match_count(), 2);
        assert_eq!(app.search.editor.wrap_status, None);

        // First Next: active_index goes to 0 (first match)
        let _ = app.update(Message::Search(SearchMessage::Next));
        assert_eq!(app.search.editor.active_index, Some(0));
        assert_eq!(app.search.editor.wrap_status, None);

        // Second Next: active_index goes to 1 (second match)
        let _ = app.update(Message::Search(SearchMessage::Next));
        assert_eq!(app.search.editor.active_index, Some(1));
        assert_eq!(app.search.editor.wrap_status, None);

        // Third Next: wraps around to 0
        let _ = app.update(Message::Search(SearchMessage::Next));
        assert_eq!(app.search.editor.active_index, Some(0));
        assert_eq!(
            app.search.editor.wrap_status,
            Some(SearchWrapStatus::WrappedForward)
        );

        // Triggering SearchPrevious when active_index is 0 wraps to 1 (last match)
        let _ = app.update(Message::Search(SearchMessage::Previous));
        assert_eq!(app.search.editor.active_index, Some(1));
        assert_eq!(
            app.search.editor.wrap_status,
            Some(SearchWrapStatus::WrappedBackward)
        );

        // 2. Query changes reset wrap_status
        let _ = app.update(Message::Search(SearchMessage::QueryChanged(
            "apple".to_string(),
        )));
        assert_eq!(app.current_document_match_count(), 1);
        assert_eq!(app.search.editor.wrap_status, None);

        // 3. Single match replace
        let _ = app.update(Message::Search(SearchMessage::ReplaceChanged(
            "pear".to_string(),
        )));
        let _ = app.update(Message::Search(SearchMessage::Next)); // Focus apple (index 0)
        assert_eq!(app.search.editor.active_index, Some(0));

        let _ = app.update(Message::Search(SearchMessage::Replace));
        assert_eq!(app.editor.buffer.text(), "banana pear banana");
        // After replacement, query "apple" has no matches in the document
        assert_eq!(app.current_document_match_count(), 0);
        assert_eq!(app.search.editor.active_index, None);
    }
}
