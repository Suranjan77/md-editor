//! Renaming files and directories while preserving wikilinks, search rows, and PDF sidecars.

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

    let backlinks: Vec<PathBuf> = if is_md_file || is_pdf_file {
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.get_backlinks(&abs_old)
    } else {
        Vec::new()
    };

    fs::rename(&abs_old, &abs_new)
        .map_err(|e| VaultError::io(format!("Failed to rename {}", abs_old.display()), e))?;

    if is_dir {
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, true, true, true)?;
        rebuild_markdown_index_and_search(state, &vault_root)?;
        return Ok(());
    }

    if !is_md_file && !is_pdf_file {
        return Ok(());
    }

    let relative_target = abs_new.strip_prefix(&vault_root).unwrap_or(&abs_new);
    let new_link_target = if is_md_file {
        relative_target.with_extension("")
    } else {
        relative_target.to_path_buf()
    };
    let new_rel_str = new_link_target.to_string_lossy().replace('\\', "/");

    apply_backlink_rewrites(
        state,
        &vault_root,
        &backlinks,
        &abs_old,
        &abs_new,
        &new_rel_str,
    )?;

    if is_pdf_file {
        rewrite_persisted_rename_paths(state, &old_rel, &new_rel, false, true, false)?;
        return Ok(());
    }

    rekey_renamed_markdown(state, &abs_old, &abs_new, &old_rel, &new_rel)?;
    rewrite_persisted_rename_paths(state, &old_rel, &new_rel, false, false, true)?;
    Ok(())
}

fn apply_backlink_rewrites(
    state: &AppState,
    vault_root: &Path,
    backlinks: &[PathBuf],
    abs_old: &Path,
    abs_new: &Path,
    new_rel_str: &str,
) -> VaultResult<()> {
    let rewrites: Vec<(PathBuf, PathBuf, String)> = {
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        backlinks
            .iter()
            .filter_map(|bl| {
                let stored_path = if bl == abs_old { abs_new } else { bl.as_path() };
                let content = read_file(stored_path).ok()?;
                let updated = index.rewrite_links_to(&content, bl, abs_old, new_rel_str)?;
                Some((bl.clone(), stored_path.to_path_buf(), updated))
            })
            .collect()
    };
    for (bl, stored_path, updated) in &rewrites {
        if write_file(stored_path, updated).is_err() {
            continue;
        }
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            if stored_path != bl {
                index.remove_file(bl);
            }
            index.update_file(stored_path, updated);
        }
        let bl_rel = path_to_relative_string(stored_path, vault_root);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![bl_rel],
        );
        if let Err(e) = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![bl_rel, updated],
        ) {
            log::warn!("Failed to reindex {bl_rel} after rename: {e}");
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
        .map_err(|e| VaultError::from(format!("Failed to persist renamed paths: {e}")))
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
