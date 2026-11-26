//! `SplitHandle`: draggable resize divider between panes (vertical, or horizontal for stacked panes) with double-click reset (MASTER §6).

use iced::widget::{Space, container, mouse_area};
use iced::{Background, Element, Length, Renderer, Theme};

use crate::design;
use crate::design::metrics::{SPACE_PX, SPLIT_HANDLE_THICKNESS};

pub fn split_handle<'a, Message: Clone + 'a>(
    dragging: bool,
    on_drag_start: Message,
    on_drag_end: Message,
    on_double_click_reset: Message,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    // Folio: the gutter between chrome and sheet already reads as a divider, so the
    // handle only shows itself while dragging.
    let line_col = if dragging {
        t.accent
    } else {
        iced::Color::TRANSPARENT
    };
    let bg_col = if dragging {
        t.accent_muted
    } else {
        iced::Color::TRANSPARENT
    };

    let line = container(Space::new())
        .width(Length::Fixed(if dragging {
            SPACE_PX * 2.0
        } else {
            SPACE_PX
        }))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(line_col)),
            ..Default::default()
        });

    let track = container(line)
        .width(Length::Fixed(SPLIT_HANDLE_THICKNESS))
        .height(Length::Fill)
        .center_x(Length::Fixed(SPLIT_HANDLE_THICKNESS))
        .style(move |_| container::Style {
            background: Some(Background::Color(bg_col)),
            ..Default::default()
        });

    mouse_area(track)
        .on_press(on_drag_start)
        .on_release(on_drag_end)
        .on_double_click(on_double_click_reset)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

/// [`split_handle`] lying across: the divider between stacked panes, dragged
/// up and down.
pub fn split_handle_across<'a, Message: Clone + 'a>(
    dragging: bool,
    on_drag_start: Message,
    on_drag_end: Message,
    on_double_click_reset: Message,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let (line_col, bg_col) = if dragging {
        (t.accent, t.accent_muted)
    } else {
        (iced::Color::TRANSPARENT, iced::Color::TRANSPARENT)
    };
    let line = container(Space::new())
        .height(Length::Fixed(if dragging {
            SPACE_PX * 2.0
        } else {
            SPACE_PX
        }))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(line_col)),
            ..Default::default()
        });
    let track = container(line)
        .height(Length::Fixed(SPLIT_HANDLE_THICKNESS))
        .width(Length::Fill)
        .center_y(Length::Fixed(SPLIT_HANDLE_THICKNESS))
        .style(move |_| container::Style {
            background: Some(Background::Color(bg_col)),
            ..Default::default()
        });
    mouse_area(track)
        .on_press(on_drag_start)
        .on_release(on_drag_end)
        .on_double_click(on_double_click_reset)
        .interaction(iced::mouse::Interaction::ResizingVertically)
        .into()
}
