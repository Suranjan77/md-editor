//! `StatusBar`: bottom workspace bar showing save state, cursor Ln/Col, word/selection counts, indexing and vault name (MASTER §6).

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Background, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{IconSize, SPACE_0, SPACE_1, SPACE_3, SPACE_PX, STATUS_BAR_HEIGHT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Saved,
    Saving,
    Unsaved,
    Failed,
    Idle,
}

pub struct StatusBarProps<'a> {
    pub save_status: SaveStatus,
    pub cursor_line_col: Option<(usize, usize)>,
    pub word_count: Option<usize>,
    pub selection_chars: Option<usize>,
    pub indexing: bool,
    pub vault_name: Option<&'a str>,
    pub focused: bool,
}

pub fn status_bar<'a, Message: 'a>(
    props: StatusBarProps<'a>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let role = TextRole::Caption;
    let mono = TextRole::MonoCaption;

    let mut left: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if let Some(vault) = props.vault_name.filter(|s| !s.is_empty()) {
        left.push(
            row![
                icon(Icon::Folder, IconSize::Sm.px(), t.fg_tertiary),
                text(vault.to_string())
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.fg_secondary),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        );
    }

    match props.save_status {
        SaveStatus::Saved => left.push(
            row![
                icon(Icon::Check, IconSize::Sm.px(), t.status_success),
                text("Saved")
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.fg_tertiary),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        ),
        SaveStatus::Saving => left.push(
            row![
                icon(Icon::Spinner, IconSize::Sm.px(), t.status_info),
                text("Saving…")
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.fg_secondary),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        ),
        SaveStatus::Unsaved => left.push(
            row![
                icon(Icon::Warning, IconSize::Sm.px(), t.status_warning),
                text("Unsaved changes")
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.status_warning),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        ),
        SaveStatus::Failed => left.push(
            row![
                icon(Icon::WarningCircle, IconSize::Sm.px(), t.status_danger),
                text("Save failed")
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.status_danger),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        ),
        SaveStatus::Idle => {}
    }

    if props.indexing {
        left.push(
            row![
                icon(Icon::Spinner, IconSize::Sm.px(), t.accent),
                text("Indexing vault…")
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.fg_secondary),
            ]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        );
    }

    let mut right: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if let Some(chars) = props.selection_chars.filter(|&c| c > 0) {
        right.push(
            text(format!("{chars} selected"))
                .size(mono.size())
                .line_height(mono.line_height())
                .font(mono.font())
                .color(t.accent)
                .into(),
        );
    }
    if let Some(words) = props.word_count {
        right.push(
            text(format!("{words} words"))
                .size(mono.size())
                .line_height(mono.line_height())
                .font(mono.font())
                .color(t.fg_tertiary)
                .into(),
        );
    }
    if let Some((ln, col)) = props.cursor_line_col {
        right.push(
            text(format!("Ln {}, Col {}", ln + 1, col + 1))
                .size(mono.size())
                .line_height(mono.line_height())
                .font(mono.font())
                .color(t.fg_secondary)
                .into(),
        );
    }

    let top_line = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(SPACE_PX))
        .style(move |_| container::Style {
            background: Some(Background::Color(t.border_subtle)),
            ..Default::default()
        });

    let bar = container(
        row![
            row(left).spacing(SPACE_3).align_y(Alignment::Center),
            Space::new().width(Length::Fill),
            row(right).spacing(SPACE_3).align_y(Alignment::Center),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(STATUS_BAR_HEIGHT))
    .padding([0.0, SPACE_3])
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.bg_surface)),
        border: ring_border(props.focused, 0.0, None),
        ..Default::default()
    });

    column![top_line, bar]
        .spacing(SPACE_0)
        .width(Length::Fill)
        .into()
}
