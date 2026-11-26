//! Headless end-to-end scenario for Wave 4 synthesis features: `@` citations, `[[note#heading` completion, inline math evaluation, and opt-in code block execution.

use std::time::Duration;

use super::boot;
use super::flows::wait_for;
use super::scenarios::research_vault;
use crate::editor::buffer::{Affinity, EditorCommand};
use crate::messages::{EditorMessage, Message, Shortcut};

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

fn type_str(
    app: &mut super::Session<
        impl iced_program::Program<State = crate::app::MdEditor, Message = Message> + 'static,
    >,
    s: &str,
) {
    for ch in s.chars() {
        app.send_quick(edit(EditorCommand::InsertText(ch.to_string())));
    }
    app.settle();
}

#[test]
fn w4_citations_headings_math_eval_and_code_runner() {
    let vault = research_vault()
        .file(
            "refs.bib",
            concat!(
                "@article{vaswani2017,\n",
                "  title = {Attention Is All You Need},\n",
                "  author = {Vaswani, Ashish and Shazeer, Noam and Parmar, Niki},\n",
                "  year = {2017},\n",
                "}\n",
            ),
        )
        .fixture("papers/vaswani2017.pdf", "pdf/dummy.pdf")
        .file(
            "Scratch.md",
            concat!(
                "# Scratch\n\n",
                "\n",
                "$2^8 + \\frac{12}{3}$\n\n",
                "```sh\n",
                "echo \"hello synthesis\"\n",
                "```\n",
            ),
        );

    let mut app = boot("w4-synthesis", &vault, Some("Scratch.md"));

    // 1. Type `@vas` -> citation completion opens and inserts PDF-linked citation.
    app.send(edit(EditorCommand::SetCursor {
        line: 2,
        col: 0,
        affinity: Affinity::Downstream,
    }));
    type_str(&mut app, "@vas");
    assert!(
        app.probe()
            .completion
            .iter()
            .any(|label| label.contains("Vaswani")),
        "completion: {:?}",
        app.probe().completion
    );
    app.send(Message::Editor(EditorMessage::CompletionAccept));
    assert!(
        app.probe()
            .buffer
            .contains("[[papers/vaswani2017.pdf|Vaswani et al. (2017)]]"),
        "{}",
        app.probe().buffer
    );

    // 2. Type ` [[Index#` -> heading completion opens and inserts anchor link.
    type_str(&mut app, " [[Index#");
    assert!(
        app.probe()
            .completion
            .iter()
            .any(|label| label == "Research Index"),
        "heading completion: {:?}",
        app.probe().completion
    );
    app.send(Message::Editor(EditorMessage::CompletionAccept));
    assert!(
        app.probe().buffer.contains("[[Index#Research Index]]"),
        "{}",
        app.probe().buffer
    );

    // 3. Evaluate inline math `$2^8 + \frac{12}{3}$` -> `$2^8 + \frac{12}{3} = 260$`, undoable in one step.
    app.send(edit(EditorCommand::SetCursor {
        line: 3,
        col: 5,
        affinity: Affinity::Downstream,
    }));
    app.send(Message::KeyboardShortcut(Shortcut::EvaluateMath));
    assert!(
        app.probe().buffer.contains("$2^8 + \\frac{12}{3} = 260$"),
        "{}",
        app.probe().buffer
    );
    app.send(edit(EditorCommand::Undo));
    assert!(
        !app.probe().buffer.contains("= 260"),
        "EvaluateMath should undo in one step: {}",
        app.probe().buffer
    );

    // 4. Opt-in code runner executes ```sh block -> inserts `> [!output]\n> hello synthesis`, updates idempotently, and undoes in one step.
    app.send(edit(EditorCommand::SetCursor {
        line: 6,
        col: 2,
        affinity: Affinity::Downstream,
    }));
    app.send(Message::KeyboardShortcut(Shortcut::RunCodeBlock));
    let probe = wait_for(&mut app, "code block output", |p| {
        p.buffer.contains("> [!output]\n> hello synthesis")
    });
    assert!(probe.buffer.contains("> [!output]\n> hello synthesis"));

    // Re-running updates in place without duplicating the callout.
    app.send(Message::KeyboardShortcut(Shortcut::RunCodeBlock));
    app.wait(Duration::from_millis(300));
    app.settle();
    let matches = app.probe().buffer.matches("> [!output]").count();
    assert_eq!(matches, 1, "callout duplicated: {}", app.probe().buffer);

    app.send(edit(EditorCommand::Undo));
    app.send(edit(EditorCommand::Undo));
    assert!(!app.probe().buffer.contains("> [!output]"));
    app.finish();
}

