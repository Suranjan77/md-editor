//! Indexing a vault on open: one walk, then only the notes and PDFs whose size or modification time changed are read and re-indexed.

use std::collections::HashMap;

use super::*;

/// What the index knew about a file last time: size, modification time (ns)
/// and, for a note, its wikilink targets as written.
struct Known {
    size: i64,
    modified_ns: i64,
    links: Vec<String>,
}

/// A file read this time, whose search row and record are (re)written.
struct Changed {
    rel_path: String,
    /// `None` for a PDF with no extractable text: no search row, but a record.
    content: Option<String>,
    size: i64,
    modified_ns: i64,
    links: Vec<String>,
}

/// Build and publish an index only if `generation` is still the requested vault.
///
/// Files whose size and modification time match `vault_file_meta` are not
/// read: their search rows are already right and their links are recorded.
/// A different vault than the one `file_search` holds is indexed in full.
pub fn index_vault_for_generation(
    state: &AppState,
    path: &str,
    generation: u64,
) -> VaultResult<Option<Vec<FileEntry>>> {
    let started = std::time::Instant::now();
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(VaultError::from(format!("Not a directory: {}", path)));
    }
    let content_revision = state.current_vault_content_revision();
    let scope = AppState::pdf_scope_prefix_for_root(&root);

    // One walk serves the tree, the notes and the PDFs.
    let entries = list_vault_entries(&root)?;
    let (known, fresh) = load_known(state, &scope)?;

    let mut changed: Vec<Changed> = Vec::new();
    let mut on_disk: Vec<String> = Vec::new();
    let mut for_graph: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for entry in entries.iter().filter(|e| !e.is_dir) {
        let abs = root.join(&entry.path);
        if !is_markdown_path(&abs) {
            continue;
        }
        let (size, modified_ns) = size_and_mtime_ns(&abs);
        let unchanged = known
            .get(&entry.path)
            .filter(|k| !fresh && k.size == size && k.modified_ns == modified_ns);
        let links = match unchanged {
            Some(k) => k.links.clone(),
            None => {
                let Ok(content) = read_file(&abs) else {
                    continue;
                };
                let links = crate::file_index::raw_targets(&content);
                changed.push(Changed {
                    rel_path: entry.path.clone(),
                    content: Some(content),
                    size,
                    modified_ns,
                    links: links.clone(),
                });
                links
            }
        };
        on_disk.push(entry.path.clone());
        for_graph.push((abs, links));
    }
    let notes = for_graph.len();
    let mut index = FileIndex::new(root.clone());
    index.rebuild_from_links(for_graph);

    let pdfs = collect_vault_pdfs(state, &root, &entries, &known, fresh, &mut changed);
    on_disk.extend(pdfs.iter().map(|(_, rel, _, _)| rel.clone()));

    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    if state.current_vault_generation() != generation
        || state.current_vault_content_revision() != content_revision
        || state.vault_root.lock().map_err(|e| e.to_string())?.as_ref() != Some(&root)
    {
        return Ok(None);
    }
    let rewritten = changed.len();
    publish_search(state, &scope, fresh, &known, &on_disk, changed)?;
    {
        let mut file_index = state.file_index.lock().map_err(|e| e.to_string())?;
        *file_index = index;
    }
    let pdf_count = pdfs.len();
    for (document_id, rel_path, size, modified_at) in pdfs {
        if let Err(error) = state.save_pdf_document(&document_id, &rel_path, size, modified_at) {
            log::warn!("Failed to scope PDF document {rel_path}: {error}");
        }
    }

    log::info!(
        "Indexed vault {path}: {notes} notes, {pdf_count} PDFs, {rewritten} (re)read, in {:?}",
        started.elapsed()
    );
    Ok(Some(entries))
}

