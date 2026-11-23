//! Placeholder matching between slide, layout and master.

use super::*;

// ── Placeholders ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Placeholder {
    pub(super) kind: String,
    pub(super) idx: Option<u32>,
}

pub(super) fn placeholder(node: Node) -> Option<Placeholder> {
    let ph = descend(non_visual(node)?, &["nvPr", "ph"])?;
    Some(Placeholder {
        kind: ph.attribute("type").unwrap_or("obj").to_string(),
        idx: attr_u32(ph, "idx"),
    })
}

/// The master placeholder type a slide or layout placeholder inherits from.
pub(super) fn master_kind(kind: &str) -> &str {
    match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => kind,
        _ => "body",
    }
}

pub(super) struct TemplatePart<'a, 'i> {
    pub(super) root: Node<'a, 'i>,
    pub(super) rels: &'a Relationships,
    pub(super) placeholders: Vec<(Placeholder, Node<'a, 'i>)>,
}

pub(super) fn template<'a, 'i>(
    part: Option<&str>,
    docs: &'a HashMap<String, Document<'i>>,
    rels: &'a HashMap<String, Relationships>,
) -> Option<TemplatePart<'a, 'i>> {
    let part = part?;
    let root = docs.get(part)?.root_element();
    let placeholders = descend(root, &["cSld", "spTree"])
        .map(|tree| {
            tree.descendants()
                .filter(|n| is(n, "sp") || is(n, "pic") || is(n, "graphicFrame"))
                .filter_map(|n| Some((placeholder(n)?, n)))
                .collect()
        })
        .unwrap_or_default();
    Some(TemplatePart {
        root,
        rels: rels.get(part)?,
        placeholders,
    })
}
