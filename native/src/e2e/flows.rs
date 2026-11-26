//! UI-flow scenarios from the e2e plan (S1, S3, S5–S10, S14): boot variants, navigation, search, palette, PDF, split view, graph and formatting. Data-safety scenarios live in `scenarios.rs`.

use std::time::Duration;

use super::scenarios::research_vault;
use super::{Session, Vault, boot, boot_with_path};
use crate::app::workspace::PaneId;
use crate::editor::buffer::{Affinity, EditorCommand};
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, Message, PdfMessage, SearchMessage, ShellMessage,
    Shortcut, VaultMessage,
};
use iced_program::Program;

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
        .file("notes/unsupported.xyz", "?")
        .file("slides/deck.pptx", "not a supported type");
    std::fs::create_dir_all(vault.root.join("images")).unwrap();
    std::fs::write(vault.root.join("images/dot.png"), tiny_png()).unwrap();
    let mut app = boot("s1-types", &vault, None);

    let entries = app.probe().entries;
    for expected in [
        "Index.md",
        "notes/Attention.md",
        "papers/dummy.pdf",
        "images/dot.png",
    ] {
        assert!(entries.iter().any(|e| e == expected), "{expected} missing");
    }
    for hidden in [
        "node_modules/ignored.md",
        ".hidden/secret.md",
        "notes/unsupported.xyz",
        "slides/deck.pptx",
    ] {
        assert!(!entries.iter().any(|e| e == hidden), "{hidden} listed");
    }

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
    // The query runs after a debounce; under a loaded test machine that can
    // take longer than any fixed wait, so poll for it (bounded).
    let mut results = Vec::new();
    for _ in 0..30 {
        app.wait(Duration::from_millis(100));
        app.settle();
        results = app.probe().search_results;
        if results.iter().any(|r| r == "notes/Optimizers.md") {
            break;
        }
    }
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

/// Pump the event loop until `done` holds for the probe, for up to 5 s.
/// Background work (PDF text, debounced search) has no fixed duration, and a
/// fixed wait flakes on a loaded machine.
pub(super) fn wait_for<P>(
    app: &mut Session<P>,
    what: &str,
    done: impl Fn(&crate::app::Probe) -> bool,
) -> crate::app::Probe
where
    P: Program<State = crate::app::MdEditor, Message = Message> + 'static,
{
    for _ in 0..50 {
        let probe = app.probe();
        if done(&probe) {
            return probe;
        }
        app.wait(Duration::from_millis(100));
    }
    let probe = app.probe();
    assert!(done(&probe), "timed out waiting for {what}: {probe:?}");
    probe
}

fn pdf(message: PdfMessage) -> Message {
    Message::Pdf(message)
}

/// `message` as `pane`'s view sends it (an interaction there).
fn in_pane(pane: PaneId, message: Message) -> Message {
    Message::FromPane(pane, Box::new(message))
}

