use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::editor::parser::StyledLine;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LineHeightCache {
    pub hash: u64,
    pub is_editing: bool,
    pub active_col: Option<usize>,
    pub height: f32,
    pub valid: bool,
}

pub(crate) fn line_hash(line: &StyledLine) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.is_code_block.hash(&mut hasher);
    line.is_math_block.hash(&mut hasher);
    line.code_block_lang.hash(&mut hasher);
    line.is_blockquote.hash(&mut hasher);
    line.block_id.hash(&mut hasher);
    line.is_block_fence.hash(&mut hasher);
    line.is_table_row.hash(&mut hasher);
    for span in &line.spans {
        hash_span(span, &mut hasher);
    }
    for cell in &line.table_cells {
        for span in cell {
            hash_span(span, &mut hasher);
        }
    }
    hasher.finish()
}

pub(crate) fn resource_hash(
    line: &StyledLine,
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for span in &line.spans {
        if let Some(path) = span.image_path.as_deref() {
            path.hash(&mut hasher);
            image_cache
                .get(path)
                .map(|(_, w, h)| (w.to_bits(), h.to_bits()))
                .hash(&mut hasher);
        }
        if span.is_math || line.is_math_block {
            let tex = span.visible_text(false).trim_matches('$').trim();
            tex.hash(&mut hasher);
            math_cache
                .get(tex)
                .map(|(_, w, h)| (w.to_bits(), h.to_bits()))
                .hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn hash_span(span: &crate::editor::parser::StyledSpan, hasher: &mut impl Hasher) {
    span.text.hash(hasher);
    span.display_text.hash(hasher);
    span.bold.hash(hasher);
    span.italic.hash(hasher);
    span.font_size.to_bits().hash(hasher);
    span.is_code.hash(hasher);
    span.is_link.hash(hasher);
    span.link_target.hash(hasher);
    span.is_heading.hash(hasher);
    span.heading_level.hash(hasher);
    span.is_checkbox.hash(hasher);
    span.is_checked.hash(hasher);
    span.is_rule.hash(hasher);
    span.is_image.hash(hasher);
    span.image_path.hash(hasher);
    span.image_alt.hash(hasher);
    span.is_math.hash(hasher);
    span.is_syntax.hash(hasher);
    span.id.hash(hasher);
}
