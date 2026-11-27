//! Callout families (`> [!note]`, `> [!warning]`, …) and the colour each is framed in.

use iced::Color;

/// The family of a `> [!type]` callout, which picks its colour. Any type is
/// accepted; unknown ones read as notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Callout {
    Note,
    Tip,
    Warning,
    Danger,
    Quote,
}

impl Callout {
    pub fn from_type(kind: &str) -> Self {
        match kind.to_ascii_lowercase().as_str() {
            "tip" | "hint" | "success" | "check" | "done" | "important" => Self::Tip,
            "warning" | "caution" | "attention" | "question" | "help" | "faq" => Self::Warning,
            "danger" | "error" | "bug" | "failure" | "fail" | "missing" => Self::Danger,
            "quote" | "cite" => Self::Quote,
            _ => Self::Note,
        }
    }

    pub fn color(self) -> Color {
        let t = crate::design::tokens();
        match self {
            Self::Note => t.status_info,
            Self::Tip => t.status_success,
            Self::Warning => t.status_warning,
            Self::Danger => t.status_danger,
            Self::Quote => t.doc_quote_bar,
        }
    }
}
