use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;
use crate::types::{
    BacklinkItem, BacklinkTarget, FileEntry, GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind,
    GraphSnapshot, SearchResult,
};

pub const IMAGE_EXTENSIONS: [&str; 8] =
    ["jpeg", "jpg", "png", "gif", "bmp", "svg", "webp", "avif"];
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];
const PDF_EXTENSIONS: [&str; 1] = ["pdf"];

/// Directory names that are never worth indexing or showing in the tree. These
/// are heavy or irrelevant in note vaults and would otherwise blow up indexing
/// time and the file listing. Dotfiles (`.git`, `.obsidian`, …) are skipped
/// separately by the `starts_with('.')` check in the walkers.
const EXCLUDED_DIRS: [&str; 6] = [
    "node_modules",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".trash",
];

/// Maximum directory depth the vault walkers descend. Guards against
/// pathologically deep trees (and, together with the symlink check, against
/// runaway recursion) without affecting any realistic vault layout.
const MAX_WALK_DEPTH: usize = 32;

/// Whether a directory should be skipped during indexing/listing: dotfolders
/// and the well-known heavy directories in [`EXCLUDED_DIRS`].
fn is_excluded_dir(name: &str) -> bool {
    name.starts_with('.') || EXCLUDED_DIRS.contains(&name)
}

/// Whether a directory entry is a symlink. Symlinked directories are not
/// followed during walks so a symlink cycle can never make indexing hang.
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).map_or(false, |m| m.file_type().is_symlink())
}

// ── Public API ──────────────────────────────────────────────────────

/// Set the vault root directory and index all markdown files.
/// Returns the file listing for the vault.
pub fn set_vault_root(state: &AppState, path: &str) -> Result<Vec<FileEntry>, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }
    let generation = state.begin_vault_change(root)?;
    loop {
        match index_vault_for_generation(state, path, generation)? {
            Some(entries) => return Ok(entries),
            None if state.current_vault_generation() == generation => {
                // A save, watcher event, or file operation landed while the
                // snapshot was being built. Rebuild from the newer disk state
                // instead of publishing the stale snapshot over that mutation.
            }
            None => return Err("Vault changed while indexing".to_string()),
        }
    }
}

/// Build and publish an index only if `generation` is still the requested
/// vault. This lets the UI discard slow A results after the user has opened B,
/// without stale work mutating the shared root, FTS database, or link index.
pub fn index_vault_for_generation(
    state: &AppState,
    path: &str,
    generation: u64,
) -> Result<Option<Vec<FileEntry>>, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }
    let content_revision = state.current_vault_content_revision();

    // Phase 1: read every file and build the link index into local structures
    // WITHOUT holding any shared lock. The disk I/O here is the slow part; if
    // it ran while holding `file_index`/`db`, the UI thread would block the
    // moment it touched the index (e.g. opening a file), reintroducing the
    // freeze this off-thread indexing is meant to avoid.
    let md_files = list_all_md_files(&root)?;
    let mut index = FileIndex::new(root.clone());
    // (relative path, content) for the FTS rebuild.
    let mut indexed: Vec<(String, String)> = Vec::with_capacity(md_files.len());
    // (absolute path, content) for the two-pass link-graph rebuild, which needs
    // every file's name registered before it can resolve bare links to files in
    // other subfolders.
    let mut for_graph: Vec<(PathBuf, String)> = Vec::with_capacity(md_files.len());
    for file_path in md_files {
        if let Ok(content) = read_file(&file_path) {
            let rel_path = path_to_relative_string(&file_path, &root);
            indexed.push((rel_path, content.clone()));
            for_graph.push((file_path, content));
        }
    }
    index.rebuild(&for_graph);

    // Phase 1b: identify every PDF so legacy annotation rows can be adopted
    // into this vault before any rename, and extract searchable text when a
    // renderer is available. All cache keys use the target root explicitly;
    // the currently published vault may change while this work is running.
    let mut pdf_indexed: Vec<(String, String)> = Vec::new();
    let mut pdf_documents: Vec<(String, String, u64, Option<i64>)> = Vec::new();
    for pdf_path in list_all_pdf_files(&root).unwrap_or_default() {
        let rel_path = path_to_relative_string(&pdf_path, &root);
        let (file_size, modified_at) = file_size_and_mtime(&pdf_path);
        // Reuse the content identity for unchanged, already-scoped PDFs. New,
        // replaced, or legacy PDFs pay the bounded 1 MiB hash once.
        let identity = state
            .stored_pdf_document_id_for_vault(
                &root,
                &rel_path,
                file_size,
                Some(modified_at),
            )
            .map(|document_id| (document_id, file_size, Some(modified_at)))
            .or_else(|| crate::pdf::compute_provisional_id(&pdf_path).ok());
        if let Some((document_id, size, modified)) = identity {
            pdf_documents.push((document_id, rel_path.clone(), size, modified));
        }

        if let Some(renderer) = state.pdf_renderer.as_ref() {
            // Reuse cached text when the PDF is unchanged; only fall back to
            // the (slow) pdfium extraction when size/mtime differ.
            let text = if let Some(cached) = state.get_cached_pdf_text_for_vault(
                &root,
                &rel_path,
                file_size,
                modified_at,
            ) {
                cached
            } else {
                match renderer.extract_document_text(&pdf_path.to_string_lossy()) {
                    Ok(text) => {
                        // Cache even empty results (e.g. scanned PDFs) so we
                        // don't re-extract them on every open.
                        state.put_cached_pdf_text_for_vault(
                            &root,
                            &rel_path,
                            file_size,
                            modified_at,
                            &text,
                        );
                        text
                    }
                    Err(_) => continue,
                }
            };
            if !text.trim().is_empty() {
                pdf_indexed.push((rel_path, text));
            }
        }
    }
    let entries = list_vault_entries(&root)?;

    // Phase 2: publish all shared stores while vault switching is excluded.
    // Slow disk/PDF work above never holds this lock.
    let _publish = state
        .vault_publish_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    if state.current_vault_generation() != generation {
        return Ok(None);
    }
    if state.current_vault_content_revision() != content_revision {
        return Ok(None);
    }
    if state
        .vault_root
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        != Some(&root)
    {
        return Ok(None);
    }
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        // Rebuild the FTS index atomically: a single transaction is far faster
        // than per-row autocommit and prevents a half-rebuilt index if
        // indexing is interrupted partway through.
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM file_search", [])
            .map_err(|e| e.to_string())?;
        for (rel_path, content) in indexed.iter().chain(pdf_indexed.iter()) {
            if let Err(e) = tx.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![rel_path, content],
            ) {
                eprintln!("Failed to index {rel_path} for search: {e}");
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    {
        let mut file_index = state.file_index.lock().map_err(|e| e.to_string())?;
        *file_index = index;
    }
    for (document_id, rel_path, size, modified_at) in pdf_documents {
        if let Err(error) =
            state.save_pdf_document(&document_id, &rel_path, size, modified_at)
        {
            eprintln!("Failed to scope PDF document {rel_path}: {error}");
        }
    }

    Ok(Some(entries))
}

