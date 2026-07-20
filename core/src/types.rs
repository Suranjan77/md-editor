use serde::{Deserialize, Serialize};

/// A file entry in the vault listing.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SearchResult {
    pub path: String,
    pub line: usize,
    pub context: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SearchResults {
    pub items: Vec<SearchResult>,
    pub truncated: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum BacklinkTarget {
    MarkdownFile {
        path: String,
    },
    PdfDocument {
        path: String,
    },
    PdfAnnotation {
        document_path: String,
        annotation_id: String,
        page: u16,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BacklinkItem {
    pub source: BacklinkTarget,
    pub label: String,
    pub context: Option<String>,
}

/// A deterministic, UI-ready snapshot of the vault's note/reference graph.
///
/// Nodes and edges are sorted by their stable vault-relative paths before the
/// snapshot is returned. Callers can therefore use vector indices as stable
/// layout seeds for as long as the underlying graph is unchanged.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphSnapshot {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GraphNode {
    /// Vault-relative path using `/` separators. This is the stable node id.
    pub path: String,
    /// A short display label, normally the filename without its extension.
    pub label: String,
    pub kind: GraphNodeKind,
    /// Whether the target currently exists in the indexed vault.
    pub exists: bool,
    /// Weighted number of incoming relationships.
    pub incoming: usize,
    /// Weighted number of outgoing relationships.
    pub outgoing: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphNodeKind {
    Markdown,
    Pdf,
    /// A wikilink or annotation endpoint that no longer resolves to a file.
    Missing,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    /// Vault-relative source node path.
    pub source: String,
    /// Vault-relative target node path.
    pub target: String,
    pub kind: GraphEdgeKind,
    /// Number of relationships represented by this aggregated edge.
    pub weight: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphEdgeKind {
    WikiLink,
    PdfAnnotation,
}
