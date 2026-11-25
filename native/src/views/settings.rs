//! Settings view (`Ctrl+,`): Appearance, Editor, Files, Keyboard shortcut table and About.

use iced::widget::{Column, Space, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    ControlSize, DIALOG_WIDTH_LG, SIDEBAR_MIN_WIDTH, SPACE_0, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
    SPACE_5,
};
use crate::design::motion::{ReducedMotionPref, reduced_motion_pref};
use crate::design::{
    ContentMeasure, ReadingFont, TextRole, ThemeMode, content_measure, editor_font_size,
    interface_scale_percent, reading_font, theme_mode, tokens,
};
use crate::messages::{AttachmentsMode, Message, SettingsMessage, SettingsSection};
use crate::shortcuts;
use crate::ui::{
    Button, ButtonVariant, Icon, IconButton, ListRowProps, TextField, card, kbd, list_row,
    segmented_choice,
};
use crate::ui_state::UiState;

pub fn view<'a>(
    ui: &'a UiState,
    vault_root: Option<&'a str>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    let header = container(
        row![
            text("Settings")
                .size(TextRole::Title.size())
                .color(t.fg_primary)
                .font(TextRole::Title.font()),
            Space::new().width(Length::Fill),
            IconButton::new(Icon::X, "Close settings")
                .shortcut("Esc")
                .on_press(Message::Settings(SettingsMessage::Close))
                .build(),
        ]
        .align_y(Alignment::Center),
    )
    .padding([SPACE_4, SPACE_5])
    .style(move |_| container::Style {
        border: Border {
            color: t.border_subtle,
            width: 1.0,
            ..Default::default()
        },
        ..Default::default()
    });

    let nav_items = [
        (SettingsSection::Appearance, Icon::Sun, "Appearance"),
        (SettingsSection::Editor, Icon::FileText, "Editor"),
        (SettingsSection::Files, Icon::Folder, "Files"),
        (SettingsSection::Keyboard, Icon::Command, "Keyboard"),
        (SettingsSection::About, Icon::Info, "About"),
    ]
    .into_iter()
    .map(|(sec, icon_name, label)| {
        list_row(ListRowProps {
            title: label.into(),
            subtitle: None,
            leading_icon: Some(icon_name),
            shortcut: None,
            selected: ui.settings_section == sec,
            active_doc: false,
            focused: false,
            drag_over: false,
            max_title_chars: 32,
            on_press: Some(Message::Settings(SettingsMessage::SelectSection(sec))),
        })
    });

    let left_nav = container(
        Column::with_children(nav_items)
            .spacing(SPACE_1)
            .padding(SPACE_3),
    )
    .width(Length::Fixed(SIDEBAR_MIN_WIDTH))
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.bg_surface)),
        border: Border {
            color: t.border_subtle,
            width: 1.0,
            ..Default::default()
        },
        ..Default::default()
    });

    let section_body = match ui.settings_section {
        SettingsSection::Appearance => appearance_section(),
        SettingsSection::Editor => editor_section(),
        SettingsSection::Files => files_section(ui),
        SettingsSection::Keyboard => keyboard_section(),
        SettingsSection::About => about_section(vault_root),
    };

    let detail = scrollable(
        container(
            container(section_body)
                .max_width(DIALOG_WIDTH_LG)
                .width(Length::Fill),
        )
        .padding(SPACE_5)
        .width(Length::Fill),
    )
    .height(Length::Fill)
    .width(Length::Fill);

    container(
        column![header, row![left_nav, detail].height(Length::Fill)]
            .spacing(SPACE_0)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.bg_canvas)),
        ..Default::default()
    })
    .into()
}

fn setting_row<'a>(
    title: &'static str,
    description: &'static str,
    control: Element<'a, Message, Theme, Renderer>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    card(
        row![
            column![
                text(title)
                    .size(TextRole::TitleSm.size())
                    .color(t.fg_primary)
                    .font(TextRole::TitleSm.font()),
                text(description)
                    .size(TextRole::Caption.size())
                    .color(t.fg_secondary)
                    .font(TextRole::Caption.font()),
            ]
            .spacing(SPACE_1)
            .width(Length::Fill),
            control,
        ]
        .spacing(SPACE_4)
        .align_y(Alignment::Center),
    )
}

