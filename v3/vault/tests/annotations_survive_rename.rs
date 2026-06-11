//! M2 gate (plan §5): "annotation survives file rename (test)". The
//! mechanism under test is the *keying decision* — annotations are keyed by
//! the document's SHA-256 content hash, so no rename, move, or vault
//! reshuffle can detach them. This drives the real flow a shell session
//! uses: hash the file on open, look up by hash, re-record the path.

use std::path::Path;

use md3_vault::{AnnotationStore, NewAnnotation, Quad, VaultError, document_hash};

fn ok<T>(r: Result<T, VaultError>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        panic!("mkdir {}: {e}", parent.display());
    }
    if let Err(e) = std::fs::write(path, bytes) {
        panic!("write {}: {e}", path.display());
    }
}

#[test]
fn annotation_survives_rename_and_move() {
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let root = dir.path();
    write(&root.join("papers/attention.pdf"), b"%PDF-1.4 fake content");

    // Session 1: open the PDF, annotate it.
    let mut store = ok(AnnotationStore::open(&root.join(".sidecar.db")));
    let hash = ok(document_hash(&root.join("papers/attention.pdf")));
    ok(store.record_document(&hash, "papers/attention.pdf"));
    ok(store.add(NewAnnotation {
        doc_hash: hash.clone(),
        page: 6,
        quads: vec![Quad {
            x0: 50.0,
            y0: 300.0,
            x1: 420.0,
            y1: 314.0,
        }],
        color: "#a9dc76".to_string(),
        note: "key claim".to_string(),
        linked_note: None,
    }));

    // The file is renamed *and* moved to another directory, vault-side or
    // externally — the store is not told.
    let new_path = root.join("archive/transformers-2017.pdf");
    if let Err(e) = std::fs::create_dir_all(root.join("archive")) {
        panic!("mkdir archive: {e}");
    }
    if let Err(e) = std::fs::rename(root.join("papers/attention.pdf"), &new_path) {
        panic!("rename: {e}");
    }

    // Session 2 (fresh store handle, as after a restart): opening the moved
    // file finds the annotation purely through its content hash.
    let mut store = ok(AnnotationStore::open(&root.join(".sidecar.db")));
    let rehash = ok(document_hash(&new_path));
    assert_eq!(rehash, hash, "content unchanged ⇒ identity unchanged");
    let anns = ok(store.annotations_for(&rehash));
    assert_eq!(anns.len(), 1, "annotation survived the rename+move");
    assert_eq!(anns[0].note, "key claim");
    assert_eq!(anns[0].page, 6);

    // The session re-records where the content lives now; the stale hint is
    // replaced, not duplicated.
    ok(store.record_document(&rehash, "archive/transformers-2017.pdf"));
    assert_eq!(
        ok(store.last_path(&rehash)).as_deref(),
        Some("archive/transformers-2017.pdf")
    );
    let docs = ok(store.known_documents());
    assert_eq!(docs.len(), 1, "one document, one identity");
}

#[test]
fn edited_content_is_a_different_document() {
    // The flip side of hash keying, stated honestly: changing the *bytes*
    // changes the identity. (Re-binding annotations across content edits is
    // a later, explicitly-fuzzy feature; silently guessing would be worse.)
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let root = dir.path();
    let pdf = root.join("doc.pdf");
    write(&pdf, b"%PDF-1.4 original");

    let mut store = ok(AnnotationStore::open(&root.join(".sidecar.db")));
    let hash = ok(document_hash(&pdf));
    ok(store.add(NewAnnotation {
        doc_hash: hash.clone(),
        page: 0,
        quads: vec![],
        color: "#ffd866".to_string(),
        note: "on the original".to_string(),
        linked_note: None,
    }));

    write(&pdf, b"%PDF-1.4 rewritten by another tool");
    let new_hash = ok(document_hash(&pdf));
    assert_ne!(new_hash, hash);
    assert!(ok(store.annotations_for(&new_hash)).is_empty());
    // The old annotations are not lost — they are reachable by the old
    // identity (orphan report material, never silent deletion).
    assert_eq!(ok(store.annotations_for(&hash)).len(), 1);
}
