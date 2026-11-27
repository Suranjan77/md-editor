//! Chrome for the handwriting surface: a single compact toolbar across the
//! top, and a floating colour palette over the bottom of the canvas.
//!
//! The palette floats rather than living in the toolbar because colour is the
//! setting reached for most often mid-page, and a pen should not have to
//! travel to the top of the screen for it.

use iced::widget::{Space, button, column, container, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, FOCUS_RING_WIDTH, RADIUS_FULL, RADIUS_LG, SPACE_1, SPACE_2, SPACE_3,
};
use crate::design::{TextRole, tokens};
use crate::ink::document::PaperStyle;
use crate::ink::{COLOR_PRESETS, EraserMode, HIGHLIGHTER_SIZE_PRESETS, SIZE_PRESETS, Tool};
use crate::messages::{InkMessage, Message};
use crate::ui::{
    BadgeTone, Button, Icon, IconButton, SegmentOption, TooltipPosition, badge, segmented,
    toolbar_frame, toolbar_separator, with_tooltip,
};

/// Everything the toolbar renders from. Grouped into a struct because passing
/// a dozen positional arguments invites silent mix-ups between the booleans.
pub struct ToolbarState<'a> {
    pub tool: Tool,
    pub eraser_mode: EraserMode,
    pub paper: PaperStyle,
    pub size: f32,
    pub zoom: f32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_strokes: bool,
    pub has_selection: bool,
    pub dirty: bool,
    pub path: Option<&'a str>,
    pub pen_available: bool,
}

/// An icon action that greys out and stops responding when `enabled` is false.
fn action<'a>(
    which: Icon,
    label: &'a str,
    shortcut: &'a str,
    message: Message,
    enabled: bool,
) -> Element<'a, Message, Theme, Renderer> {
    IconButton::new(which, label)
        .shortcut(shortcut)
        .on_press_maybe(enabled.then_some(message))
        .build()
}

/// The single top row: tools, widths, edit actions, zoom, paper, then the
/// page's name, unsaved state, Save and Close.
pub fn toolbar(state: ToolbarState<'_>) -> Element<'_, Message, Theme, Renderer> {
    let tool =
        |which: Icon, label: &'static str, value: Tool| -> Element<'_, Message, Theme, Renderer> {
            IconButton::new(which, label)
                .selected(state.tool == value)
                .on_press(Message::Ink(InkMessage::ToolSelected(value)))
                .build()
        };

    // The highlighter carries its own widths; a 3px highlighter is useless.
    let width_presets: &[(&str, f32)] = if state.tool == Tool::Highlighter {
        &HIGHLIGHTER_SIZE_PRESETS
    } else {
        &SIZE_PRESETS
    };
    let sizes = segmented(
        width_presets
            .iter()
            .map(|&(label, preset)| {
                SegmentOption::new(
                    label,
                    (state.size - preset).abs() < 0.01,
                    Message::Ink(InkMessage::SizeSelected(preset)),
                )
            })
            .collect(),
        ControlSize::Sm,
    );

    let papers = segmented(
        PaperStyle::ALL
            .iter()
            .map(|&(label, style)| {
                SegmentOption::new(
                    label,
                    state.paper == style,
                    Message::Ink(InkMessage::PaperSelected(style)),
                )
            })
            .collect(),
        ControlSize::Sm,
    );

    let mut items: Vec<Element<'_, Message, Theme, Renderer>> = vec![
        tool(Icon::Pen, "Pen", Tool::Pen),
        tool(Icon::Highlighter, "Highlighter", Tool::Highlighter),
        tool(Icon::Eraser, "Eraser", Tool::Eraser),
        tool(Icon::Lasso, "Lasso select", Tool::Lasso),
    ];
    // Eraser modes only appear while the eraser is active, so the toolbar does
    // not carry controls that currently do nothing.
    if state.tool == Tool::Eraser {
        items.push(segmented(
            vec![
                SegmentOption::new(
                    "Part",
                    state.eraser_mode == EraserMode::Partial,
                    Message::Ink(InkMessage::EraserModeSelected(EraserMode::Partial)),
                ),
                SegmentOption::new(
                    "Whole",
                    state.eraser_mode == EraserMode::Stroke,
                    Message::Ink(InkMessage::EraserModeSelected(EraserMode::Stroke)),
                ),
            ],
            ControlSize::Sm,
        ));
    }
    items.extend([
        toolbar_separator(),
        sizes,
        toolbar_separator(),
        action(
            Icon::Undo,
            "Undo",
            "Ctrl Z",
            Message::Ink(InkMessage::Undo),
            state.can_undo,
        ),
        action(
            Icon::Redo,
            "Redo",
            "Ctrl Y",
            Message::Ink(InkMessage::Redo),
            state.can_redo,
        ),
        action(
            Icon::Trash,
            "Delete selection",
            "Del",
            Message::Ink(InkMessage::DeleteSelection),
            state.has_selection,
        ),
        Button::ghost("Clear")
            .size(ControlSize::Sm)
            .on_press_maybe(state.has_strokes.then_some(Message::Ink(InkMessage::Clear)))
            .tooltip("Erase the whole page", None)
            .build(),
        toolbar_separator(),
        // Zoom lives in the toolbar because a pen has no scroll wheel.
        action(
            Icon::Minus,
            "Zoom out",
            "",
            Message::Ink(InkMessage::ZoomStep(-2.0)),
            state.zoom > 0.21,
        ),
        Button::ghost(format!("{:.0}%", state.zoom * 100.0))
            .size(ControlSize::Sm)
            .on_press(Message::Ink(InkMessage::ResetView))
            .tooltip("Reset to 100%", None)
            .build(),
        action(
            Icon::Plus,
            "Zoom in",
            "",
            Message::Ink(InkMessage::ZoomStep(2.0)),
            state.zoom < 7.9,
        ),
        toolbar_separator(),
        papers,
        Space::new().width(Length::Fill).into(),
    ]);

    items.extend(page_status(&state));

    toolbar_frame(items, false)
}

