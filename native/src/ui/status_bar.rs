//! `StatusBar`: bottom workspace bar showing save state, cursor Ln/Col, word/selection counts, indexing and vault name (MASTER §6).

use iced::widget::{Space, container, row, text};
use iced::{Alignment, Background, Color, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{IconSize, SPACE_1, SPACE_3, SPACE_4, STATUS_BAR_HEIGHT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Saved,
    Saving,
    Unsaved,
    Failed,
    Idle,
}

pub struct StatusBarProps<'a, Message> {
    pub save_status: SaveStatus,
    pub cursor_line_col: Option<(usize, usize)>,
    pub word_count: Option<usize>,
    pub selection_chars: Option<usize>,
    pub indexing: bool,
    /// Leading content, usually the vault › folder › file breadcrumbs.
    pub leading: Option<Element<'a, Message, Theme, Renderer>>,
    pub focused: bool,
}

pub fn status_bar<'a, Message: 'a>(
    props: StatusBarProps<'a, Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();

    let mut left: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if let Some(leading) = props.leading {
        left.push(leading);
    }
    let save = match props.save_status {
        SaveStatus::Saved => Some((Icon::Check, t.status_success, "Saved", t.fg_tertiary)),
        SaveStatus::Saving => Some((Icon::Spinner, t.status_info, "Saving…", t.fg_secondary)),
        SaveStatus::Unsaved => Some((
            Icon::Warning,
            t.status_warning,
            "Unsaved changes",
            t.status_warning,
        )),
        SaveStatus::Failed => Some((
            Icon::WarningCircle,
            t.status_danger,
            "Save failed",
            t.status_danger,
        )),
        SaveStatus::Idle => None,
    };
    if let Some((glyph, glyph_color, label, label_color)) = save {
        left.push(segment(glyph, glyph_color, label.to_string(), label_color));
    }
    if props.indexing {
        left.push(segment(
            Icon::Spinner,
            t.accent,
            "Indexing vault…".to_string(),
            t.fg_secondary,
        ));
    }

    let mut right: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if let Some(chars) = props.selection_chars.filter(|&c| c > 0) {
        right.push(mono_label(format!("{chars} selected"), t.accent));
    }
    if let Some(words) = props.word_count {
        right.push(mono_label(format!("{words} words"), t.fg_tertiary));
    }
    if let Some((ln, col)) = props.cursor_line_col {
        right.push(mono_label(
            // Already 1-based: the shell converts from the buffer's 0-based caret.
            format!("Ln {ln}, Col {col}"),
            t.fg_secondary,
        ));
    }

    let bar = container(
        row![
            row(left).spacing(SPACE_4).align_y(Alignment::Center),
            Space::new().width(Length::Fill),
            row(right).spacing(SPACE_3).align_y(Alignment::Center),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(STATUS_BAR_HEIGHT))
    .padding([0.0, SPACE_3])
    .align_y(Alignment::Center)
    .clip(true)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.bg_surface)),
        border: ring_border(props.focused, 0.0, None),
        ..Default::default()
    });

    bar.into()
}

/// A left-side segment: small icon plus caption text.
fn segment<'a, Message: 'a>(
    glyph: Icon,
    glyph_color: Color,
    label: String,
    label_color: Color,
) -> Element<'a, Message, Theme, Renderer> {
    let role = TextRole::Caption;
    row![
        icon(glyph, IconSize::Sm.px(), glyph_color),
        text(label)
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .color(label_color),
    ]
    .spacing(SPACE_1)
    .align_y(Alignment::Center)
    .into()
}

/// A right-side counter in the monospace caption role.
fn mono_label<'a, Message: 'a>(
    label: String,
    color: Color,
) -> Element<'a, Message, Theme, Renderer> {
    let mono = TextRole::MonoCaption;
    text(label)
        .size(mono.size())
        .line_height(mono.line_height())
        .font(mono.font())
        .color(color)
        .into()
}
