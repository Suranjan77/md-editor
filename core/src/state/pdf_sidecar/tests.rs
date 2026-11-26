use std::path::{Path, PathBuf};

use super::*;
use crate::pdf::{PdfAnnotationColor, PdfAnnotationKind};

const DOC: &str = "a3f1c2d4e5b6978812345678901234567890abcdefabcdefabcdefabcdef0123";

/// One machine: its own database, its own copy of the vault.
struct Machine {
    state: AppState,
    root: PathBuf,
}

impl Machine {
    fn new(base: &Path, name: &str) -> Self {
        let root = base.join(name);
        std::fs::create_dir_all(&root).unwrap();
        let state = AppState::new_in_memory();
        *state.vault_root.lock().unwrap() = Some(root.clone());
        state
            .save_pdf_document(DOC, "papers/paper.pdf", 100, Some(1))
            .unwrap();
        Self { state, root }
    }

    fn file(&self) -> PathBuf {
        sidecar_path(&self.root, DOC)
    }

    fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .state
            .get_pdf_annotations(DOC, None)
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        ids.sort();
        ids
    }

    fn get(&self, id: &str) -> PdfAnnotation {
        self.state
            .get_pdf_annotations(DOC, None)
            .unwrap()
            .into_iter()
            .find(|a| a.id == id)
            .unwrap()
    }

    /// The vault file arrives from elsewhere (a pull, Syncthing).
    fn receive(&self, from: &Machine) {
        std::fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        std::fs::copy(from.file(), self.file()).unwrap();
    }

    /// A highlight that reached SQLite but not the vault file (the write failed).
    fn insert_unexported(&self, ann: &PdfAnnotation) {
        let storage = self.state.pdf_storage_document_id(DOC).unwrap();
        let db = self.state.db.lock().unwrap();
        crate::state::pdf_store::upsert_annotation(&db, &storage, ann).unwrap();
    }
}

fn highlight(id: &str, created_at: i64, updated_at: i64, note: &str) -> PdfAnnotation {
    PdfAnnotation {
        id: id.to_string(),
        document_id: DOC.to_string(),
        page_index: 2,
        kind: PdfAnnotationKind::Highlight,
        color: PdfAnnotationColor::Yellow,
        selected_text: format!("text of {id}"),
        ranges: vec![],
        rects: vec![],
        note: Some(note.to_string()),
        linked_note_path: None,
        markdown_anchor: None,
        ink_stroke: None,
        created_at,
        updated_at,
    }
}

fn base(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("md_sidecar_{tag}_{}", uuid::Uuid::new_v4()))
}