/// The records for `scope`, and whether the index must be rebuilt in full
/// (none yet, or `file_search` holds another vault's rows).
fn load_known(state: &AppState, scope: &str) -> VaultResult<(HashMap<String, Known>, bool)> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let other_vault: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM vault_file_meta WHERE scope != ?1)",
            rusqlite::params![scope],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT path, size, modified_ns, links_json FROM vault_file_meta WHERE scope = ?1")
        .map_err(|e| e.to_string())?;
    let known: HashMap<String, Known> = stmt
        .query_map(rusqlite::params![scope], |row| {
            let links_json: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                Known {
                    size: row.get(1)?,
                    modified_ns: row.get(2)?,
                    links: serde_json::from_str(&links_json).unwrap_or_default(),
                },
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    let fresh = other_vault || known.is_empty();
    Ok((known, fresh))
}

/// Bring `file_search` and the records in line with what is on disk now.
fn publish_search(
    state: &AppState,
    scope: &str,
    fresh: bool,
    known: &HashMap<String, Known>,
    on_disk: &[String],
    changed: Vec<Changed>,
) -> VaultResult<()> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let run = |sql: &str, params: &[&dyn rusqlite::ToSql]| {
        tx.execute(sql, params)
            .map_err(|e| VaultError::from(e.to_string()))
    };
    if fresh {
        run("DELETE FROM file_search", &[])?;
        run("DELETE FROM vault_file_meta", &[])?;
    } else {
        let present: std::collections::HashSet<&str> = on_disk.iter().map(String::as_str).collect();
        for gone in known.keys().filter(|path| !present.contains(path.as_str())) {
            run("DELETE FROM file_search WHERE path = ?1", &[gone])?;
            run(
                "DELETE FROM vault_file_meta WHERE scope = ?1 AND path = ?2",
                &[&scope, gone],
            )?;
        }
    }
    for file in changed {
        run("DELETE FROM file_search WHERE path = ?1", &[&file.rel_path])?;
        if let Some(content) = &file.content
            && let Err(e) = tx.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![file.rel_path, content],
            )
        {
            log::warn!("Failed to index {} for search: {e}", file.rel_path);
            continue;
        }
        let links_json = serde_json::to_string(&file.links).unwrap_or_else(|_| "[]".into());
        run(
            "INSERT OR REPLACE INTO vault_file_meta (scope, path, size, modified_ns, links_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                &scope,
                &file.rel_path,
                &file.size,
                &file.modified_ns,
                &links_json,
            ],
        )?;
    }
    tx.commit().map_err(|e| VaultError::from(e.to_string()))
}

/// Every PDF's identity, for scoping its annotations; and, for each PDF that
/// is new or changed, its text to index (pushed to `changed`). A PDF whose
/// text could not be read gets no record, so it is tried again next time.
fn collect_vault_pdfs(
    state: &AppState,
    root: &Path,
    entries: &[FileEntry],
    known: &HashMap<String, Known>,
    fresh: bool,
    changed: &mut Vec<Changed>,
) -> Vec<(String, String, u64, Option<i64>)> {
    let mut pdf_documents = Vec::new();
    for entry in entries.iter().filter(|e| !e.is_dir) {
        let pdf_path = root.join(&entry.path);
        if !is_pdf_path(&pdf_path) {
            continue;
        }
        let rel_path = entry.path.clone();
        let (file_size, modified_at) = file_size_and_mtime(&pdf_path);
        let identity = state
            .stored_pdf_document_id_for_vault(root, &rel_path, file_size, Some(modified_at))
            .map(|document_id| (document_id, file_size, Some(modified_at)))
            .or_else(|| crate::pdf::compute_provisional_id(&pdf_path).ok());
        if let Some((document_id, size, modified)) = identity {
            pdf_documents.push((document_id, rel_path.clone(), size, modified));
        }

        let (size, modified_ns) = size_and_mtime_ns(&pdf_path);
        if known
            .get(&rel_path)
            .is_some_and(|k| !fresh && k.size == size && k.modified_ns == modified_ns)
        {
            continue;
        }
        let Some(renderer) = state.pdf_renderer.as_ref() else {
            continue;
        };
        let text =
            match state.get_cached_pdf_text_for_vault(root, &rel_path, file_size, modified_at) {
                Some(cached) => cached,
                None => match renderer.extract_document_text(&pdf_path.to_string_lossy()) {
                    Ok(text) => {
                        state.put_cached_pdf_text_for_vault(
                            root,
                            &rel_path,
                            file_size,
                            modified_at,
                            &text,
                        );
                        text
                    }
                    Err(_) => continue,
                },
            };
        changed.push(Changed {
            rel_path,
            content: (!text.trim().is_empty()).then_some(text),
            size,
            modified_ns,
            links: Vec::new(),
        });
    }
    pdf_documents
}

/// Size and modification time in nanoseconds (0 when unknown, which never
/// matches a real record and so forces a read).
fn size_and_mtime_ns(path: &Path) -> (i64, i64) {
    let Ok(meta) = fs::metadata(path) else {
        return (0, 0);
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64);
    (meta.len() as i64, modified)
}
