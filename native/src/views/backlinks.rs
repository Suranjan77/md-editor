//! Backlinks side panel: notes and PDF highlights pointing to the active document.

use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::design::metrics::{IconSize, RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, PdfMessage, Shortcut, VaultMessage};
use crate::ui::{
    BadgeTone, Icon, IconButton, PanelSide, badge, ellipsize_single_line, empty_state, icon, panel,
};

pub fn view<'a>(
    backlinks: &'a [md_editor_core::types::BacklinkItem],
    visible: bool,
    width: f32,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
    }

    let t = tokens();
    let header_actions = vec![
        badge(&backlinks.len().to_string(), BadgeTone::Accent, None),
        IconButton::new(Icon::X, "Close backlinks")
            .on_press(Message::KeyboardShortcut(Shortcut::ToggleBacklinks))
            .build(),
    ];

    let body: Element<'a, Message, Theme, Renderer> = if backlinks.is_empty() {
        empty_state(
            Icon::Link,
            "No backlinks yet",
            "Link here with [[Note]] or attach a PDF highlight to see incoming references.",
            None,
        )
    } else {
        let items = backlinks.iter().map(|item| {
            let (kind_icon, msg) = match &item.source {
                md_editor_core::types::BacklinkTarget::MarkdownFile { path } => (
                    Icon::FileText,
                    Message::Vault(VaultMessage::SidebarFileClicked(path.clone())),
                ),
                md_editor_core::types::BacklinkTarget::PdfDocument { path } => (
                    Icon::FilePdf,
                    Message::Vault(VaultMessage::SidebarFileClicked(path.clone())),
                ),
                md_editor_core::types::BacklinkTarget::PdfAnnotation {
                    document_path,
                    annotation_id,
                    page,
                } => (
                    Icon::Highlighter,
                    Message::Pdf(PdfMessage::AnnotationFocused {
                        document_path: document_path.clone(),
                        annotation_id: annotation_id.clone(),
                        page: *page,
                    }),
                ),
            };

            let mut item_col = column![
                row![
                    icon(kind_icon, IconSize::Sm.px(), t.accent),
                    text(ellipsize_single_line(&item.label, 30).0)
                        .size(TextRole::Caption.size())
                        .color(t.fg_primary)
                        .font(crate::design::fonts::UI_FONT_MEDIUM),
                ]
                .spacing(SPACE_2)
                .align_y(Alignment::Center)
            ]
            .spacing(SPACE_1);

            if let Some(ctx) = &item.context
                && !ctx.trim().is_empty()
            {
                item_col = item_col.push(
                    text(ellipsize_single_line(ctx.trim(), 56).0)
                        .size(TextRole::Caption.size())
                        .color(t.fg_tertiary)
                        .font(TextRole::Caption.font()),
                );
            }

            button(item_col)
                .on_press(msg)
                .padding([SPACE_2, SPACE_3])
                .width(Length::Fill)
                .style(move |_, status| {
                    let bg = matches!(status, button::Status::Hovered | button::Status::Pressed)
                        .then_some(Background::Color(t.bg_raised));
                    button::Style {
                        background: bg,
                        text_color: t.fg_primary,
                        border: Border {
                            color: t.border_subtle,
                            width: 1.0,
                            radius: RADIUS_SM.into(),
                        },
                        ..Default::default()
                    }
                })
                .into()
        });

        scrollable(
            Column::with_children(items)
                .spacing(SPACE_2)
                .padding([SPACE_2, SPACE_3]),
        )
        .height(Length::Fill)
        .into()
    };

    container(panel(
        "Backlinks",
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
