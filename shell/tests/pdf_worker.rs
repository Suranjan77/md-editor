#![cfg(feature = "pdfium")]

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, Key, Mods};
use md_shell::gui::keys::KeyEvent;
use md_shell::gui::worker::{self, PdfJob, PdfJobOutput};
use md_shell::gui::{Message, Shell};

fn shell(root: &Path) -> Shell {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(keymap) => keymap,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new(registry, keymap, root.to_path_buf()).0
}

fn press(shell: &mut Shell, chord: &str) {
    let chord = match Chord::parse(chord) {
        Ok(chord) => chord,
        Err(e) => panic!("chord: {e}"),
    };
    let _ = shell.update(Message::Key(KeyEvent {
        chord: Some(chord),
        text: None,
    }));
}

fn type_text(shell: &mut Shell, text: &str) {
    for ch in text.chars() {
        let _ = shell.update(Message::Key(KeyEvent {
            chord: Some(Chord::new(Mods::NONE, Key::Char(ch))),
            text: Some(ch.to_string()),
        }));
    }
}

#[test]
fn worker_handshake_schedules_visible_pdf_work_and_applies_results() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    if let Err(e) = std::fs::write(dir.path().join("paper.pdf"), b"%PDF-fake") {
        panic!("fixture: {e}");
    }
    let mut shell = shell(dir.path());
    press(&mut shell, "ctrl+p");
    type_text(&mut shell, "paper.pdf");
    press(&mut shell, "enter");
    shell.inject_pdf_session_layout(md_pdf::DocLayout::new(vec![(612.0, 792.0)], 1.0, 16.0));

    let (tx, rx) = mpsc::channel();
    let handle = worker::spawn(
        move |job| {
            let _ = tx.send(job.clone());
            None
        },
        |_| {},
    );
    let _ = shell.update(Message::PdfWorkerReady(handle));

    let mut saw_glyphs = false;
    let mut saw_links = false;
    let mut saw_tile = false;
    for _ in 0..3 {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(PdfJob::PageGlyphs { page: 0, .. }) => saw_glyphs = true,
            Ok(PdfJob::PageLinks { page: 0, .. }) => saw_links = true,
            Ok(PdfJob::Tile { key, .. }) if key.page == 0 => saw_tile = true,
            Ok(other) => panic!("unexpected job: {other:?}"),
            Err(e) => panic!("worker job: {e}"),
        }
    }
    assert!(saw_glyphs && saw_links && saw_tile);

    let abs_path = dir.path().join("paper.pdf");
    let _ = shell.update(Message::PdfWorker(PdfJobOutput::PageGlyphs {
        path: abs_path,
        page: 0,
        chars: Vec::new(),
    }));
    let session = match shell.focused_pdf() {
        Some(session) => session,
        None => panic!("pdf session missing"),
    };
    assert!(session.chars.contains_key(&0));
    assert!(!session.chars_pending.contains(&0));
}
