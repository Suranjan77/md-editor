//! Outline panel: Markdown headings and PDF bookmarks/synthesized headings.

use iced::widget::{Column, button, container, scrollable, text};
use iced::{Background, Border, Element, Length, Padding, Renderer, Theme};

use crate::design::metrics::{RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::messages::{EditorMessage, Message};
use crate::ui::{
    BadgeTone, Icon, IconButton, PanelSide, badge, ellipsize_single_line, empty_state, panel,
};

pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

pub fn get_toc(buffer_text: &str) -> Vec<TocEntry> {
    let mut toc = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, line) in buffer_text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| *c == '\x60' || *c == '~');
        let marker_len = marker
            .map(|marker| trimmed.chars().take_while(|c| *c == marker).count())
            .unwrap_or(0);
        if let Some((fence_marker, fence_len)) = fence {
            if marker == Some(fence_marker) && marker_len >= fence_len {
                fence = None;
            }
            continue;
        } else if let Some(marker) = marker.filter(|_| marker_len >= 3) {
            fence = Some((marker, marker_len));
            continue;
        }

        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&level) && trimmed.chars().nth(level).is_some_and(|c| c == ' ') {
            let text = trimmed[level + 1..].trim().to_string();
            if !text.is_empty() {
                toc.push(TocEntry {
                    level: level as u8,
                    text,
                    line: i,
                });
            }
        }
    }
    toc
}

/// Find the index of the active outline entry for `current_line_or_page`.
pub fn active_toc_index(toc: &[TocEntry], current_line_or_page: usize) -> Option<usize> {
    toc.iter()
        .rposition(|entry| entry.line <= current_line_or_page)
}

pub fn view<'a>(
    toc: &'a [TocEntry],
    is_synthetic: bool,
    current_pos: usize,
    width: f32,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let active_idx = active_toc_index(toc, current_pos);

    let mut header_actions = Vec::with_capacity(2);
    if is_synthetic {
        header_actions.push(badge("Generated", BadgeTone::Neutral, None));
    }
    header_actions.push(
        IconButton::new(Icon::X, "Close outline")
            .on_press(Message::Editor(EditorMessage::ToggleTOC))
            .build(),
    );

    let body: Element<'a, Message, Theme, Renderer> = if toc.is_empty() {
        empty_state(
            Icon::ListTree,
            "No headings",
            "Add # headings to navigate sections from the outline.",
            None,
        )
    } else {
        let items = toc.iter().enumerate().map(|(idx, entry)| {
            let is_current = active_idx == Some(idx);
            let indent = (entry.level.saturating_sub(1) as f32) * SPACE_3;
            let (label, _) = ellipsize_single_line(&entry.text, 34);

            container(
                button(
                    text(label)
                        .size(TextRole::Caption.size())
                        .color(if is_current {
                            t.fg_primary
                        } else {
                            t.fg_secondary
                        })
                        .font(if is_current {
                            crate::design::fonts::UI_FONT_MEDIUM
                        } else {
                            TextRole::Caption.font()
                        }),
                )
                .on_press(Message::Editor(EditorMessage::TocClicked(entry.line)))
                .padding([SPACE_1, SPACE_2])
                .width(Length::Fill)
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    let bg = if is_current {
                        Some(Background::Color(t.bg_overlay))
                    } else if hovered {
                        Some(Background::Color(t.bg_raised))
                    } else {
                        None
                    };
                    button::Style {
                        background: bg,
                        text_color: if is_current {
                            t.fg_primary
                        } else {
                            t.fg_secondary
                        },
                        border: Border {
                            radius: RADIUS_SM.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            )
            .padding(Padding {
                top: SPACE_0,
                right: SPACE_0,
                bottom: SPACE_0,
                left: indent,
            })
            .into()
        });

        scrollable(
            Column::with_children(items)
                .spacing(SPACE_1 * 0.5)
                .padding([SPACE_2, SPACE_3]),
        )
        .height(Length::Fill)
        .into()
    };

    container(panel(
        "Outline",
        None,
        header_actions,
        body,
        PanelSide::Right,
        focused,
    ))
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::{active_toc_index, get_toc};

    #[test]
    fn toc_skips_fenced_comments_and_tags() {
        let toc =
            get_toc("# Real\n\n\x60\x60\x60bash\n# not a heading\n\x60\x60\x60\n#tag\n## Heading");
        assert_eq!(toc.len(), 2);
        assert_eq!((toc[0].text.as_str(), toc[0].line), ("Real", 0));
        assert_eq!((toc[1].text.as_str(), toc[1].line), ("Heading", 6));
        assert_eq!(active_toc_index(&toc, 3), Some(0));
        assert_eq!(active_toc_index(&toc, 8), Some(1));
    }

    #[test]
    fn toc_tracks_tilde_fences() {
        let toc = get_toc("~~~text\n## hidden\n~~~~\n### Visible");
        assert_eq!(toc.len(), 1);
        assert_eq!((toc[0].text.as_str(), toc[0].line), ("Visible", 3));
    }
}
