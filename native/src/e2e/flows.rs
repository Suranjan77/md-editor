//! UI-flow scenarios from the e2e plan (S1, S3, S5–S10, S14): boot variants, navigation, search, palette, PDF, split view, graph and formatting. Data-safety scenarios live in `scenarios.rs`.

use std::time::Duration;

use super::scenarios::research_vault;
use super::{Vault, boot, boot_with_path};
use crate::editor::buffer::{Affinity, EditorCommand};
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, Message, PdfMessage, SearchMessage, ShellMessage,
    Shortcut, VaultMessage,
};

fn open(path: &str) -> Message {
    Message::Vault(VaultMessage::SidebarFileClicked(path.to_string()))
}

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

fn select(line: usize, from: usize, to: usize) -> Message {
    edit(EditorCommand::SetSelection {
        anchor_line: line,
        anchor_col: from,
        focus_line: line,
        focus_col: to,
        affinity: Affinity::Downstream,
    })
}

fn shortcut(s: Shortcut) -> Message {
    Message::KeyboardShortcut(s)
}

/// A 1×1 PNG, written with the `image` crate.
fn tiny_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([200, 30, 30, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

/// S1: a vault with every file type boots with a tree that matches disk (the
/// invariant check), skipping dot and heavy folders; every type opens.
#[test]
fn s1_boot_vault_with_every_file_type() {
    let vault = research_vault()
        .file("node_modules/ignored.md", "# ignored\n")
        .file(".hidden/secret.md", "# hidden\n")
        .file("notes/unsupported.xyz", "?");
    std::fs::create_dir_all(vault.root.join("images")).unwrap();
    std::fs::write(vault.root.join("images/dot.png"), tiny_png()).unwrap();
    let mut app = boot("s1-types", &vault, None);

    let entries = app.probe().entries;
    for expected in [
        "Index.md",
        "notes/Attention.md",
        "papers/dummy.pdf",
        "slides/deck.pptx",
        "images/dot.png",
    ] {
        assert!(entries.iter().any(|e| e == expected), "{expected} missing");
    }
    for hidden in [
        "node_modules/ignored.md",
        ".hidden/secret.md",
        "notes/unsupported.xyz",
    ] {
        assert!(!entries.iter().any(|e| e == hidden), "{hidden} listed");
    }

    app.send(open("slides/deck.pptx"));
    assert!(app.probe().pptx_loaded, "deck did not load");
    app.send(open("images/dot.png"));
    app.screenshot("image");
    app.send(open("papers/dummy.pdf"));
    assert!(app.probe().pdf_pages > 0, "pdf did not load");
    app.send(open("Index.md"));
    assert_eq!(app.probe().active_path.as_deref(), Some("Index.md"));
    app.finish();
}

/// S1: `md-editor <file>` opens that file, with its folder as the vault.
#[test]
fn s1_boot_with_a_file_path() {
    let vault = research_vault();
    let app = boot_with_path("s1-file", vault.root.join("notes/Attention.md"));
    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some("Attention.md"));
    assert_eq!(
        probe.vault_root.as_deref(),
        Some(vault.root.join("notes").to_string_lossy().as_ref())
    );
    app.finish();
}

/// S1: a path that does not exist must not crash or open anything odd.
#[test]
fn s1_boot_with_a_missing_path() {
    let vault = Vault::new("s1-missing");
    let gone = vault.root.join("nope/missing.md");
    let mut app = boot_with_path("s1-missing", gone);
    app.screenshot("missing");
    let probe = app.finish();
    assert_eq!(probe.active_path, None);
}

/// S3: link navigation flushes edits, and coming back keeps undo history.
#[test]
fn s3_link_navigation_keeps_undo_history() {
    let vault = research_vault();
    let mut app = boot("s3-nav", &vault, Some("Index.md"));

    // A wikilink click publishes the raw target; it resolves vault-wide.
    app.send(open("Attention"));
    assert_eq!(
        app.probe().active_path.as_deref(),
        Some("notes/Attention.md")
    );

    app.send_quick(edit(EditorCommand::InsertText("typed ".to_string())));
    app.send(open("Index"));
    assert_eq!(app.probe().active_path.as_deref(), Some("Index.md"));
    assert!(
        vault.read("notes/Attention.md").contains("typed "),
        "edits were not flushed when navigating away"
    );

    app.send(open("notes/Attention.md"));
    app.send(edit(EditorCommand::Undo));
    let probe = app.probe();
    assert!(
        !probe.buffer.contains("typed "),
        "undo history did not survive the round trip"
    );

    // Heading anchors open the file too.
    app.send(open("Index#Research Index"));
    assert_eq!(app.probe().active_path.as_deref(), Some("Index.md"));
    app.finish();
}

/// S5: vault search finds notes after the debounce and opens a result.
#[test]
fn s5_vault_search() {
    let vault = research_vault();
    let mut app = boot("s5-search", &vault, Some("Index.md"));

    app.send(Message::Search(SearchMessage::GlobalSearchOpen));
    app.send(Message::Search(SearchMessage::QueryChanged(
        "moving averages".to_string(),
    )));
    app.wait(Duration::from_millis(500));
    app.settle();
    let results = app.probe().search_results;
    assert!(
        results.iter().any(|r| r == "notes/Optimizers.md"),
        "search results: {results:?}"
    );
    app.screenshot("results");

    app.send(Message::Search(SearchMessage::ResultClicked(
        "notes/Optimizers.md".to_string(),
    )));
    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some("notes/Optimizers.md"));
    assert!(!probe.search_visible);
    app.finish();
}