/// Open a file from the vault. Returns raw bytes.
pub fn open_file(state: &AppState, path: &str) -> Result<Vec<u8>, String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;

    if is_image_path(&abs_path) {
        read_image(&abs_path)
    } else {
        let content = read_file(&abs_path)?;
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, &content);
        Ok(content.into_bytes())
    }
}

/// Save file content.
pub fn save_file(state: &AppState, path: &str, content: &str) -> Result<(), String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    write_file(&abs_path, content)?;

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file(&abs_path, content);

    let db = state.db.lock().map_err(|e| e.to_string())?;
    if let Err(e) = db.execute(
        "DELETE FROM file_search WHERE path = ?1",
        rusqlite::params![path],
    ) {
        eprintln!("Failed to clear stale search index for {path}: {e}");
    }
    if let Err(e) = db.execute(
        "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
        rusqlite::params![path, content],
    ) {
        eprintln!("Failed to update search index for {path}: {e}");
    }

    Ok(())
}

/// Reconcile the link/search index with the on-disk state of a single
/// vault-relative markdown path after an *external* change (a filesystem watcher
/// event, a git pull, another editor). Existing files are re-read and
/// re-indexed; vanished files are removed from the index and search. Non-
/// markdown paths are ignored. Safe to call redundantly — it's idempotent.
pub fn sync_path_from_disk(state: &AppState, rel_path: &str) -> Result<(), String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("No vault root set")?.clone()
    };
    let abs = resolve_vault_path_checked(&vault_root, rel_path)?;
    if !is_markdown_path(&abs) {
        return Ok(());
    }

    if abs.is_file() {
        let Ok(content) = read_file(&abs) else {
            return Ok(());
        };
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            index.update_file(&abs, &content);
        }
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![rel_path],
        );
        let _ = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![rel_path, content],
        );
    } else {
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            index.remove_file(&abs);
        }
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![rel_path],
        );
    }
    Ok(())
}

/// Create a new empty file.
pub fn create_file(state: &AppState, path: &str) -> Result<(), String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    if abs_path.exists() {
        return Err(format!("File already exists: {}", abs_path.display()));
    }
    write_file(&abs_path, "")?;

    // Make a newly-created note visible to backlinks and the knowledge graph
    // immediately instead of waiting for the filesystem watcher round-trip.
    if is_markdown_path(&abs_path) {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, "");
        drop(index);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, '')",
            rusqlite::params![path],
        );
    }
    Ok(())
}

/// Create a new directory.
pub fn create_dir(state: &AppState, path: &str) -> Result<(), String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    if abs_path.exists() {
        return Err(format!("Directory already exists: {}", abs_path.display()));
    }
    fs::create_dir_all(&abs_path)
        .map_err(|e| format!("Failed to create directory {}: {}", abs_path.display(), e))
}

