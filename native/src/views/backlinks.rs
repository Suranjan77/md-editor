use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

fn backlink_button_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = button::text(theme, status);
        style.border.radius = theme::RADIUS_SMALL.into();

        if status == button::Status::Hovered || status == button::Status::Pressed {
            style.background = Some(Background::Color(theme::bg_tertiary()));
        }
        style
    }
}

/// Render the backlinks panel.
pub fn view<'a>(
    backlinks: &'a [md_editor_core::types::BacklinkItem],
    visible: bool,
    width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let header_row = row![
        text("BACKLINKS")
            .size(11)
            .color(theme::text_muted())
            .font(iced::Font::default()),
        Space::new().width(Length::Fill),
        if backlinks.is_empty() {
            text("0").size(11).color(theme::text_muted())
        } else {
            text(format!("{}", backlinks.len()))
                .size(11)
                .color(theme::accent())
        }
    ]
    .align_y(Alignment::Center)
    .padding([12, 14]);

    let divider = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::border_subtle())),
            ..Default::default()
        });

    let header = column![header_row, divider];

    let mut list = Column::new().spacing(6);

    if backlinks.is_empty() {
        list = list.push(
            container(
                text("No backlinks found")
                    .size(12)
                    .color(theme::text_muted()),
            )
            .padding([12, 0]),
        );
    } else {
        for item in backlinks {
            let (msg, is_annotation) = match &item.source {
                md_editor_core::types::BacklinkTarget::MarkdownFile { path } => {
                    (Message::SidebarFileClicked(path.clone()), false)
                }
                md_editor_core::types::BacklinkTarget::PdfDocument { path } => {
                    (Message::SidebarFileClicked(path.clone()), false)
                }
                md_editor_core::types::BacklinkTarget::PdfAnnotation {
                    document_path,
                    annotation_id,
                    page,
                } => (
                    Message::PdfAnnotationFocused {
                        document_path: document_path.clone(),
                        annotation_id: annotation_id.clone(),
                        page: *page,
                    },
                    true,
                ),
            };

            let mut btn_content =
                column![text(&item.label).size(12).color(theme::text_primary())].spacing(2);

            if let Some(ctx) = &item.context {
                if !ctx.trim().is_empty() {
                    btn_content = btn_content.push(text(ctx).size(10).color(theme::text_muted()));
                }
            }

            let mut row_content = row![];

            if is_annotation {
                row_content = row_content.push(
                    container(Space::new())
                        .width(Length::Fixed(2.0))
                        .height(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(theme::accent_secondary())),
                            ..Default::default()
                        }),
                );
                row_content = row_content.push(Space::new().width(Length::Fixed(6.0)));
            }

            row_content = row_content.push(btn_content);

            let btn: iced::widget::Button<'_, Message, Theme, Renderer> = button(row_content)
                .on_press(msg)
                .padding([6, 10])
                .width(Length::Fill)
                .style(backlink_button_style());

            list = list.push(btn);
        }
    }

    let content = column![
        header,
        scrollable(list.padding([12, 14])).height(Length::Fill),
    ]
    .width(Length::Fixed(width));

    container(content)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::bg_secondary())),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_visible_backlinks_panel_renders_empty_state() {
        let mut ui = iced_test::simulator(view(&[], true, 220.0));

        ui.find("No backlinks found")
            .expect("visible backlinks panel should explain empty backlink state");
    }
}
