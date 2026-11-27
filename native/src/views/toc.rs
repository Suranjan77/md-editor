//! Outline panel: Markdown headings and PDF bookmarks/synthesized headings, then a PDF's embedded files.

use iced::widget::{Column, button, container, scrollable, text};
use iced::{Background, Border, Element, Length, Padding, Renderer, Theme};

use crate::design::metrics::ControlSize;
use crate::design::metrics::{RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::messages::{EditorMessage, Message, PdfMessage};
use crate::ui::{
    BadgeTone, Icon, IconButton, ListRowProps, PanelSide, badge, ellipsize_single_line,
    empty_state, list_row, panel,
};
use md_editor_core::pdf::PdfAttachmentInfo;

#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

pub fn get_toc(buffer_text: &str) -> Vec<TocEntry> {
    toc_from_lines(buffer_text.split('\n'))
}

/// [`get_toc`] over a note's lines, so the open buffer's outline is read
/// straight from its rope instead of a copy of the whole document.
pub fn toc_from_lines<L: AsRef<str>>(lines: impl Iterator<Item = L>) -> Vec<TocEntry> {
    let mut toc = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, line) in lines.enumerate() {
        let line = line.as_ref();
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

/// A PDF's embedded files, listed under its outline.
pub struct OutlineAttachments<'a> {
    /// Vault path of the PDF they are in.
    pub pdf_path: &'a str,
    pub files: &'a [PdfAttachmentInfo],
}

/// `1.5 MB`-style size of an attachment.
pub fn format_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn attachments_section<'a>(
    attachments: &OutlineAttachments<'a>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let role = TextRole::Caption;
    let mut rows: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        text(format!("Attachments · {}", attachments.files.len()))
            .size(role.size())
            .font(role.font())
            .color(t.fg_tertiary)
            .into(),
    ];
    for file in attachments.files {
        let open = Message::Pdf(PdfMessage::OpenAttachment(
            attachments.pdf_path.to_string(),
            file.clone(),
        ));
        let save = Message::Pdf(PdfMessage::SaveAttachment(
            attachments.pdf_path.to_string(),
            file.clone(),
        ));
        let mut props =
            ListRowProps::simple(file.name.as_str(), Some(Icon::Paperclip), false, open);
        props.subtitle = Some(format_size(file.size).into());
        props.max_title_chars = 24;
        rows.push(
            iced::widget::row![
                container(list_row(props)).width(Length::Fill),
                IconButton::new(Icon::DownloadSimple, "Save to vault")
                    .size(ControlSize::Sm)
                    .on_press(save)
                    .build(),
            ]
            .spacing(SPACE_1)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }
    Column::with_children(rows)
        .spacing(SPACE_1)
        .padding([SPACE_2, SPACE_3])
        .into()
}

pub fn view<'a>(
    toc: &'a [TocEntry],
    is_synthetic: bool,
    current_pos: usize,
    attachments: Option<OutlineAttachments<'a>>,
    width: f32,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let attachments = attachments.filter(|a| !a.files.is_empty());
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

    let body: Element<'a, Message, Theme, Renderer> = if toc.is_empty()
        && let Some(attachments) = &attachments
    {
        scrollable(attachments_section(attachments))
            .height(Length::Fill)
            .into()
    } else if toc.is_empty() {
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

        let headings = Column::with_children(items)
            .spacing(SPACE_1 * 0.5)
            .padding([SPACE_2, SPACE_3]);
        let content = match &attachments {
            Some(attachments) => Column::new()
                .push(headings)
                .push(attachments_section(attachments)),
            None => Column::new().push(headings),
        };
        scrollable(content).height(Length::Fill).into()
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
    use super::{active_toc_index, format_size, get_toc};

    #[test]
    fn attachment_sizes_read_naturally() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(200 * 1024), "200 KB");
        assert_eq!(format_size(3 * 1024 * 1024 + 400 * 1024), "3.4 MB");
    }

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