/// Rename (or move) a file or directory.
///
/// When a Markdown or PDF file is renamed, every `[[wikilink]]` that pointed at
/// it is rewritten in the files that linked to it, so backlinks survive.
/// PDF annotation links and document identities follow renamed notes/PDFs, and
/// directory moves rebuild the markdown graph/search index under the new path.
pub fn rename_entry(state: &AppState, old_path: &str, new_path: &str) -> Result<(), String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("No vault root set")?.clone()
    };
    let abs_old = resolve_vault_path_checked(&vault_root, old_path)?;
    let abs_new = resolve_vault_path_checked(&vault_root, new_path)?;

    if abs_new.exists() {
        return Err(format!("Target already exists: {}", abs_new.display()));
    }

    let is_dir = abs_old.is_dir();
    let is_md_file = abs_old.is_file() && is_markdown_path(&abs_old);
    let is_pdf_file = abs_old.is_file() && is_pdf_path(&abs_old);
    let old_rel = path_to_relative_string(&abs_old, &vault_root);
    let new_rel = path_to_relative_string(&abs_new, &vault_root);

    if old_rel.is_empty() || new_rel.is_empty() {
        return Err("Cannot rename the vault root".to_string());
    }

    // Snapshot the files that link to this note *before* mutating the index —
    // these are the ones whose `[[wikilinks]]` need rewriting.
    let backlinks: Vec<PathBuf> = if is_md_file || is_pdf_file {
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.get_backlinks(&abs_old)
    } else {
        Vec::new()
    };

    // Do the physical rename first; if it fails, nothing else has changed.
    fs::rename(&abs_old, &abs_new)
        .map_err(|e| format!("Failed to rename {}: {}", abs_old.display(), e))?;

    if is_dir {
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, true, true, true)?;
        rebuild_markdown_index_and_search(state, &vault_root)?;
        return Ok(());
    }

    if !is_md_file && !is_pdf_file {
        return Ok(());
    }

    // Markdown wikilinks conventionally omit their extension. PDF links keep
    // it so the resolver does not reinterpret the target as a Markdown note.
    let relative_target = abs_new.strip_prefix(&vault_root).unwrap_or(&abs_new);
    let new_link_target = if is_md_file {
        relative_target.with_extension("")
    } else {
        relative_target.to_path_buf()
    };
    let new_rel_str = new_link_target.to_string_lossy().replace('\\', "/");

    // Rewrite links in each backlinking file that resolved to the old path.
    for bl in &backlinks {
        // The renamed Markdown note may link to itself. Resolve its old link
        // spelling against the old path, but read/write it at the new path.
        let stored_path = if bl == &abs_old { &abs_new } else { bl };
        let Ok(content) = read_file(stored_path) else {
            continue;
        };
        let Some(updated) =
            crate::file_index::rewrite_links_to(&content, &vault_root, bl, &abs_old, &new_rel_str)
        else {
            continue;
        };
        if write_file(stored_path, &updated).is_err() {
            continue;
        }
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            if stored_path != bl {
                index.remove_file(bl);
            }
            index.update_file(stored_path, &updated);
        }
        let bl_rel = path_to_relative_string(stored_path, &vault_root);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![bl_rel],
        );
        if let Err(e) = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![bl_rel, updated],
        ) {
            eprintln!("Failed to reindex {bl_rel} after rename: {e}");
        }
    }

    if is_pdf_file {
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, false, true, false)?;
        return Ok(());
    }

    // Re-key the renamed file in the link graph and search index.
    {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.remove_file(&abs_old);
        if let Ok(content) = read_file(&abs_new) {
            index.update_file(&abs_new, &content);
            drop(index);
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db.execute(
                "DELETE FROM file_search WHERE path = ?1",
                rusqlite::params![old_rel],
            );
            let _ = db.execute(
                "DELETE FROM file_search WHERE path = ?1",
                rusqlite::params![new_rel],
            );
            if let Err(e) = db.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![new_rel, content],
            ) {
                eprintln!("Failed to index renamed file {new_rel}: {e}");
            }
        }
    }

    rewrite_persisted_rename_paths(state, &old_rel, &new_rel, false, false, true)?;

    Ok(())
}

/// Move durable path references after a successful filesystem rename.
///
/// `subtree` rewrites both an exact match and every slash-separated descendant.
/// The remaining flags select PDF source paths and PDF-annotation note targets;
/// search rows always follow the rename so extracted PDF text is not stranded.
fn rewrite_persisted_rename_paths(
    state: &AppState,
    old_path: &str,
    new_path: &str,
    subtree: bool,
    update_pdf_documents: bool,
    update_linked_notes: bool,
) -> Result<(), String> {
    let scope_prefix = state
        .pdf_scope_prefix()?
        .ok_or("No vault root set")?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;

    rewrite_path_column(
        &tx,
        "file_search",
        "path",
        old_path,
        new_path,
        subtree,
        None,
    )?;
    if update_pdf_documents {
        rewrite_path_column(
            &tx,
            "pdf_documents",
            "vault_relative_path",
            old_path,
            new_path,
            subtree,
            Some(&scope_prefix),
        )?;
    }
    if update_linked_notes {
        rewrite_path_column(
            &tx,
            "pdf_annotations",
            "linked_note_path",
            old_path,
            new_path,
            subtree,
            Some(&scope_prefix),
        )?;
    }

    tx.commit()
        .map_err(|e| format!("Failed to persist renamed paths: {e}"))
}