/// BUG-01, with panes: linking a highlight to the note open beside the PDF
/// keeps the typing and the undo history. Leaving the note's pane saves it;
/// the link is one more undo step in it.
#[test]
fn s8c_link_highlight_to_open_note_keeps_unsaved_edits() {
    let vault = research_vault().fixture("papers/large.pdf", "pdf/large-500-pages.pdf");
    let mut app = boot("s8c-link-open-note", &vault, Some("papers/large.pdf"));
    wait_for(&mut app, "page text and document id", |p| {
        p.pdf_texts_loaded > 0 && p.pdf_document_ready
    });

    // First highlight, linked to Index.md: opens it in a pane beside the PDF.
    app.send(pdf(PdfMessage::SelectionFinished(0, 0, 3)));
    app.send(pdf(PdfMessage::QuickHighlight));
    let first = app.probe().pdf_annotation_ids[0].clone();
    app.send(pdf(PdfMessage::LinkNote(first, "Index.md".to_string())));
    let probe = app.probe();
    assert!(probe.split_view, "{probe:?}");
    assert_eq!(probe.focused_pane, PaneId::B, "the note's pane has focus");
    assert_eq!(probe.active_path.as_deref(), Some("Index.md"));
    assert_eq!(probe.other_pane_path.as_deref(), Some("papers/large.pdf"));

    // Type in the note.
    app.send(edit(EditorCommand::SelectAll));
    app.send(edit(EditorCommand::MoveCursor {
        movement: crate::editor::buffer::Movement::Right,
        extend: false,
    }));
    app.send_quick(edit(EditorCommand::InsertText(
        "\n\nUNSAVED THOUGHT".to_string(),
    )));
    assert!(app.probe().dirty);

    // Back to the PDF: the note is saved as its pane loses focus.
    app.send_quick(in_pane(
        PaneId::A,
        pdf(PdfMessage::SelectionFinished(0, 5, 5)),
    ));
    assert_eq!(app.probe().focused_pane, PaneId::A);
    assert!(
        vault.read("Index.md").contains("UNSAVED THOUGHT"),
        "leaving a pane must save its note"
    );
    app.send_quick(in_pane(PaneId::A, pdf(PdfMessage::QuickHighlight)));
    let ids = app.probe().pdf_annotation_ids;
    let second = ids.iter().find(|id| **id != ids[0]).unwrap().clone();
    app.send(in_pane(
        PaneId::A,
        pdf(PdfMessage::LinkNote(second.clone(), "Index.md".to_string())),
    ));

    // The note's pane comes forward with both, saved.
    let probe = app.probe();
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(probe.active_path.as_deref(), Some("Index.md"));
    assert!(probe.buffer.contains("UNSAVED THOUGHT"), "{}", probe.buffer);
    assert!(probe.buffer.contains(&second), "second section missing");
    assert!(!probe.dirty, "linking should flush the note");
    assert_eq!(
        vault.read("Index.md"),
        probe.buffer,
        "disk differs from editor"
    );

    // The link is one undo step; the typing is still undoable after it.
    app.send(edit(EditorCommand::Undo));
    let probe = app.probe();
    assert!(!probe.buffer.contains(&second), "undo kept the section");
    assert!(probe.buffer.contains("UNSAVED THOUGHT"));
    app.send(edit(EditorCommand::Undo));
    assert!(
        !app.probe().buffer.contains("UNSAVED THOUGHT"),
        "typing not undoable"
    );
    app.finish();
}