/// S6 / H4: in-note replace is one undoable edit; an invalid regex leaves the
/// note alone and says why.
#[test]
fn s6_find_and_replace() {
    let vault = research_vault();
    let mut app = boot("s6-replace", &vault, Some("notes/Optimizers.md"));
    let original = app.probe().buffer;

    app.send(shortcut(Shortcut::Search));
    app.send(Message::Search(SearchMessage::QueryChanged(
        "Adam".to_string(),
    )));
    app.send(Message::Search(SearchMessage::ReplaceChanged(
        "AdamW".to_string(),
    )));
    app.send(Message::Search(SearchMessage::ReplaceAll));
    let replaced = app.probe().buffer;
    assert!(replaced.contains("AdamW keeps"), "{replaced:?}");
    assert!(
        vault.read("notes/Optimizers.md").contains("AdamW"),
        "not saved"
    );

    app.send(edit(EditorCommand::Undo));
    assert_eq!(
        app.probe().buffer,
        original,
        "replace-all was not one undo step"
    );

    app.send(Message::Search(SearchMessage::RegexToggled(true)));
    app.send(Message::Search(SearchMessage::QueryChanged(
        "(unclosed".to_string(),
    )));
    app.expect_error_toast();
    app.send(Message::Search(SearchMessage::ReplaceAll));
    let probe = app.probe();
    assert_eq!(probe.buffer, original);
    assert!(probe.toast.is_some_and(|t| t.contains("Invalid regex")));
    app.finish();
}

/// S7: the palette opens files by fuzzy name and closes on Escape.
#[test]
fn s7_command_palette() {
    let vault = research_vault();
    let mut app = boot("s7-palette", &vault, Some("Index.md"));

    app.send(Message::Shell(ShellMessage::CommandPaletteOpen));
    assert!(app.probe().palette_open);
    app.send(Message::Shell(ShellMessage::CommandPaletteQueryChanged(
        "optim".to_string(),
    )));
    app.screenshot("palette");
    app.send(Message::Shell(ShellMessage::OverlaySubmitCurrent));
    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some("notes/Optimizers.md"));
    assert!(!probe.palette_open);

    // No match: Enter does nothing, Escape closes.
    app.send(Message::Shell(ShellMessage::CommandPaletteOpen));
    app.send(Message::Shell(ShellMessage::CommandPaletteQueryChanged(
        "zzzz-no-such-thing".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::OverlaySubmitCurrent));
    assert_eq!(
        app.probe().active_path.as_deref(),
        Some("notes/Optimizers.md")
    );
    app.send(shortcut(Shortcut::Escape));
    assert!(!app.probe().palette_open);
    app.finish();
}