fn rewrite_path_column(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    old_path: &str,
    new_path: &str,
    subtree: bool,
    document_scope: Option<&str>,
) -> Result<(), String> {
    let (sql, changed) = if let Some(scope) = document_scope.filter(|_| subtree) {
        let prefix = format!("{}/", old_path.trim_end_matches('/'));
        let sql = format!(
            "UPDATE {table}
             SET {column} = ?1 || substr(replace({column}, char(92), '/'), length(?2) + 1)
             WHERE (replace({column}, char(92), '/') = ?2
                    OR substr(replace({column}, char(92), '/'), 1, length(?3)) = ?3)
               AND substr(document_id, 1, length(?4)) = ?4"
        );
        let changed = tx.execute(
            &sql,
            rusqlite::params![new_path, old_path, prefix, scope],
        );
        (sql, changed)
    } else if subtree {
        let prefix = format!("{}/", old_path.trim_end_matches('/'));
        let sql = format!(
            "UPDATE {table}
             SET {column} = ?1 || substr(replace({column}, char(92), '/'), length(?2) + 1)
             WHERE replace({column}, char(92), '/') = ?2
                OR substr(replace({column}, char(92), '/'), 1, length(?3)) = ?3"
        );
        let changed = tx.execute(&sql, rusqlite::params![new_path, old_path, prefix]);
        (sql, changed)
    } else if let Some(scope) = document_scope {
        let sql = format!(
            "UPDATE {table} SET {column} = ?1
             WHERE replace({column}, char(92), '/') = ?2
               AND substr(document_id, 1, length(?3)) = ?3"
        );
        let changed = tx.execute(&sql, rusqlite::params![new_path, old_path, scope]);
        (sql, changed)
    } else {
        let sql = format!(
            "UPDATE {table} SET {column} = ?1
             WHERE replace({column}, char(92), '/') = ?2"
        );
        let changed = tx.execute(&sql, rusqlite::params![new_path, old_path]);
        (sql, changed)
    };
    changed
        .map(|_| ())
        .map_err(|e| format!("Failed to update renamed path with `{sql}`: {e}"))
}

/// Rebuild the markdown graph in two passes after a directory move and replace
/// only markdown FTS rows, preserving already-extracted PDF search content.
fn rebuild_markdown_index_and_search(state: &AppState, root: &Path) -> Result<(), String> {
    let markdown_files = list_all_md_files(root)?;
    let mut graph_files = Vec::with_capacity(markdown_files.len());
    let mut search_rows = Vec::with_capacity(markdown_files.len());
    for path in markdown_files {
        if let Ok(content) = read_file(&path) {
            search_rows.push((path_to_relative_string(&path, root), content.clone()));
            graph_files.push((path, content));
        }
    }

    let mut rebuilt = FileIndex::new(root.to_path_buf());
    rebuilt.rebuild(&graph_files);

    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM file_search
             WHERE lower(path) LIKE '%.md' OR lower(path) LIKE '%.markdown'",
            [],
        )
        .map_err(|e| format!("Failed to clear markdown search index after rename: {e}"))?;
        for (path, content) in search_rows {
            tx.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![path, content],
            )
            .map_err(|e| format!("Failed to rebuild search index for {path}: {e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("Failed to commit renamed search index: {e}"))?;
    }

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    *index = rebuilt;
    Ok(())
}

/// Delete a file or directory.
pub fn delete_entry(state: &AppState, path: &str) -> Result<(), String> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;

    if abs_path.is_file() {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.remove_file(&abs_path);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![path],
        ) {
            eprintln!("Failed to remove {path} from search index: {e}");
        }
        fs::remove_file(&abs_path)
            .map_err(|e| format!("Failed to delete file {}: {}", abs_path.display(), e))
    } else if abs_path.is_dir() {
        let indexed_files = list_all_md_files(&abs_path)?;
        fs::remove_dir_all(&abs_path)
            .map_err(|e| format!("Failed to delete directory {}: {}", abs_path.display(), e))?;
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            for indexed_file in indexed_files {
                index.remove_file(&indexed_file);
            }
        }
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM file_search
             WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
            rusqlite::params![path, prefix],
        )
        .map_err(|e| format!("Failed to remove {path} subtree from search index: {e}"))?;
        Ok(())
    } else {
        Err(format!("Path does not exist: {}", abs_path.display()))
    }
}

/// List all entries in the vault.
pub fn list_vault(state: &AppState) -> Result<Vec<FileEntry>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    list_vault_entries(vault_root)
}

