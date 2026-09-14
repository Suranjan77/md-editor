//! The presentation viewer: every slide of the open deck in one continuous
//! scrolling column, with a toolbar to step between slides and zoom.
//!
//! Each slide is a stack of canvases, one per layer from
//! [`crate::slides::layer_ranges`], so shapes, pictures and text keep the
//! stacking order the deck gives them.

use std::sync::Arc;

use iced::widget::{Space, button, canvas, column, container, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::pptx_pane::PptxPane;
use crate::slides::LoadedDeck;
use crate::slides::draw::{SlideLayer, SlidePart};
use crate::theme;
use crate::views::icons::{self, Icon};

pub const PPTX_SCROLLABLE_ID: &str = "pptx_scrollable";
/// Space around the column of slides.
pub const SLIDE_LIST_PADDING: f32 = theme::SPACE_6;
/// Gap between one slide and the next slide's label.
pub const SLIDE_SPACING: f32 = theme::SPACE_5;
/// Height of the number-and-title row above each slide.
pub const SLIDE_LABEL_HEIGHT: f32 = theme::SPACE_6;
/// Width of the rule drawn around each slide.
const SLIDE_BORDER: f32 = 1.0;
/// Width kept free for the vertical scrollbar, so a slide fitted to the pane
/// never needs a horizontal one.
const SCROLLBAR_GUTTER: f32 = theme::SPACE_4;
/// Narrowest a slide is shown at, however narrow the pane.
const MIN_SLIDE_WIDTH: f32 = 160.0;

/// Where slides sit in the column at one pane width and zoom. Scrolling to a
/// slide and working out which slide is showing both read these, so they
/// must match how [`view`] lays the column out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideMetrics {
    pub width: f32,
    pub height: f32,
    /// From the top of one slide's label to the top of the next's.
    pub pitch: f32,
}

impl SlideMetrics {
    pub fn content_height(&self, count: usize) -> f32 {
        2.0 * SLIDE_LIST_PADDING + (count as f32 * self.pitch - SLIDE_SPACING).max(0.0)
    }
}

pub fn slide_metrics(deck: &LoadedDeck, available_width: f32, zoom: f32) -> SlideMetrics {
    let fitted = (available_width - 2.0 * (SLIDE_LIST_PADDING + SLIDE_BORDER) - SCROLLBAR_GUTTER)
        .max(MIN_SLIDE_WIDTH);
    let width = fitted * zoom;
    let height = width * deck.height / deck.width.max(1.0);
    SlideMetrics {
        width,
        height,
        pitch: SLIDE_LABEL_HEIGHT + height + 2.0 * SLIDE_BORDER + SLIDE_SPACING,
    }
}

pub fn view<'a>(pane: &'a PptxPane, available_width: f32) -> Element<'a, Message, Theme, Renderer> {
    let Some(deck) = &pane.deck else {
        let (message, color) = match &pane.error {
            Some(err) => (
                format!("This presentation can't be shown. {err}"),
                theme::DANGER,
            ),
            None => ("Loading presentation…".to_string(), theme::TEXT_MUTED),
        };
        return container(text(message).size(theme::TEXT_BASE).color(color))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(SLIDE_LIST_PADDING)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_PRIMARY)),
                ..Default::default()
            })
            .into();
    };
    column![
        slide_list(deck, available_width, pane.zoom),
        toolbar(pane, deck, available_width)
    ]
    .height(Length::Fill)
    .into()
}

/// One slide, `width` × `height` pixels: its layers stacked and clipped to
/// the page.
pub fn slide_page<'a, M: 'a>(
    deck: &Arc<LoadedDeck>,
    index: usize,
    width: f32,
    height: f32,
) -> Element<'a, M, Theme, Renderer> {
    let Some(prepared) = deck.prepared.get(index) else {
        return Space::new()
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into();
    };
    let layer = |part| -> Element<'a, M, Theme, Renderer> {
        canvas(SlideLayer::new(deck.clone(), index, part))
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into()
    };
    let mut parts = Vec::with_capacity(prepared.layers.len() + 1);
    if prepared.background.is_some() {
        parts.push(layer(SlidePart::Background));
    }
    parts.extend((0..prepared.layers.len()).map(|i| layer(SlidePart::Layer(i))));
    container(
        stack(parts)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height)),
    )
    .clip(true)
    .into()
}

