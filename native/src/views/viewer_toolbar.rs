//! The shared viewer toolbar (`pages/viewers.md`) for PDFs and images: outline, editable page field, zoom field and steps, fit width/page, find, plus view-specific controls.

use iced::widget::{Space, container, row, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::design::metrics::{ControlSize, SPACE_1};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, Shortcut};
use crate::shortcuts;
use crate::ui::{Icon, IconButton, TextField, toolbar_frame, toolbar_separator};

/// Width of the page and zoom fields: four tabular digits and a little air.
const FIELD_WIDTH: f32 = 56.0;

/// An editable "page 3 / 12" (or slide) position.
pub struct PositionField<'a> {
    /// 1-based number shown when the field is not being edited.
    pub current: usize,
    pub total: usize,
    /// Text being typed, if the field is being edited.
    pub draft: Option<&'a str>,
    pub on_input: fn(String) -> Message,
    pub on_submit: Message,
    /// "Page" or "Slide", for the field's tooltip.
    pub noun: &'static str,
}

/// Zoom percentage field, steps and fit modes.
pub struct ZoomControls<'a> {
    pub percent: u32,
    pub draft: Option<&'a str>,
    pub on_input: Option<fn(String) -> Message>,
    pub on_submit: Option<Message>,
    pub zoom_out: Message,
    pub zoom_in: Message,
    /// Whether fit-to-width is on, and the message that turns it on.
    pub fit_width: Option<(bool, Message)>,
    /// Whether the whole page (or image) is fitted, and the message that fits it.
    pub fit_page: Option<(bool, Message)>,
}

pub struct ViewerToolbar<'a> {
    /// Outline panel visible, and the message toggling it.
    pub outline: Option<(bool, Message)>,
    pub position: Option<PositionField<'a>>,
    pub zoom: Option<ZoomControls<'a>>,
    /// Opens find-in-document.
    pub find: Option<Message>,
    /// Shown before the trailing group: selection and annotation tools.
    pub context: Vec<Element<'a, Message, Theme, Renderer>>,
    /// Label on the left when there is no outline button (an image's name).
    pub title: Option<String>,
}

fn caption(content: String) -> Element<'static, Message, Theme, Renderer> {
    let role = TextRole::MonoCaption;
    text(content)
        .size(role.size())
        .line_height(role.line_height())
        .font(role.font())
        .color(tokens().fg_secondary)
        .into()
}

fn small_field<'a>(
    value: &'a str,
    placeholder: String,
    on_input: fn(String) -> Message,
    on_submit: Message,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        TextField::new(placeholder, value)
            .monospace(true)
            .size(ControlSize::Sm)
            .on_input(on_input)
            .on_submit(on_submit)
            .build(),
    )
    .width(Length::Fixed(FIELD_WIDTH))
    .into()
}

pub fn view<'a>(bar: ViewerToolbar<'a>) -> Element<'a, Message, Theme, Renderer> {
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();

    if let Some((visible, toggle)) = bar.outline {
        items.push(
            IconButton::new(Icon::ListTree, "Outline")
                .shortcut(shortcuts::chord(Shortcut::TableOfContents))
                .selected(visible)
                .on_press(toggle)
                .build(),
        );
    }
    if let Some(title) = bar.title {
        let role = TextRole::Body;
        items.push(
            text(title)
                .size(role.size())
                .font(role.font())
                .color(tokens().fg_primary)
                .into(),
        );
    }

    if let Some(position) = bar.position {
        if !items.is_empty() {
            items.push(toolbar_separator());
        }
        let shown = position.current.to_string();
        let value = position.draft.unwrap_or("");
        let field = if position.total == 0 {
            caption("—".to_string())
        } else {
            row![
                small_field(value, shown, position.on_input, position.on_submit),
                caption(format!("/ {}", position.total)),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into()
        };
        items.push(crate::ui::with_tooltip(
            field,
            &format!("{} — type a number and press Enter", position.noun),
            None,
            crate::ui::TooltipPosition::Bottom,
        ));
    }

    items.push(Space::new().width(Length::Fill).into());
    items.extend(bar.context);
    items.push(Space::new().width(Length::Fill).into());

    if let Some(zoom) = bar.zoom {
        items.push(
            IconButton::new(Icon::Minus, "Zoom out")
                .on_press(zoom.zoom_out)
                .build(),
        );
        let percent = format!("{}%", zoom.percent);
        match (zoom.on_input, zoom.on_submit) {
            (Some(on_input), Some(on_submit)) => items.push(small_field(
                zoom.draft.unwrap_or(""),
                percent,
                on_input,
                on_submit,
            )),
            _ => items.push(caption(percent)),
        }
        items.push(
            IconButton::new(Icon::Plus, "Zoom in")
                .on_press(zoom.zoom_in)
                .build(),
        );
        if let Some((active, fit)) = zoom.fit_width {
            items.push(
                IconButton::new(Icon::FitWidth, "Fit width")
                    .selected(active)
                    .on_press(fit)
                    .build(),
            );
        }
        if let Some((active, fit)) = zoom.fit_page {
            items.push(
                IconButton::new(Icon::FitPage, "Fit page")
                    .selected(active)
                    .on_press(fit)
                    .build(),
            );
        }
    }

    if let Some(find) = bar.find {
        items.push(toolbar_separator());
        items.push(
            IconButton::new(Icon::Search, "Find in document")
                .shortcut(shortcuts::chord(Shortcut::Search))
                .on_press(find)
                .build(),
        );
    }

    toolbar_frame(items, false)
}
