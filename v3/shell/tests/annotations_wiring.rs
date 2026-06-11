//! Annotations v2 at the shell layer (plan §3.3 / M2 gate follow-through):
//! the persistent sidecar, document identity recording on PDF open, and the
//! highlight workflow — all driven windowlessly through `Shell::update`.
//!
//! The identity/sidecar/export tests run everywhere (hashing needs no
//! pdfium); the drag-select → highlight → delete flow needs real glyph
//! geometry and is feature-gated like the reading suite.

use std::path::{Path, PathBuf};

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, Key, Mods};
use md3_shell::gui::keys::KeyEvent;
use md3_shell::gui::{Message, Shell};
use md3_vault::{AnnotationStore, NewAnnotation, Quad, document_hash};
use tempfile::TempDir;

fn chord(s: &str) -> Chord {
    match Chord::parse(s) {
        Ok(c) => c,
        Err(e) => panic!("bad chord `{s}`: {e}"),
    }
}

fn press(shell: &mut Shell, s: &str) {
    let _ = shell.update(Message::Key(KeyEvent {
        chord: Some(chord(s)),
        text: None,
    }));
}

fn type_text(shell: &mut Shell, text: &str) {
    for c in text.chars() {
        let _ = shell.update(Message::Key(KeyEvent {
            chord: Some(Chord::new(Mods::NONE, Key::Char(c.to_ascii_lowercase()))),
            text: Some(c.to_string()),
        }));
    }
}

fn new_shell(root: &Path) -> Shell {
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new(registry, keymap, root.to_path_buf())
}

fn open_via_quick_open(shell: &mut Shell, name: &str) {
    press(shell, "ctrl+p");
    type_text(shell, name);
    press(shell, "enter");
}

fn sidecar(root: &Path) -> PathBuf {
    root.join(".md3/sidecar.db")
}

/// A vault containing a file named `paper.pdf`. The bytes are an arbitrary
/// stand-in unless the pdfium fixture is requested — identity hashing and
/// sidecar wiring are byte-level concerns, not PDF ones.
fn vault_with_pdf(real_fixture: bool) -> (TempDir, PathBuf) {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let target = dir.path().join("paper.pdf");
    if real_fixture {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests-fixtures/pdf/multipage-outline.pdf");
        if let Err(e) = std::fs::copy(&fixture, &target) {
            panic!("copy fixture: {e}");
        }
    } else if let Err(e) = std::fs::write(&target, b"%PDF-not-really, but hashable bytes") {
        panic!("write fake pdf: {e}");
    }
    (dir, target)
}

fn hash_of(path: &Path) -> String {
    match document_hash(path) {
        Ok(h) => h,
        Err(e) => panic!("hash: {e}"),
    }
}

// ------------------------------------------------------------ always-on --

#[test]
fn opening_a_pdf_records_its_identity_in_the_persistent_sidecar() {
    let (dir, target) = vault_with_pdf(false);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    drop(shell); // the sidecar must outlive the session

    let store = match AnnotationStore::open(&sidecar(dir.path())) {
        Ok(s) => s,
        Err(e) => panic!("sidecar did not persist: {e}"),
    };
    let last = match store.last_path(&hash_of(&target)) {
        Ok(p) => p,
        Err(e) => panic!("last_path: {e}"),
    };
    assert_eq!(
        last.as_deref(),
        Some("paper.pdf"),
        "open recorded hash → path"
    );
}

#[test]
fn highlight_without_a_selection_is_a_friendly_no_op() {
    let (dir, _) = vault_with_pdf(false);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    press(&mut shell, "ctrl+h");
    assert!(
        shell.status().contains("select text"),
        "status guides the user: {}",
        shell.status()
    );
}

