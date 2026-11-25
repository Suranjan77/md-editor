//! Tests for the app shell: link resolution, routing and document state.

use super::*;
use crate::messages::{EditorMessage, GraphMessage, PdfMessage};

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
fn test_resolve_vault_note_by_name() {
    use md_editor_core::types::FileEntry;
    let entry = |path: &str, name: &str, is_dir: bool| FileEntry {
        path: path.to_string(),
        name: name.to_string(),
        is_dir,
    };
    let entries = vec![
        entry("notes", "notes", true),
        entry("notes/Alpha.md", "Alpha.md", false),
        entry("archive/old/Beta.md", "Beta.md", false),
        entry("Gamma.pdf", "Gamma.pdf", false),
        entry("a/Dup.md", "Dup.md", false),
        entry("deep/nested/Dup.md", "Dup.md", false),
    ];

    // Bare name in a subfolder, case-insensitive, with/without extension.
    assert_eq!(
        resolve_vault_note_by_name(&entries, "Alpha").as_deref(),
        Some("notes/Alpha.md")
    );
    assert_eq!(
        resolve_vault_note_by_name(&entries, "beta").as_deref(),
        Some("archive/old/Beta.md")
    );
    // Non-md extension matches on full name.
    assert_eq!(
        resolve_vault_note_by_name(&entries, "Gamma.pdf").as_deref(),
        Some("Gamma.pdf")
    );
    // Only the final path component is used as the target name.
    assert_eq!(
        resolve_vault_note_by_name(&entries, "folder/Alpha").as_deref(),
        Some("notes/Alpha.md")
    );
    // Shortest path wins when paths differ in length.
    assert_eq!(
        resolve_vault_note_by_name(&entries, "Dup").as_deref(),
        Some("a/Dup.md")
    );
    // No match.
    assert_eq!(resolve_vault_note_by_name(&entries, "Missing"), None);
}

#[test]
fn ink_path_is_derived_from_the_open_note() {
    assert_eq!(
        derive_ink_path(Some("notes/algebra.md"), "20260805-101500"),
        "notes/algebra.ink"
    );
}

#[test]
fn ink_path_falls_back_to_a_timestamp_with_no_note_open() {
    assert_eq!(
        derive_ink_path(None, "20260805-101500"),
        "handwritten-20260805-101500.ink"
    );
}

#[test]
fn ink_path_only_strips_an_extension_from_the_final_component() {
    // A dot in a directory name must not be mistaken for the extension.
    assert_eq!(
        derive_ink_path(Some("course-v1.2/lecture"), "20260805-101500"),
        "course-v1.2/lecture.ink"
    );
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

    let resolved = resolve_relative_link_path(Some(vault_root), Some(active_path), "another_file");
    assert_eq!(resolved, "subdir/another_file");

    let _ = std::fs::remove_dir_all(&target_dir);
}

#[test]
fn test_new_entry_parent_follows_only_visible_selection() {
    let entry = |path: &str, is_dir: bool| md_editor_core::types::FileEntry {
        path: path.to_string(),
        name: path.rsplit('/').next().unwrap().to_string(),
        is_dir,
    };
    let entries = vec![
        entry("notes", true),
        entry("notes/a.md", false),
        entry("notes/deep", true),
        entry("root.md", false),
    ];
    let expanded: std::collections::BTreeSet<String> = ["notes".to_string()].into();
    let parent = |selected| MdEditor::new_entry_parent(selected, &entries, &expanded);

    // A visible file inside a folder: new entries go beside it.
    assert_eq!(parent(Some("notes/a.md")), Some("notes".to_string()));
    // A visible folder: new entries go inside it.
    assert_eq!(parent(Some("notes/deep")), Some("notes/deep".to_string()));
    // Root-level selection, or none: vault root.
    assert_eq!(parent(Some("root.md")), None);
    assert_eq!(parent(None), None);
    // A selection that no longer exists doesn't count.
    assert_eq!(parent(Some("notes/gone.md")), None);
    // Collapsing the folder hides the selection, so it doesn't count.
    let collapsed = std::collections::BTreeSet::new();
    assert_eq!(
        MdEditor::new_entry_parent(Some("notes/a.md"), &entries, &collapsed),
        None
    );
}

#[test]
fn test_find_heading_or_widget_line() {
    let text = "Line 0\n$$E = mc^2$$ \\label{equation-1}\nLine 2\n<div id=\"figure-1\">\nLine 4\n$$E = h\\nu$$ { #equation-2 }";
    let highlighted = crate::editor::highlight::highlight_markdown(text);
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
    let dyn_highlighted = crate::editor::highlight::highlight_markdown(dynamic_text);
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
    assert!(editor_command_keeps_cursor_visible(
        &EditorCommand::InsertText("\n".to_string())
    ));
}

