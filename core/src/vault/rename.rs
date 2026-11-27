//! Renaming files and directories while preserving wikilinks, search rows, and PDF sidecars.

use super::relink;
use super::*;

/// Rename (or move) a file or directory, rewriting incoming wikilinks and
/// persisted PDF/search paths so backlinks and annotations survive.
pub fn rename_entry(state: &AppState, old_path: &str, new_path: &str) -> VaultResult<()> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or(VaultError::NoVault)?.clone()
    };
    let abs_old = resolve_vault_path_checked(&vault_root, old_path)?;
    let abs_new = resolve_vault_path_checked(&vault_root, new_path)?;

    if abs_new.exists() {
        return Err(VaultError::AlreadyExists(abs_new));
    }

    let is_dir = abs_old.is_dir();
    let is_md_file = abs_old.is_file() && is_markdown_path(&abs_old);
    let is_pdf_file = abs_old.is_file() && is_pdf_path(&abs_old);
    let old_rel = path_to_relative_string(&abs_old, &vault_root);
    let new_rel = path_to_relative_string(&abs_new, &vault_root);

    if old_rel.is_empty() || new_rel.is_empty() {
        return Err(VaultError::from("Cannot rename the vault root"));
    }

    // Plan link rewrites against the vault as it is now, before the move.
    let relinks = plan_relinks(state, &vault_root, &old_rel, &new_rel)?;

    fs::rename(&abs_old, &abs_new)
        .map_err(|e| VaultError::io(format!("Failed to rename {}", abs_old.display()), e))?;

    let rewritten = write_relinks(relinks);

    if is_dir {
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, true, true, true)?;
        rebuild_markdown_index_and_search(state, &vault_root)?;
        return Ok(());
    }

    reindex_rewritten(state, &vault_root, &rewritten, &abs_new)?;

    if is_pdf_file {
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, false, true, false)?;
    } else if is_md_file {
        rekey_renamed_markdown(state, &abs_old, &abs_new, &old_rel, &new_rel)?;
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, false, false, true)?;
    }
    Ok(())
}

/// A note whose links a rename breaks: where it is now, where it will be, and
/// its content with those links fixed.
struct Relink {
    new_abs: PathBuf,
    content: String,
}

/// Every note with a link that moving `old_rel` to `new_rel` would break —
/// wikilinks and Markdown links or images to the moved file or anything under
/// the moved folder, and relative links out of notes that move — with the
/// fix. Read before the move, so links resolve as the user wrote them.
fn plan_relinks(
    state: &AppState,
    root: &Path,
    old_rel: &str,
    new_rel: &str,
) -> VaultResult<Vec<Relink>> {
    let mv = relink::Move {
        old: old_rel,
        new: new_rel,
    };
    // A note can only link to the moved entry if it names it somewhere.
    let name = Path::new(old_rel)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name_encoded = name.replace(' ', "%20");
    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let exists = |rel: &str| root.join(rel).exists();
    let resolve = |target: &str, note: &str| {
        index
            .resolve_link(target, &root.join(note))
            .map(|abs| path_to_relative_string(&abs, root))
    };
    let old = relink::OldVault {
        exists: &exists,
        resolve_wikilink: &resolve,
    };
    let mut plan = Vec::new();
    for abs in list_all_md_files(root)? {
        let rel = path_to_relative_string(&abs, root);
        let moved = mv.map(&rel);
        let Ok(content) = read_file(&abs) else {
            continue;
        };
        if moved.is_none() {
            let lower = content.to_lowercase();
            if !lower.contains(&name) && !lower.contains(&name_encoded) {
                continue;
            }
        }
        let note_new = moved.unwrap_or_else(|| rel.clone());
        if let Some(content) = relink::relink_note(&content, &rel, &note_new, mv, &old) {
            plan.push(Relink {
                new_abs: root.join(&note_new),
                content,
            });
        }
    }
    Ok(plan)
}

/// Write the planned notes (now at their new paths). A note that cannot be
/// written (read-only, say) keeps its old links; the rename still stands.
fn write_relinks(plan: Vec<Relink>) -> Vec<Relink> {
    plan.into_iter()
        .filter(
            |relink| match write_file(&relink.new_abs, &relink.content) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!(
                        "Renamed, but could not update links in {}: {e}",
                        relink.new_abs.display()
                    );
                    false
                }
            },
        )
        .collect()
}

/// Refresh the index and search rows of rewritten notes. The renamed note
/// itself is re-keyed separately.
fn reindex_rewritten(
    state: &AppState,
    vault_root: &Path,
    rewritten: &[Relink],
    renamed: &Path,
) -> VaultResult<()> {
    for relink in rewritten.iter().filter(|r| r.new_abs != renamed) {
        state
            .file_index
            .lock()
            .map_err(|e| e.to_string())?
            .update_file(&relink.new_abs, &relink.content);
        let rel = path_to_relative_string(&relink.new_abs, vault_root);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![rel],
        );
        if let Err(e) = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![rel, relink.content],
        ) {
            log::warn!("Failed to reindex {rel} after rename: {e}");
        }
    }
    Ok(())
}

fn rekey_renamed_markdown(
    state: &AppState,
    abs_old: &Path,
    abs_new: &Path,
    old_rel: &str,
    new_rel: &str,
) -> VaultResult<()> {
    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.remove_file(abs_old);
    if let Ok(content) = read_file(abs_new) {
        index.update_file(abs_new, &content);
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
            log::warn!("Failed to index renamed file {new_rel}: {e}");
        }
    }
    Ok(())
}

fn rewrite_persisted_rename_paths(
    state: &AppState,
    old_path: &str,
    new_path: &str,
    subtree: bool,
    update_pdf_documents: bool,
    update_linked_notes: bool,
) -> VaultResult<()> {
    let scope_prefix = state.pdf_scope_prefix()?.ok_or(VaultError::NoVault)?;
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
        .map_err(|e| VaultError::from(format!("Failed to persist renamed paths: {e}")))?;
    drop(db);
    if update_linked_notes {
        // Highlights linking the renamed notes now name the new paths.
        state.export_all_pdf_sidecars();
    }
    Ok(())
}

fn rewrite_path_column(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    old_path: &str,
    new_path: &str,
    subtree: bool,
    document_scope: Option<&str>,
) -> VaultResult<()> {
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
        .map_err(|e| VaultError::from(format!("Failed to update renamed path with `{sql}`: {e}")))
}

fn rebuild_markdown_index_and_search(state: &AppState, root: &Path) -> VaultResult<()> {
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
        .map_err(|e| {
            VaultError::from(format!(
                "Failed to clear markdown search index after rename: {e}"
            ))
        })?;
        for (path, content) in search_rows {
            tx.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![path, content],
            )
            .map_err(|e| {
                VaultError::from(format!("Failed to rebuild search index for {path}: {e}"))
            })?;
        }
        tx.commit()
            .map_err(|e| VaultError::from(format!("Failed to commit renamed search index: {e}")))?;
    }

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    *index = rebuilt;
    Ok(())
}
