//! Chrome for the handwriting surface: a single compact toolbar across the
//! top, and a floating colour palette over the bottom of the canvas.
//!
//! The palette floats rather than living in the toolbar because colour is the
//! setting reached for most often mid-page, and a pen should not have to
//! travel to the top of the screen for it.

use iced::widget::{Space, button, column, container, row, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use crate::ink::document::PaperStyle;
use crate::ink::{COLOR_PRESETS, EraserMode, HIGHLIGHTER_SIZE_PRESETS, SIZE_PRESETS, Tool};
use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

/// Icon glyph size. The surrounding padding gives roughly a 28px hit target,
/// which is about the floor for landing a pen nib without careful aim.
const ICON_SIZE: f32 = 18.0;
const ICON_PADDING: u16 = 5;

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

fn chip_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(Background::Color(if selected {
            theme::ACCENT
        } else {
            theme::BG_TERTIARY
        })),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Wrap a control in a tooltip, so an icon-only toolbar stays legible.
fn hinted<'a>(
    control: impl Into<Element<'a, Message, Theme, Renderer>>,
    hint: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    tooltip(
        control,
        container(text(hint).size(11).color(theme::TEXT_SECONDARY))
            .padding([4, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_TERTIARY)),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
        tooltip::Position::Bottom,
    )
    .into()
}

/// An icon button that greys out and stops responding when `enabled` is false.
fn icon_action<'a>(
    icon: Icon,
    hint: &'a str,
    message: Message,
    enabled: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let mut control = button(icons::view(
        icon,
        if enabled {
            theme::TEXT_SECONDARY
        } else {
            theme::TEXT_MUTED
        },
        ICON_SIZE,
    ))
    .padding(ICON_PADDING)
    .style(chip_style(false));

    if enabled {
        control = control.on_press(message);
    }
    hinted(control, hint)
}

/// A short text button, for actions with no clear glyph.
fn label_action<'a>(
    label: &'a str,
    message: Message,
    enabled: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let mut control = button(text(label).size(11).color(if enabled {
        theme::TEXT_SECONDARY
    } else {
        theme::TEXT_MUTED
    }))
    .padding([6, 9])
    .style(chip_style(false));

    if enabled {
        control = control.on_press(message);
    }
    control.into()
}

fn divider<'a>() -> Element<'a, Message, Theme, Renderer> {
    container(Space::new().width(Length::Fixed(1.0)).height(Length::Fixed(20.0)))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BORDER)),
            ..Default::default()
        })
        .into()
}

