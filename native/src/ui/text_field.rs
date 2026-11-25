//! `TextField`: default and search inputs with >= 3:1 boundary, clear button and inline validation message (MASTER §6).

use iced::widget::{column, container, row, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use super::icon::{Icon, icon};
use super::icon_button::IconButton;
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, FOCUS_RING_WIDTH, IconSize, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3,
};

pub struct TextField<'a, Message> {
    id: Option<iced::widget::Id>,
    placeholder: String,
    value: &'a str,
    search: bool,
    monospace: bool,
    size: ControlSize,
    width: Length,
    error: Option<String>,
    on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_submit: Option<Message>,
    on_clear: Option<Message>,
}

impl<'a, Message: Clone + 'a> TextField<'a, Message> {
    pub fn new(placeholder: impl Into<String>, value: &'a str) -> Self {
        Self {
            id: None,
            placeholder: placeholder.into(),
            value,
            search: false,
            monospace: false,
            size: ControlSize::Md,
            width: Length::Fill,
            error: None,
            on_input: None,
            on_submit: None,
            on_clear: None,
        }
    }

    pub fn search(placeholder: impl Into<String>, value: &'a str) -> Self {
        let mut s = Self::new(placeholder, value);
        s.search = true;
        s
    }

    pub fn id(mut self, id: impl Into<iced::widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn monospace(mut self, mono: bool) -> Self {
        self.monospace = mono;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn error(mut self, message: Option<String>) -> Self {
        self.error = message.filter(|m| !m.is_empty());
        self
    }

    pub fn on_input(mut self, f: impl Fn(String) -> Message + 'a) -> Self {
        self.on_input = Some(Box::new(f));
        self
    }

    pub fn on_submit(mut self, msg: Message) -> Self {
        self.on_submit = Some(msg);
        self
    }

    pub fn on_clear(mut self, msg: Message) -> Self {
        self.on_clear = Some(msg);
        self
    }

    pub fn build(self) -> Element<'a, Message, Theme, Renderer> {
        let t = design::tokens();
        let has_error = self.error.is_some();
        let role = if self.monospace {
            TextRole::MonoBody
        } else {
            TextRole::Body
        };

        let mut input = text_input(&self.placeholder, self.value)
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .width(Length::Fill)
            .padding([SPACE_1, if self.search { SPACE_1 } else { SPACE_3 }])
            .style(move |_theme, status| {
                let t = design::tokens();
                if self.search {
                    text_input::Style {
                        background: Background::Color(Color::TRANSPARENT),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 0.0.into(),
                        },
                        icon: t.fg_tertiary,
                        placeholder: t.fg_tertiary,
                        value: t.fg_primary,
                        selection: t.doc_selection,
                    }
                } else {
                    let (border_col, border_w) = if has_error {
                        (t.status_danger, FOCUS_RING_WIDTH)
                    } else if matches!(status, text_input::Status::Focused { .. }) {
                        (t.focus_ring, FOCUS_RING_WIDTH)
                    } else {
                        (t.border_control, BORDER_WIDTH)
                    };
                    text_input::Style {
                        background: Background::Color(t.bg_canvas),
                        border: Border {
                            color: border_col,
                            width: border_w,
                            radius: RADIUS_MD.into(),
                        },
                        icon: t.fg_tertiary,
                        placeholder: t.fg_tertiary,
                        value: t.fg_primary,
                        selection: t.doc_selection,
                    }
                }
            });

        if let Some(id) = self.id {
            input = input.id(id);
        }
        if let Some(on_input) = self.on_input {
            input = input.on_input(on_input);
        }
        if let Some(on_submit) = self.on_submit {
            input = input.on_submit(on_submit);
        }

        let field_box: Element<'a, Message, Theme, Renderer> = if self.search {
            let mut inner: Vec<Element<'a, Message, Theme, Renderer>> = vec![
                icon(Icon::Search, IconSize::Md.px(), t.fg_tertiary),
                input.into(),
            ];
            if !self.value.is_empty()
                && let Some(clear_msg) = self.on_clear
            {
                inner.push(
                    IconButton::new(Icon::X, "Clear search")
                        .size(ControlSize::Sm)
                        .on_press(clear_msg)
                        .build(),
                );
            }
            let bg = t.bg_canvas;
            let border_col = if has_error {
                t.status_danger
            } else {
                t.border_control
            };
            container(row(inner).spacing(SPACE_1).align_y(Alignment::Center))
                .width(self.width)
                .height(Length::Fixed(self.size.height()))
                .padding([0.0, SPACE_2])
                .align_y(Alignment::Center)
                .style(move |_| container::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        color: border_col,
                        width: if has_error {
                            FOCUS_RING_WIDTH
                        } else {
                            BORDER_WIDTH
                        },
                        radius: RADIUS_MD.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else {
            container(input).width(self.width).into()
        };

        if let Some(err) = self.error {
            let cap = TextRole::Caption;
            column![
                field_box,
                row![
                    icon(Icon::WarningCircle, IconSize::Sm.px(), t.status_danger),
                    text(err)
                        .size(cap.size())
                        .line_height(cap.line_height())
                        .font(cap.font())
                        .color(t.status_danger),
                ]
                .spacing(SPACE_1)
                .align_y(Alignment::Center),
            ]
            .spacing(SPACE_1)
            .width(self.width)
            .into()
        } else {
            field_box
        }
    }
}

impl<'a, Message: Clone + 'a> From<TextField<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(field: TextField<'a, Message>) -> Self {
        field.build()
    }
}
