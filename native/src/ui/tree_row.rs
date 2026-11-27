//! `TreeRow`: folder and file rows for the vault sidebar with indent guides, single-line ellipsis + tooltip and right/middle click hooks (MASTER §6).

use iced::widget::text::Wrapping;
use iced::widget::{Space, button, container, mouse_area, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use super::list_row::ellipsize_single_line;
use super::tooltip::{TooltipPosition, with_tooltip};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{
    ControlSize, IconSize, RADIUS_MD, RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3, SPACE_PX,
};

pub struct TreeRowProps<'a, Message> {
    pub name: &'a str,
    pub full_path: &'a str,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub selected: bool,
    pub active_doc: bool,
    pub focused: bool,
    pub drag_over: bool,
    pub max_chars: usize,
    pub on_click: Message,
    pub on_middle_click: Option<Message>,
    pub on_right_click: Option<Message>,
    pub actions: Vec<Element<'a, Message, Theme, Renderer>>,
}

pub fn tree_row<'a, Message: Clone + 'a>(
    props: TreeRowProps<'a, Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let row_h = ControlSize::Md.height();
    let (display_name, truncated) = ellipsize_single_line(props.name, props.max_chars.max(12));

    let mut line: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();

    // Vertical indent guides (12px per nesting level).
    for _ in 0..props.depth {
        let guide_col = t.border_subtle;
        line.push(
            container(
                container(Space::new())
                    .width(Length::Fixed(SPACE_PX))
                    .height(Length::Fixed(row_h - SPACE_1))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(guide_col)),
                        ..Default::default()
                    }),
            )
            .width(Length::Fixed(SPACE_3))
            .center_x(Length::Fixed(SPACE_3))
            .into(),
        );
    }

    // Disclosure chevron for folders, or matching spacer for files.
    if props.is_dir {
        let chevron = if props.expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        line.push(icon(chevron, IconSize::Sm.px(), t.fg_tertiary));
    } else if props.active_doc {
        let bar_col = t.accent;
        line.push(
            container(
                container(Space::new())
                    .width(Length::Fixed(SPACE_1 * 0.75))
                    .height(Length::Fixed(IconSize::Sm.px()))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(bar_col)),
                        border: Border {
                            radius: RADIUS_SM.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            )
            .width(Length::Fixed(IconSize::Sm.px()))
            .center_x(Length::Fixed(IconSize::Sm.px()))
            .into(),
        );
    } else {
        line.push(Space::new().width(Length::Fixed(IconSize::Sm.px())).into());
    }

    let kind_icon = Icon::for_vault_path(props.full_path, props.is_dir, props.expanded);
    let icon_col = if props.active_doc || props.selected {
        t.accent
    } else if props.is_dir {
        t.fg_secondary
    } else {
        t.fg_tertiary
    };
    line.push(icon(kind_icon, IconSize::Md.px(), icon_col));

    let role = if props.active_doc || props.selected {
        TextRole::BodyStrong
    } else {
        TextRole::Body
    };
    line.push(
        text(display_name)
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .wrapping(Wrapping::None)
            .color(t.fg_primary)
            .into(),
    );
    line.push(Space::new().width(Length::Fill).into());

    let selected = props.selected;
    let active_doc = props.active_doc;
    let focused = props.focused;
    let drag_over = props.drag_over;

    let main_btn = button(
        row(line)
            .spacing(SPACE_1)
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .on_press(props.on_click)
    .width(Length::Fill)
    .height(Length::Fixed(row_h))
    .padding([SPACE_0, SPACE_2])
    .clip(true)
    .style(|_theme, _status| button::Style {
        background: None,
        ..Default::default()
    });

    let mut outer_items: Vec<Element<'a, Message, Theme, Renderer>> = vec![main_btn.into()];
    outer_items.extend(props.actions);

    let bg = if drag_over || selected {
        Some(t.bg_overlay)
    } else if active_doc {
        Some(t.accent_muted)
    } else {
        None
    };
    let rest_border = if drag_over { Some(t.accent) } else { None };
    let row_box = container(row(outer_items).spacing(SPACE_1).align_y(Alignment::Center))
        .width(Length::Fill)
        .height(Length::Fixed(row_h))
        .padding([SPACE_0, SPACE_1])
        .style(move |_| container::Style {
            background: bg.map(Background::Color),
            border: ring_border(focused, RADIUS_MD, rest_border),
            ..Default::default()
        });

    let mut area = mouse_area(row_box);
    if let Some(mid) = props.on_middle_click {
        area = area.on_middle_press(mid);
    }
    if let Some(right) = props.on_right_click {
        area = area.on_right_press(right);
    }

    let wrapped: Element<'a, Message, Theme, Renderer> = area.into();
    if truncated {
        with_tooltip(wrapped, props.full_path, None, TooltipPosition::Right)
    } else {
        wrapped
    }
}

pub const fn _unused_color() -> Color {
    Color::TRANSPARENT
}
