use iced::widget::{Space, button, column, container, scrollable, text};
use iced::{Element, Length, Padding, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

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
        let marker = trimmed
            .chars()
            .next()
            .filter(|c| *c == '\x60' || *c == '~');
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
        if (1..=6).contains(&level)
            && trimmed.chars().nth(level).is_some_and(|c| c == ' ')
        {
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

#[cfg(test)]
mod tests {
    use super::get_toc;

    #[test]
    fn toc_skips_fenced_comments_and_tags() {
        let toc = get_toc(
            "# Real\n\n\x60\x60\x60bash\n# not a heading\n\x60\x60\x60\n#tag\n## Heading",
        );
        assert_eq!(toc.len(), 2);
        assert_eq!((toc[0].text.as_str(), toc[0].line), ("Real", 0));
        assert_eq!((toc[1].text.as_str(), toc[1].line), ("Heading", 6));
    }

    #[test]
    fn toc_tracks_tilde_fences() {
        let toc = get_toc("~~~text\n## hidden\n~~~~\n### Visible");
        assert_eq!(toc.len(), 1);
        assert_eq!((toc[0].text.as_str(), toc[0].line), ("Visible", 3));
    }
}

pub fn view<'a>(
    toc: &'a [TocEntry],
    is_synthetic: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let title = text("Table of Contents")
        .size(16)
        .color(theme::TEXT_PRIMARY);

    // Subtle badge so the user knows a bookmark-less PDF's outline was
    // generated heuristically from page text rather than embedded bookmarks.
    let badge: Element<'a, Message, Theme, Renderer> = if is_synthetic {
        container(text("Generated").size(11).color(theme::TEXT_MUTED))
            .padding(Padding {
                top: 2.0,
                right: 6.0,
                bottom: 2.0,
                left: 6.0,
            })
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_PRIMARY)),
                border: iced::Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    let items = toc.iter().map(|entry| {
        let indent = (entry.level.saturating_sub(1) as f32) * 15.0;

        container(
            button(text(&entry.text).size(14).color(theme::TEXT_SECONDARY))
                .on_press(Message::TocClicked(entry.line))
                .padding([4, 8])
                .style(button::text)
                .width(Length::Fill),
        )
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: indent,
        })
        .into()
    });

    container(
        column![
            title,
            badge,
            Space::new().height(Length::Fixed(10.0)),
            scrollable(column(items).spacing(2))
        ]
        .spacing(4)
        .padding(15),
    )
    .width(Length::Fixed(250.0))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}