#[test]
fn rename_updates_open_descendant_paths() {
    let mut path = Some("folder/sub/note.md".to_string());
    rename_open_path(&mut path, "folder", "renamed");
    assert_eq!(path.as_deref(), Some("renamed/sub/note.md"));
}

#[test]
fn pdf_search_scroll_targets_match_rect_not_just_page_top() {
    assert_eq!(
        crate::pdf_pane::search_match_scroll_y_from(1000.0, Some(250.0), 20.0, 792.0, 2.0, 5000.0),
        1948.0
    );
    assert_eq!(
        crate::pdf_pane::search_match_scroll_y_from(20.0, Some(780.0), 10.0, 792.0, 1.0, 5000.0),
        0.0
    );
}

#[test]
fn pdf_placeholder_size_scales_with_zoom() {
    assert_eq!(
        crate::pdf_pane::placeholder_display_size_from(Some((612.0, 792.0)), None, None, 2.0),
        (1224.0, 1584.0)
    );
}

#[test]
fn pdf_placeholder_prefers_first_page_size_over_rendered_dimensions() {
    assert_eq!(
        crate::pdf_pane::placeholder_display_size_from(
            Some((612.0, 792.0)),
            Some((300.0, 300.0)),
            Some((5000, 5000)),
            1.5,
        ),
        (918.0, 1188.0)
    );
}

#[test]
fn default_pdf_note_path_uses_pdf_name_page_and_annotation_prefix() {
    let ann = md_editor_core::pdf::PdfAnnotation {
        id: "abcdef123456".to_string(),
        document_id: "doc".to_string(),
        page_index: 4,
        kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
        color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
        selected_text: "Important field result".to_string(),
        ranges: vec![],
        rects: vec![],
        note: None,
        linked_note_path: None,
        markdown_anchor: None,
        created_at: 0,
        updated_at: 0,
    };

    let mut app = MdEditor::new_with_startup_file(None).0;
    app.pdf.active_path = Some("papers/My PDF File.pdf".to_string());
    assert_eq!(
        app.default_pdf_note_path(&ann),
        "pdf-notes/my-pdf-file-p5-abcdef12.md"
    );
}

#[test]
fn same_file_search_result_does_not_reload_open_buffer() {
    assert!(!search_result_should_reload(
        Some("notes/current.md"),
        "notes/current.md"
    ));
    assert!(search_result_should_reload(
        Some("notes/current.md"),
        "notes/other.md"
    ));
}

#[test]
fn stale_pdf_search_generation_is_rejected() {
    assert!(!pdf_search_generation_is_current(8, 7));
    assert!(pdf_search_generation_is_current(8, 8));
}

#[test]
fn editor_cursor_move_reclaims_active_panel_from_pdf() {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.pdf.render_generation = 8;
    app.search.pdf_error = Some("keep".to_string());
    let _ = app.update_inner(Message::Pdf(PdfMessage::SearchResult(
        7,
        Err("stale result".to_string()),
    )));
    assert_eq!(app.search.pdf_error.as_deref(), Some("keep"));

    app.active_panel = ActivePanel::Pdf;
    let _ = app.update_inner(Message::Editor(EditorMessage::Command(
        EditorCommand::SetCursor {
            line: 0,
            col: 0,
            affinity: crate::editor::buffer::Affinity::Downstream,
        },
    )));
    assert_eq!(app.active_panel, ActivePanel::Markdown);
}

#[test]
fn graph_toggle_and_escape_preserve_the_open_document_mode() {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.graph.visible = false;
    app.vault.root = Some("/test-vault".to_string());
    app.graph
        .set_snapshot(md_editor_core::types::GraphSnapshot::default());
    app.active_path = Some("notes/current.md".to_string());

    let _ = app.update_inner(Message::Graph(GraphMessage::Toggle));
    assert!(app.graph.visible);

    let _ = app.update_inner(Message::KeyboardShortcut(Shortcut::Escape));
    assert!(!app.graph.visible);
    assert_eq!(app.active_path.as_deref(), Some("notes/current.md"));
}

#[test]
fn graph_shortcut_is_a_noop_without_a_vault() {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.vault.root = None;
    app.graph.visible = false;

    let _ = app.update_inner(Message::Graph(GraphMessage::Toggle));

    assert!(!app.graph.visible);
    assert!(
        !app.title(iced::window::Id::unique())
            .contains("Knowledge Graph")
    );
}