#[test]
fn w5_pdf_margin_ink_persists_through_sidecar_restart_and_erasable() {
    let vault = research_vault().fixture("papers/vaswani2017.pdf", "pdf/dummy.pdf");
    let mut app = boot("w5-pdf-margin-ink", &vault, None);

    app.send(Message::Vault(
        crate::messages::VaultMessage::SidebarFileClicked("papers/vaswani2017.pdf".to_string()),
    ));
    let probe = wait_for(&mut app, "pdf ready for margin ink", |p| {
        p.pdf_document_ready && p.pdf_rendered > 0
    });
    assert_eq!(probe.pdf_annotations, 0);

    // Enable Margin Ink Pen and pick Pink so strokes stand out clearly on white PDF pages.
    app.send(Message::Pdf(crate::messages::PdfMessage::InkToolToggled(
        Some(crate::pdf_pane::PdfInkTool::Pen),
    )));
    app.send(Message::Pdf(crate::messages::PdfMessage::InkColorSelected(
        md_editor_core::pdf::PdfAnnotationColor::Pink,
    )));
    app.send(Message::Pdf(
        crate::messages::PdfMessage::InkStrokeCommitted {
            page: 0,
            points: vec![[0.74, 0.20, 0.55], [0.84, 0.24, 0.70], [0.92, 0.21, 0.55]],
        },
    ));
    app.send(Message::Pdf(
        crate::messages::PdfMessage::InkStrokeCommitted {
            page: 0,
            points: vec![[0.74, 0.30, 0.55], [0.88, 0.34, 0.65], [0.92, 0.39, 0.55]],
        },
    ));
    let probe = app.probe();
    assert_eq!(probe.pdf_annotations, 2);
    app.screenshot("pdf-margin-ink");

    // Verify portable `.md-editor/annotations/<doc_id>.jsonl` sidecar contains the ink strokes.
    let annotations_dir = vault.root.join(".md-editor/annotations");
    let mut sidecar_files: Vec<_> = std::fs::read_dir(&annotations_dir)
        .expect("annotations sidecar dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    assert_eq!(sidecar_files.len(), 1);
    let jsonl = std::fs::read_to_string(sidecar_files.pop().unwrap()).expect("read sidecar");
    assert!(jsonl.contains("\"kind\":\"Ink\""), "{jsonl}");
    assert!(jsonl.contains("\"ink_stroke\""), "{jsonl}");

    // Restart and confirm non-destructive margin ink strokes reload automatically.
    let mut app = app.restart("w5-pdf-margin-ink-restart", None);
    app.send(Message::Vault(
        crate::messages::VaultMessage::SidebarFileClicked("papers/vaswani2017.pdf".to_string()),
    ));
    let probe = wait_for(&mut app, "pdf reloaded with ink", |p| {
        p.pdf_document_ready && p.pdf_annotations == 2
    });
    assert_eq!(probe.pdf_annotations, 2);

    // Undo removes the latest stroke; Eraser removes the remaining stroke by coordinates.
    app.send(Message::Pdf(crate::messages::PdfMessage::UndoInkStroke));
    assert_eq!(app.probe().pdf_annotations, 1);

    app.send(Message::Pdf(crate::messages::PdfMessage::InkToolToggled(
        Some(crate::pdf_pane::PdfInkTool::Eraser),
    )));
    app.send(Message::Pdf(crate::messages::PdfMessage::InkEraseAt {
        page: 0,
        x_frac: 0.84,
        y_frac: 0.24,
    }));
    assert_eq!(app.probe().pdf_annotations, 0);

    app.send(Message::Pdf(crate::messages::PdfMessage::InkToolToggled(
        None,
    )));
    app.finish();
}

#[test]
fn w5_sketch_open_to_side_rename_and_delete_edge_cases() {
    let svg = crate::ink::svg::to_svg(&crate::ink::document::InkDocument::new()).unwrap();
    let vault = research_vault().file("notes/Sketch.ink.svg", &svg);
    let mut app = boot("w5-sketch-edges", &vault, Some("notes/Attention.md"));

    // OpenToSide on `.ink.svg` opens the Handwriting Canvas directly instead of splitting a static tab.
    app.send(Message::Vault(crate::messages::VaultMessage::OpenToSide(
        "notes/Sketch.ink.svg".to_string(),
    )));
    let probe = app.probe();
    assert!(probe.ink_window_open);
    assert!(!probe.split_view);
    assert_eq!(probe.ink_path.as_deref(), Some("notes/Sketch.ink.svg"));

    // Renaming the open sketch updates `ink.path` so autosave targets the new path.
    app.send(Message::Vault(
        crate::messages::VaultMessage::RenameEntryDialog("notes/Sketch.ink.svg".to_string()),
    ));
    app.send(Message::Vault(
        crate::messages::VaultMessage::NameModalSubmit("SketchRenamed.ink.svg".to_string()),
    ));
    let probe = app.probe();
    assert_eq!(
        probe.ink_path.as_deref(),
        Some("notes/SketchRenamed.ink.svg")
    );
    assert!(vault.exists("notes/SketchRenamed.ink.svg"));

    // Deleting the open (even dirty) sketch clears `ink.path`/`ink.dirty` so autosave never resurrects it.
    app.send(Message::Ink(crate::messages::InkMessage::PointerDown(
        30.0, 30.0,
    )));
    app.send(Message::Ink(crate::messages::InkMessage::PointerMove(
        90.0, 50.0,
    )));
    app.send(Message::Ink(crate::messages::InkMessage::PointerUp));
    assert!(app.probe().ink_dirty);

    app.send(Message::Vault(crate::messages::VaultMessage::DeleteFile(
        "notes/SketchRenamed.ink.svg".to_string(),
    )));
    let probe = app.probe();
    assert_eq!(probe.ink_path, None);
    assert!(!probe.ink_dirty);
    assert!(!vault.exists("notes/SketchRenamed.ink.svg"));

    app.wait(Duration::from_millis(1200));
    assert!(
        !vault.exists("notes/SketchRenamed.ink.svg"),
        "autosave must not recreate deleted sketch"
    );
    app.finish();
}
