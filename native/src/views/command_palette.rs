#![allow(dead_code)]

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::messages::{Message, Shortcut};
use crate::theme;
use crate::views::icons::{self, Icon};

pub(crate) const COMMAND_PALETTE_INPUT_ID: &str = "command_palette_input";

#[derive(Debug, Clone)]
pub(crate) struct Command {
    pub name: String,
    pub shortcut: Shortcut,
    pub icon: String,
    pub group_name: &'static str,
    pub shortcut_label: Option<String>,
    pub disabled_reason: Option<&'static str>,
}

pub(crate) fn insert_pdf_quote_command() -> Command {
    Command {
        name: "Insert PDF Quote".to_string(),
        shortcut: Shortcut::InsertPdfQuote,
        icon: "Q".to_string(), // we can keep icon field but we'll use shortcut mapping for visual
        group_name: "Research",
        shortcut_label: Some("Quote".to_string()),
        disabled_reason: None,
    }
}

pub(crate) fn insert_pdf_highlight_command() -> Command {
    Command {
        name: "Insert PDF Highlight".to_string(),
        shortcut: Shortcut::InsertPdfHighlight,
        icon: "H".to_string(),
        group_name: "Research",
        shortcut_label: Some("Cite".to_string()),
        disabled_reason: None,
    }
}

pub(crate) fn get_commands() -> Vec<Command> {
    crate::command_registry::get_command_registry()
        .into_iter()
        .map(|meta| Command {
            name: meta.name.to_string(),
            shortcut: meta.id,
            icon: meta.icon.to_string(),
            group_name: match meta.group {
                crate::app_shell::CommandGroup::File => "File",
                crate::app_shell::CommandGroup::Edit => "Edit",
                crate::app_shell::CommandGroup::Navigation => "Navigation",
                crate::app_shell::CommandGroup::View => "View",
                crate::app_shell::CommandGroup::Research => "Research",
                crate::app_shell::CommandGroup::Annotation => "Annotation",
                crate::app_shell::CommandGroup::Search => "Search",
            },
            shortcut_label: meta.default_shortcut.map(|s| s.to_string()),
            disabled_reason: None,
        })
        .collect()
}

fn command_button_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = button::text(theme, status);
        style.border.radius = theme::RADIUS_SMALL.into();

        if status == button::Status::Hovered || status == button::Status::Pressed {
            style.background = Some(Background::Color(theme::bg_tertiary()));
        }
        style
    }
}

fn focus_visible_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    if matches!(status, text_input::Status::Focused { .. }) {
        style.border.color = theme::accent();
        style.border.width = 2.0;
    }
    style
}

fn group_rank(group_name: &str) -> usize {
    match group_name {
        "File" => 0,
        "Edit" => 1,
        "Search" => 2,
        "Navigation" => 3,
        "View" => 4,
        "Research" => 5,
        "Annotation" => 6,
        _ => 7,
    }
}

fn palette_width(window_width: f32) -> f32 {
    let available_width = (window_width - 40.0).max(0.0);
    if available_width < 300.0 {
        available_width.max(160.0)
    } else {
        available_width.min(520.0)
    }
}

fn group_header(
    group_name: &'static str,
    show_divider: bool,
) -> Element<'static, Message, Theme, Renderer> {
    let label =
        container(text(group_name).size(11).color(theme::text_muted())).padding(iced::Padding {
            top: if show_divider { 10.0 } else { 8.0 },
            right: 12.0,
            bottom: 4.0,
            left: 12.0,
        });

    if !show_divider {
        return label.into();
    }

    column![
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_| container::Style {
                background: Some(Background::Color(theme::border_subtle())),
                ..Default::default()
            }),
        label,
    ]
    .into()
}

