//! `Tabs` (underline section tabs) and `TabStrip` (document tabs with dirty dot, missing badge, drag reorder and overflow menu).

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use super::icon::{Icon, icon};
use super::icon_button::IconButton;
use super::list_row::ellipsize_single_line;
use super::tooltip::{TooltipPosition, with_tooltip};
use crate::design;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, FOCUS_RING_WIDTH, IconSize, RADIUS_LG, RADIUS_MD, RADIUS_SM,
    SPACE_0, SPACE_1, SPACE_2, SPACE_PX, TAB_STRIP_HEIGHT,
};
use crate::design::{TextRole, fade};
use crate::tabs::{Tab, TabId, TabKind};

// ── Underline Tabs (Tracker / Settings) ──────────────────────────────

pub struct UnderlineTab<Message> {
    pub label: String,
    pub selected: bool,
    pub on_select: Message,
}

pub fn underline_tabs<'a, Message: Clone + 'a>(
    tabs: Vec<UnderlineTab<Message>>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(tabs.len());

    for tab in tabs {
        let selected = tab.selected;
        let role = if selected {
            TextRole::BodyStrong
        } else {
            TextRole::Body
        };
        let fg = if selected {
            t.fg_primary
        } else {
            t.fg_secondary
        };
        let bar_col = if selected {
            t.accent
        } else {
            Color::TRANSPARENT
        };

        let underline = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(FOCUS_RING_WIDTH))
            .style(move |_| container::Style {
                background: Some(Background::Color(bar_col)),
                border: Border {
                    radius: RADIUS_SM.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

        items.push(
            button(
                column![
                    container(
                        text(tab.label)
                            .size(role.size())
                            .line_height(role.line_height())
                            .font(role.font())
                            .color(fg),
                    )
                    .padding([SPACE_1, SPACE_2]),
                    underline,
                ]
                .spacing(SPACE_0)
                .align_x(Alignment::Center),
            )
            .padding(SPACE_0)
            .on_press(tab.on_select)
            .style(|_theme, status| {
                let t = design::tokens();
                let bg = match status {
                    button::Status::Hovered => Some(t.bg_raised),
                    button::Status::Pressed => Some(t.bg_overlay),
                    _ => None,
                };
                button::Style {
                    background: bg.map(Background::Color),
                    border: Border {
                        radius: RADIUS_MD.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into(),
        );
    }

    row(items)
        .spacing(SPACE_1)
        .align_y(Alignment::Center)
        .into()
}

// ── Document TabStrip ───────────────────────────────────────────────

pub struct DocTabItem<'a> {
    pub tab: &'a Tab,
    pub label: String,
    pub dirty: bool,
    pub is_active: bool,
    pub is_split_secondary: bool,
}

pub struct TabStripEvents<Message> {
    pub on_activate: Box<dyn Fn(TabId) -> Message>,
    pub on_close: Box<dyn Fn(TabId) -> Message>,
    pub on_drag_start: Box<dyn Fn(TabId) -> Message>,
    pub on_drag_over: Box<dyn Fn(usize) -> Message>,
    pub on_drag_end: Message,
    /// Right-click on a tab (Close, Close others, Reveal, Copy path…).
    pub on_context_menu: Box<dyn Fn(TabId) -> Message>,
    pub on_new_tab: Message,
    pub on_toggle_overflow: Message,
}

pub fn tab_strip<'a, Message: Clone + 'a>(
    items: Vec<DocTabItem<'a>>,
    dragging: Option<TabId>,
    overflow_open: bool,
    events: TabStripEvents<Message>,
    actions: Vec<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let mut tab_cells: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(items.len());

    for (idx, item) in items.into_iter().enumerate() {
        tab_cells.push(doc_tab(idx, item, dragging, &events));
    }

    let strip_scroll = scrollable(row(tab_cells).spacing(SPACE_1).align_y(Alignment::End))
        .direction(Direction::Horizontal(
            Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(TAB_STRIP_HEIGHT));

    let mut trailing = row![
        IconButton::new(Icon::Plus, "New note")
            .shortcut("Ctrl+N")
            .size(ControlSize::Sm)
            .on_press(events.on_new_tab),
        IconButton::new(Icon::ChevronDown, "All open tabs")
            .size(ControlSize::Sm)
            .selected(overflow_open)
            .on_press(events.on_toggle_overflow),
    ]
    .spacing(SPACE_1)
    .align_y(Alignment::Center);
    if !actions.is_empty() {
        trailing = trailing.push(super::toolbar::toolbar_separator());
        for action in actions {
            trailing = trailing.push(action);
        }
    }

    // Folio: the strip sits on the chrome ground; the active tab shares the sheet's
    // colour below it, so the two read as one surface.
    let bg = t.bg_surface;
    container(
        row![strip_scroll, container(trailing).padding([0.0, SPACE_2])].align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(TAB_STRIP_HEIGHT))
    .style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        ..Default::default()
    })
    .into()
}

/// The icon, label and dirty/missing marker inside one tab.
fn tab_label<'a, Message: 'a>(item: &DocTabItem<'a>) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let is_active = item.is_active;
    let is_split = item.is_split_secondary;
    let missing = item.tab.missing;
    let kind_icon = match item.tab.kind {
        TabKind::Note => Icon::FileText,
        TabKind::Pdf => Icon::FilePdf,
        TabKind::Image => Icon::Image,
    };
    let icon_col = if missing {
        t.status_warning
    } else if is_active || is_split {
        t.accent
    } else {
        t.fg_tertiary
    };
    let fg = if missing {
        fade(t.fg_tertiary, 0.75)
    } else if is_active {
        t.fg_primary
    } else {
        t.fg_secondary
    };

    let (short_label, _) = ellipsize_single_line(&item.label, 24);
    let role = if is_active {
        TextRole::BodyStrong
    } else {
        TextRole::Body
    };

    let mut content_row: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        icon(kind_icon, IconSize::Sm.px(), icon_col),
        text(short_label)
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .wrapping(Wrapping::None)
            .color(fg)
            .into(),
    ];

    if item.dirty {
        let dot_col = t.status_warning;
        content_row.push(
            container(Space::new())
                .width(Length::Fixed(SPACE_2 - SPACE_PX * 2.0))
                .height(Length::Fixed(SPACE_2 - SPACE_PX * 2.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(dot_col)),
                    border: Border {
                        radius: RADIUS_SM.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
        );
    } else if missing {
        content_row.push(icon(Icon::Warning, IconSize::Sm.px(), t.status_warning));
    }

    row(content_row)
        .spacing(SPACE_1)
        .align_y(Alignment::Center)
        .into()
}

/// One tab: label button, close button, active indicator and drag handling.
fn doc_tab<'a, Message: Clone + 'a>(
    idx: usize,
    item: DocTabItem<'a>,
    dragging: Option<TabId>,
    events: &TabStripEvents<Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let id = item.tab.id;
    let is_active = item.is_active;
    let is_split = item.is_split_secondary;
    let missing = item.tab.missing;
    let is_dragging = dragging == Some(id);
    let drag_active = dragging.is_some();

    let activate_msg = (events.on_activate)(id);
    let close_msg = (events.on_close)(id);

    let select_btn = button(tab_label(&item))
        .on_press(activate_msg)
        .padding([SPACE_0, SPACE_2])
        .height(Length::Fixed(ControlSize::Md.height()))
        .style(|_theme, _status| button::Style {
            background: None,
            ..Default::default()
        });

    let close_btn = IconButton::new(Icon::X, "Close tab")
        .shortcut("Ctrl+W")
        .size(ControlSize::Sm)
        .on_press(close_msg.clone());

    let tab_bg = if is_active {
        t.bg_canvas
    } else if is_split {
        t.accent_muted
    } else {
        Color::TRANSPARENT
    };
    // A border would draw a line between the active tab and the sheet, so only a
    // dragged tab gets one.
    let edge = if is_dragging {
        t.accent
    } else {
        Color::TRANSPARENT
    };
    let tab_height = TAB_STRIP_HEIGHT - SPACE_1;

    let chip = container(
        row![select_btn, close_btn]
            .spacing(SPACE_0)
            .align_y(Alignment::Center),
    )
    .padding([0.0, SPACE_1])
    .height(Length::Fixed(tab_height))
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(Background::Color(tab_bg)),
        border: Border {
            color: edge,
            width: if edge == Color::TRANSPARENT {
                0.0
            } else {
                BORDER_WIDTH
            },
            radius: iced::border::Radius::new(RADIUS_LG).bottom(0.0),
        },
        ..Default::default()
    });

    let mut area = mouse_area(chip)
        .on_middle_press(close_msg)
        .on_right_press((events.on_context_menu)(id))
        .on_press((events.on_drag_start)(id))
        .on_release(events.on_drag_end.clone());
    if drag_active && !is_dragging {
        area = area.on_enter((events.on_drag_over)(idx));
    }

    let tip_text = if missing {
        format!("{} (missing on disk)", item.tab.path)
    } else {
        item.tab.path.clone()
    };
    with_tooltip(area, &tip_text, None, TooltipPosition::Bottom)
}
