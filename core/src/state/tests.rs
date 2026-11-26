//! Tests for `state`.

use super::*;

#[test]
fn vault_mutation_guard_advances_content_revision() {
    let state = AppState::new_in_memory();
    let before = state.current_vault_content_revision();
    {
        let _mutation = state.vault_mutation_guard().unwrap();
        assert_eq!(state.current_vault_content_revision(), before);
    }
    assert_eq!(
        state.current_vault_content_revision(),
        before.wrapping_add(1)
    );
}

#[test]
fn migrates_legacy_db_with_sidecars_when_new_location_empty() {
    let base = std::env::temp_dir().join(format!("md_editor_mig_{}", uuid::Uuid::new_v4()));
    let legacy_dir = base.join("legacy");
    let new_dir = base.join("xdg");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();

    let legacy = legacy_dir.join(DB_FILE_NAME);
    let new_path = new_dir.join(DB_FILE_NAME);
    std::fs::write(&legacy, b"DBDATA").unwrap();
    std::fs::write(sidecar(&legacy, "-wal"), b"WAL").unwrap();

    migrate_legacy_db(&legacy, &new_path);

    assert_eq!(std::fs::read(&new_path).unwrap(), b"DBDATA");
    assert_eq!(std::fs::read(sidecar(&new_path, "-wal")).unwrap(), b"WAL");
    // Legacy file is left untouched.
    assert!(legacy.exists());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pdf_text_cache_hits_only_when_size_and_mtime_match() {
    let state = AppState::new_in_memory();
    assert!(state.get_cached_pdf_text("a.pdf", 100, 5).is_none());

    state.put_cached_pdf_text("a.pdf", 100, 5, "hello world");
    assert_eq!(
        state.get_cached_pdf_text("a.pdf", 100, 5).as_deref(),
        Some("hello world")
    );
    // A changed size or mtime is a miss (the PDF was modified).
    assert!(state.get_cached_pdf_text("a.pdf", 101, 5).is_none());
    assert!(state.get_cached_pdf_text("a.pdf", 100, 6).is_none());

    // Re-extraction updates the entry in place.
    state.put_cached_pdf_text("a.pdf", 101, 6, "new text");
    assert_eq!(
        state.get_cached_pdf_text("a.pdf", 101, 6).as_deref(),
        Some("new text")
    );
}

#[test]
fn annotations_follow_document_id_change_for_same_path_and_size() {
    use crate::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind};

    let state = AppState::new_in_memory();
    state
        .save_pdf_document("old-id", "papers/a.pdf", 1234, Some(100))
        .unwrap();
    state
        .save_pdf_annotation(&PdfAnnotation {
            id: "ann-1".to_string(),
            document_id: "old-id".to_string(),
            page_index: 0,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: "hello".to_string(),
            ranges: Vec::new(),
            rects: Vec::new(),
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            ink_stroke: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

    // Same path and size, new id (e.g. legacy mtime-based id): annotations
    // are adopted by the new id.
    state
        .save_pdf_document("new-id", "papers/a.pdf", 1234, Some(200))
        .unwrap();
    let anns = state.get_pdf_annotations("new-id", None).unwrap();
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].id, "ann-1");
    assert!(
        state
            .get_pdf_annotations("old-id", None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(state.get_pdf_path_by_id("old-id").unwrap(), None);

    // A replacement file at the same path still adopts the newest row, so
    // annotations follow the user-facing path instead of being orphaned.
    state
        .save_pdf_document("other-id", "papers/a.pdf", 9999, Some(300))
        .unwrap();
    assert_eq!(
        state.get_pdf_annotations("other-id", None).unwrap().len(),
        1
    );
}

#[test]
fn pdf_text_cache_is_isolated_by_vault() {
    let base = std::env::temp_dir().join(format!("md_pdf_cache_{}", uuid::Uuid::new_v4()));
    let first = base.join("first");
    let second = base.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let state = AppState::new_in_memory();

    state.put_cached_pdf_text_for_vault(&first, "paper.pdf", 10, 1, "first text");
    assert!(
        state
            .get_cached_pdf_text_for_vault(&second, "paper.pdf", 10, 1)
            .is_none()
    );
    state.put_cached_pdf_text_for_vault(&second, "paper.pdf", 10, 1, "second text");
    assert_eq!(
        state
            .get_cached_pdf_text_for_vault(&first, "paper.pdf", 10, 1)
            .as_deref(),
        Some("first text")
    );

    drop(state);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn migration_never_overwrites_an_existing_new_db() {
    let base = std::env::temp_dir().join(format!("md_editor_mig_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let legacy = base.join("legacy.sqlite");
    let new_path = base.join("new.sqlite");
    std::fs::write(&legacy, b"OLD").unwrap();
    std::fs::write(&new_path, b"CURRENT").unwrap();

    migrate_legacy_db(&legacy, &new_path);

    assert_eq!(std::fs::read(&new_path).unwrap(), b"CURRENT");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn corrupt_database_is_preserved_and_recreated() {
    let base = std::env::temp_dir().join(format!("md_editor_corrupt_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let db_path = base.join(DB_FILE_NAME);
    std::fs::write(&db_path, b"this is not sqlite").unwrap();

    let state = AppState::new_with_db_path(&db_path);

    assert!(db_path.exists());
    assert!(
        state
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row
                .get::<_, i64>(0))
            .is_ok()
    );
    let notice = state.take_startup_notice().expect("recovery notice");
    assert!(notice.contains("has been reset"));
    let backup = std::fs::read_dir(&base)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("md_editor_settings.sqlite.corrupt-")
                        && !name.ends_with("-wal")
                })
        })
        .expect("quarantined database");
    assert_eq!(std::fs::read(&backup).unwrap(), b"this is not sqlite");
    assert!(state.take_startup_notice().is_none());

    drop(state);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn quarantining_database_preserves_sidecars() {
    let base = std::env::temp_dir().join(format!("md_editor_sidecars_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let db_path = base.join(DB_FILE_NAME);
    std::fs::write(&db_path, b"database").unwrap();
    std::fs::write(sidecar(&db_path, "-wal"), b"wal").unwrap();
    std::fs::write(sidecar(&db_path, "-shm"), b"shm").unwrap();

    let backup = quarantine_database(&db_path).unwrap();

    assert_eq!(std::fs::read(&backup).unwrap(), b"database");
    assert_eq!(std::fs::read(sidecar(&backup, "-wal")).unwrap(), b"wal");
    assert_eq!(std::fs::read(sidecar(&backup, "-shm")).unwrap(), b"shm");
    assert!(!db_path.exists());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pdf_document_reconciliation_rekeys_existing_annotations() {
    use sha2::{Digest, Sha256};

    let state = AppState::new_in_memory();
    let base = std::env::temp_dir().join(format!("md_editor_pdf_id_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("paper.pdf");
    let bytes = b"stable pdf bytes";
    std::fs::write(&path, bytes).unwrap();
    let (_, size, mtime) = crate::pdf::compute_provisional_id(&path).unwrap();

    let mut old_hasher = Sha256::new();
    old_hasher.update(bytes);
    old_hasher.update(size.to_be_bytes());
    old_hasher.update(mtime.unwrap_or_default().to_be_bytes());
    let old_id = format!("{:x}", old_hasher.finalize());
    state
        .save_pdf_document(&old_id, "paper.pdf", size, mtime)
        .unwrap();
    state
        .save_pdf_annotation(&PdfAnnotation {
            id: "annotation-1".to_string(),
            document_id: old_id.clone(),
            page_index: 0,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: "stable".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            ink_stroke: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let bumped = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    file.set_times(std::fs::FileTimes::new().set_modified(bumped))
        .unwrap();
    let (new_id, new_size, new_mtime) = crate::pdf::compute_provisional_id(&path).unwrap();
    assert_ne!(old_id, new_id);

    state
        .save_pdf_document(&new_id, "paper.pdf", new_size, new_mtime)
        .unwrap();

    assert!(state.get_pdf_annotations(&old_id, None).unwrap().is_empty());
    let annotations = state.get_pdf_annotations(&new_id, None).unwrap();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, "annotation-1");
    assert_eq!(annotations[0].document_id, new_id);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pdf_document_reconciliation_prefers_preservation_on_same_path_size_change() {
    let state = AppState::new_in_memory();
    state
        .save_pdf_document("old", "paper.pdf", 10, Some(1))
        .unwrap();
    state
        .save_pdf_annotation(&PdfAnnotation {
            id: "annotation-2".to_string(),
            document_id: "old".to_string(),
            page_index: 0,
            kind: PdfAnnotationKind::Note,
            color: PdfAnnotationColor::Blue,
            selected_text: String::new(),
            ranges: vec![],
            rects: vec![],
            note: Some("keep me".to_string()),
            linked_note_path: None,
            markdown_anchor: None,
            ink_stroke: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

    state
        .save_pdf_document("new", "paper.pdf", 20, Some(2))
        .unwrap();

    assert_eq!(state.get_pdf_annotations("new", None).unwrap().len(), 1);
}

#[test]
fn pdf_annotations_are_isolated_between_vaults_with_identical_document_ids() {
    let base = std::env::temp_dir().join(format!("md_pdf_scopes_{}", uuid::Uuid::new_v4()));
    let first_root = base.join("first");
    let second_root = base.join("second");
    std::fs::create_dir_all(&first_root).unwrap();
    std::fs::create_dir_all(&second_root).unwrap();
    let state = AppState::new_in_memory();

    let annotation = |id: &str, text: &str| PdfAnnotation {
        id: id.to_string(),
        document_id: "same-content-id".to_string(),
        page_index: 0,
        kind: PdfAnnotationKind::Highlight,
        color: PdfAnnotationColor::Yellow,
        selected_text: text.to_string(),
        ranges: vec![],
        rects: vec![],
        note: None,
        linked_note_path: Some(format!("{text}.md")),
        markdown_anchor: None,
        ink_stroke: None,
        created_at: 1,
        updated_at: 1,
    };

    *state.vault_root.lock().unwrap() = Some(first_root);
    state
        .save_pdf_document("same-content-id", "paper.pdf", 10, Some(1))
        .unwrap();
    state
        .save_pdf_annotation(&annotation("first-annotation", "first-note"))
        .unwrap();

    *state.vault_root.lock().unwrap() = Some(second_root);
    state
        .save_pdf_document("same-content-id", "paper.pdf", 10, Some(1))
        .unwrap();
    assert!(
        state
            .get_pdf_annotations("same-content-id", None)
            .unwrap()
            .is_empty()
    );
    state
        .save_pdf_annotation(&annotation("second-annotation", "second-note"))
        .unwrap();
    assert_eq!(
        state.get_pdf_annotations("same-content-id", None).unwrap()[0].id,
        "second-annotation"
    );

    *state.vault_root.lock().unwrap() = Some(base.join("first"));
    let first_annotations = state.get_pdf_annotations("same-content-id", None).unwrap();
    assert_eq!(first_annotations.len(), 1);
    assert_eq!(first_annotations[0].id, "first-annotation");

    let stored_documents: i64 = state
        .db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM pdf_documents", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored_documents, 2);

    drop(state);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn opening_pdf_adopts_legacy_unscoped_annotations_into_current_vault() {
    let base = std::env::temp_dir().join(format!("md_pdf_legacy_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let state = AppState::new_in_memory();
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO pdf_documents (
                 document_id, vault_relative_path, file_size, modified_at,
                 created_at, updated_at
             ) VALUES ('legacy-id', 'paper.pdf', 10, 1, 1, 1)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO pdf_annotations (
                 id, document_id, page_index, kind, color, selected_text,
                 ranges_json, rects_json, note, linked_note_path,
                 markdown_anchor, created_at, updated_at
             ) VALUES ('legacy-annotation', 'legacy-id', 0, 'Highlight',
                       'Yellow', 'legacy', '[]', '[]', NULL, 'note.md',
                       NULL, 1, 1)",
            [],
        )
        .unwrap();
    }

    *state.vault_root.lock().unwrap() = Some(base.clone());
    state
        .save_pdf_document("legacy-id", "paper.pdf", 10, Some(1))
        .unwrap();

    let annotations = state.get_pdf_annotations("legacy-id", None).unwrap();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, "legacy-annotation");
    assert_eq!(annotations[0].document_id, "legacy-id");
    let unscoped_documents: i64 = state
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM pdf_documents WHERE document_id = 'legacy-id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unscoped_documents, 0);

    drop(state);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn xdg_dir_prefers_an_absolute_variable_then_home() {
    use std::ffi::OsString;
    let some = |s: &str| Some(OsString::from(s));

    assert_eq!(
        xdg_dir(some("/data"), some("/home/me"), ".local/share"),
        Some(PathBuf::from("/data"))
    );
    // Relative values must be ignored, per the XDG Base Directory spec.
    assert_eq!(
        xdg_dir(some("relative/dir"), some("/home/me"), ".local/share"),
        Some(PathBuf::from("/home/me/.local/share"))
    );
    assert_eq!(
        xdg_dir(None, some("/home/me"), ".local/state"),
        Some(PathBuf::from("/home/me/.local/state"))
    );
    assert_eq!(xdg_dir(None, None, ".local/share"), None);
    assert_eq!(xdg_dir(None, some("not-absolute"), ".local/share"), None);
}

#[test]
fn portable_db_path_is_beside_the_executable() {
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(portable_db_path(), Some(exe_dir.join(DB_FILE_NAME)));
}

#[test]
fn clearing_settings_keeps_study_history() {
    let state = AppState::new_in_memory();
    crate::config::set_sys_config(&state, "last_vault", "/somewhere").unwrap();
    crate::tracker::save_session(
        &state,
        crate::tracker::StudySession {
            id: 0,
            uid: String::new(),
            date: "2026-11-24".to_string(),
            hours: 1.0,
            activity_type: "Manual".to_string(),
            phase: "Manual".to_string(),
            notes: None,
            updated_at: 0,
        },
    )
    .unwrap();
    crate::config::clear_sys_config(&state).unwrap();
    assert_eq!(
        crate::config::get_sys_config(&state, "last_vault").unwrap(),
        None
    );
    assert_eq!(crate::tracker::get_sessions(&state).unwrap().len(), 1);
}

#[test]
fn recovered_pdf_outline_is_cached_and_follows_a_document_id_change() {
    use crate::pdf::TocEntry;

    let state = AppState::new_in_memory();
    state
        .save_pdf_document("old-id", "papers/b.pdf", 4321, Some(100))
        .unwrap();
    assert_eq!(state.get_pdf_outline("old-id"), None, "nothing cached yet");

    let outline = vec![TocEntry {
        title: "1 Introduction".to_string(),
        page_index: Some(2),
        children: vec![TocEntry {
            title: "1.1 Motivation".to_string(),
            page_index: Some(3),
            children: Vec::new(),
        }],
    }];
    state.put_pdf_outline("old-id", &outline);
    assert_eq!(state.get_pdf_outline("old-id"), Some(outline.clone()));

    // An empty outline is a real answer ("no structure found") and is cached
    // too, so the document is not rescanned on every open.
    state.put_pdf_outline("old-id", &[]);
    assert_eq!(state.get_pdf_outline("old-id"), Some(Vec::new()));
    state.put_pdf_outline("old-id", &outline);

    // The id moves with the file, as annotations and references do.
    state
        .save_pdf_document("new-id", "papers/b.pdf", 4321, Some(200))
        .unwrap();
    assert_eq!(state.get_pdf_outline("new-id"), Some(outline));
}

#[test]
fn an_outline_cached_by_an_older_recovery_version_is_ignored() {
    let state = AppState::new_in_memory();
    state
        .save_pdf_document("doc", "papers/c.pdf", 1, Some(1))
        .unwrap();
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO pdf_outline (document_id, outline_json, updated_at) VALUES (?1, ?2, 0)",
            rusqlite::params![
                state.pdf_storage_document_id("doc").unwrap(),
                format!("[{}, []]", crate::pdf::OUTLINE_VERSION + 99)
            ],
        )
        .unwrap();
    }
    assert_eq!(state.get_pdf_outline("doc"), None);
}

#[test]
fn v5_migration_backfills_unique_uids_and_is_idempotent() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE tracker_sessions (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             date TEXT NOT NULL,
             hours REAL NOT NULL,
             activity_type TEXT NOT NULL,
             phase TEXT NOT NULL,
             notes TEXT
         );
         CREATE TABLE tracker_activity (id INTEGER PRIMARY KEY AUTOINCREMENT, type TEXT NOT NULL, text TEXT NOT NULL, time TEXT NOT NULL);
         CREATE TABLE tracker_kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE VIRTUAL TABLE file_search USING fts5(path, content);
         CREATE TABLE pdf_documents (document_id TEXT PRIMARY KEY, vault_relative_path TEXT NOT NULL, file_size INTEGER NOT NULL, modified_at INTEGER, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE pdf_annotations (id TEXT PRIMARY KEY, document_id TEXT NOT NULL, page_index INTEGER NOT NULL, kind TEXT NOT NULL, color TEXT NOT NULL, selected_text TEXT NOT NULL, ranges_json TEXT NOT NULL, rects_json TEXT NOT NULL, note TEXT, linked_note_path TEXT, markdown_anchor TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE pdf_text_cache (path TEXT PRIMARY KEY, file_size INTEGER NOT NULL, modified_at INTEGER NOT NULL, content TEXT NOT NULL);
         CREATE TABLE pdf_references (document_id TEXT PRIMARY KEY, links_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE pdf_outline (document_id TEXT PRIMARY KEY, outline_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE vault_file_meta (scope TEXT NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL, modified_ns INTEGER NOT NULL, links_json TEXT NOT NULL, PRIMARY KEY (scope, path));
         CREATE TABLE pdf_sidecar_sync (document_id TEXT PRIMARY KEY, content_hash TEXT NOT NULL, ids_json TEXT NOT NULL);
         INSERT INTO settings (key, value) VALUES ('last_vault', '/tmp/legacy-vault');
         INSERT INTO tracker_sessions (date, hours, activity_type, phase, notes) VALUES
             ('2026-11-20', 1.5, 'Study', '1A', 'duplicate'),
             ('2026-11-20', 1.5, 'Study', '1A', 'duplicate');
         PRAGMA user_version = 4;",
    )
    .unwrap();

    apply_migrations(&db).unwrap();
    apply_migrations(&db).unwrap();

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    let expected_scope = AppState::pdf_scope_prefix_for_root(Path::new("/tmp/legacy-vault"));
    let rows: Vec<(String, String)> = {
        let mut stmt = db
            .prepare("SELECT uid, vault_scope FROM tracker_sessions ORDER BY id ASC")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .flatten()
            .collect()
    };
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].0.is_empty() && !rows[1].0.is_empty());
    assert_ne!(rows[0].0, rows[1].0);
    assert_eq!(rows[0].1, expected_scope);
    assert_eq!(rows[1].1, expected_scope);
}