pub(crate) fn view<'a>(
    query: &str,
    commands: Vec<Command>,
    window_width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let input = text_input("Type a command...", query)
        .id(iced::advanced::widget::Id::new(COMMAND_PALETTE_INPUT_ID))
        .on_input(Message::CommandPaletteQueryChanged)
        .padding(12)
        .size(16)
        .style(focus_visible_input_style);

    let mut list = column![].spacing(5);

    let mut filtered = commands;
    if !query.is_empty() {
        let q = query.to_lowercase();
        filtered.retain(|c| c.name.to_lowercase().contains(&q));
        filtered.sort_by(|a, b| {
            let a_enabled = a.disabled_reason.is_none();
            let b_enabled = b.disabled_reason.is_none();
            if a_enabled != b_enabled {
                return b_enabled.cmp(&a_enabled);
            }
            let a_starts = a.name.to_lowercase().starts_with(&q);
            let b_starts = b.name.to_lowercase().starts_with(&q);
            if a_starts != b_starts {
                return b_starts.cmp(&a_starts);
            }
            a.name.cmp(&b.name)
        });
    } else {
        filtered.sort_by(|a, b| {
            let a_enabled = a.disabled_reason.is_none();
            let b_enabled = b.disabled_reason.is_none();
            if a_enabled != b_enabled {
                return b_enabled.cmp(&a_enabled);
            }
            let group_cmp = group_rank(a.group_name).cmp(&group_rank(b.group_name));
            if group_cmp != std::cmp::Ordering::Equal {
                return group_cmp;
            }
            a.name.cmp(&b.name)
        });
    }

    let mut last_group = "";
    let mut has_rendered_group = false;

    for cmd in filtered {
        if cmd.group_name != last_group {
            if query.is_empty() {
                list = list.push(group_header(cmd.group_name, has_rendered_group));
                has_rendered_group = true;
            }
            last_group = cmd.group_name;
        }

        let is_disabled = cmd.disabled_reason.is_some();
        let icon_widget = container(icons::view(
            command_icon(&cmd),
            if is_disabled {
                theme::text_muted()
            } else {
                theme::text_secondary()
            },
            14.0,
        ))
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0))
        .center_x(Length::Fixed(24.0))
        .center_y(Length::Fixed(24.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(theme::bg_tertiary())),
            border: Border {
                color: if is_disabled {
                    theme::border_subtle()
                } else {
                    theme::border()
                },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        let content = if let Some(reason) = cmd.disabled_reason {
            row![
                icon_widget,
                column![
                    text(cmd.name.clone()).size(14).color(theme::text_muted()),
                    text(reason).size(11).color(theme::danger())
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                text(cmd.group_name).size(10).color(theme::text_muted()),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding([8, 12])
        } else {
            row![
                icon_widget,
                text(cmd.name.clone()).size(14).color(theme::text_primary()),
                Space::new().width(Length::Fill),
                text(
                    cmd.shortcut_label
                        .clone()
                        .unwrap_or_else(|| shortcut_label(cmd.shortcut).to_string())
                )
                .size(11)
                .color(theme::text_muted()),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding([8, 12])
        };

        let btn = button(content).width(Length::Fill);
        let btn = if is_disabled {
            btn.style(button::text)
        } else {
            btn.on_press(Message::CommandPaletteCommandClicked(cmd.shortcut))
                .style(command_button_style())
        };
        list = list.push(btn);
    }

    let palette_width = palette_width(window_width);

    container(
        column![
            container(input).style(|_| container::Style {
                border: Border {
                    color: theme::border(),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
            scrollable(list).height(Length::Fixed(320.0)),
        ]
        .spacing(0),
    )
    .width(Length::Fixed(palette_width))
    .style(|_| container::Style {
        background: Some(Background::Color(theme::bg_secondary())),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn command_icon(command: &Command) -> Icon {
    match command.icon.as_str() {
        "/" => Icon::Search,
        "<" => Icon::ChevronLeft,
        ">" => Icon::ChevronRight,
        "|" => Icon::Split,
        "+" => Icon::File,
        "0" => Icon::Search,
        _ => shortcut_to_icon(command.shortcut),
    }
}

fn shortcut_to_icon(shortcut: Shortcut) -> Icon {
    match shortcut {
        Shortcut::Save => Icon::FileText,
        Shortcut::OpenVault => Icon::FolderOpen,
        Shortcut::NewFile => Icon::File,
        Shortcut::Search => Icon::Search,
        Shortcut::CommandPalette => Icon::Command,
        Shortcut::ToggleSidebar => Icon::LayoutPanelLeft,
        Shortcut::NavBack => Icon::ChevronLeft,
        Shortcut::NavForward => Icon::ChevronRight,
        Shortcut::ToggleBacklinks => Icon::ListTree,
        Shortcut::FocusMode => Icon::Split,
        Shortcut::TableOfContents => Icon::ListTree,
        Shortcut::StudyTracker => Icon::Clock,
        Shortcut::SplitView => Icon::Split,
        Shortcut::ZoomIn => Icon::Search,
        Shortcut::ZoomOut => Icon::Search,
        Shortcut::ZoomFit => Icon::Search,
        Shortcut::GoToPage => Icon::FileText,
        Shortcut::PdfSearch => Icon::Search,
        Shortcut::PdfHighlight => Icon::FileText,
        Shortcut::InsertPdfQuote => Icon::FileText,
        Shortcut::InsertPdfHighlight => Icon::FileText,
        Shortcut::PdfFirstPage => Icon::ChevronUp,
        Shortcut::PdfLastPage => Icon::ChevronDown,
        Shortcut::FollowCitation => Icon::FileText,
        Shortcut::ShowUsages => Icon::ListTree,
        Shortcut::CitationPalette => Icon::Command,
        Shortcut::ExcerptModeToggle => Icon::ListTree,
        Shortcut::ExcerptInsertBatch => Icon::File,
        Shortcut::ThemeDark => Icon::Command,
        Shortcut::ThemeLight => Icon::Command,
        Shortcut::ThemeHighContrast => Icon::Command,
        Shortcut::ToggleReducedMotion => Icon::Command,
        Shortcut::HelpAndShortcuts => Icon::Command,
        Shortcut::SwitchPane => Icon::Split,
        _ => Icon::Command,
    }
}

fn shortcut_label(shortcut: Shortcut) -> &'static str {
    match shortcut {
        Shortcut::Save => "Ctrl S",
        Shortcut::OpenVault => "Ctrl O",
        Shortcut::NewFile => "Ctrl N",
        Shortcut::Search => "Ctrl F",
        Shortcut::CommandPalette => "Ctrl P",
        Shortcut::ToggleSidebar => "Ctrl B",
        Shortcut::NavBack => "Alt Left",
        Shortcut::NavForward => "Alt Right",
        Shortcut::ToggleBacklinks => "Ctrl Alt B",
        Shortcut::FocusMode => "Focus",
        Shortcut::TableOfContents => "Ctrl T",
        Shortcut::StudyTracker => "Ctrl Alt S",
        Shortcut::SplitView => "Split",
        Shortcut::Escape => "Esc",
        Shortcut::ZoomIn => "Ctrl +",
        Shortcut::ZoomOut => "Ctrl -",
        Shortcut::ZoomFit => "Ctrl 0",
        Shortcut::GoToPage => "Ctrl G",
        Shortcut::PdfSearch => "Ctrl R",
        Shortcut::PdfHighlight => "Ctrl H",
        Shortcut::PdfUnderline => "Ctrl Shift H",
        Shortcut::PdfStrike => "Ctrl Alt H",
        Shortcut::PdfOpenCompanionNote => "Alt N",
        Shortcut::InsertPdfQuote => "Quote",
        Shortcut::InsertPdfHighlight => "Cite",
        Shortcut::PdfFirstPage => "Home",
        Shortcut::PdfLastPage => "End",
        Shortcut::PdfZoomInput => "Ctrl Z",
        Shortcut::FollowCitation => "Alt G",
        Shortcut::ShowUsages => "Alt U",
        Shortcut::CitationPalette => "Alt C",
        Shortcut::ExcerptModeToggle => "Alt E",
        Shortcut::ExcerptInsertBatch => "Alt I",
        Shortcut::Submit => "Enter",
        Shortcut::ThemeDark => "Dark Theme",
        Shortcut::ThemeLight => "Light Theme",
        Shortcut::ThemeHighContrast => "High Contrast",
        Shortcut::ToggleReducedMotion => "Reduced Motion",
        Shortcut::HelpAndShortcuts => "Help",
        Shortcut::SwitchPane => "Alt P",
        Shortcut::ToggleDiagnostics => "Ctrl Shift D",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_palette_pdf_quote_click_emits_shortcut() {
        let commands = vec![insert_pdf_quote_command()];
        let mut ui = iced_test::simulator(view("", commands, 1000.0));

        ui.click("Insert PDF Quote")
            .expect("PDF quote command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(
                Shortcut::InsertPdfQuote
            )]
        ));
    }

    #[test]
    fn command_palette_input_has_focusable_id() {
        let mut ui = iced_test::simulator(view("", get_commands(), 1000.0));

        ui.find(iced_test::selector::id(COMMAND_PALETTE_INPUT_ID))
            .expect("command palette input should expose deterministic focus id");
    }

    #[test]
    fn command_palette_input_focus_uses_visible_accent_ring() {
        let theme = Theme::Dark;
        let active = focus_visible_input_style(&theme, text_input::Status::Active);
        let focused =
            focus_visible_input_style(&theme, text_input::Status::Focused { is_hovered: false });

        assert_eq!(focused.border.color, theme::accent());
        assert_eq!(focused.border.width, 2.0);
        assert_ne!(focused.border, active.border);
    }

    #[test]
    fn command_palette_pdf_highlight_click_emits_shortcut() {
        let commands = vec![insert_pdf_highlight_command()];
        let mut ui = iced_test::simulator(view("", commands, 1000.0));

        ui.click("Insert PDF Highlight")
            .expect("PDF highlight command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(
                Shortcut::InsertPdfHighlight
            )]
        ));
    }

    #[test]
    fn command_palette_navigation_clicks_emit_cross_pane_shortcuts() {
        let commands = get_commands();
        let mut ui = iced_test::simulator(view("navigate", commands, 1000.0));

        ui.click("Navigate Back")
            .expect("navigation back command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(Shortcut::NavBack)]
        ));

        let commands = get_commands();
        let mut ui = iced_test::simulator(view("forward", commands, 1000.0));
        ui.click("Navigate Forward")
            .expect("navigation forward command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(Shortcut::NavForward)]
        ));
    }

    #[test]
    fn command_palette_renders_group_separator_when_query_empty() {
        let commands = vec![insert_pdf_quote_command()];
        let mut ui = iced_test::simulator(view("", commands, 1000.0));

        ui.find("Research")
            .expect("group separator should be visible when query is empty");
    }

    #[test]
    fn palette_width_stays_inside_narrow_windows() {
        assert_eq!(palette_width(200.0), 160.0);
        assert_eq!(palette_width(340.0), 300.0);
        assert_eq!(palette_width(900.0), 520.0);
    }

    #[test]
    fn command_groups_use_shell_order() {
        assert!(group_rank("File") < group_rank("Navigation"));
        assert!(group_rank("Navigation") < group_rank("View"));
        assert!(group_rank("View") < group_rank("Research"));
    }
}