/// S8 / H10: a PDF opens, renders, zooms and fits; switching away while
/// pages are still rendering leaves nothing pending (invariant check).
#[test]
fn s8_pdf_open_zoom_and_switch_mid_render() {
    let vault = research_vault().fixture("papers/large.pdf", "pdf/large-500-pages.pdf");
    let mut app = boot("s8-pdf", &vault, Some("papers/dummy.pdf"));
    let probe = app.probe();
    assert!(probe.pdf_pages > 0 && probe.pdf_rendered > 0, "{probe:?}");
    app.screenshot("pdf");

    app.send(Message::Pdf(PdfMessage::ZoomChanged(1.5)));
    assert!((app.probe().pdf_zoom - 1.5).abs() < 0.01);
    app.send(Message::Pdf(PdfMessage::FitToWidth));
    assert!(app.probe().pdf_rendered > 0);

    // Open a big PDF and leave before it can finish rendering.
    app.send_quick(open("papers/large.pdf"));
    app.send(open("Index.md"));
    assert_eq!(app.probe().active_path.as_deref(), Some("Index.md"));

    // And back: the big one opens cleanly.
    app.send(open("papers/large.pdf"));
    let probe = app.probe();
    assert_eq!(probe.pdf_pages, 500);
    assert!(probe.pdf_rendered > 0);
    app.finish();
}

/// S8: PDF text search finds matches.
#[test]
fn s8_pdf_search() {
    let vault = research_vault();
    let mut app = boot("s8-pdf-search", &vault, Some("papers/dummy.pdf"));
    app.send(shortcut(Shortcut::Search));
    app.send(Message::Search(SearchMessage::QueryChanged(
        "dummy".to_string(),
    )));
    app.wait(Duration::from_millis(500));
    app.settle();
    assert!(app.probe().pdf_search_results > 0, "no PDF search results");
    app.finish();
}

/// S9: in split view, Ctrl+F searches whichever pane was touched last.
#[test]
fn s9_split_view_routes_search_to_the_active_pane() {
    let vault = research_vault();
    let mut app = boot("s9-split", &vault, Some("Index.md"));
    app.send(open("papers/dummy.pdf"));
    app.send(open("Index.md"));
    app.send(Message::Shell(ShellMessage::SplitViewToggle));
    let probe = app.probe();
    assert!(probe.split_view, "split view did not open");
    assert_eq!(probe.active_path.as_deref(), Some("Index.md"));
    assert!(probe.pdf_path.is_some(), "the PDF pane is empty");
    app.screenshot("split");

    app.send(Message::Pdf(PdfMessage::LeftClicked(
        0,
        20.0,
        20.0,
        iced::keyboard::Modifiers::empty(),
    )));
    assert!(
        app.probe().pdf_panel_active,
        "clicking the PDF did not focus it"
    );

    app.send(edit(EditorCommand::SetCursor {
        line: 0,
        col: 0,
        affinity: Affinity::Downstream,
    }));
    assert!(
        !app.probe().pdf_panel_active,
        "the editor did not take focus back"
    );
    app.send(shortcut(Shortcut::Search));
    assert!(app.probe().file_search_visible);
    app.finish();
}

/// S10: the graph opens with nodes, switches scope, opens a node, and Escape
/// closes it.
#[test]
fn s10_knowledge_graph() {
    let vault = research_vault();
    let mut app = boot("s10-graph", &vault, Some("Index.md"));

    app.send(Message::Graph(GraphMessage::Toggle));
    app.wait(Duration::from_millis(300));
    app.settle();
    let probe = app.probe();
    assert!(probe.graph_visible);
    assert!(
        probe.graph_nodes >= 3,
        "graph has {} nodes",
        probe.graph_nodes
    );
    app.screenshot("graph");

    app.send(Message::Graph(GraphMessage::ScopeLocal));
    assert!(app.probe().graph_local);
    app.send(Message::Graph(GraphMessage::ScopeGlobal));

    app.send(Message::Graph(GraphMessage::NodeOpen(
        "notes/Attention.md".to_string(),
    )));
    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some("notes/Attention.md"));

    app.send(Message::Graph(GraphMessage::Toggle));
    app.send(shortcut(Shortcut::Escape));
    assert!(!app.probe().graph_visible);
    app.finish();
}