/// Full-text search across the vault using FTS5.
pub fn search_vault(state: &AppState, query: &str) -> Result<crate::types::SearchResults, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let fts_query = query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");

    let mut stmt = db
        .prepare(
            "SELECT path, snippet(file_search, 1, '<b>', '</b>', '...', 15) FROM file_search WHERE content MATCH ?1 ORDER BY rank LIMIT 101",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(rusqlite::params![&fts_query], |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                line: 1,
                context: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        if let Ok(r) = row {
            results.push(r);
        }
    }
    let truncated = results.len() > 100;
    results.truncate(100);
    Ok(crate::types::SearchResults {
        items: results,
        truncated,
    })
}

/// Get backlinks for a file.
pub fn get_backlinks(state: &AppState, path: &str) -> Result<Vec<String>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);

    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let backlinks = index.get_backlinks(&abs_path);

    Ok(backlinks
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Get mixed backlinks (markdown files, PDF documents, and PDF annotations).
pub fn get_mixed_backlinks(state: &AppState, path: &str) -> Result<Vec<BacklinkItem>, String> {
    let (vault_root, pdf_scope_prefix, generation) = {
        let _publish = state
            .vault_publish_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let root = state
            .vault_root
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .ok_or("No vault root set")?
            .clone();
        let scope_prefix = AppState::pdf_scope_prefix_for_root(&root);
        (root, scope_prefix, state.current_vault_generation())
    };

    let mut results = Vec::new();

    if is_pdf_path(Path::new(path)) {
        // PDF Case:
        // 1. Get incoming backlinks from FileIndex (markdown files linking to this PDF)
        let abs_path = resolve_vault_path(&vault_root, path);
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        let backlinks = index.get_backlinks(&abs_path);
        for bl in backlinks {
            let rel_path = path_to_relative_string(&bl, &vault_root);
            let name = bl
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            results.push(BacklinkItem {
                source: BacklinkTarget::MarkdownFile { path: rel_path },
                label: name,
                context: None,
            });
        }

        // 2. Query notes linked from PDF annotations of this PDF document
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare(
                "SELECT document_id FROM pdf_documents
                 WHERE vault_relative_path = ?1
                   AND substr(document_id, 1, length(?2)) = ?2
                 ORDER BY updated_at DESC LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(rusqlite::params![path, &pdf_scope_prefix])
            .map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let doc_id: String = row.get(0).map_err(|e| e.to_string())?;

            let mut stmt2 = db
                .prepare(
                    "SELECT linked_note_path, selected_text FROM pdf_annotations
                     WHERE document_id = ?1 AND linked_note_path IS NOT NULL AND linked_note_path != ''",
                )
                .map_err(|e| e.to_string())?;
            let mut rows2 = stmt2.query([doc_id]).map_err(|e| e.to_string())?;
            while let Some(row2) = rows2.next().map_err(|e| e.to_string())? {
                let note_path: String = row2.get(0).map_err(|e| e.to_string())?;
                let selected_text: String = row2.get(1).map_err(|e| e.to_string())?;

                let note_name = Path::new(&note_path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| note_path.clone());

                results.push(BacklinkItem {
                    source: BacklinkTarget::MarkdownFile { path: note_path },
                    label: note_name,
                    context: Some(selected_text),
                });
            }
        }
    } else {
        // Markdown Case:
        // 1. Standard incoming backlinks from FileIndex
        let abs_path = resolve_vault_path(&vault_root, path);
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        let backlinks = index.get_backlinks(&abs_path);
        for bl in backlinks {
            let rel_path = path_to_relative_string(&bl, &vault_root);
            let name = bl
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            results.push(BacklinkItem {
                source: BacklinkTarget::MarkdownFile { path: rel_path },
                label: name,
                context: None,
            });
        }

        // 2. Query annotations from SQLite referencing this note (linked_note_path)
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare(
                "SELECT a.id, a.page_index, a.selected_text, d.vault_relative_path
                 FROM pdf_annotations a
                 JOIN pdf_documents d ON a.document_id = d.document_id
                 WHERE a.linked_note_path = ?1
                   AND substr(d.document_id, 1, length(?2)) = ?2",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(rusqlite::params![path, &pdf_scope_prefix])
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let ann_id: String = row.get(0).map_err(|e| e.to_string())?;
            let page_idx: i32 = row.get(1).map_err(|e| e.to_string())?;
            let selected_text: String = row.get(2).map_err(|e| e.to_string())?;
            let doc_path: String = row.get(3).map_err(|e| e.to_string())?;

            results.push(BacklinkItem {
                source: BacklinkTarget::PdfAnnotation {
                    document_path: doc_path,
                    annotation_id: ann_id,
                    page: (page_idx + 1) as u16,
                },
                label: format!("Page {} highlight", page_idx + 1),
                context: Some(selected_text),
            });
        }
    }

    let _publish = state
        .vault_publish_lock
        .lock()
        .map_err(|e| e.to_string())?;
    if state.current_vault_generation() != generation
        || state
            .vault_root
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            != Some(&vault_root)
    {
        return Err("Vault changed while reading backlinks".to_string());
    }

    Ok(results)
}

/// Return a deterministic snapshot of the vault's research graph.
///
/// Markdown-to-file edges come from the already-maintained [`FileIndex`]. PDF
/// annotation edges are read from the sidecar database and aggregated by
/// document/note pair. All indexed markdown files and all PDFs on disk are
/// included, even when they have no edges; unresolved endpoints are emitted as
/// [`GraphNodeKind::Missing`] nodes.
///
/// The vault-root and file-index locks are held only long enough to clone their
/// current values. Directory walking and database work happen afterwards, so a
/// graph refresh cannot hold up editor navigation or incremental indexing.
pub fn get_graph_snapshot(state: &AppState) -> Result<GraphSnapshot, String> {
    // Capture the root, vault identity, PDF namespace, and link index under the
    // same short publication/mutation barrier. A concurrent A -> B switch can
    // therefore never mix A's filesystem with B's annotations or FileIndex.
    let (root, pdf_scope_prefix, generation, content_revision, indexed_outgoing) = {
        let _publish = state
            .vault_publish_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let _mutation = state.vault_publish_mutation_lock()?;
        let root = state
            .vault_root
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .ok_or("No vault root set")?
            .clone();
        let pdf_scope_prefix = AppState::pdf_scope_prefix_for_root(&root);
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        let indexed_outgoing: Vec<(PathBuf, Vec<PathBuf>)> = index
            .outgoing
            .iter()
            .map(|(source, targets)| (source.clone(), targets.iter().cloned().collect::<Vec<_>>()))
            .collect();
        (
            root,
            pdf_scope_prefix,
            state.current_vault_generation(),
            state.current_vault_content_revision(),
            indexed_outgoing,
        )
    };

    let markdown_paths: BTreeSet<String> = indexed_outgoing
        .iter()
        .filter_map(|(path, _)| graph_relative_path(path, &root))
        .collect();

    // PDFs are not source keys in FileIndex, so explicitly include all of them
    // to preserve unconnected reference material in the global graph.
    let pdf_paths: BTreeSet<String> = list_all_pdf_files(&root)?
        .into_iter()
        .filter_map(|path| graph_relative_path(&path, &root))
        .collect();

    // BTreeMap both aggregates duplicate relationships and establishes the
    // snapshot's deterministic source/target/kind order.
    let mut edge_weights: BTreeMap<(String, String, GraphEdgeKind), usize> = BTreeMap::new();

    for (source, targets) in indexed_outgoing {
        let Some(source) = graph_relative_path(&source, &root) else {
            continue;
        };
        for target in targets {
            let Some(target) = graph_relative_path(&target, &root) else {
                continue;
            };
            // Images can be valid wikilink targets, but this graph deliberately
            // models knowledge documents only: markdown, PDFs, and missing
            // document targets.
            if is_image_path(Path::new(&target)) {
                continue;
            }
            let weight = edge_weights
                .entry((source.clone(), target, GraphEdgeKind::WikiLink))
                .or_default();
            *weight = weight.saturating_add(1);
        }
    }

    // Query grouped rows so the SQLite lock is held for only a compact indexed
    // read. A second aggregation below also merges legacy slash/path variants
    // once they are normalized to the same vault-relative identifiers.
    let annotation_edges: Vec<(String, String, usize)> = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare(
                "SELECT d.vault_relative_path, a.linked_note_path, COUNT(*)
                 FROM pdf_annotations a
                 JOIN pdf_documents d ON a.document_id = d.document_id
                 WHERE a.linked_note_path IS NOT NULL
                   AND TRIM(a.linked_note_path) != ''
                   AND substr(d.document_id, 1, length(?1)) = ?1
                 GROUP BY d.vault_relative_path, a.linked_note_path",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&pdf_scope_prefix], |row| {
                let count: i64 = row.get(2)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    usize::try_from(count).unwrap_or(usize::MAX),
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    for (source, target, count) in annotation_edges {
        let Some(source) = normalize_stored_graph_path(&root, &source) else {
            continue;
        };
        // Ignore deleted sources. Document IDs are already vault-scoped, and
        // same-path reconciliation intentionally keeps annotation links valid
        // when a PDF is touched or replaced.
        if !pdf_paths.contains(&source) {
            continue;
        }
        let Some(target) = normalize_stored_graph_path(&root, &target) else {
            continue;
        };
        if is_image_path(Path::new(&source)) || is_image_path(Path::new(&target)) {
            continue;
        }
        let weight = edge_weights
            .entry((source, target, GraphEdgeKind::PdfAnnotation))
            .or_default();
        *weight = weight.saturating_add(count);
    }

    let mut node_kinds: BTreeMap<String, GraphNodeKind> = BTreeMap::new();
    for path in &markdown_paths {
        node_kinds.insert(path.clone(), GraphNodeKind::Markdown);
    }
    for path in &pdf_paths {
        node_kinds.insert(path.clone(), GraphNodeKind::Pdf);
    }
    for (source, target, _) in edge_weights.keys() {
        for path in [source, target] {
            node_kinds
                .entry(path.clone())
                .or_insert_with(|| graph_node_kind(path, &markdown_paths, &pdf_paths));
        }
    }

    let mut incoming: BTreeMap<String, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<String, usize> = BTreeMap::new();
    let edges = edge_weights
        .into_iter()
        .map(|((source, target, kind), weight)| {
            let source_degree = outgoing.entry(source.clone()).or_default();
            *source_degree = source_degree.saturating_add(weight);
            let target_degree = incoming.entry(target.clone()).or_default();
            *target_degree = target_degree.saturating_add(weight);
            GraphEdge {
                source,
                target,
                kind,
                weight,
            }
        })
        .collect();

    let nodes = node_kinds
        .into_iter()
        .map(|(path, kind)| GraphNode {
            label: graph_node_label(&path),
            exists: kind != GraphNodeKind::Missing,
            incoming: incoming.get(&path).copied().unwrap_or_default(),
            outgoing: outgoing.get(&path).copied().unwrap_or_default(),
            path,
            kind,
        })
        .collect();

    // Filesystem/database work above deliberately ran without shared vault
    // locks. Validate the captured identity under the same barrier before
    // exposing the snapshot; callers can retry if anything changed meanwhile.
    let _publish = state
        .vault_publish_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    if state.current_vault_generation() != generation
        || state.current_vault_content_revision() != content_revision
        || state
            .vault_root
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            != Some(&root)
    {
        return Err("Vault changed while building graph snapshot".to_string());
    }

    Ok(GraphSnapshot { nodes, edges })
}

/// Read raw image bytes from the vault.
pub fn read_vault_image(state: &AppState, path: &str) -> Result<Vec<u8>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    read_image(&abs_path)
}

// ── Internal helpers ────────────────────────────────────────────────

/// Resolve a vault-relative path to an absolute path, guaranteed to stay
/// within `vault_root`.
///
/// The relative path is normalized lexically: `.` is dropped, `..` pops a
/// component, and any attempt to escape the root (leading `..` or an absolute
/// path) is clamped so the result can never point outside the vault. Use
/// [`resolve_vault_path_checked`] when an escape attempt should be a hard
/// error rather than silently clamped.
pub fn resolve_vault_path(vault_root: &Path, relative_path: &str) -> PathBuf {
    normalize_within_root(vault_root, relative_path).0
}

/// Like [`resolve_vault_path`] but returns an error if `relative_path` tries to
/// escape the vault root. Use for filesystem mutations (save/create/delete/
/// rename) and reads where operating on the wrong file would be harmful.
pub fn resolve_vault_path_checked(
    vault_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let (resolved, escaped) = normalize_within_root(vault_root, relative_path);
    if escaped {
        return Err(format!("Path escapes the vault root: {relative_path}"));
    }
    let canonical_root = fs::canonicalize(vault_root)
        .map_err(|e| format!("Failed to resolve vault root {}: {e}", vault_root.display()))?;
    let mut existing = resolved.as_path();
    while fs::symlink_metadata(existing).is_err() {
        existing = existing.parent().ok_or_else(|| {
            format!("Could not resolve an existing ancestor for {}", resolved.display())
        })?;
    }
    let canonical_existing = fs::canonicalize(existing).map_err(|e| {
        format!(
            "Failed to resolve path inside vault {}: {e}",
            existing.display()
        )
    })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(format!("Path resolves outside the vault root: {relative_path}"));
    }
    Ok(resolved)
}

