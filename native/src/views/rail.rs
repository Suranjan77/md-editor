//! Activity rail: the narrow icon column at the window's left edge holding navigation, workspaces and app controls.

use iced::widget::{Column, Space, column, container};
use iced::{Alignment, Background, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    ControlSize, RADIUS_SM, RAIL_WIDTH, SPACE_1, SPACE_2, SPACE_3, SPACE_PX,
};
use crate::design::{ActiveTheme, ThemeMode, active_theme, tokens};
use crate::messages::{
    DesksMessage, GraphMessage, InkMessage, Message, SearchMessage, SettingsMessage, ShellMessage,
    Shortcut, VaultMessage,
};
use crate::shortcuts;
use crate::ui::{Icon, IconButton, TooltipPosition, ring_border};

/// What the rail needs to mark its buttons as selected.
pub struct RailProps {
    pub sidebar_visible: bool,
    pub graph_visible: bool,
    pub tracker_visible: bool,
    pub settings_visible: bool,
    pub ink_visible: bool,
    pub desks_open: bool,
    /// The rail is the focused region (F6): draw the focus ring.
    pub region_focused: bool,
}

fn rail_button<'a>(
    which: Icon,
    label: &str,
    shortcut: Option<Shortcut>,
    selected: bool,
    msg: Message,
) -> Element<'a, Message, Theme, Renderer> {
    let mut b = IconButton::new(which, label)
        .size(ControlSize::Lg)
        .selected(selected)
        .tooltip_position(TooltipPosition::Right)
        .on_press(msg);
    if let Some(s) = shortcut {
        b = b.shortcut(shortcuts::chord(s));
    }
    b.build()
}

/// A short horizontal rule separating rail groups.
fn rail_divider<'a>() -> Element<'a, Message, Theme, Renderer> {
    let col = tokens().border_default;
    container(
        container(Space::new())
            .width(Length::Fixed(SPACE_3 + SPACE_2))
            .height(Length::Fixed(SPACE_PX))
            .style(move |_| container::Style {
                background: Some(Background::Color(col)),
                border: iced::Border {
                    radius: RADIUS_SM.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .padding([SPACE_1, 0.0])
    .into()
}

pub fn view<'a>(props: RailProps) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    let navigate = column![
        rail_button(
            Icon::LayoutPanelLeft,
            "Files",
            Some(Shortcut::ToggleSidebar),
            props.sidebar_visible,
            Message::Vault(VaultMessage::SidebarToggle),
        ),
        rail_button(
            Icon::Search,
            "Search vault",
            Some(Shortcut::GlobalSearch),
            false,
            Message::Search(SearchMessage::GlobalSearchOpen),
        ),
        rail_button(
            Icon::Command,
            "Command palette",
            Some(Shortcut::CommandPalette),
            false,
            Message::Shell(ShellMessage::CommandPaletteOpen),
        ),
    ]
    .spacing(SPACE_1)
    .align_x(Alignment::Center);

    let workspaces = column![
        rail_button(
            Icon::Network,
            "Knowledge graph",
            Some(Shortcut::KnowledgeGraph),
            props.graph_visible,
            Message::Graph(GraphMessage::Toggle),
        ),
        rail_button(
            Icon::Pen,
            "Handwriting canvas",
            Some(Shortcut::Handwriting),
            props.ink_visible,
            Message::Ink(InkMessage::Toggle),
        ),
        rail_button(
            Icon::Clock,
            "Study tracker",
            Some(Shortcut::StudyTracker),
            props.tracker_visible,
            Message::Shell(ShellMessage::TrackerToggle),
        ),
        rail_button(
            Icon::Desktop,
            "Desks",
            Some(Shortcut::SwitchDesk),
            props.desks_open,
            Message::Desks(DesksMessage::OpenDialog),
        ),
        rail_button(
            Icon::Target,
            "Daily desk",
            Some(Shortcut::DailyNote),
            false,
            Message::KeyboardShortcut(Shortcut::DailyNote),
        ),
    ]
    .spacing(SPACE_1)
    .align_x(Alignment::Center);

    let (theme_icon, theme_label, next_mode) = match active_theme() {
        ActiveTheme::Dark => (Icon::Sun, "Switch to light theme", ThemeMode::Light),
        ActiveTheme::Light => (Icon::Moon, "Switch to dark theme", ThemeMode::Dark),
    };
    let app = column![
        rail_button(
            theme_icon,
            theme_label,
            None,
            false,
            Message::Settings(SettingsMessage::ThemeModeSelected(next_mode)),
        ),
        rail_button(
            Icon::Gear,
            "Settings",
            Some(Shortcut::Settings),
            props.settings_visible,
            Message::Settings(SettingsMessage::Toggle),
        ),
    ]
    .spacing(SPACE_1)
    .align_x(Alignment::Center);

    let body: Column<'a, Message, Theme, Renderer> = column![
        navigate,
        rail_divider(),
        workspaces,
        Space::new().height(Length::Fill),
        app,
    ]
    .spacing(SPACE_2)
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fill);

    let focused = props.region_focused;
    container(body)
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fill)
        .padding([SPACE_2, 0.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(t.bg_surface)),
            border: ring_border(focused, 0.0, None),
            ..Default::default()
        })
        .into()
}