#[test]
fn stale_pdf_identity_result_cannot_attach_to_another_document() {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.pdf.render_generation = 8;
    app.pdf.active_path = Some("current.pdf".to_string());
    app.pdf.document_id = None;

    let _ = app.update_inner(Message::Pdf(PdfMessage::DocumentIdComputed(
        7,
        Some((
            "current.pdf".to_string(),
            "stale-id".to_string(),
            10,
            Some(1),
        )),
    )));
    assert!(app.pdf.document_id.is_none());

    let _ = app.update_inner(Message::Pdf(PdfMessage::DocumentIdComputed(
        8,
        Some((
            "other.pdf".to_string(),
            "wrong-file".to_string(),
            10,
            Some(1),
        )),
    )));
    assert!(app.pdf.document_id.is_none());
}

#[test]
fn ctrl_shortcuts_a_focused_widget_captured_do_not_fire_app_actions() {
    use iced::event::Status;
    use iced::keyboard::{Key, Modifiers};
    let b = Key::Character("b".into());

    // Ctrl+B in the editor is bold (captured): the sidebar must not toggle.
    assert!(
        super::subscription::keyboard_message(&b, Modifiers::CTRL, Status::Captured, false, false)
            .is_none()
    );
    // Nothing focused claimed it: the app shortcut runs.
    assert!(matches!(
        super::subscription::keyboard_message(&b, Modifiers::CTRL, Status::Ignored, false, false),
        Some(Message::KeyboardShortcut(Shortcut::ToggleSidebar))
    ));
    // Unmodified keys and Escape keep working regardless.
    assert!(matches!(
        super::subscription::keyboard_message(
            &Key::Named(iced::keyboard::key::Named::Escape),
            Modifiers::empty(),
            Status::Captured,
            false,
            false,
        ),
        Some(Message::KeyboardShortcut(Shortcut::Escape))
    ));
}

#[test]
fn literal_replace_all_inserts_dollar_signs_verbatim() {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.active_path = Some("prices.md".to_string());
    app.editor.buffer = DocBuffer::from_text("price: PRICE");
    app.search.query = "price".to_string();
    app.search.replace = "$5".to_string();

    // Plain text, either case mode: the replacement is text, not a template.
    for match_case in [false, true] {
        app.editor.buffer = DocBuffer::from_text("price: price");
        app.search.match_case = match_case;
        app.search.regex = false;
        let (count, _) = app.replace_all_in_current_document().unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            app.editor.buffer.text(),
            "$5: $5",
            "match_case={match_case}"
        );
    }

    // Regex mode keeps capture-group expansion.
    app.editor.buffer = DocBuffer::from_text("a1 b2");
    app.search.regex = true;
    app.search.query = r"([a-z])(\d)".to_string();
    app.search.replace = "$2$1".to_string();
    let _ = app.replace_all_in_current_document().unwrap();
    assert_eq!(app.editor.buffer.text(), "1a 2b");
}

/// Parse a registry chord ("Ctrl Shift T", "F6", "Esc") into what the
/// keyboard subscription receives.
fn parse_chord(chord: &str) -> (iced::keyboard::Key, iced::keyboard::Modifiers) {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};
    let mut modifiers = Modifiers::empty();
    let mut key = None;
    for part in chord.split_whitespace() {
        match part {
            "Ctrl" => modifiers |= Modifiers::CTRL,
            "Shift" => modifiers |= Modifiers::SHIFT,
            "Tab" => key = Some(Key::Named(Named::Tab)),
            "Esc" => key = Some(Key::Named(Named::Escape)),
            "F2" => key = Some(Key::Named(Named::F2)),
            "F6" => key = Some(Key::Named(Named::F6)),
            "F11" => key = Some(Key::Named(Named::F11)),
            other => key = Some(Key::Character(other.to_lowercase().into())),
        }
    }
    (key.expect("chord names a key"), modifiers)
}

/// The shortcut registry is what menus, tooltips, the palette and Settings
/// show; the keyboard subscription must do what it says, or a tooltip lies.
#[test]
fn every_registry_chord_triggers_its_shortcut() {
    use crate::messages::{InkMessage, TabsMessage};
    for spec in crate::shortcuts::REGISTRY
        .iter()
        .filter(|s| !s.chord.is_empty())
    {
        let (key, modifiers) = parse_chord(spec.chord);
        let message = super::subscription::keyboard_message(
            &key,
            modifiers,
            iced::event::Status::Ignored,
            false,
            false,
        );
        let matches = match (&message, spec.shortcut) {
            (Some(Message::KeyboardShortcut(got)), want) => *got == want,
            // Equivalents the registry names by their user-facing shortcut.
            (Some(Message::Tabs(TabsMessage::MruNext)), Shortcut::NextTab)
            | (Some(Message::Tabs(TabsMessage::MruPrevious)), Shortcut::PreviousTab)
            | (Some(Message::Ink(InkMessage::Toggle)), Shortcut::Handwriting) => true,
            _ => false,
        };
        assert!(
            matches,
            "{} ({}) produced {message:?}",
            spec.label, spec.chord
        );
    }
}