/// The right end: pen availability, the page's name, unsaved state, Save and Close.
fn page_status<'a>(state: &ToolbarState<'a>) -> Vec<Element<'a, Message, Theme, Renderer>> {
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if !state.pen_available {
        let why = if cfg!(windows) {
            "No pen input: enable Windows Ink for pressure and the eraser end"
        } else {
            "No pen input on this system: strokes are drawn with the mouse, without pressure"
        };
        items.push(with_tooltip(
            badge("Mouse only", BadgeTone::Warning, None),
            why,
            None,
            TooltipPosition::Bottom,
        ));
    }
    let role = TextRole::Caption;
    items.push(
        text(state.path.unwrap_or("Unsaved page").to_string())
            .size(role.size())
            .font(role.font())
            .color(tokens().fg_secondary)
            .into(),
    );
    if state.dirty {
        items.push(badge("Unsaved", BadgeTone::Warning, None));
    }
    items.push(
        Button::primary("Save")
            .size(ControlSize::Sm)
            .on_press(Message::Ink(InkMessage::Save))
            .tooltip("Save page", Some("Ctrl S"))
            .build(),
    );
    items.push(
        IconButton::new(Icon::X, "Close handwriting")
            .shortcut("Ctrl I")
            .on_press(Message::Ink(InkMessage::Toggle))
            .build(),
    );

    items
}

/// Width of the palette strip down the right-hand edge.
///
/// Reserved as real layout rather than floated over the canvas. Overlaying it
/// looked the same but was unusable with a pen: the canvas claims all pen
/// input inside its own rect, so presses on a panel drawn *on top* of it never
/// reached the buttons. Giving the strip its own column takes it out of that
/// rect entirely.
pub const PALETTE_WIDTH: f32 = 62.0;

/// Diameter of a colour swatch.
const SWATCH_SIZE: f32 = 28.0;

/// The colour palette, a vertical strip down the right-hand edge.
pub fn color_palette<'a>(active: Color) -> Element<'a, Message, Theme, Renderer> {
    let mut swatches = column![].spacing(SPACE_3).align_x(Alignment::Center);

    for (name, preset) in COLOR_PRESETS {
        let selected = crate::ink::same_color(active, preset);
        // A 28px swatch: a target a pen can hit without careful aim. The
        // active one carries the focus ring, so selection is not colour-only.
        let swatch = button(Space::new())
            .width(Length::Fixed(SWATCH_SIZE))
            .height(Length::Fixed(SWATCH_SIZE))
            .on_press(Message::Ink(InkMessage::ColorSelected(preset)))
            .style(move |_, _| {
                let t = tokens();
                button::Style {
                    background: Some(Background::Color(preset)),
                    border: Border {
                        radius: RADIUS_FULL.into(),
                        width: if selected {
                            FOCUS_RING_WIDTH
                        } else {
                            BORDER_WIDTH
                        },
                        color: if selected {
                            t.focus_ring
                        } else {
                            t.border_control
                        },
                    },
                    ..Default::default()
                }
            });
        swatches = swatches.push(with_tooltip(swatch, name, None, TooltipPosition::Left));
    }

    container(container(swatches.padding([SPACE_3, SPACE_2])).style(|_| {
        let t = tokens();
        container::Style {
            background: Some(Background::Color(t.bg_surface)),
            border: Border {
                color: t.border_default,
                width: BORDER_WIDTH,
                radius: RADIUS_LG.into(),
            },
            ..Default::default()
        }
    }))
    .width(Length::Fixed(PALETTE_WIDTH))
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .padding([SPACE_3, SPACE_1])
    .into()
}