fn appearance_section<'a>() -> Element<'a, Message, Theme, Renderer> {
    let cur_mode = theme_mode();
    let cur_scale = interface_scale_percent();
    let cur_motion = reduced_motion_pref();

    let theme_seg = segmented_choice(
        &[
            (ThemeMode::System, "System", None),
            (ThemeMode::Dark, "Dark", Some(Icon::Moon)),
            (ThemeMode::Light, "Light", Some(Icon::Sun)),
        ],
        cur_mode,
        |m| Message::Settings(SettingsMessage::ThemeModeSelected(m)),
        ControlSize::Md,
    );

    let scale_seg = segmented_choice(
        &[
            (90_u32, "90%", None),
            (100, "100%", None),
            (110, "110%", None),
            (120, "120%", None),
            (130, "130%", None),
        ],
        cur_scale,
        |pct| Message::Settings(SettingsMessage::InterfaceScaleSelected(pct)),
        ControlSize::Md,
    );

    let motion_seg = segmented_choice(
        &[
            (ReducedMotionPref::System, "System", None),
            (ReducedMotionPref::Off, "Standard", None),
            (ReducedMotionPref::On, "Reduced", None),
        ],
        cur_motion,
        |p| Message::Settings(SettingsMessage::ReducedMotionSelected(p)),
        ControlSize::Md,
    );

    column![
        setting_row(
            "Theme",
            "Follow OS light/dark preference or pin Dark or Light paper mode.",
            theme_seg,
        ),
        setting_row(
            "Interface scale",
            "Scale typography, controls and spacing proportionally across all panels.",
            scale_seg,
        ),
        setting_row(
            "Reduced motion",
            "Disable panel slides, scroll glides, shimmer and physics animations.",
            motion_seg,
        ),
    ]
    .spacing(SPACE_3)
    .into()
}

fn editor_section<'a>() -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let cur_font = reading_font();
    let cur_px = editor_font_size();
    let cur_measure = content_measure();

    let font_seg = segmented_choice(
        &[
            (ReadingFont::Sans, "Inter (Sans)", None),
            (ReadingFont::Serif, "Source Serif", None),
        ],
        cur_font,
        |f| Message::Settings(SettingsMessage::ReadingFontSelected(f)),
        ControlSize::Md,
    );

    let size_stepper = row![
        IconButton::new(Icon::Minus, "Smaller text")
            .on_press_maybe((cur_px > 14.0).then_some(Message::Settings(
                SettingsMessage::EditorFontSizeChanged((cur_px - 1.0).max(14.0),)
            )),)
            .build(),
        text(format!("{cur_px:.0} px"))
            .size(TextRole::MonoCaption.size())
            .color(t.fg_primary)
            .font(TextRole::MonoCaption.font()),
        IconButton::new(Icon::Plus, "Larger text")
            .on_press_maybe((cur_px < 22.0).then_some(Message::Settings(
                SettingsMessage::EditorFontSizeChanged((cur_px + 1.0).min(22.0),)
            )),)
            .build(),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);

    let measure_seg = segmented_choice(
        &[
            (ContentMeasure::Narrow, "Narrow (640)", None),
            (ContentMeasure::Default, "Default (720)", None),
            (ContentMeasure::Wide, "Wide (880)", None),
        ],
        cur_measure,
        |m| Message::Settings(SettingsMessage::ContentMeasureSelected(m)),
        ControlSize::Md,
    );

    column![
        setting_row(
            "Reading font",
            "Body typeface used for Markdown notes; code always uses JetBrains Mono.",
            font_seg,
        ),
        setting_row(
            "Font size",
            "Base body text size in the Markdown editor (14–22 px).",
            size_stepper.into(),
        ),
        setting_row(
            "Line width",
            "Maximum reading column measure (~68 characters at 720 px).",
            measure_seg,
        ),
    ]
    .spacing(SPACE_3)
    .into()
}