#[test]
fn export_command_writes_a_markdown_summary_into_the_vault() {
    let (dir, target) = vault_with_pdf(false);
    let hash = hash_of(&target);

    // Seed an annotation as a previous session would have left it.
    {
        if let Err(e) = std::fs::create_dir_all(dir.path().join(".md3")) {
            panic!("mkdir .md3: {e}");
        }
        let mut store = match AnnotationStore::open(&sidecar(dir.path())) {
            Ok(s) => s,
            Err(e) => panic!("seed store: {e}"),
        };
        let added = store.add(NewAnnotation {
            doc_hash: hash.clone(),
            page: 0,
            quads: vec![Quad {
                x0: 10.0,
                y0: 20.0,
                x1: 200.0,
                y1: 32.0,
            }],
            color: "#ffd866".to_string(),
            note: "the central claim".to_string(),
            linked_note: None,
        });
        if let Err(e) = added {
            panic!("seed annotation: {e}");
        }
    }

    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    // Palette-only command: ctrl+shift+p → "export annotations".
    press(&mut shell, "ctrl+shift+p");
    type_text(&mut shell, "export annot");
    press(&mut shell, "enter");
    assert!(
        shell.status().contains("exported"),
        "status: {}",
        shell.status()
    );

    let exported = dir.path().join("paper-annotations.md");
    let body = match std::fs::read_to_string(&exported) {
        Ok(b) => b,
        Err(e) => panic!("export file missing: {e}"),
    };
    assert!(body.contains("# Annotations — paper.pdf"), "{body}");
    assert!(body.contains("## Page 1"), "{body}");
    assert!(body.contains("the central claim"), "{body}");
}

#[test]
fn search_index_persists_in_the_sidecar_across_sessions() {
    let (dir, _) = vault_with_pdf(false);
    if let Err(e) = std::fs::write(dir.path().join("note.md"), "tile renderer notes") {
        panic!("write note: {e}");
    }

    let mut shell = new_shell(dir.path());
    press(&mut shell, "ctrl+shift+f"); // builds the index on the sidecar
    press(&mut shell, "escape");
    drop(shell);

    // A fresh index on the same sidecar sees everything as unchanged —
    // the cross-run cold-start guarantee, now real in the shell.
    let mut index = match md3_vault::SearchIndex::open(&sidecar(dir.path())) {
        Ok(i) => i,
        Err(e) => panic!("sidecar index: {e}"),
    };
    let report = match index.sync(dir.path()) {
        Ok(r) => r,
        Err(e) => panic!("sync: {e}"),
    };
    assert_eq!(report.indexed, 0, "nothing re-read on cold start");
    assert!(report.unchanged >= 1, "the note was already indexed");
}

// -------------------------------------------------------- pdfium-gated --

/// Drives a real drag over page 0's glyphs. Returns `None` (skip) when
/// pdfium isn't bound or the page has no text.
#[cfg(feature = "pdfium")]
fn shell_with_selection() -> Option<(TempDir, Shell)> {
    let (dir, _) = vault_with_pdf(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    if shell.focused_pdf().is_none_or(|s| s.layout.is_none()) {
        eprintln!("skipping: libpdfium not available");
        return None;
    }
    let tab = match shell.workspace().focused_tab() {
        Some(t) => t,
        None => panic!("no focused tab"),
    };
    let viewport = (1000.0, 750.0);

    // First press anywhere on the sheet loads the page's glyph geometry.
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: (500.0, 50.0),
        viewport,
    });
    let session = match shell.focused_pdf() {
        Some(s) => s,
        None => panic!("pdf session lost"),
    };
    let chars = session.chars.get(&0).cloned().unwrap_or_default();
    if chars.is_empty() {
        eprintln!("skipping: fixture page 0 has no text");
        return None;
    }
    // Project the first and last glyph centers into viewport coordinates.
    let layout = match &session.layout {
        Some(l) => l.clone(),
        None => panic!("layout lost"),
    };
    let page = layout.placed_pages(session.scroll, viewport)[0];
    let zoom = layout.zoom();
    let center = |c: &md3_pdf::CharBox| {
        (
            page.x + (c.x0 + c.x1) / 2.0 * zoom,
            page.y + (c.y0 + c.y1) / 2.0 * zoom,
        )
    };
    let (start, end) = (center(&chars[0]), center(&chars[chars.len() - 1]));

    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: start,
        viewport,
    });
    let _ = shell.update(Message::PdfMouseDragged {
        tab,
        pos: end,
        viewport,
    });
    let _ = shell.update(Message::PdfMouseUp { tab });
    Some((dir, shell))
}

#[cfg(feature = "pdfium")]
fn pdf(shell: &Shell) -> &md3_shell::gui::session::PdfSession {
    match shell.focused_pdf() {
        Some(s) => s,
        None => panic!("no focused pdf session"),
    }
}