#[test]
fn every_change_rewrites_the_vault_file_one_line_per_highlight() {
    let base = base("write");
    let a = Machine::new(&base, "a");
    a.state
        .save_pdf_annotation(&highlight("h2", 20, 20, "second"))
        .unwrap();
    a.state
        .save_pdf_annotation(&highlight("h1", 10, 10, "first"))
        .unwrap();

    let text = std::fs::read_to_string(a.file()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("\"h1\"") && lines[1].contains("\"h2\""),
        "oldest first: {text}"
    );
    assert!(
        std::fs::read_to_string(a.root.join(".md-editor/.gitattributes"))
            .unwrap()
            .contains("*.jsonl merge=union")
    );

    a.state.delete_pdf_annotation("h1").unwrap();
    a.state.delete_pdf_annotation("h2").unwrap();
    assert_eq!(
        std::fs::read_to_string(a.file()).unwrap(),
        "",
        "deleting the last highlight leaves an empty file, not none"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn highlights_and_deletions_travel_between_machines() {
    let base = base("travel");
    let (a, b) = (Machine::new(&base, "a"), Machine::new(&base, "b"));
    a.state
        .save_pdf_annotation(&highlight("h1", 10, 10, "from a"))
        .unwrap();

    b.receive(&a);
    assert!(b.state.sync_pdf_sidecar(DOC).unwrap());
    assert_eq!(b.ids(), ["h1"]);
    assert_eq!(b.get("h1").note.as_deref(), Some("from a"));

    b.state.delete_pdf_annotation("h1").unwrap();
    b.state
        .save_pdf_annotation(&highlight("h2", 20, 20, "from b"))
        .unwrap();
    a.receive(&b);
    assert!(a.state.sync_pdf_sidecar(DOC).unwrap());
    assert_eq!(a.ids(), ["h2"], "h1 was deleted on b; h2 added there");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_highlight_the_file_never_saw_survives_a_remote_change() {
    let base = base("local_new");
    let (a, b) = (Machine::new(&base, "a"), Machine::new(&base, "b"));
    a.state
        .save_pdf_annotation(&highlight("h1", 10, 10, "shared"))
        .unwrap();
    b.receive(&a);
    b.state.sync_pdf_sidecar(DOC).unwrap();
    b.state
        .save_pdf_annotation(&highlight("h3", 30, 30, "from b"))
        .unwrap();

    a.insert_unexported(&highlight("h2", 20, 20, "only in a's database"));
    a.receive(&b);
    a.state.sync_pdf_sidecar(DOC).unwrap();
    assert_eq!(a.ids(), ["h1", "h2", "h3"]);
    let text = std::fs::read_to_string(a.file()).unwrap();
    assert_eq!(text.lines().count(), 3, "the merge is written back: {text}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_later_edit_of_a_highlight_wins_and_a_tie_goes_to_the_file() {
    let base = base("edits");
    let (a, b) = (Machine::new(&base, "a"), Machine::new(&base, "b"));
    a.state
        .save_pdf_annotation(&highlight("h1", 10, 10, "original"))
        .unwrap();
    b.receive(&a);
    b.state.sync_pdf_sidecar(DOC).unwrap();

    a.state
        .save_pdf_annotation(&highlight("h1", 10, 50, "edited later on a"))
        .unwrap();
    b.state
        .save_pdf_annotation(&highlight("h1", 10, 40, "edited on b"))
        .unwrap();
    b.receive(&a);
    b.state.sync_pdf_sidecar(DOC).unwrap();
    assert_eq!(b.get("h1").note.as_deref(), Some("edited later on a"));

    // A rename elsewhere rewrites a linked note path without a new updated_at.
    let mut renamed = highlight("h1", 10, 50, "edited later on a");
    renamed.linked_note_path = Some("notes/renamed.md".to_string());
    a.state.save_pdf_annotation(&renamed).unwrap();
    b.receive(&a);
    b.state.sync_pdf_sidecar(DOC).unwrap();
    assert_eq!(
        b.get("h1").linked_note_path.as_deref(),
        Some("notes/renamed.md")
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_missing_file_is_written_from_the_database_never_read_as_deletions() {
    let base = base("missing");
    let a = Machine::new(&base, "a");
    // Highlights from before the vault files existed.
    a.insert_unexported(&highlight("h1", 10, 10, "old"));
    assert!(!a.file().exists());
    assert_eq!(a.state.sync_pdf_sidecars().unwrap(), 0);
    assert!(
        std::fs::read_to_string(a.file())
            .unwrap()
            .contains("\"h1\"")
    );

    std::fs::remove_dir_all(a.root.join(".md-editor")).unwrap();
    a.state.sync_pdf_sidecar(DOC).unwrap();
    assert_eq!(a.ids(), ["h1"]);
    assert!(a.file().exists(), "rewritten");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn opening_the_vault_imports_files_for_pdfs_never_opened_here() {
    let base = base("import_all");
    let (a, b) = (Machine::new(&base, "a"), Machine::new(&base, "b"));
    a.state
        .save_pdf_annotation(&highlight("h1", 10, 10, "from a"))
        .unwrap();
    b.receive(&a);
    // A stray file in the folder is ignored.
    std::fs::write(b.file().with_file_name("notes.jsonl"), "{}\n").unwrap();
    assert_eq!(b.state.sync_pdf_sidecars().unwrap(), 1);
    assert_eq!(b.ids(), ["h1"]);
    assert_eq!(b.state.sync_pdf_sidecars().unwrap(), 0, "in sync now");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_union_merged_file_keeps_the_later_copy_of_a_line_and_skips_garbage() {
    let older = serde_json::to_string(&highlight("h1", 10, 10, "older")).unwrap();
    let newer = serde_json::to_string(&highlight("h1", 10, 20, "newer")).unwrap();
    let text = format!("{newer}\nnot json\n{older}\n\n");
    let parsed = parse(DOC, &text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].note.as_deref(), Some("newer"));
}

#[test]
fn only_sha256_hex_names_a_document_file() {
    assert!(is_document_id(DOC));
    for bad in [
        "",
        "../../etc/passwd",
        "notes",
        &DOC.to_uppercase(),
        &DOC[1..],
    ] {
        assert!(!is_document_id(bad), "{bad}");
    }
}

#[test]
fn pdf_margin_ink_strokes_round_trip_through_sqlite_and_three_way_sidecar_merge() {
    let base = base("pdf_ink");
    let (a, b) = (Machine::new(&base, "a"), Machine::new(&base, "b"));

    let ink_ann = PdfAnnotation::new_ink(
        "ink-1".to_string(),
        DOC.to_string(),
        0,
        PdfAnnotationColor::Blue,
        crate::pdf::PdfInkStroke {
            size: 0.0038,
            points: vec![[0.75, 0.18, 0.55], [0.88, 0.22, 0.55], [0.86, 0.27, 0.55]],
        },
        (612.0, 792.0),
        42,
    );
    a.state.save_pdf_annotation(&ink_ann).unwrap();

    let loaded_a = a.get("ink-1");
    assert_eq!(loaded_a.kind, PdfAnnotationKind::Ink);
    assert_eq!(loaded_a.ink_stroke, ink_ann.ink_stroke);
    assert_eq!(loaded_a.rects.len(), 1);

    // Sidecar JSONL carries the ink stroke to Machine B.
    let sidecar_text = std::fs::read_to_string(a.file()).unwrap();
    assert!(sidecar_text.contains("\"ink_stroke\""));
    b.receive(&a);
    assert_eq!(b.state.sync_pdf_sidecars().unwrap(), 1);
    let loaded_b = b.get("ink-1");
    assert_eq!(loaded_b.kind, PdfAnnotationKind::Ink);
    assert_eq!(loaded_b.ink_stroke, ink_ann.ink_stroke);

    let _ = std::fs::remove_dir_all(&base);
}
