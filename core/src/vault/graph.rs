//! The knowledge-graph snapshot: nodes for notes, PDFs and missing targets, edges from wikilinks and PDF annotations.

use super::*;

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
        let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
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
    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    if state.current_vault_generation() != generation
        || state.current_vault_content_revision() != content_revision
        || state.vault_root.lock().map_err(|e| e.to_string())?.as_ref() != Some(&root)
    {
        return Err("Vault changed while building graph snapshot".to_string());
    }

    Ok(GraphSnapshot { nodes, edges })
}
