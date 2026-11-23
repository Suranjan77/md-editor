//! Small roxmltree helpers: child lookup and typed attributes.

use super::*;

// ── XML helpers ──────────────────────────────────────────────────────

pub(super) fn parse_xml<'i>(text: &'i str, part: &str) -> Result<Document<'i>, String> {
    Document::parse(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{part} is not valid XML: {e}"))
}

pub(super) fn is(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

pub(super) fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| is(n, name))
}

pub(super) fn children_named<'a, 'i>(
    node: Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| is(n, name))
}

pub(super) fn descend<'a, 'i>(node: Node<'a, 'i>, path: &[&str]) -> Option<Node<'a, 'i>> {
    path.iter().try_fold(node, |node, name| child(node, name))
}

pub(super) fn attr_f64(node: Node, name: &str) -> Option<f64> {
    node.attribute(name)?.trim().parse().ok()
}

pub(super) fn attr_f32(node: Node, name: &str) -> Option<f32> {
    node.attribute(name)?.trim().parse().ok()
}

pub(super) fn attr_u32(node: Node, name: &str) -> Option<u32> {
    node.attribute(name)?.trim().parse().ok()
}

pub(super) fn attr_bool(node: Node, name: &str) -> Option<bool> {
    match node.attribute(name)? {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

/// A length attribute in EMUs, as points.
pub(super) fn emu_attr(node: Node, name: &str) -> Option<f32> {
    attr_f64(node, name).map(|emu| (emu / EMU_PER_POINT) as f32)
}

/// An attribute in thousandths of a percent (`100000` is 100%), as a fraction.
pub(super) fn fraction_attr(node: Node, name: &str) -> Option<f32> {
    attr_f32(node, name).map(|value| value / 100_000.0)
}

/// A relationship-id attribute (`r:id`, `r:embed`). Matched by local name
/// with any namespace, since strict OOXML uses a different namespace URI.
pub(super) fn rel_attr<'a>(node: Node<'a, '_>, local: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attr| attr.name() == local && attr.namespace().is_some())
        .map(|attr| attr.value())
}

/// The `nvSpPr`/`nvPicPr`/… element of a shape.
pub(super) fn non_visual<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name().starts_with("nv"))
}

pub(super) fn is_hidden(node: Node) -> bool {
    non_visual(node)
        .and_then(|nv| child(nv, "cNvPr"))
        .and_then(|pr| attr_bool(pr, "hidden"))
        .unwrap_or(false)
}
