//! Table style parts.

// ── Table styles ─────────────────────────────────────────────────────

/// The built-in table styles the viewer approximates. PowerPoint does not
/// store built-in styles in the file, only their ids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum TableLook {
    Plain,
    Grid,
    /// "Medium Style 2": a solid header in the accent colour, tinted banded
    /// rows, and white rules between cells — PowerPoint's default table.
    Medium2 {
        accent: &'static str,
    },
}

pub(super) fn table_look(style_id: &str) -> TableLook {
    match style_id.to_ascii_uppercase().as_str() {
        "" => TableLook::Plain,
        "{2D5ABB26-0587-4C30-8999-92F81FD0307C}" => TableLook::Plain,
        "{5940675A-B579-460E-94D1-54222C63F5DA}" => TableLook::Grid,
        "{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}" => TableLook::Medium2 { accent: "dk1" },
        "{21E4AEA4-8DFA-4A89-87EB-49C32662AFE8}" => TableLook::Medium2 { accent: "accent2" },
        "{F5AB1C69-6EDB-4FF4-983F-18BD219EF322}" => TableLook::Medium2 { accent: "accent3" },
        "{00A15C55-8517-42AA-B614-E9B94910E393}" => TableLook::Medium2 { accent: "accent4" },
        "{7DF18680-E054-41AD-8BC1-D1AEF772440D}" => TableLook::Medium2 { accent: "accent5" },
        "{93296810-A885-4BE3-A3E7-6D5BEEA58F35}" => TableLook::Medium2 { accent: "accent6" },
        // Medium Style 2 – Accent 1 itself, and any style not listed: most
        // built-in styles are accent-coloured banded tables like it.
        _ => TableLook::Medium2 { accent: "accent1" },
    }
}
