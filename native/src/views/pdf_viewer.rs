use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length, Theme, Renderer};

use crate::messages::Message;
use crate::theme;
use crate::views::interactive_pdf::InteractivePdf;

pub fn view<'a>(
    image_handle: Option<&iced::widget::image::Handle>,
    current_page: u16,
    total_pages: u16,
    zoom: f32,
    dimensions: Option<(u32, u32)>,
) -> Element<'a, Message, Theme, Renderer> {
    let controls = row![
        button(text("◀").size(14))
            .on_press(Message::PdfPageChanged(current_page.saturating_sub(1)))
            .padding(8)
            .style(button::text),
        text(format!("Page {} of {}", current_page + 1, total_pages))
            .size(14)
            .color(theme::TEXT_PRIMARY),
        button(text("▶").size(14))
            .on_press(Message::PdfPageChanged((current_page + 1).min(total_pages.saturating_sub(1))))
            .padding(8)
            .style(button::text),
        Space::new().width(Length::Fill),
        text(format!("{:.0}%", zoom * 100.0)).size(12).color(theme::TEXT_MUTED),
        button(text("-").size(16))
            .on_press(Message::PdfZoomChanged((zoom - 0.1).max(0.5)))
            .padding([4, 10])
            .style(button::text),
        button(text("+").size(16))
            .on_press(Message::PdfZoomChanged((zoom + 0.1).min(3.0)))
            .padding([4, 10])
            .style(button::text),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .padding(10);

    let viewer = if let Some(handle) = image_handle {
        let (w, h) = dimensions.unwrap_or((800, 1100));
        container(InteractivePdf::new(
            handle.clone(),
            w as f32,
            h as f32,
            move |x, y| Message::PdfLeftClicked(current_page, x, y),
            move |x, y| Message::PdfRightClicked(current_page, x, y),
        ))
        .width(Length::Fill)
        .center_x(Length::Fill)
    } else {
        container(text("Rendering PDF...").color(theme::TEXT_MUTED))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    };

    container(column![controls, viewer])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_PRIMARY)),
            ..Default::default()
        })
        .into()
}

pub fn toolbar<'a>(
    current_page: u16,
    total_pages: u16,
    zoom: f32,
    toc_visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let page_label = if total_pages == 0 {
        "No PDF".to_string()
    } else {
        format!("{} / {}", current_page + 1, total_pages)
    };

    container(
        row![
            button(text("☰").size(14).color(if toc_visible { theme::ACCENT } else { theme::TEXT_MUTED }))
                .on_press(Message::ToggleTOC)
                .padding(8)
                .style(button::text),
            Space::new().width(Length::Fill),
            button(text("-").size(16))
                .on_press(Message::PdfZoomChanged((zoom - 0.1).max(0.5)))
                .padding([4, 10])
                .style(button::text),
            text(format!("{:.0}%", zoom * 100.0)).size(12).color(theme::TEXT_MUTED),
            button(text("+").size(16))
                .on_press(Message::PdfZoomChanged((zoom + 0.1).min(4.0)))
                .padding([4, 10])
                .style(button::text),
            Space::new().width(Length::Fill),
            text(page_label).size(12).color(theme::TEXT_SECONDARY),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([6, 12])
    )
    .width(Length::Fill)
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

pub fn view_continuous<'a>(
    pages: &'a [Option<iced::widget::image::Handle>],
    zoom: f32,
    dimensions: &'a [Option<(u32, u32)>],
) -> Element<'a, Message, Theme, Renderer> {
    let mut page_list = column![].spacing(20).padding(20).align_x(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    for (i, page_opt) in pages.iter().enumerate() {
        if let Some(handle) = page_opt {
            let (w, h) = dimensions[i].unwrap_or((800, 1100));
            page_list = page_list.push(
                container(InteractivePdf::new(
                    handle.clone(),
                    w as f32,
                    h as f32,
                    move |x, y| Message::PdfLeftClicked(i as u16, x, y),
                    move |x, y| Message::PdfRightClicked(i as u16, x, y),
                ))
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::WHITE)),
                    shadow: iced::Shadow {
                        color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                        offset: iced::Vector::new(0.0, 4.0),
                        blur_radius: 10.0,
                    },
                    ..Default::default()
                })
            );
        } else {
            page_list = page_list.push(
                container(text(format!("Loading Page {}...", i + 1)).color(theme::TEXT_MUTED))
                    .width(Length::Fixed(800.0 * zoom))
                    .height(Length::Fixed(1100.0 * zoom))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
            );
        }
    }

    container(page_list)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_PRIMARY)),
            ..Default::default()
        })
        .into()
}