fn files_section<'a>(ui: &'a UiState) -> Element<'a, Message, Theme, Renderer> {
    let att_seg = segmented_choice(
        &[
            (AttachmentsMode::VaultRoot, "Vault root", None),
            (AttachmentsMode::BesideNote, "Beside note", None),
            (AttachmentsMode::Custom, "Custom folder", None),
        ],
        ui.attachments_mode,
        |m| Message::Settings(SettingsMessage::AttachmentsModeSelected(m)),
        ControlSize::Md,
    );

    let mut col = column![setting_row(
        "Attachments folder",
        "Where pasted screenshots and imported images are stored inside the vault.",
        att_seg,
    )]
    .spacing(SPACE_3);

    if ui.attachments_mode == AttachmentsMode::Custom {
        let custom_controls = row![
            container(
                TextField::new("attachments", &ui.attachments_custom)
                    .on_input(|v| Message::Settings(SettingsMessage::AttachmentsCustomChanged(v)))
                    .build(),
            )
            .width(Length::Fixed(SIDEBAR_MIN_WIDTH)),
            Button::new("Browse…")
                .variant(ButtonVariant::Secondary)
                .icon(Icon::FolderOpen)
                .on_press(Message::Settings(SettingsMessage::PickAttachmentsFolder))
                .build(),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center);

        col = col.push(setting_row(
            "Custom attachments path",
            "Vault-relative folder path for pasted and dropped images.",
            custom_controls.into(),
        ));
    }

    col = col.push(setting_row(
        "Excluded folders",
        "Comma-separated folder names the vault skips, besides node_modules, target, build, dist and dot-folders. Press Enter or Apply to re-index.",
        row![
            container(
                TextField::new("drafts, archive", &ui.excluded_folders)
                    .on_input(|v| Message::Settings(SettingsMessage::ExcludedFoldersChanged(v)))
                    .on_submit(Message::Settings(SettingsMessage::ExcludedFoldersApply))
                    .build(),
            )
            .width(Length::Fixed(SIDEBAR_MIN_WIDTH)),
            Button::new("Apply")
                .variant(ButtonVariant::Secondary)
                .on_press(Message::Settings(SettingsMessage::ExcludedFoldersApply))
                .build(),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into(),
    ));

    col.into()
}

fn keyboard_section<'a>() -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let rows = shortcuts::REGISTRY.iter().map(|spec| {
        let chord: Element<'a, Message, Theme, Renderer> = if spec.chord.is_empty() {
            text("Command palette")
                .size(TextRole::Caption.size())
                .color(t.fg_tertiary)
                .font(TextRole::Caption.font())
                .into()
        } else {
            kbd(spec.chord)
        };
        row![
            text(spec.category)
                .size(TextRole::Caption.size())
                .color(t.fg_tertiary)
                .font(TextRole::Caption.font())
                .width(Length::Fixed(SIDEBAR_MIN_WIDTH)),
            text(spec.label)
                .size(TextRole::Caption.size())
                .color(t.fg_primary)
                .font(TextRole::Caption.font())
                .width(Length::Fill),
            chord,
        ]
        .spacing(SPACE_3)
        .padding([SPACE_2, SPACE_3])
        .align_y(Alignment::Center)
        .into()
    });

    card(Column::with_children(rows).spacing(SPACE_1))
}

fn about_section<'a>(vault_root: Option<&'a str>) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let data_dir = md_editor_core::state::data_dir()
        .to_string_lossy()
        .into_owned();
    let log_dir = md_editor_core::state::log_dir()
        .to_string_lossy()
        .into_owned();

    card(
        column![
            text(format!("Md-editor v{}", env!("CARGO_PKG_VERSION")))
                .size(TextRole::Title.size())
                .color(t.fg_primary)
                .font(TextRole::Title.font()),
            text("Local-first research workspace for Markdown, PDFs, presentations, knowledge graphs and ink.")
                .size(TextRole::Caption.size())
                .color(t.fg_secondary)
                .font(TextRole::Caption.font()),
            text("License: GPL-3.0-or-later · Bundled fonts (Inter, JetBrains Mono, Source Serif Pro) under SIL OFL-1.1.")
                .size(TextRole::Caption.size())
                .color(t.fg_tertiary)
                .font(TextRole::Caption.font()),
            text(format!("Active vault: {}", vault_root.unwrap_or("None")))
                .size(TextRole::MonoCaption.size())
                .color(t.fg_secondary)
                .font(TextRole::MonoCaption.font()),
            text(format!("Data directory: {data_dir}"))
                .size(TextRole::MonoCaption.size())
                .color(t.fg_secondary)
                .font(TextRole::MonoCaption.font()),
            text(format!("Log directory: {log_dir}"))
                .size(TextRole::MonoCaption.size())
                .color(t.fg_secondary)
                .font(TextRole::MonoCaption.font()),
        ]
        .spacing(SPACE_2),
    )
}
