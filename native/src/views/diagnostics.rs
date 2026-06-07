#![allow(dead_code)]

use iced::widget::{Column, Space, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub fn view<'a>(
    pdf_cached_pages: usize,
    pdf_cache_bytes: usize,
    pdf_text_pages: usize,
    outgoing_links: usize,
    incoming_backlinks: usize,
    fts_documents: usize,
    active_editor_chars: usize,
    active_editor_lines: usize,
    visible: bool,
    width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let header_row = row![
        text("DIAGNOSTICS")
            .size(11)
            .color(theme::text_muted())
            .font(iced::Font::default()),
        Space::new().width(Length::Fill),
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

    let item = |label: &'static str, val: String| {
        row![
            text(label).size(12).color(theme::text_secondary()),
            Space::new().width(Length::Fill),
            text(val).size(12).color(theme::text_primary())
        ]
        .align_y(Alignment::Center)
        .padding([4, 0])
    };

    let list = Column::new()
        .spacing(8)
        .push(item("PDF Cached Pages", format!("{}", pdf_cached_pages)))
        .push(item("PDF Cache Bytes", format!("{}", pdf_cache_bytes)))
        .push(item("PDF Text Pages", format!("{}", pdf_text_pages)))
        .push(item("Outgoing Links", format!("{}", outgoing_links)))
        .push(item(
            "Incoming Backlinks",
            format!("{}", incoming_backlinks),
        ))
        .push(item("FTS Documents", format!("{}", fts_documents)))
        .push(item("Editor Chars", format!("{}", active_editor_chars)))
        .push(item("Editor Lines", format!("{}", active_editor_lines)));

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
    fn visible_panel_reports_runtime_counters() {
        let mut ui = iced_test::simulator(view(3, 4096, 2, 4, 5, 6, 120, 12, true, 260.0));

        ui.find("PDF Cached Pages")
            .expect("diagnostics label should render");
        ui.find("4096").expect("cache byte count should render");
        ui.find("Editor Lines")
            .expect("editor line count should render");
    }
}