/// S8b: read a PDF like a student — select, highlight (kept across a
/// restart), search from the page being read, and keep searching while
/// switching documents with find open.
#[test]
fn s8b_pdf_select_highlight_and_search() {
    let vault = research_vault().fixture("papers/large.pdf", "pdf/large-500-pages.pdf");
    let mut app = boot("s8b-pdf-study", &vault, Some("papers/large.pdf"));
    wait_for(&mut app, "page text and document id", |p| {
        p.pdf_texts_loaded > 0 && p.pdf_document_ready
    });

    // Page 1 reads "Page 1 of 500 …": select the first word.
    app.send(pdf(PdfMessage::SelectionFinished(0, 0, 3)));
    assert_eq!(app.probe().pdf_selected_text.as_deref(), Some("Page"));
    app.send(pdf(PdfMessage::QuickHighlight));
    let probe = app.probe();
    assert_eq!(probe.pdf_annotations, 1, "highlight not created");
    assert_eq!(
        probe.pdf_selected_text, None,
        "selection kept after highlighting"
    );
    app.screenshot("highlight");

    let mut app = app.restart("s8b-pdf-study-restart", None);
    app.send(open("papers/large.pdf"));
    wait_for(&mut app, "the highlight to reload", |p| {
        p.pdf_annotations == 1
    });

    // Find opens on page 1; jump to page 120 and search from there.
    app.send(shortcut(Shortcut::Search));
    app.send(Message::Search(SearchMessage::QueryChanged(
        "Stress fixture".to_string(),
    )));
    wait_for(&mut app, "search results", |p| p.pdf_search_results == 500);
    app.send(pdf(PdfMessage::PageInputChanged("120".to_string())));
    app.send(pdf(PdfMessage::PageInputSubmit));
    assert_eq!(app.probe().pdf_page, 119);
    app.send(Message::Search(SearchMessage::Next));
    assert_eq!(
        app.probe().pdf_match_index,
        Some(119),
        "the first Next should land on the page being read, not page 1"
    );

    // Switching PDFs with find open searches the new document; nothing stale.
    app.send(open("papers/dummy.pdf"));
    let probe = wait_for(&mut app, "dummy.pdf to load", |p| p.pdf_pages > 0);
    assert_eq!(probe.pdf_search_results, 0, "stale matches from large.pdf");
    app.send(open("papers/large.pdf"));
    wait_for(&mut app, "large.pdf to be searched again", |p| {
        p.pdf_search_results == 500
    });
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

/// S9: Ctrl+\\ splits a note and a PDF into two panes; touching a pane
/// focuses it, and Ctrl+F searches the focused pane.
#[test]
fn s9_split_view_routes_search_to_the_active_pane() {
    let vault = research_vault();
    let mut app = boot("s9-split", &vault, Some("Index.md"));
    app.send(open("papers/dummy.pdf"));
    app.send(open("Index.md"));
    app.send(Message::Shell(ShellMessage::SplitViewToggle));
    let probe = app.probe();
    assert!(probe.split_view, "split view did not open");
    assert_eq!(probe.focused_pane, PaneId::A);
    assert_eq!(probe.active_path.as_deref(), Some("Index.md"));
    assert_eq!(
        probe.other_pane_path.as_deref(),
        Some("papers/dummy.pdf"),
        "the PDF moved to the new pane"
    );
    app.screenshot("split");

    app.send(in_pane(
        PaneId::B,
        Message::Pdf(PdfMessage::LeftClicked(
            0,
            20.0,
            20.0,
            iced::keyboard::Modifiers::empty(),
        )),
    ));
    let probe = app.probe();
    assert_eq!(
        probe.focused_pane,
        PaneId::B,
        "clicking the PDF focused its pane"
    );
    assert!(probe.pdf_panel_active);
    // Its pages render in the second pane: results find their way back there.
    wait_for(&mut app, "pages in the second pane", |p| p.pdf_rendered > 0);
    app.screenshot("split-rendered");

    app.send(in_pane(
        PaneId::A,
        edit(EditorCommand::SetCursor {
            line: 0,
            col: 0,
            affinity: Affinity::Downstream,
        }),
    ));
    let probe = app.probe();
    assert_eq!(probe.focused_pane, PaneId::A, "the note took focus back");
    assert!(!probe.pdf_panel_active);
    app.send(shortcut(Shortcut::Search));
    assert!(app.probe().file_search_visible);

    // Unsplitting brings the PDF's tab back beside the note's.
    app.send(Message::Shell(ShellMessage::SplitViewToggle));
    let probe = app.probe();
    assert!(!probe.split_view);
    assert!(
        probe.open_tabs.contains(&"papers/dummy.pdf".to_string()),
        "{probe:?}"
    );
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

/// Draw one straight mouse stroke on the canvas without letting the event
/// loop run, so autosave has not fired when it returns.
fn draw_stroke<P>(app: &mut Session<P>, start: (f32, f32), end: (f32, f32))
where
    P: Program<State = crate::app::MdEditor, Message = Message> + 'static,
{
    app.send_quick(ink(InkMessage::PointerDown(start.0, start.1)));
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        app.send_quick(ink(InkMessage::PointerMove(
            start.0 + (end.0 - start.0) * t,
            start.1 + (end.1 - start.1) * t,
        )));
    }
    app.send_quick(ink(InkMessage::PointerUp));
}

/// S12b: handwritten pages are never lost — autosaved without Ctrl+S, a new
/// page beside a note never overwrites the note's existing page, and opening
/// another page writes the current one first.
#[test]
fn s12b_handwriting_is_never_lost() {
    let vault = research_vault();
    let mut app = boot("s12b-ink-durability", &vault, Some("notes/Attention.md"));

    app.send(ink(InkMessage::Toggle));
    draw_stroke(&mut app, (40.0, 40.0), (200.0, 120.0));
    assert!(app.probe().ink_dirty);
    app.wait(Duration::from_millis(1500));
    let probe = app.probe();
    assert!(!probe.ink_dirty, "the page was not autosaved");
    assert_eq!(probe.ink_path.as_deref(), Some("notes/Attention.ink"));
    let first_page = vault.read("notes/Attention.ink");

    // A fresh session starts a new page for the same note.
    let mut app = app.restart("s12b-ink-durability-restart", None);
    app.send(ink(InkMessage::Toggle));
    draw_stroke(&mut app, (60.0, 200.0), (260.0, 220.0));
    app.wait(Duration::from_millis(1500));
    let second = app.probe().ink_path.expect("second page saved");
    assert_eq!(second, "notes/Attention-2.ink");
    assert_eq!(
        vault.read("notes/Attention.ink"),
        first_page,
        "the note's first page was overwritten"
    );

    // Unsaved strokes survive switching to another page.
    draw_stroke(&mut app, (60.0, 300.0), (260.0, 320.0));
    assert!(app.probe().ink_dirty);
    app.send_quick(open("notes/Attention.ink"));
    let probe = app.probe();
    assert_eq!(probe.ink_path.as_deref(), Some("notes/Attention.ink"));
    assert_eq!(probe.ink_strokes, 1);
    app.send(open(&second));
    assert_eq!(
        app.probe().ink_strokes,
        2,
        "strokes drawn before switching pages were lost"
    );
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

fn paste(payload: crate::platform::ClipboardPayload) -> Message {
    crate::platform::test_clipboard::set(payload);
    Message::Editor(EditorMessage::Paste)
}

/// I2: pasting a bitmap stores a PNG under attachments/, inserts an image
/// line (one undo step), shows the file in the tree and renders it.
#[test]
fn paste_screenshot_into_a_note() {
    let vault = research_vault();
    let mut app = boot("paste-bitmap", &vault, Some("notes/Attention.md"));
    app.send(edit(EditorCommand::SetCursor {
        line: 2,
        col: 0,
        affinity: Affinity::Downstream,
    }));

    let rgba = [[20u8, 120, 220, 255]; 64 * 32].concat();
    app.send(paste(crate::platform::ClipboardPayload::Image {
        width: 64,
        height: 32,
        rgba,
    }));
    let probe = app.probe();
    let stored = probe
        .entries
        .iter()
        .find(|e| e.starts_with("attachments/Pasted image "))
        .cloned()
        .expect("pasted image not in the tree");
    let decoded = image::open(vault.root.join(&stored)).expect("a readable PNG");
    assert_eq!((decoded.width(), decoded.height()), (64, 32));
    let link = format!("![](<../{stored}>)");
    assert!(
        probe.buffer.contains(&link),
        "{link} not in {:?}",
        probe.buffer
    );
    assert!(
        probe
            .toast
            .is_some_and(|t| t.starts_with("Added attachments/"))
    );
    app.screenshot("pasted");

    // One undo removes the line; the file stays (losing a screenshot is worse
    // than an orphan).
    app.send(edit(EditorCommand::Undo));
    assert!(!app.probe().buffer.contains("Pasted image"));
    assert!(vault.exists(&stored));

    // Pasting the same image again reuses the file.
    let rgba = [[20u8, 120, 220, 255]; 64 * 32].concat();
    app.send(paste(crate::platform::ClipboardPayload::Image {
        width: 64,
        height: 32,
        rgba,
    }));
    let count = app
        .probe()
        .entries
        .iter()
        .filter(|e| e.starts_with("attachments/"))
        .count();
    assert_eq!(count, 1, "the second paste should reuse the stored image");
    app.finish();
}

/// I3: pasting copied files imports images (copying those outside the vault,
/// linking those inside it), links notes, and refuses other files.
#[test]
fn paste_copied_files() {
    let vault = research_vault();
    let outside = Vault::new("paste-outside");
    std::fs::write(outside.root.join("photo.png"), tiny_png()).unwrap();
    std::fs::write(outside.root.join("data.csv"), "a,b").unwrap();
    std::fs::create_dir_all(vault.root.join("img")).unwrap();
    std::fs::write(vault.root.join("img/in vault.png"), tiny_png()).unwrap();
    let mut app = boot("paste-files", &vault, Some("Index.md"));

    app.send(paste(crate::platform::ClipboardPayload::Files(vec![
        outside.root.join("photo.png"),
        vault.root.join("img/in vault.png"),
        vault.root.join("notes/Optimizers.md"),
    ])));
    let probe = app.probe();
    assert!(
        vault.exists("attachments/photo.png"),
        "outside image not copied"
    );
    assert!(
        !vault.exists("attachments/in vault.png"),
        "in-vault image was copied"
    );
    for expected in [
        "![](attachments/photo.png)",
        "![](<img/in vault.png>)",
        "[[notes/Optimizers]]",
    ] {
        assert!(
            probe.buffer.contains(expected),
            "{expected} missing: {:?}",
            probe.buffer
        );
    }

    // Only non-images from outside: refused, nothing inserted.
    let before = app.probe().buffer;
    app.send(paste(crate::platform::ClipboardPayload::Files(vec![
        outside.root.join("data.csv"),
    ])));
    let probe = app.probe();
    assert_eq!(probe.buffer, before);
    assert_eq!(probe.toast.as_deref(), Some("Only images can be inserted"));
    app.finish();
}

/// S12c: Ctrl+Shift+I in a note starts a sketch beside it, shows it in the
/// note at the caret, and the note's picture follows the drawing as the page
/// autosaves.
#[test]
fn s12c_sketch_in_a_note_follows_the_drawing() {
    let vault = research_vault();
    let mut app = boot("s12c-sketch", &vault, Some("notes/Attention.md"));

    app.send(shortcut(Shortcut::InsertSketch));
    let probe = app.probe();
    assert!(probe.ink_window_open);
    assert_eq!(
        probe.ink_path.as_deref(),
        Some("notes/Attention-sketch.ink")
    );
    assert!(
        vault.exists("notes/Attention-sketch.ink"),
        "the page exists at once"
    );
    assert!(
        probe.buffer.contains("![sketch](Attention-sketch.ink)"),
        "{}",
        probe.buffer
    );
    assert!(
        probe
            .images
            .iter()
            .all(|(key, _)| key != "Attention-sketch.ink"),
        "an empty page shows nothing yet"
    );

    draw_stroke(&mut app, (40.0, 40.0), (240.0, 60.0));
    draw_stroke(&mut app, (40.0, 120.0), (240.0, 100.0));
    // Past the ink autosave (1 s), then let the redraw land.
    app.wait(Duration::from_millis(1500));
    app.settle();
    let width = app
        .probe()
        .images
        .iter()
        .find(|(key, _)| key == "Attention-sketch.ink")
        .map(|(_, width)| *width)
        .expect("the note shows the sketch once it has ink");
    assert!(
        width > 200.0,
        "cropped to the strokes plus margins: {width}"
    );

    // A second sketch in the same note gets its own page.
    app.send(shortcut(Shortcut::InsertSketch));
    assert_eq!(
        app.probe().ink_path.as_deref(),
        Some("notes/Attention-sketch-2.ink")
    );
    app.finish();
}
