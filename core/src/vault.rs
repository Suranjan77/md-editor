use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;

mod files;
mod graph;

use crate::types::{
    BacklinkItem, BacklinkTarget, FileEntry, GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind,
    GraphSnapshot, SearchResult,
};
pub use files::*;
pub use graph::*;

pub const IMAGE_EXTENSIONS: [&str; 8] = ["jpeg", "jpg", "png", "gif", "bmp", "svg", "webp", "avif"];
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];
const PDF_EXTENSIONS: [&str; 1] = ["pdf"];
const PPTX_EXTENSIONS: [&str; 1] = ["pptx"];

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
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
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
            .stored_pdf_document_id_for_vault(&root, &rel_path, file_size, Some(modified_at))
            .map(|document_id| (document_id, file_size, Some(modified_at)))
            .or_else(|| crate::pdf::compute_provisional_id(&pdf_path).ok());
        if let Some((document_id, size, modified)) = identity {
            pdf_documents.push((document_id, rel_path.clone(), size, modified));
        }

        if let Some(renderer) = state.pdf_renderer.as_ref() {
            // Reuse cached text when the PDF is unchanged; only fall back to
            // the (slow) pdfium extraction when size/mtime differ.
            let text = if let Some(cached) =
                state.get_cached_pdf_text_for_vault(&root, &rel_path, file_size, modified_at)
            {
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
    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    if state.current_vault_generation() != generation {
        return Ok(None);
    }
    if state.current_vault_content_revision() != content_revision {
        return Ok(None);
    }
    if state.vault_root.lock().map_err(|e| e.to_string())?.as_ref() != Some(&root) {
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
        if let Err(error) = state.save_pdf_document(&document_id, &rel_path, size, modified_at) {
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
    let scope_prefix = state.pdf_scope_prefix()?.ok_or("No vault root set")?;
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
        let changed = tx.execute(&sql, rusqlite::params![new_path, old_path, prefix, scope]);
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
        match row {
            Ok(r) => results.push(r),
            Err(e) => eprintln!("Skipping malformed file_search row: {e}"),
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
        let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
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

    // A note that both wikilinks a PDF and is linked from one of its
    // annotations shows up twice; drop the plain file-index entry and keep the
    // annotation-derived one (it carries the highlighted text as context).
    let annotation_note_paths: std::collections::HashSet<String> = results
        .iter()
        .filter(|item| item.context.is_some())
        .filter_map(|item| match &item.source {
            BacklinkTarget::MarkdownFile { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    results.retain(|item| {
        item.context.is_some()
            || match &item.source {
                BacklinkTarget::MarkdownFile { path } => !annotation_note_paths.contains(path),
                _ => true,
            }
    });
    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    if state.current_vault_generation() != generation
        || state.vault_root.lock().map_err(|e| e.to_string())?.as_ref() != Some(&vault_root)
    {
        return Err("Vault changed while reading backlinks".to_string());
    }

    Ok(results)
}

/// Read raw image bytes from the vault.
pub fn read_vault_image(state: &AppState, path: &str) -> Result<Vec<u8>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    read_image(&abs_path)
}

#[cfg(test)]
mod tests;