/// S14: formatting commands wrap the selection and the checkbox toggles.
#[test]
fn s14_formatting_commands() {
    let vault = Vault::new("s14-format").file("F.md", "alpha beta gamma\n- [ ] task\n");
    let mut app = boot("s14-format", &vault, Some("F.md"));

    app.send(select(0, 0, 5));
    app.send(edit(EditorCommand::FormatBold));
    assert!(
        app.probe().buffer.starts_with("**alpha** beta"),
        "{:?}",
        app.probe().buffer
    );

    app.send(select(0, 10, 14));
    app.send(edit(EditorCommand::FormatItalic));
    assert!(
        app.probe().buffer.contains("*beta*"),
        "{:?}",
        app.probe().buffer
    );

    let line0 = app.probe().buffer.lines().next().unwrap().to_string();
    let start = line0.find("gamma").unwrap();
    app.send(select(0, start, start + 5));
    app.send(edit(EditorCommand::FormatInlineCode));
    assert!(
        app.probe().buffer.contains("`gamma`"),
        "{:?}",
        app.probe().buffer
    );

    app.send(Message::Editor(EditorMessage::CheckboxToggle(1)));
    assert!(
        app.probe().buffer.contains("- [x] task"),
        "{:?}",
        app.probe().buffer
    );

    // Undo walks the formatting back one step at a time.
    app.send(edit(EditorCommand::Undo));
    assert!(app.probe().buffer.contains("- [ ] task"));

    app.wait(Duration::from_millis(600));
    app.settle();
    assert_eq!(vault.read("F.md"), app.probe().buffer, "not saved");
    app.finish();
}

/// S1: the remembered vault was deleted between launches. The app says so
/// and does not try to open the remembered file in no vault.
#[test]
fn s1_remembered_vault_was_deleted() {
    let vault = research_vault();
    let app = boot("s1-gone-1", &vault, Some("notes/Attention.md"));
    let root = vault.root.to_string_lossy().into_owned();
    drop(vault);

    let mut app = app.restart("s1-gone-2", None);
    app.screenshot("gone");
    let probe = app.finish();
    assert_eq!(probe.active_path, None);
    assert_eq!(probe.vault_root, None);
    let toast = probe.toast.unwrap_or_default();
    assert!(
        toast.contains("moved or deleted") && toast.contains(&root),
        "unexpected toast {toast:?}"
    );
}

fn ink(message: InkMessage) -> Message {
    Message::Ink(message)
}

/// S12 / H6: draw, undo, redo and save a handwritten page beside the open
/// note; it appears in the tree at once (the invariant compares the tree
/// with disk) and reopens with its strokes.
#[test]
fn s12_handwriting_save_and_reopen() {
    let vault = research_vault();
    let mut app = boot("s12-ink", &vault, Some("notes/Attention.md"));

    app.send(ink(InkMessage::Toggle));
    assert!(app.probe().ink_window_open);
    for (start, end) in [
        ((40.0, 40.0), (200.0, 120.0)),
        ((60.0, 200.0), (260.0, 220.0)),
    ] {
        app.send_quick(ink(InkMessage::PointerDown(start.0, start.1)));
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            app.send_quick(ink(InkMessage::PointerMove(
                start.0 + (end.0 - start.0) * t,
                start.1 + (end.1 - start.1) * t,
            )));
        }
        app.send(ink(InkMessage::PointerUp));
    }
    assert_eq!(app.probe().ink_strokes, 2);
    app.screenshot("strokes");

    app.send(ink(InkMessage::Undo));
    assert_eq!(app.probe().ink_strokes, 1);
    app.send(ink(InkMessage::Redo));
    assert_eq!(app.probe().ink_strokes, 2);

    app.send(ink(InkMessage::Save));
    let probe = app.probe();
    let saved = probe
        .ink_path
        .clone()
        .expect("the page has a path after saving");
    assert!(!probe.ink_dirty);
    assert!(
        saved.starts_with("notes/"),
        "not saved beside the note: {saved}"
    );
    assert!(vault.exists(&saved), "{saved} not on disk");
    assert!(
        probe.entries.contains(&saved),
        "{saved} not in the tree yet"
    );

    // Close, then reopen from the sidebar.
    app.send(ink(InkMessage::Toggle));
    app.send(open(&saved));
    let probe = app.probe();
    assert_eq!(probe.ink_path.as_deref(), Some(saved.as_str()));
    assert_eq!(probe.ink_strokes, 2, "strokes lost on reopen");
    app.finish();
}

