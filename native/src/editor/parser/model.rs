use crate::theme;
use iced::Color;

#[derive(Debug, Clone)]
pub struct StyledSpan {
    /// The raw markdown source text for this span.
    pub text: String,
    /// Text to display in preview/rendered mode. If `None`, uses `text`.
    /// Set to `Some("")` to hide syntax markers like `**`, `#`, `$`, etc.
    pub display_text: Option<String>,
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
    pub font_size: f32,
    pub is_code: bool,
    pub is_link: bool,
    pub link_target: Option<String>,
    pub is_heading: bool,
    pub heading_level: u8,
    pub is_checkbox: bool,
    pub is_checked: bool,
    pub is_rule: bool,
    pub is_image: bool,
    pub image_path: Option<String>,
    /// Alt text for images, used for caption rendering.
    pub image_alt: Option<String>,
    pub is_math: bool,
    /// True if this span is a syntax marker (**, `, $, etc.) that should
    /// be hidden in preview mode.
    pub is_syntax: bool,
    /// Unique HTML-like identifier for this span.
    pub id: Option<String>,
}

impl StyledSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            display_text: None,
            color: theme::text_primary(),
            bold: false,
            italic: false,
            font_size: 17.0,
            is_code: false,
            is_link: false,
            link_target: None,
            is_heading: false,
            heading_level: 0,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            image_alt: None,
            is_math: false,
            is_syntax: false,
            id: None,
        }
    }

    /// Create a syntax-marker span that is hidden in preview mode.
    pub fn syntax(text: &str, color: Color, font_size: f32) -> Self {
        Self {
            text: text.to_string(),
            display_text: Some(String::new()),
            color,
            is_syntax: true,
            font_size,
            ..Self::plain("")
        }
    }

    /// Get the text to display based on editing mode.
    pub fn visible_text(&self, editing: bool) -> &str {
        if editing {
            &self.text
        } else if let Some(ref dt) = self.display_text {
            dt.as_str()
        } else {
            &self.text
        }
    }
}

/// A line of styled spans for the editor to render.
#[derive(Debug, Clone)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
    pub is_code_block: bool,
    pub is_math_block: bool,
    pub code_block_lang: Option<String>,
    pub is_blockquote: bool,
    /// Groups consecutive lines into blocks (code blocks, math blocks).
    /// Lines in the same block share the same `block_id`.
    /// Regular lines each get their own unique block_id.
    pub block_id: usize,
    /// True if this line is a fence line (```` ``` ```` or `$$`) — hidden in preview mode.
    pub is_block_fence: bool,
    pub is_table_row: bool,
    pub table_cells: Vec<Vec<StyledSpan>>,
}

impl StyledLine {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            is_code_block: false,
            is_math_block: false,
            code_block_lang: None,
            is_blockquote: false,
            block_id: 0,
            is_block_fence: false,
            is_table_row: false,
            table_cells: Vec::new(),
        }
    }
}