#[cfg(feature = "pdfium")]
#[test]
fn drag_select_then_highlight_persists_and_reloads() {
    let Some((dir, mut shell)) = shell_with_selection() else {
        return;
    };
    {
        let session = pdf(&shell);
        let sel = match &session.selection {
            Some(s) => s,
            None => panic!("drag produced no selection"),
        };
        assert!(!sel.text.is_empty(), "selected real text");
        assert!(!sel.quads.is_empty());
        assert!(shell.status().contains("selected"), "{}", shell.status());
    }

    press(&mut shell, "ctrl+h");
    let (id, hash) = {
        let session = pdf(&shell);
        assert_eq!(session.annotations.len(), 1, "highlight in the cache");
        assert!(session.selection.is_none(), "selection consumed");
        let id = match session.selected_annotation {
            Some(id) => id,
            None => panic!("new highlight not picked"),
        };
        let hash = match &session.doc_hash {
            Some(h) => h.clone(),
            None => panic!("no doc hash"),
        };
        (id, hash)
    };

    // Close and reopen the document: the highlight comes back from the
    // sidecar, keyed by content hash.
    press(&mut shell, "ctrl+w");
    open_via_quick_open(&mut shell, "paper.pdf");
    {
        let session = pdf(&shell);
        assert_eq!(session.annotations.len(), 1, "reloaded after reopen");
        assert_eq!(session.annotations[0].id, id);
        assert_eq!(session.annotations[0].doc_hash, hash);
        assert!(!session.annotations[0].quads.is_empty());
    }
    drop(shell);

    // And it is in the store itself, not just session state.
    let store = match AnnotationStore::open(&sidecar(dir.path())) {
        Ok(s) => s,
        Err(e) => panic!("sidecar: {e}"),
    };
    let anns = match store.annotations_for(&hash) {
        Ok(a) => a,
        Err(e) => panic!("annotations_for: {e}"),
    };
    assert_eq!(anns.len(), 1);
}

#[cfg(feature = "pdfium")]
#[test]
fn click_picks_a_highlight_note_edits_and_delete_removes() {
    let Some((dir, mut shell)) = shell_with_selection() else {
        return;
    };
    press(&mut shell, "ctrl+h");
    let (quad, hash) = {
        let session = pdf(&shell);
        let a = &session.annotations[0];
        (
            a.quads[0],
            match &session.doc_hash {
                Some(h) => h.clone(),
                None => panic!("no doc hash"),
            },
        )
    };

    // Click squarely inside the stored quad: the highlight is picked.
    let tab = match shell.workspace().focused_tab() {
        Some(t) => t,
        None => panic!("no focused tab"),
    };
    let viewport = (1000.0, 750.0);
    let (layout, scroll) = {
        let session = pdf(&shell);
        (
            match &session.layout {
                Some(l) => l.clone(),
                None => panic!("layout lost"),
            },
            session.scroll,
        )
    };
    let page = layout.placed_pages(scroll, viewport)[0];
    let pos = (
        page.x + ((quad.x0 + quad.x1) / 2.0) as f32 * layout.zoom(),
        page.y + ((quad.y0 + quad.y1) / 2.0) as f32 * layout.zoom(),
    );
    let _ = shell.update(Message::PdfMouseDown { tab, pos, viewport });
    let _ = shell.update(Message::PdfMouseUp { tab });
    assert!(
        pdf(&shell).selected_annotation.is_some(),
        "click inside the quad picks the annotation"
    );

    // ctrl+n opens the note overlay; typing + enter saves the note.
    press(&mut shell, "ctrl+n");
    assert!(shell.overlay().is_some(), "note overlay open");
    type_text(&mut shell, "key insight");
    press(&mut shell, "enter");
    assert_eq!(pdf(&shell).annotations[0].note, "key insight");

    // Delete removes it everywhere.
    press(&mut shell, "delete");
    assert!(pdf(&shell).annotations.is_empty());
    drop(shell);
    let store = match AnnotationStore::open(&sidecar(dir.path())) {
        Ok(s) => s,
        Err(e) => panic!("sidecar: {e}"),
    };
    match store.annotations_for(&hash) {
        Ok(anns) => assert!(anns.is_empty(), "removed from the store too"),
        Err(e) => panic!("annotations_for: {e}"),
    }
}
