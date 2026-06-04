use iced::widget::{Space, button, column, container, scrollable, text};
use iced::{Element, Length, Padding, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub type TocEntry = crate::editor::highlight::OutlineEntry;

const BOLD_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

pub fn get_toc(lines: &[crate::editor::highlight::StyledLine]) -> Vec<TocEntry> {
    crate::editor::highlight::extract_outline(lines)
}

pub fn view<'a>(
    md_toc: &'a [TocEntry],
    pdf_toc: &'a [TocEntry],
    width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let title = text("Outline & TOC")
        .size(16)
        .font(BOLD_FONT)
        .color(theme::text_primary());

    let has_md = !md_toc.is_empty();
    let has_pdf = !pdf_toc.is_empty();

    let mut content = column![].spacing(12);

    if has_md {
        content = content.push(
            text("Markdown Outline")
                .size(13)
                .font(BOLD_FONT)
                .color(theme::text_primary()),
        );
        let md_items = md_toc.iter().map(|entry| {
            let indent = (entry.level.saturating_sub(1) as f32) * 15.0;
            container(
                button(text(&entry.text).size(13).color(theme::text_secondary()))
                    .on_press(Message::TocClicked(entry.line))
                    .padding([2, 4])
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
        content = content.push(column(md_items).spacing(2));
    }

    if has_pdf {
        if has_md {
            content = content.push(Space::new().height(Length::Fixed(10.0)));
        }
        content = content.push(
            text("PDF Table of Contents")
                .size(13)
                .font(BOLD_FONT)
                .color(theme::text_primary()),
        );
        let pdf_items = pdf_toc.iter().map(|entry| {
            let indent = (entry.level.saturating_sub(1) as f32) * 15.0;
            container(
                button(text(&entry.text).size(13).color(theme::text_secondary()))
                    .on_press(Message::PdfTocClicked(entry.line))
                    .padding([2, 4])
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
        content = content.push(column(pdf_items).spacing(2));
    }

    if !has_md && !has_pdf {
        content = content.push(
            text("No outline or TOC available")
                .size(13)
                .color(theme::text_muted()),
        );
    }

    container(
        column![
            title,
            Space::new().height(Length::Fixed(10.0)),
            scrollable(content)
        ]
        .padding(15),
    )
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::bg_secondary())),
        border: iced::Border {
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
        let mut ui = iced_test::simulator(view(&[], &[], 250.0));

        ui.find("No outline or TOC available")
            .expect("visible TOC panel should explain empty outline state");
    }
}
