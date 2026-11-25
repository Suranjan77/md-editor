//! `Toast`: info, success, warning and persistent error notifications at Elevation 3 (MASTER §6).

use iced::widget::{container, row, text};
use iced::{Alignment, Background, Border, Element, Renderer, Theme};

use super::button::Button;
use super::icon::{Icon, icon};
use super::icon_button::IconButton;
use crate::design;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, Elevation, IconSize, RADIUS_LG, SPACE_2, SPACE_3,
};
use crate::design::{TextRole, fade};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl ToastKind {
    pub fn infer(message: &str) -> Self {
        let low = message.to_ascii_lowercase();
        if low.contains("fail")
            || low.contains("error")
            || low.contains("cannot")
            || low.contains("refused")
            || low.contains("read-only")
            || low.contains("corrupt")
            || low.contains("invalid")
        {
            Self::Error
        } else if low.contains("saved") || low.contains("added ") || low.contains("copied") {
            Self::Success
        } else if low.contains("warning")
            || low.contains("conflict")
            || low.contains("moved or deleted")
            || low.contains("large")
        {
            Self::Warning
        } else {
            Self::Info
        }
    }

    pub const fn persistent(self) -> bool {
        matches!(self, Self::Error)
    }
}

#[derive(Debug, Clone)]
pub struct ToastItem<Message> {
    pub text: String,
    pub kind: ToastKind,
    pub action: Option<(String, Message)>,
}

impl<Message> ToastItem<Message> {
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let kind = ToastKind::infer(&text);
        Self {
            text,
            kind,
            action: None,
        }
    }

    pub fn error_with_action(
        text: impl Into<String>,
        action_label: impl Into<String>,
        action_msg: Message,
    ) -> Self {
        Self {
            text: text.into(),
            kind: ToastKind::Error,
            action: Some((action_label.into(), action_msg)),
        }
    }
}

pub fn toast<'a, Message: Clone + 'a>(
    message: &str,
    kind: ToastKind,
    action: Option<(&str, Message)>,
    on_dismiss: Message,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let (which, accent_col) = match kind {
        ToastKind::Info => (Icon::Info, t.status_info),
        ToastKind::Success => (Icon::CheckCircle, t.status_success),
        ToastKind::Warning => (Icon::Warning, t.status_warning),
        ToastKind::Error => (Icon::WarningCircle, t.status_danger),
    };

    let role = TextRole::Body;
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        icon(which, IconSize::Md.px(), fade(accent_col, opacity)),
        text(message.to_string())
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .color(fade(t.fg_primary, opacity))
            .into(),
    ];

    if let Some((label, msg)) = action {
        items.push(
            Button::secondary(label)
                .size(ControlSize::Sm)
                .on_press(msg)
                .build(),
        );
    }

    items.push(
        IconButton::new(Icon::X, "Dismiss notification")
            .size(ControlSize::Sm)
            .on_press(on_dismiss)
            .build(),
    );

    let bg = fade(t.bg_overlay, opacity);
    let border_col = fade(
        if kind == ToastKind::Error {
            t.status_danger
        } else {
            t.border_default
        },
        opacity,
    );
    let mut shadow = Elevation::Toast.shadow();
    shadow.color = fade(shadow.color, opacity);

    container(row(items).spacing(SPACE_2).align_y(Alignment::Center))
        .padding([SPACE_2, SPACE_3])
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_col,
                width: BORDER_WIDTH,
                radius: RADIUS_LG.into(),
            },
            shadow,
            ..Default::default()
        })
        .into()
}