/// The single top row.
pub fn toolbar(state: ToolbarState<'_>) -> Element<'_, Message, Theme, Renderer> {
    let tool_button = |icon: Icon, hint: &'static str, value: Tool| {
        let selected = state.tool == value;
        hinted(
            button(icons::view(
                icon,
                if selected {
                    theme::BG_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
                ICON_SIZE,
            ))
            .on_press(Message::InkToolSelected(value))
            .padding(ICON_PADDING)
            .style(chip_style(selected)),
            hint,
        )
    };

    // The highlighter carries its own widths; a 3px highlighter is useless.
    let width_presets: &[(&str, f32)] = if state.tool == Tool::Highlighter {
        &HIGHLIGHTER_SIZE_PRESETS
    } else {
        &SIZE_PRESETS
    };

    let mut sizes = row![].spacing(3).align_y(Alignment::Center);
    for (label, preset) in width_presets.iter().copied() {
        let selected = (state.size - preset).abs() < 0.01;
        sizes = sizes.push(
            button(text(label).size(11).color(if selected {
                theme::BG_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            }))
            .on_press(Message::InkSizeSelected(preset))
            .padding([5, 7])
            .style(chip_style(selected)),
        );
    }

    // Eraser modes only appear while the eraser is active, so the toolbar does
    // not carry controls that currently do nothing.
    let eraser_modes: Element<'_, Message, Theme, Renderer> = if state.tool == Tool::Eraser {
        let mode_button = |label: &'static str, hint: &'static str, value: EraserMode| {
            let selected = state.eraser_mode == value;
            hinted(
                button(text(label).size(11).color(if selected {
                    theme::BG_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                }))
                .on_press(Message::InkEraserModeSelected(value))
                .padding([5, 7])
                .style(chip_style(selected)),
                hint,
            )
        };
        row![
            divider(),
            mode_button("Part", "Rub out part of a stroke", EraserMode::Partial),
            mode_button("Whole", "Take the whole stroke", EraserMode::Stroke),
        ]
        .spacing(3)
        .align_y(Alignment::Center)
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let mut papers = row![].spacing(3).align_y(Alignment::Center);
    for (label, style) in PaperStyle::ALL {
        let selected = state.paper == style;
        papers = papers.push(
            button(text(label).size(11).color(if selected {
                theme::BG_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            }))
            .on_press(Message::InkPaperSelected(style))
            .padding([5, 7])
            .style(chip_style(selected)),
        );
    }

    let status: Element<'_, Message, Theme, Renderer> = if state.pen_available {
        Space::new().width(Length::Fixed(0.0)).into()
    } else {
        text("Mouse only — enable Windows Ink")
            .size(11)
            .color(theme::WARNING)
            .into()
    };

    let label = match (state.path, state.dirty) {
        (Some(path), true) => format!("● {path}"),
        (Some(path), false) => path.to_string(),
        (None, _) => "Unsaved page".to_string(),
    };

    let content = row![
        tool_button(Icon::Pen, "Pen", Tool::Pen),
        tool_button(Icon::Highlighter, "Highlighter", Tool::Highlighter),
        tool_button(Icon::Eraser, "Eraser", Tool::Eraser),
        tool_button(Icon::Lasso, "Lasso select", Tool::Lasso),
        eraser_modes,
        divider(),
        sizes,
        divider(),
        icon_action(Icon::Undo, "Undo  Ctrl Z", Message::InkUndo, state.can_undo),
        icon_action(Icon::Redo, "Redo  Ctrl Y", Message::InkRedo, state.can_redo),
        icon_action(
            Icon::Trash,
            "Delete selection  Del",
            Message::InkDeleteSelection,
            state.has_selection
        ),
        label_action("Clear", Message::InkClear, state.has_strokes),
        divider(),
        // Zoom lives in the toolbar because a pen has no scroll wheel.
        label_action("−", Message::InkZoomStep(-2.0), state.zoom > 0.21),
        hinted(
            button(
                text(format!("{:.0}%", state.zoom * 100.0))
                    .size(11)
                    .color(theme::TEXT_SECONDARY)
            )
            .on_press(Message::InkResetView)
            .padding([6, 6])
            .style(chip_style(false)),
            "Reset to 100%",
        ),
        label_action("+", Message::InkZoomStep(2.0), state.zoom < 7.9),
        divider(),
        papers,
        Space::new().width(Length::Fill),
        status,
        text(label).size(11).color(theme::TEXT_MUTED),
        icon_action(Icon::File, "Save  Ctrl S", Message::InkSave, true),
        icon_action(Icon::X, "Close  Ctrl I", Message::InkToggle, true),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .padding([5, 10]);

    container(content)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_SECONDARY)),
            border: Border {
                color: theme::BORDER_SUBTLE,
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Width of the palette strip down the right-hand edge.
///
/// Reserved as real layout rather than floated over the canvas. Overlaying it
/// looked the same but was unusable with a pen: the canvas claims all pen
/// input inside its own rect, so presses on a panel drawn *on top* of it never
/// reached the buttons. Giving the strip its own column takes it out of that
/// rect entirely.
pub const PALETTE_WIDTH: f32 = 62.0;

/// The colour palette, a vertical strip down the right-hand edge.
pub fn color_palette<'a>(active: Color) -> Element<'a, Message, Theme, Renderer> {
    let mut swatches = column![].spacing(10).align_x(Alignment::Center);

    for (name, preset) in COLOR_PRESETS {
        let selected = crate::ink::same_color(active, preset);
        // The button is deliberately larger than the dot it shows: a pen needs
        // a target it can hit without careful aim.
        let dot = button(
            Space::new()
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0)),
        )
        .on_press(Message::InkColorSelected(preset))
        .padding(5)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(preset)),
            border: Border {
                radius: 15.0.into(),
                // A ring plus a gap reads as selection far better than a
                // hairline directly on the colour.
                width: if selected { 2.5 } else { 0.0 },
                color: theme::TEXT_PRIMARY,
            },
            ..Default::default()
        });

        swatches = swatches.push(hinted(dot, name));
    }

    container(
        container(swatches.padding([12, 8]))
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_SECONDARY)),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 24.0.into(),
                },
                ..Default::default()
            }),
    )
    .width(Length::Fixed(PALETTE_WIDTH))
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .padding([12, 8])
    .into()
}