/// Lexically normalize `relative_path` against `vault_root`. Returns the
/// clamped absolute path and whether the input attempted to escape the root.
fn normalize_within_root(vault_root: &Path, relative_path: &str) -> (PathBuf, bool) {
    use std::path::Component;

    let mut stack: Vec<std::ffi::OsString> = Vec::new();
    let mut escaped = false;

    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(c) => stack.push(c.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    escaped = true;
                }
            }
            // An absolute path supplied where a relative one was expected is
            // an escape attempt; ignore the anchor and keep building under root.
            Component::RootDir | Component::Prefix(_) => escaped = true,
        }
    }

    let mut resolved = vault_root.to_path_buf();
    for c in stack {
        resolved.push(c);
    }
    (resolved, escaped)
}

/// File size in bytes and modified time as a Unix timestamp (seconds), used as
/// the PDF text cache key. Returns `(0, 0)` when the file can't be stat'd, which
/// simply forces a fresh extraction.
fn file_size_and_mtime(path: &Path) -> (u64, i64) {
    let Ok(meta) = fs::metadata(path) else {
        return (0, 0);
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (meta.len(), mtime)
}

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file {}: {}", path.display(), e))
}

fn read_image(path: &Path) -> Result<Vec<u8>, String> {
    if !is_image_path(path) {
        return Err(format!("Not an image: {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("Failed to read image {}: {}", path.display(), e))
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    fs::write(path, content).map_err(|e| format!("Failed to write file {}: {}", path.display(), e))
}

pub fn is_image(ext: &str) -> bool {
    IMAGE_EXTENSIONS
        .iter()
        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
}

pub fn ext_matches(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

pub fn is_markdown_path(path: &Path) -> bool {
    ext_matches(path, &MARKDOWN_EXTENSIONS)
}

pub fn is_pdf_path(path: &Path) -> bool {
    ext_matches(path, &PDF_EXTENSIONS)
}

pub fn is_image_path(path: &Path) -> bool {
    ext_matches(path, &IMAGE_EXTENSIONS)
}

pub fn is_supported_vault_path(path: &Path) -> bool {
    is_markdown_path(path) || is_pdf_path(path) || is_image_path(path)
}

fn list_vault_entries(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    list_vault_recursive(root, root, &mut entries, 0)?;
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

fn list_vault_recursive(
    root: &Path,
    dir: &Path,
    entries: &mut Vec<FileEntry>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_WALK_DEPTH {
        return Ok(());
    }
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            // Skip excluded/dot directories and never follow directory
            // symlinks (which could form a cycle).
            if is_excluded_dir(&name) || is_symlink(&path) {
                continue;
            }
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: true,
            });
            list_vault_recursive(root, &path, entries, depth + 1)?;
        } else if name.starts_with('.') || is_symlink(&path) {
            continue;
        } else if is_supported_vault_path(&path) {
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: false,
            });
        }
    }

    Ok(())
}

