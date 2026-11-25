//! The knowledge-graph snapshot: nodes for notes, PDFs and missing targets, edges from wikilinks and PDF annotations.

use super::*;
use super::{VaultError, VaultResult};

struct CapturedGraphInputs {
    root: PathBuf,
    pdf_scope_prefix: String,
    generation: u64,
    content_revision: u64,
    indexed_outgoing: Vec<(PathBuf, Vec<PathBuf>)>,
}

/// Return a deterministic snapshot of the vault's research graph.
pub fn get_graph_snapshot(state: &AppState) -> VaultResult<GraphSnapshot> {
    let inputs = capture_graph_inputs(state)?;

    let markdown_paths: BTreeSet<String> = inputs
        .indexed_outgoing
        .iter()
        .filter_map(|(path, _)| graph_relative_path(path, &inputs.root))
        .collect();

    let pdf_paths: BTreeSet<String> = list_all_pdf_files(&inputs.root)?
        .into_iter()
        .filter_map(|path| graph_relative_path(&path, &inputs.root))
        .collect();

    let mut edge_weights: BTreeMap<(String, String, GraphEdgeKind), usize> = BTreeMap::new();
    collect_wikilink_edges(&inputs.root, inputs.indexed_outgoing, &mut edge_weights);

    let annotation_edges = query_annotation_edges(state, &inputs.pdf_scope_prefix)?;
    collect_annotation_edges(
        &inputs.root,
        &pdf_paths,
        annotation_edges,
        &mut edge_weights,
    );

    let (nodes, edges) = assemble_graph_snapshot(&markdown_paths, &pdf_paths, edge_weights);

    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    if state.current_vault_generation() != inputs.generation
        || state.current_vault_content_revision() != inputs.content_revision
        || state.vault_root.lock().map_err(|e| e.to_string())?.as_ref() != Some(&inputs.root)
    {
        return Err(VaultError::from(
            "Vault changed while building graph snapshot",
        ));
    }

    Ok(GraphSnapshot { nodes, edges })
}

fn capture_graph_inputs(state: &AppState) -> VaultResult<CapturedGraphInputs> {
    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    let _mutation = state.vault_publish_mutation_lock()?;
    let root = state
        .vault_root
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .ok_or(VaultError::NoVault)?
        .clone();
    let pdf_scope_prefix = AppState::pdf_scope_prefix_for_root(&root);
    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let indexed_outgoing: Vec<(PathBuf, Vec<PathBuf>)> = index
        .outgoing
        .iter()
        .map(|(source, targets)| (source.clone(), targets.iter().cloned().collect::<Vec<_>>()))
        .collect();
    Ok(CapturedGraphInputs {
        root,
        pdf_scope_prefix,
        generation: state.current_vault_generation(),
        content_revision: state.current_vault_content_revision(),
        indexed_outgoing,
    })
}

fn collect_wikilink_edges(
    root: &Path,
    indexed_outgoing: Vec<(PathBuf, Vec<PathBuf>)>,
    edge_weights: &mut BTreeMap<(String, String, GraphEdgeKind), usize>,
) {
    for (source, targets) in indexed_outgoing {
        let Some(source) = graph_relative_path(&source, root) else {
            continue;
        };
        for target in targets {
            let Some(target) = graph_relative_path(&target, root) else {
                continue;
            };
            if is_image_path(Path::new(&target)) {
                continue;
            }
            let weight = edge_weights
                .entry((source.clone(), target, GraphEdgeKind::WikiLink))
                .or_default();
            *weight = weight.saturating_add(1);
        }
    }
}

fn query_annotation_edges(
    state: &AppState,
    pdf_scope_prefix: &str,
) -> VaultResult<Vec<(String, String, usize)>> {
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
        .query_map([pdf_scope_prefix], |row| {
            let count: i64 = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                usize::try_from(count).unwrap_or(usize::MAX),
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| VaultError::from(e.to_string()))
}

fn collect_annotation_edges(
    root: &Path,
    pdf_paths: &BTreeSet<String>,
    annotation_edges: Vec<(String, String, usize)>,
    edge_weights: &mut BTreeMap<(String, String, GraphEdgeKind), usize>,
) {
    for (source, target, count) in annotation_edges {
        let Some(source) = normalize_stored_graph_path(root, &source) else {
            continue;
        };
        if !pdf_paths.contains(&source) {
            continue;
        }
        let Some(target) = normalize_stored_graph_path(root, &target) else {
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
}

fn assemble_graph_snapshot(
    markdown_paths: &BTreeSet<String>,
    pdf_paths: &BTreeSet<String>,
    edge_weights: BTreeMap<(String, String, GraphEdgeKind), usize>,
) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut node_kinds: BTreeMap<String, GraphNodeKind> = BTreeMap::new();
    for path in markdown_paths {
        node_kinds.insert(path.clone(), GraphNodeKind::Markdown);
    }
    for path in pdf_paths {
        node_kinds.insert(path.clone(), GraphNodeKind::Pdf);
    }
    for (source, target, _) in edge_weights.keys() {
        for path in [source, target] {
            node_kinds
                .entry(path.clone())
                .or_insert_with(|| graph_node_kind(path, markdown_paths, pdf_paths));
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
    (nodes, edges)
}