/// A long note mixing every block type, `lines` lines long.
fn large_note(lines: usize) -> String {
    let block = "## Section\n\nSome **bold** text with a [[link]] and `code` and $x^2$.\n\n- [ ] a task\n- item\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quote\n\n";
    let per = block.lines().count();
    block.repeat(lines / per + 1)
}

/// S13 / H9: a 20k-line note opens, takes typing and scrolls within the
/// per-update budget (checked by the invariant after every step).
#[test]
#[ignore = "slow (~20 s): run before releases with `cargo test -p md-editor-native s13 -- --ignored`"]
fn s13_large_note() {
    let vault = Vault::new("s13-note").file("Big.md", &large_note(20_000));
    let mut app = boot("s13-note", &vault, None);
    // Opening highlights the whole note synchronously: a one-off cost, given
    // its own budget (debug build). Keystrokes get the default 100 ms.
    app.set_update_budget(Duration::from_millis(400));
    app.send(open("Big.md"));
    app.set_update_budget(Duration::from_millis(100));
    assert!(app.probe().buffer.lines().count() >= 20_000);

    for i in 0..5 {
        app.send(edit(EditorCommand::SetCursor {
            line: i * 4_000,
            col: 0,
            affinity: Affinity::Downstream,
        }));
        app.send(edit(EditorCommand::InsertText("typed ".to_string())));
    }
    // Whole-document edits rehighlight everything: one-off budget again.
    app.set_update_budget(Duration::from_millis(400));
    app.send(edit(EditorCommand::SelectAll));
    app.send(edit(EditorCommand::FormatBold));
    app.send(edit(EditorCommand::Undo));
    app.wait(Duration::from_millis(600));
    app.settle();
    assert!(vault.read("Big.md").contains("typed "));
    app.finish();
}

/// S13: a 2,000-note vault boots, searches and opens within budget.
#[test]
#[ignore = "slow (~25 s): run before releases with `cargo test -p md-editor-native s13 -- --ignored`"]
fn s13_large_vault() {
    let mut vault = Vault::new("s13-vault");
    for i in 0..2_000 {
        vault = vault.file(
            &format!("folder{}/note{i}.md", i % 40),
            &format!(
                "# Note {i}\n\nLinks to [[note{}]] and [[note{}]].\n\nunique-token-{i}\n",
                (i + 1) % 2_000,
                (i * 7) % 2_000
            ),
        );
    }
    let mut app = boot("s13-vault", &vault, None);
    assert_eq!(
        app.probe()
            .entries
            .iter()
            .filter(|e| e.ends_with(".md"))
            .count(),
        2_000
    );

    app.send(Message::Search(SearchMessage::GlobalSearchOpen));
    app.send(Message::Search(SearchMessage::QueryChanged(
        "unique-token-1234".to_string(),
    )));
    app.wait(Duration::from_millis(500));
    app.settle();
    assert!(
        app.probe()
            .search_results
            .iter()
            .any(|r| r == "folder34/note1234.md")
    );

    app.send(open("note999"));
    assert_eq!(
        app.probe().active_path.as_deref(),
        Some("folder39/note999.md")
    );
    app.send(Message::Graph(GraphMessage::Toggle));
    app.wait(Duration::from_millis(500));
    app.settle();
    assert!(app.probe().graph_nodes >= 2_000);
    app.finish();
}