fn path_to_relative_string(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Convert a FileIndex/disk path to the stable slash-separated graph id.
/// Paths outside the active vault are ignored rather than leaking an absolute
/// filesystem path into the snapshot.
fn graph_relative_path(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Normalize paths persisted in SQLite. Current records are vault-relative,
/// but accepting absolute in-vault paths and legacy backslashes makes graph
/// snapshots resilient to older sidecars and cross-platform vault moves.
fn normalize_stored_graph_path(root: &Path, stored: &str) -> Option<String> {
    let stored = stored.trim().replace('\\', "/");
    if stored.is_empty() {
        return None;
    }
    let path = Path::new(&stored);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let (resolved, escaped) = normalize_within_root(root, &stored);
        if escaped {
            return None;
        }
        resolved
    };
    graph_relative_path(&absolute, root)
}

fn graph_node_kind(
    path: &str,
    markdown_paths: &BTreeSet<String>,
    pdf_paths: &BTreeSet<String>,
) -> GraphNodeKind {
    if markdown_paths.contains(path) {
        GraphNodeKind::Markdown
    } else if pdf_paths.contains(path) {
        GraphNodeKind::Pdf
    } else {
        GraphNodeKind::Missing
    }
}

fn graph_node_label(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .or_else(|| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| path.to_string())
}

pub fn list_all_md_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_all_md_files_recursive(root, &mut files, 0)?;
    Ok(files)
}

