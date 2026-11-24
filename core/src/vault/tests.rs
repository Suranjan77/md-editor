//! Tests for `vault`.

use super::*;

fn insert_annotation_link(state: &AppState, id: &str, document_id: &str, note_path: &str) {
    let storage_document_id = state.pdf_storage_document_id(document_id).unwrap();
    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO pdf_annotations (
             id, document_id, page_index, kind, color, selected_text,
             ranges_json, rects_json, note, linked_note_path,
             markdown_anchor, created_at, updated_at
         ) VALUES (?1, ?2, 0, 'highlight', 'yellow', 'text',
                   '[]', '[]', NULL, ?3, NULL, 1, 1)",
        rusqlite::params![id, storage_document_id, note_path],
    )
    .unwrap();
}

#[test]
fn write_file_replaces_content_and_leaves_no_temp_files() {
    let base = std::env::temp_dir().join(format!("md_atomic_{}", uuid::Uuid::new_v4()));
    let target = base.join("notes/a.md");

    // Parent directories are created on demand.
    write_file(&target, "first").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "first");

    // Overwriting replaces the content wholesale.
    write_file(&target, "second, rather longer than the first").unwrap();
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "second, rather longer than the first"
    );

    // The temporary file used for the atomic swap must not survive.
    let leftovers: Vec<_> = fs::read_dir(base.join("notes"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "atomic write left temp files behind: {leftovers:?}"
    );

    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn write_file_preserves_existing_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let base = std::env::temp_dir().join(format!("md_perm_{}", uuid::Uuid::new_v4()));
    let target = base.join("private.md");
    write_file(&target, "secret").unwrap();

    // Lock the note down, then save over it the way autosave would.
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    write_file(&target, "secret, revised").unwrap();

    let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "replacing the file must not widen its permissions"
    );

    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn write_file_writes_through_a_symlink() {
    let base = std::env::temp_dir().join(format!("md_link_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let real = base.join("real.md");
    let link = base.join("link.md");
    fs::write(&real, "original").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    write_file(&link, "updated").unwrap();

    assert_eq!(
        fs::read_to_string(&real).unwrap(),
        "updated",
        "the symlink target should have been rewritten"
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink itself must survive, not be replaced by a regular file"
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn write_file_never_leaves_a_truncated_file() {
    // The point of the temp-file-and-rename dance: the destination is only
    // ever the complete old content or the complete new content, so a
    // reader between writes can never observe a partial file.
    let base = std::env::temp_dir().join(format!("md_atomic_{}", uuid::Uuid::new_v4()));
    let target = base.join("note.md");

    let long = "x".repeat(64 * 1024);
    write_file(&target, &long).unwrap();

    let short = "y";
    write_file(&target, short).unwrap();

    let observed = fs::read_to_string(&target).unwrap();
    assert!(
        observed == long || observed == short,
        "observed a partially written file of {} bytes",
        observed.len()
    );
    assert_eq!(observed, short);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn resolve_normal_paths_stay_within_root() {
    let root = Path::new("/vault");
    assert_eq!(
        resolve_vault_path(root, "notes/a.md"),
        PathBuf::from("/vault/notes/a.md")
    );
    assert_eq!(
        resolve_vault_path(root, "./a.md"),
        PathBuf::from("/vault/a.md")
    );
    // Interior `..` that stays inside the vault is allowed.
    assert_eq!(
        resolve_vault_path(root, "notes/../a.md"),
        PathBuf::from("/vault/a.md")
    );
}

#[test]
fn checked_rejects_traversal_escapes() {
    let base = std::env::temp_dir().join(format!("md_checked_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let root = base.as_path();
    assert!(resolve_vault_path_checked(root, "../etc/passwd").is_err());
    assert!(resolve_vault_path_checked(root, "notes/../../secret").is_err());
    assert!(resolve_vault_path_checked(root, "/etc/passwd").is_err());
    assert!(resolve_vault_path_checked(root, "notes/a.md").is_ok());
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn walk_skips_excluded_and_dot_dirs() {
    let base = std::env::temp_dir().join(format!("md_walk_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("notes")).unwrap();
    fs::create_dir_all(base.join("node_modules/pkg")).unwrap();
    fs::create_dir_all(base.join(".obsidian")).unwrap();
    fs::write(base.join("notes/a.md"), "a").unwrap();
    fs::write(base.join("node_modules/pkg/b.md"), "b").unwrap();
    fs::write(base.join(".obsidian/c.md"), "c").unwrap();

    let files = list_all_md_files(&base).unwrap();
    assert_eq!(files.len(), 1, "only notes/a.md should be indexed");
    assert!(files[0].ends_with("notes/a.md"));

    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn walk_does_not_follow_symlink_cycles() {
    let base = std::env::temp_dir().join(format!("md_symlink_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("real")).unwrap();
    fs::write(base.join("real/a.md"), "a").unwrap();
    // A symlink pointing back at the vault root would loop forever if followed.
    std::os::unix::fs::symlink(&base, base.join("real/loop")).unwrap();

    // Must terminate and find the single real file.
    let files = list_all_md_files(&base).unwrap();
    assert_eq!(files.len(), 1);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn resolve_clamps_escapes_into_root() {
    let root = Path::new("/vault");
    // Even the non-checked variant must never escape the root.
    let resolved = resolve_vault_path(root, "../../etc/passwd");
    assert!(resolved.starts_with("/vault"));
    let resolved_abs = resolve_vault_path(root, "/etc/passwd");
    assert!(resolved_abs.starts_with("/vault"));
}

#[test]
fn deleting_directory_removes_subtree_from_indexes() {
    let base = std::env::temp_dir().join(format!("md_delete_dir_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("dir")).unwrap();
    fs::write(base.join("dir/a.md"), "[[dir/b]] alpha").unwrap();
    fs::write(base.join("dir/b.md"), "beta").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();

    delete_entry(&state, "dir").unwrap();

    assert!(!base.join("dir").exists());
    let index = state.file_index.lock().unwrap();
    assert!(index.get_outgoing_links(&base.join("dir/a.md")).is_empty());
    assert!(index.get_backlinks(&base.join("dir/b.md")).is_empty());
    drop(index);
    let db = state.db.lock().unwrap();
    let remaining: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM file_search WHERE substr(path, 1, 4) = 'dir/'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);

    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn file_symlinks_are_hidden_and_cannot_escape_vault() {
    let base = std::env::temp_dir().join(format!("md_file_symlink_{}", uuid::Uuid::new_v4()));
    let outside = std::env::temp_dir().join(format!("md_outside_{}.md", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, base.join("linked.md")).unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();

    assert!(list_vault(&state).unwrap().is_empty());
    assert!(list_all_md_files(&base).unwrap().is_empty());
    assert!(open_file(&state, "linked.md").is_err());
    assert!(save_file(&state, "linked.md", "changed").is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "outside");

    let dangling_target = outside.with_extension("missing");
    std::os::unix::fs::symlink(&dangling_target, base.join("dangling.md")).unwrap();
    assert!(create_file(&state, "dangling.md").is_err());
    assert!(!dangling_target.exists());

    let _ = fs::remove_dir_all(&base);
    let _ = fs::remove_file(&outside);
}

#[test]
fn save_file_surfaces_write_failures() {
    let base = std::env::temp_dir().join(format!("md_save_failure_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("blocked.md")).unwrap();
    let state = AppState::new_in_memory();
    {
        let mut root = state.vault_root.lock().unwrap();
        *root = Some(base.clone());
    }

    let error = save_file(&state, "blocked.md", "content").unwrap_err();
    assert!(error.contains("Failed to"), "unexpected error: {error}");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn listing_accepts_supported_extensions_case_insensitively() {
    let base = std::env::temp_dir().join(format!("md_extensions_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    for name in ["README.MD", "Notes.PDF", "IMG.JPG", "anim.gif", "scan.bmp"] {
        fs::write(base.join(name), "content").unwrap();
    }

    let mut names: Vec<String> = list_vault_entries(&base)
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["IMG.JPG", "Notes.PDF", "README.MD", "anim.gif", "scan.bmp"]
    );

    let markdown = list_all_md_files(&base).unwrap();
    assert_eq!(markdown, vec![base.join("README.MD")]);
    let pdfs = list_all_pdf_files(&base).unwrap();
    assert_eq!(pdfs, vec![base.join("Notes.PDF")]);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn graph_snapshot_is_sorted_aggregated_and_includes_health_nodes() {
    let base = std::env::temp_dir().join(format!("md_graph_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("notes")).unwrap();
    fs::write(
        base.join("notes/a.md"),
        "[[notes/b]] [[notes/b|again]] [[paper.pdf]] [[missing]] [[gone.pdf]] [[diagram.png]]",
    )
    .unwrap();
    fs::write(base.join("notes/b.md"), "[[notes/a]]").unwrap();
    fs::write(base.join("paper.pdf"), b"not parsed by the graph").unwrap();
    fs::write(base.join("orphan.pdf"), b"unconnected reference").unwrap();
    fs::write(base.join("diagram.png"), b"image target").unwrap();

    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();
    state
        .save_pdf_document("paper-doc", "paper.pdf", 23, None)
        .unwrap();
    let paper_storage_id = state.pdf_storage_document_id("paper-doc").unwrap();

    // Two path spellings normalize to one weighted PDF -> note edge. The
    // escaping third path is ignored rather than leaking outside the vault.
    {
        let db = state.db.lock().unwrap();
        for (id, linked_note) in [
            ("ann-1", "notes/b.md"),
            ("ann-2", r"notes\b.md"),
            ("ann-escaped", "../../outside.md"),
        ] {
            db.execute(
                "INSERT INTO pdf_annotations (
                     id, document_id, page_index, kind, color, selected_text,
                     ranges_json, rects_json, note, linked_note_path,
                     markdown_anchor, created_at, updated_at
                 ) VALUES (?1, ?2, 0, 'highlight', 'yellow', 'text',
                           '[]', '[]', NULL, ?3, NULL, 1, 1)",
                rusqlite::params![id, &paper_storage_id, linked_note],
            )
            .unwrap();
        }
        let foreign_document_id = "another-vault:foreign-doc";
        db.execute(
            "INSERT INTO pdf_documents (
                 document_id, vault_relative_path, file_size, modified_at,
                 created_at, updated_at
             ) VALUES (?1, 'foreign.pdf', 17, NULL, 1, 1)",
            rusqlite::params![foreign_document_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO pdf_annotations (
                 id, document_id, page_index, kind, color, selected_text,
                 ranges_json, rects_json, note, linked_note_path,
                 markdown_anchor, created_at, updated_at
             ) VALUES (?1, ?2, 0, 'highlight', 'yellow', 'historical',
                       '[]', '[]', NULL, 'notes/a.md', NULL, 1, 1)",
            rusqlite::params![
                format!("{foreign_document_id}-annotation"),
                foreign_document_id
            ],
        )
        .unwrap();
    }

    let snapshot = get_graph_snapshot(&state).unwrap();
    assert_eq!(snapshot, get_graph_snapshot(&state).unwrap());

    let node_paths: Vec<&str> = snapshot
        .nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect();
    assert_eq!(
        node_paths,
        vec![
            "gone.pdf",
            "missing.md",
            "notes/a.md",
            "notes/b.md",
            "orphan.pdf",
            "paper.pdf",
        ]
    );
    assert!(
        snapshot
            .edges
            .windows(2)
            .all(|pair| (&pair[0].source, &pair[0].target, pair[0].kind)
                <= (&pair[1].source, &pair[1].target, pair[1].kind))
    );
    assert!(
        snapshot.nodes.iter().all(|node| node.path != "diagram.png"),
        "existing image wikilinks are outside the knowledge graph"
    );
    assert!(
        snapshot.nodes.iter().all(|node| node.path != "foreign.pdf"),
        "historical PDF records from another vault must not leak into this graph"
    );

    let find_node = |path: &str| {
        snapshot
            .nodes
            .iter()
            .find(|node| node.path == path)
            .unwrap()
    };
    assert_eq!(find_node("notes/a.md").kind, GraphNodeKind::Markdown);
    assert_eq!(find_node("notes/a.md").incoming, 1);
    assert_eq!(find_node("notes/a.md").outgoing, 4);
    assert_eq!(find_node("notes/b.md").incoming, 3);
    assert_eq!(find_node("notes/b.md").outgoing, 1);
    assert_eq!(find_node("paper.pdf").kind, GraphNodeKind::Pdf);
    assert_eq!(find_node("paper.pdf").incoming, 1);
    assert_eq!(find_node("paper.pdf").outgoing, 2);
    assert_eq!(find_node("orphan.pdf").kind, GraphNodeKind::Pdf);
    assert_eq!(find_node("orphan.pdf").incoming, 0);
    assert_eq!(find_node("orphan.pdf").outgoing, 0);
    assert_eq!(find_node("missing.md").kind, GraphNodeKind::Missing);
    assert!(!find_node("missing.md").exists);
    assert_eq!(find_node("gone.pdf").kind, GraphNodeKind::Missing);

    let annotation_edge = snapshot
        .edges
        .iter()
        .find(|edge| edge.kind == GraphEdgeKind::PdfAnnotation)
        .unwrap();
    assert_eq!(annotation_edge.source, "paper.pdf");
    assert_eq!(annotation_edge.target, "notes/b.md");
    assert_eq!(annotation_edge.weight, 2);
    assert_eq!(
        snapshot
            .edges
            .iter()
            .filter(|edge| edge.kind == GraphEdgeKind::PdfAnnotation)
            .count(),
        1,
        "other-vault PDF identities must not contribute annotation edges"
    );

    let repeated_wikilink = snapshot
        .edges
        .iter()
        .find(|edge| {
            edge.kind == GraphEdgeKind::WikiLink
                && edge.source == "notes/a.md"
                && edge.target == "notes/b.md"
        })
        .unwrap();
    assert_eq!(repeated_wikilink.weight, 1);

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn graph_snapshot_requires_an_open_vault() {
    let state = AppState::new_in_memory();
    assert_eq!(get_graph_snapshot(&state).unwrap_err(), "No vault root set");
}

#[test]
fn graph_pdf_annotation_edges_are_isolated_by_vault() {
    let base = std::env::temp_dir().join(format!("md_graph_scopes_{}", uuid::Uuid::new_v4()));
    let first = base.join("first");
    let second = base.join("second");
    for (root, note) in [(&first, "first.md"), (&second, "second.md")] {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(note), note).unwrap();
        fs::write(root.join("paper.pdf"), b"identical paper").unwrap();
    }
    let state = AppState::new_in_memory();

    set_vault_root(&state, first.to_str().unwrap()).unwrap();
    let (first_size, first_mtime) = file_size_and_mtime(&first.join("paper.pdf"));
    state
        .save_pdf_document("same-pdf", "paper.pdf", first_size, Some(first_mtime))
        .unwrap();
    insert_annotation_link(&state, "first-link", "same-pdf", "first.md");

    set_vault_root(&state, second.to_str().unwrap()).unwrap();
    let (second_size, second_mtime) = file_size_and_mtime(&second.join("paper.pdf"));
    state
        .save_pdf_document("same-pdf", "paper.pdf", second_size, Some(second_mtime))
        .unwrap();
    insert_annotation_link(&state, "second-link", "same-pdf", "second.md");
    let second_snapshot = get_graph_snapshot(&state).unwrap();
    assert!(
        second_snapshot.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::PdfAnnotation && edge.target == "second.md"
        })
    );
    assert!(
        second_snapshot
            .edges
            .iter()
            .all(|edge| edge.target != "first.md")
    );

    set_vault_root(&state, first.to_str().unwrap()).unwrap();
    let first_snapshot = get_graph_snapshot(&state).unwrap();
    assert!(
        first_snapshot
            .edges
            .iter()
            .any(|edge| { edge.kind == GraphEdgeKind::PdfAnnotation && edge.target == "first.md" })
    );
    assert!(
        first_snapshot
            .edges
            .iter()
            .all(|edge| edge.target != "second.md")
    );

    let paper = fs::OpenOptions::new()
        .write(true)
        .open(first.join("paper.pdf"))
        .unwrap();
    let bumped = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    paper
        .set_times(fs::FileTimes::new().set_modified(bumped))
        .unwrap();
    let touched_snapshot = get_graph_snapshot(&state).unwrap();
    assert!(
        touched_snapshot
            .edges
            .iter()
            .any(|edge| { edge.kind == GraphEdgeKind::PdfAnnotation && edge.target == "first.md" })
    );

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn stale_vault_index_generation_cannot_publish_over_newer_vault() {
    let base = std::env::temp_dir().join(format!("md_vault_race_{}", uuid::Uuid::new_v4()));
    let first = base.join("first");
    let second = base.join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join("a.md"), "first-only-token").unwrap();
    fs::write(second.join("b.md"), "second-only-token").unwrap();
    let state = AppState::new_in_memory();

    let first_generation = state.begin_vault_change(first.clone()).unwrap();
    assert!(
        index_vault_for_generation(&state, first.to_str().unwrap(), first_generation)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        search_vault(&state, "first-only-token")
            .unwrap()
            .items
            .len(),
        1
    );

    let second_generation = state.begin_vault_change(second.clone()).unwrap();
    assert!(
        search_vault(&state, "first-only-token")
            .unwrap()
            .items
            .is_empty(),
        "opening a new vault must hide the old vault's search rows before indexing finishes"
    );
    assert!(
        state.file_index.lock().unwrap().outgoing.is_empty(),
        "opening a new vault must hide the old vault's link index before indexing finishes"
    );
    assert!(
        index_vault_for_generation(&state, first.to_str().unwrap(), first_generation)
            .unwrap()
            .is_none()
    );
    assert!(
        index_vault_for_generation(&state, second.to_str().unwrap(), second_generation)
            .unwrap()
            .is_some()
    );

    assert_eq!(state.vault_root.lock().unwrap().as_ref(), Some(&second));
    assert!(
        search_vault(&state, "first-only-token")
            .unwrap()
            .items
            .is_empty()
    );
    let results = search_vault(&state, "second-only-token").unwrap();
    assert_eq!(results.items.len(), 1);
    assert_eq!(results.items[0].path, "b.md");
    assert!(
        state
            .file_index
            .lock()
            .unwrap()
            .outgoing
            .contains_key(&second.join("b.md"))
    );

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn newly_created_note_is_immediately_available_to_graph() {
    let base = std::env::temp_dir().join(format!("md_graph_new_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();

    create_file(&state, "capture.md").unwrap();

    let snapshot = get_graph_snapshot(&state).unwrap();
    assert!(snapshot.nodes.iter().any(|node| {
        node.path == "capture.md"
            && node.kind == GraphNodeKind::Markdown
            && node.incoming == 0
            && node.outgoing == 0
    }));

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn renaming_markdown_extension_rewrites_backlinks() {
    let base = std::env::temp_dir().join(format!("md_rename_markdown_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    fs::write(base.join("target.markdown"), "target [[target]]").unwrap();
    fs::write(base.join("source.md"), "[[target]]").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();

    rename_entry(&state, "target.markdown", "renamed.markdown").unwrap();

    assert_eq!(
        fs::read_to_string(base.join("source.md")).unwrap(),
        "[[renamed]]"
    );
    assert_eq!(
        fs::read_to_string(base.join("renamed.markdown")).unwrap(),
        "target [[renamed]]"
    );
    assert!(base.join("renamed.markdown").exists());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn renaming_markdown_updates_pdf_annotation_note_path() {
    let base = std::env::temp_dir().join(format!("md_rename_note_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    fs::write(base.join("old.md"), "linked note").unwrap();
    fs::write(base.join("paper.pdf"), b"pdf identity").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();
    let (size, modified_at) = file_size_and_mtime(&base.join("paper.pdf"));
    state
        .save_pdf_document("paper", "paper.pdf", size, Some(modified_at))
        .unwrap();
    insert_annotation_link(&state, "annotation", "paper", "old.md");

    rename_entry(&state, "old.md", "renamed.md").unwrap();

    let linked_note: String = state
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT linked_note_path FROM pdf_annotations WHERE id = 'annotation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked_note, "renamed.md");
    let snapshot = get_graph_snapshot(&state).unwrap();
    assert!(snapshot.edges.iter().any(|edge| {
        edge.kind == GraphEdgeKind::PdfAnnotation
            && edge.source == "paper.pdf"
            && edge.target == "renamed.md"
    }));

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn renaming_pdf_updates_document_and_search_paths() {
    let base = std::env::temp_dir().join(format!("md_rename_pdf_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("library")).unwrap();
    fs::write(base.join("note.md"), "[[paper.pdf]] linked note").unwrap();
    fs::write(base.join("paper.pdf"), b"pdf identity").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();
    let (size, modified_at) = file_size_and_mtime(&base.join("paper.pdf"));
    state
        .save_pdf_document("paper", "paper.pdf", size, Some(modified_at))
        .unwrap();
    insert_annotation_link(&state, "annotation", "paper", "note.md");
    state
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO file_search (path, content) VALUES ('paper.pdf', 'pdf-search-token')",
            [],
        )
        .unwrap();

    rename_entry(&state, "paper.pdf", "library/paper.pdf").unwrap();

    assert_eq!(
        fs::read_to_string(base.join("note.md")).unwrap(),
        "[[library/paper.pdf]] linked note"
    );
    assert_eq!(
        state.get_pdf_path_by_id("paper").unwrap().as_deref(),
        Some("library/paper.pdf")
    );
    let search = search_vault(&state, "pdf-search-token").unwrap();
    assert_eq!(search.items.len(), 1);
    assert_eq!(search.items[0].path, "library/paper.pdf");
    let snapshot = get_graph_snapshot(&state).unwrap();
    assert!(snapshot.nodes.iter().all(|node| node.path != "paper.pdf"));
    assert!(snapshot.edges.iter().any(|edge| {
        edge.kind == GraphEdgeKind::WikiLink
            && edge.source == "note.md"
            && edge.target == "library/paper.pdf"
    }));
    assert!(snapshot.edges.iter().any(|edge| {
        edge.kind == GraphEdgeKind::PdfAnnotation
            && edge.source == "library/paper.pdf"
            && edge.target == "note.md"
    }));

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn renaming_directory_rebuilds_indexes_and_moves_persisted_paths() {
    let base = std::env::temp_dir().join(format!("md_rename_dir_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("old")).unwrap();
    fs::write(
        base.join("old/note.md"),
        "directory-search-token [[./peer]]",
    )
    .unwrap();
    fs::write(base.join("old/peer.md"), "peer").unwrap();
    fs::write(base.join("old/paper.pdf"), b"pdf identity").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();
    let (size, modified_at) = file_size_and_mtime(&base.join("old/paper.pdf"));
    state
        .save_pdf_document("paper", r"old\paper.pdf", size, Some(modified_at))
        .unwrap();
    insert_annotation_link(&state, "annotation", "paper", r"old\note.md");
    state
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO file_search (path, content)
             VALUES ('old/paper.pdf', 'moved-pdf-search-token')",
            [],
        )
        .unwrap();

    rename_entry(&state, "old", "moved").unwrap();

    {
        let index = state.file_index.lock().unwrap();
        assert!(
            index
                .get_outgoing_links(&base.join("old/note.md"))
                .is_empty()
        );
        assert_eq!(
            index.get_outgoing_links(&base.join("moved/note.md")),
            vec![base.join("moved/peer.md")]
        );
    }
    let markdown_search = search_vault(&state, "directory-search-token").unwrap();
    assert_eq!(markdown_search.items.len(), 1);
    assert_eq!(markdown_search.items[0].path, "moved/note.md");
    let pdf_search = search_vault(&state, "moved-pdf-search-token").unwrap();
    assert_eq!(pdf_search.items.len(), 1);
    assert_eq!(pdf_search.items[0].path, "moved/paper.pdf");
    assert_eq!(
        state.get_pdf_path_by_id("paper").unwrap().as_deref(),
        Some("moved/paper.pdf")
    );
    let linked_note: String = state
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT linked_note_path FROM pdf_annotations WHERE id = 'annotation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked_note, "moved/note.md");
    let snapshot = get_graph_snapshot(&state).unwrap();
    assert!(snapshot.edges.iter().any(|edge| {
        edge.kind == GraphEdgeKind::PdfAnnotation
            && edge.source == "moved/paper.pdf"
            && edge.target == "moved/note.md"
    }));

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn indexing_adopts_legacy_pdf_rows_before_directory_rename() {
    let base = std::env::temp_dir().join(format!("md_legacy_rename_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("old")).unwrap();
    fs::write(base.join("old/note.md"), "linked note").unwrap();
    fs::write(base.join("old/paper.pdf"), b"legacy pdf identity").unwrap();
    let (_, size, modified_at) =
        crate::pdf::compute_provisional_id(&base.join("old/paper.pdf")).unwrap();
    let state = AppState::new_in_memory();
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO pdf_documents (
                 document_id, vault_relative_path, file_size, modified_at,
                 created_at, updated_at
             ) VALUES ('legacy-document', 'old/paper.pdf', ?1, ?2, 1, 1)",
            rusqlite::params![size as i64, modified_at],
        )
        .unwrap();
    }
    insert_annotation_link(
        &state,
        "legacy-directory-link",
        "legacy-document",
        "old/note.md",
    );

    set_vault_root(&state, base.to_str().unwrap()).unwrap();
    rename_entry(&state, "old", "moved").unwrap();

    let snapshot = get_graph_snapshot(&state).unwrap();
    assert!(snapshot.edges.iter().any(|edge| {
        edge.kind == GraphEdgeKind::PdfAnnotation
            && edge.source == "moved/paper.pdf"
            && edge.target == "moved/note.md"
    }));
    let legacy_rows: i64 = state
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM pdf_documents
             WHERE document_id = 'legacy-document'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_rows, 0);

    drop(state);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn handwritten_pages_are_listed_in_the_vault() {
    assert!(is_supported_vault_path(Path::new("notes/lecture.ink")));
    assert!(is_supported_vault_path(Path::new("notes/LECTURE.INK")));
    assert!(!is_supported_vault_path(Path::new("notes/lecture.inkx")));
}

#[test]
fn guarded_save_keeps_an_external_edit_as_a_conflict_copy() {
    let base = std::env::temp_dir().join(format!("md_editor_conflict_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join("notes")).unwrap();
    std::fs::write(base.join("notes/B.md"), "original").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();
    let known = content_hash("original");

    // Unchanged on disk: a plain save, no copy.
    let copy = save_file_guarded(&state, "notes/B.md", "mine 1", Some(known), "S").unwrap();
    assert_eq!(copy, None);
    let known = content_hash("mine 1");

    // Changed underneath us: theirs is kept beside ours.
    std::fs::write(base.join("notes/B.md"), "theirs").unwrap();
    let copy = save_file_guarded(&state, "notes/B.md", "mine 2", Some(known), "S").unwrap();
    assert_eq!(copy.as_deref(), Some("notes/B (conflict S).md"));
    assert_eq!(
        std::fs::read_to_string(base.join("notes/B.md")).unwrap(),
        "mine 2"
    );
    assert_eq!(
        std::fs::read_to_string(base.join("notes/B (conflict S).md")).unwrap(),
        "theirs"
    );

    // A second conflict in the same second gets a numbered name.
    std::fs::write(base.join("notes/B.md"), "theirs again").unwrap();
    let copy = save_file_guarded(
        &state,
        "notes/B.md",
        "mine 3",
        Some(content_hash("mine 2")),
        "S",
    )
    .unwrap();
    assert_eq!(copy.as_deref(), Some("notes/B (conflict S 2).md"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn renaming_a_note_rewrites_bare_links_from_its_sibling_folder() {
    let base = std::env::temp_dir().join(format!("md_editor_rename_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join("notes")).unwrap();
    std::fs::write(base.join("notes/A.md"), "Links: [[B]]\n").unwrap();
    std::fs::write(base.join("notes/B.md"), "# B\n\nSelf link: [[B]]\n").unwrap();
    let state = AppState::new_in_memory();
    set_vault_root(&state, base.to_str().unwrap()).unwrap();

    rename_entry(&state, "notes/B.md", "notes/C.md").unwrap();

    let a = std::fs::read_to_string(base.join("notes/A.md")).unwrap();
    assert!(!a.contains("[[B]]"), "A.md still links the old name: {a:?}");
    let c = std::fs::read_to_string(base.join("notes/C.md")).unwrap();
    assert!(
        !c.contains("[[B]]"),
        "C.md's self link not rewritten: {c:?}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn write_file_refuses_a_read_only_note() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!("md_editor_ro_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let note = base.join("locked.md");
    std::fs::write(&note, "keep me").unwrap();
    std::fs::set_permissions(&note, std::fs::Permissions::from_mode(0o444)).unwrap();

    let err = write_file(&note, "overwrite").unwrap_err();
    assert!(err.contains("read-only"), "{err}");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "keep me");

    std::fs::set_permissions(&note, std::fs::Permissions::from_mode(0o644)).unwrap();
    let _ = std::fs::remove_dir_all(&base);
}