fn slide_list<'a>(
    deck: &Arc<LoadedDeck>,
    available_width: f32,
    zoom: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let metrics = slide_metrics(deck, available_width, zoom);
    let mut slides = column![]
        .spacing(SLIDE_SPACING)
        .padding(SLIDE_LIST_PADDING)
        .align_x(Alignment::Center);

    for (index, slide) in deck.slides.iter().enumerate() {
        let mut label = row![
            text(slide.number.to_string())
                .size(theme::TEXT_SM)
                .color(theme::TEXT_SECONDARY)
        ]
        .spacing(theme::SPACE_3)
        .align_y(Alignment::Center);
        if slide.hidden {
            label = label.push(text("Hidden").size(theme::TEXT_XS).color(theme::TEXT_MUTED));
        }
        if let Some(title) = &slide.title {
            label = label.push(
                text(title.clone())
                    .size(theme::TEXT_SM)
                    .color(theme::TEXT_MUTED)
                    .wrapping(text::Wrapping::None),
            );
        }

        let page = container(slide_page(deck, index, metrics.width, metrics.height))
            .padding(SLIDE_BORDER)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BORDER_SUBTLE)),
                ..Default::default()
            });

        slides = slides.push(column![
            container(label)
                .width(Length::Fixed(metrics.width + 2.0 * SLIDE_BORDER))
                .center_y(Length::Fixed(SLIDE_LABEL_HEIGHT))
                .clip(true),
            page,
        ]);
    }

    // A slide at or below the fitted width scrolls only vertically, centred;
    // zoomed in past it, the column scrolls both ways.
    let fits = zoom <= 1.0;
    let (content, direction) = if fits {
        (
            container(slides).width(Length::Fill),
            scrollable::Direction::Vertical(scrollable::Scrollbar::default()),
        )
    } else {
        (
            container(slides),
            scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            },
        )
    };
    container(
        scrollable(content)
            .id(iced::advanced::widget::Id::new(PPTX_SCROLLABLE_ID))
            .direction(direction)
            .on_scroll(|viewport| Message::PptxScrolled {
                x: viewport.absolute_offset().x,
                y: viewport.absolute_offset().y,
                viewport_height: viewport.bounds().height,
            })
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_PRIMARY)),
        ..Default::default()
    })
    .into()
}

fn toolbar<'a>(
    pane: &PptxPane,
    deck: &LoadedDeck,
    available_width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let count = deck.slides.len();
    let current = pane.current_slide(available_width);
    let position = if count == 0 {
        "No slides".to_string()
    } else {
        format!("{} / {}", current + 1, count)
    };
    let zoom = if (pane.zoom - 1.0).abs() < 0.01 {
        "Fit".to_string()
    } else {
        format!("{:.0}%", pane.zoom * 100.0)
    };

    container(
        row![
            button(icons::view(Icon::ChevronUp, theme::TEXT_MUTED, 16.0))
                .on_press_maybe((current > 0).then_some(Message::PptxStep(-1)))
                .padding(theme::SPACE_3)
                .style(button::text),
            text(position)
                .size(theme::TEXT_SM)
                .color(theme::TEXT_SECONDARY),
            button(icons::view(Icon::ChevronDown, theme::TEXT_MUTED, 16.0))
                .on_press_maybe((current + 1 < count).then_some(Message::PptxStep(1)))
                .padding(theme::SPACE_3)
                .style(button::text),
            Space::new().width(Length::Fill),
            button(text("-").size(theme::TEXT_MD))
                .on_press(Message::PptxZoomOut)
                .padding([theme::SPACE_2, theme::SPACE_4])
                .style(button::text),
            text(zoom).size(theme::TEXT_SM).color(theme::TEXT_MUTED),
            button(text("+").size(theme::TEXT_MD))
                .on_press(Message::PptxZoomIn)
                .padding([theme::SPACE_2, theme::SPACE_4])
                .style(button::text),
            button(text("Fit").size(theme::TEXT_SM))
                .on_press(Message::PptxZoomFit)
                .padding([theme::SPACE_2, theme::SPACE_4])
                .style(button::text),
        ]
        .spacing(theme::SPACE_4)
        .align_y(Alignment::Center)
        .padding([theme::SPACE_3, theme::SPACE_4]),
    )
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}
