use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Element, Length, Padding, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub(crate) type TocEntry = crate::editor::parser::OutlineEntry;

pub(crate) fn get_toc(lines: &[crate::editor::parser::StyledLine]) -> Vec<TocEntry> {
    crate::editor::parser::extract_outline(lines)
}

pub(crate) fn view<'a>(
    md_toc: &'a [TocEntry],
    pdf_toc: &'a [TocEntry],
    width: f32,
    active_md_line: Option<usize>,
    active_pdf_page: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    let header = row![
        text("OUTLINE")
            .size(11)
            .color(theme::text_muted())
            .font(iced::Font::default()),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .padding([12, 16]);

    let divider = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::border_subtle())),
            ..Default::default()
        });

    let has_md = !md_toc.is_empty();
    let has_pdf = !pdf_toc.is_empty();

    let mut content = column![].spacing(12).padding([8, 8]);

    if has_md {
        content = content
            .push(container(text("MARKDOWN").size(11).color(theme::text_muted())).padding([4, 8]));
        let md_items = md_toc.iter().map(|entry| {
            let indent = (entry.level.saturating_sub(1) as f32) * 12.0;
            let active = active_md_line.map_or(false, |line| line == entry.line);

            let color = match entry.level {
                1 => theme::text_primary(),
                2 => theme::text_secondary(),
                _ => theme::text_muted(),
            };

            container(
                crate::views::focus_button::focus_button(text(&entry.text).size(13).color(color))
                    .on_press(Message::TocClicked(entry.line))
                    .padding(8.0)
                    .subtle(!active)
                    .active(active)
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
        content = content.push(column(md_items).spacing(2));
    }

    if has_pdf {
        if has_md {
            content = content.push(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fixed(1.0))
                    .style(|_| container::Style {
                        background: Some(Background::Color(theme::border_subtle())),
                        ..Default::default()
                    }),
            );
        }
        content = content
            .push(container(text("PDF").size(11).color(theme::text_muted())).padding([4, 8]));
        let pdf_items = pdf_toc.iter().map(|entry| {
            let indent = (entry.level.saturating_sub(1) as f32) * 12.0;
            let active = active_pdf_page.map_or(false, |page| page == entry.line);

            let color = match entry.level {
                1 => theme::text_primary(),
                2 => theme::text_secondary(),
                _ => theme::text_muted(),
            };

            container(
                crate::views::focus_button::focus_button(text(&entry.text).size(13).color(color))
                    .on_press(Message::PdfTocClicked(entry.line))
                    .padding(8.0)
                    .subtle(!active)
                    .active(active)
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
        content = content.push(column(pdf_items).spacing(2));
    }

    if !has_md && !has_pdf {
        content = content.push(
            container(
                text("No outline or TOC available")
                    .size(13)
                    .color(theme::text_muted()),
            )
            .padding([8, 8]),
        );
    }

    container(column![
        header,
        divider,
        scrollable(content).height(Length::Fill)
    ])
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(theme::bg_secondary())),
        border: Border {
            color: theme::border(),
            width: 1.0,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toc_renders_empty_state() {
        let mut ui = iced_test::simulator(view(&[], &[], 250.0, None, None));

        ui.find("No outline or TOC available")
            .expect("visible TOC panel should explain empty outline state");
    }
}
