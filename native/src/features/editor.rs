use std::time::Instant;

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::editor::parser;
use crate::messages::{EditorBlockActionKind, EditorLinkActionKind};
use crate::views;

#[derive(Debug, Clone)]
pub(crate) enum EditorMessage {
    Command(EditorCommand),
    CommandNoScroll(EditorCommand),
    Save(bool),
    CheckboxToggle(usize),
    BlockContextMenu {
        line_idx: usize,
        absolute_pos: iced::Point,
    },
    BlockAction {
        line_idx: usize,
        action: EditorBlockActionKind,
    },
    ContextMenu {
        line_idx: usize,
        col: usize,
        absolute_pos: iced::Point,
    },
    LinkAction {
        line_idx: usize,
        start_col: usize,
        end_col: usize,
        link_target: String,
        action: EditorLinkActionKind,
    },
    CursorMove(usize, usize),
    Scrolled {
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    },
    ScrollToTarget(f32),
    HighlightReady(u64, Vec<parser::StyledLine>),
    HighlightDebounceElapsed,
    AutosaveElapsed,
    MathRendered(
        String,
        Result<(iced::widget::image::Handle, f32, f32), String>,
    ),
    ImageLoadFailed(String, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditorEffect {
    None,
    ActivateMarkdown,
    LoadMedia,
    ShowToast(String),
}

pub(crate) struct EditorFeatureState {
    pub(crate) buffer: DocBuffer,
    pub(crate) highlighted_lines: Vec<parser::StyledLine>,
    pub(crate) highlight_generation: u64,
    pub(crate) pending_highlight_generation: Option<u64>,
    pub(crate) pending_highlight_requested_at: Option<Instant>,
    pub(crate) pending_highlight_text: Option<String>,
    pub(crate) pending_save: Option<Instant>,
    pub(crate) image_cache:
        std::collections::HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    pub(crate) math_cache:
        std::collections::HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    pub(crate) image_errors: std::collections::HashMap<String, String>,
    pub(crate) math_errors: std::collections::HashMap<String, String>,
    pub(crate) toc_entries: Vec<views::toc::TocEntry>,
    pub(crate) scroll_y: f32,
    pub(crate) viewport_width: f32,
    pub(crate) viewport_height: f32,
    pub(crate) active_image_path: Option<String>,
    pub(crate) active_image: Option<(iced::widget::image::Handle, f32, f32)>,
}

impl EditorFeatureState {
    pub(crate) fn update_local(&mut self, message: EditorMessage) -> EditorEffect {
        match message {
            EditorMessage::MathRendered(tex, result) => match result {
                Ok(rendered) => {
                    self.math_errors.remove(&tex);
                    self.math_cache.insert(tex, rendered);
                    EditorEffect::None
                }
                Err(error) => {
                    self.math_errors.insert(tex, error.clone());
                    EditorEffect::ShowToast(format!("Math render failed: {error}"))
                }
            },
            EditorMessage::ImageLoadFailed(path, error) => {
                self.image_errors.insert(path.clone(), error.clone());
                EditorEffect::ShowToast(format!("Image load failed: {path}: {error}"))
            }
            EditorMessage::Scrolled {
                y,
                viewport_width,
                viewport_height,
            } => {
                self.scroll_y = y;
                self.viewport_width = viewport_width;
                self.viewport_height = viewport_height;
                EditorEffect::ActivateMarkdown
            }
            EditorMessage::HighlightReady(generation, lines) => {
                if generation != self.highlight_generation {
                    return EditorEffect::None;
                }
                self.highlighted_lines = lines;
                self.toc_entries = views::toc::get_toc(&self.highlighted_lines);
                EditorEffect::LoadMedia
            }
            _ => EditorEffect::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorEffect, EditorFeatureState, EditorMessage};
    use crate::editor::buffer::DocBuffer;

    fn state() -> EditorFeatureState {
        EditorFeatureState {
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            highlight_generation: 7,
            pending_highlight_generation: None,
            pending_highlight_requested_at: None,
            pending_highlight_text: None,
            pending_save: None,
            image_cache: Default::default(),
            math_cache: Default::default(),
            image_errors: Default::default(),
            math_errors: Default::default(),
            toc_entries: Vec::new(),
            scroll_y: 0.0,
            viewport_width: 0.0,
            viewport_height: 0.0,
            active_image_path: None,
            active_image: None,
        }
    }

    #[test]
    fn scroll_updates_viewport_and_requests_markdown_activation() {
        let mut state = state();

        let effect = state.update_local(EditorMessage::Scrolled {
            y: 42.0,
            viewport_width: 800.0,
            viewport_height: 600.0,
        });

        assert_eq!(effect, EditorEffect::ActivateMarkdown);
        assert_eq!(state.scroll_y, 42.0);
        assert_eq!(state.viewport_width, 800.0);
        assert_eq!(state.viewport_height, 600.0);
    }

    #[test]
    fn stale_highlight_does_not_replace_projection() {
        let mut state = state();
        let mut line = crate::editor::parser::StyledLine::new();
        line.spans
            .push(crate::editor::parser::StyledSpan::plain("stale"));

        let effect = state.update_local(EditorMessage::HighlightReady(6, vec![line]));

        assert_eq!(effect, EditorEffect::None);
        assert!(state.highlighted_lines.is_empty());
    }
}