fn list_all_md_files_recursive(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    list_files_matching(dir, files, depth, &is_markdown_path)
}

/// List every `.pdf` file in the vault, applying the same exclusion/symlink/
/// depth guards as the markdown walker.
pub fn list_all_pdf_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_files_matching(root, &mut files, 0, &is_pdf_path)?;
    Ok(files)
}

/// Recursively collect files whose extension satisfies `keep`, skipping
/// dotfiles, excluded/dot directories, and directory symlinks, bounded by
/// [`MAX_WALK_DEPTH`].
fn list_files_matching(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
    keep: &dyn Fn(&Path) -> bool,
) -> Result<(), String> {
    if depth >= MAX_WALK_DEPTH {
        return Ok(());
    }
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if is_excluded_dir(&name) || is_symlink(&path) {
                continue;
            }
            list_files_matching(&path, files, depth + 1, keep)?;
        } else if name.starts_with('.') || is_symlink(&path) {
            continue;
        } else if keep(&path) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
    fn resolve_normal_paths_stay_within_root() {
        let root = Path::new("/vault");
        assert_eq!(resolve_vault_path(root, "notes/a.md"), PathBuf::from("/vault/notes/a.md"));
        assert_eq!(resolve_vault_path(root, "./a.md"), PathBuf::from("/vault/a.md"));
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
        assert!(error.contains("Failed to write file"));

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
        assert!(snapshot.edges.windows(2).all(|pair| (
            &pair[0].source,
            &pair[0].target,
            pair[0].kind
        ) <= (
            &pair[1].source,
            &pair[1].target,
            pair[1].kind
        )));
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
        assert!(second_snapshot.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::PdfAnnotation && edge.target == "second.md"
        }));
        assert!(
            second_snapshot
                .edges
                .iter()
                .all(|edge| edge.target != "first.md")
        );

        set_vault_root(&state, first.to_str().unwrap()).unwrap();
        let first_snapshot = get_graph_snapshot(&state).unwrap();
        assert!(first_snapshot.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::PdfAnnotation && edge.target == "first.md"
        }));
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
        assert!(touched_snapshot.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::PdfAnnotation && edge.target == "first.md"
        }));

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
        assert!(search_vault(&state, "first-only-token").unwrap().items.is_empty());
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

        assert_eq!(fs::read_to_string(base.join("source.md")).unwrap(), "[[renamed]]");
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
}
