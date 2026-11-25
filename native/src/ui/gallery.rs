//! Component gallery tests: renders every component in Dark & Light at 100% and 130% interface scale (UI overhaul §2.2 Guardrail 3).

use iced::widget::{column, row};
use iced::{Element, Length, Renderer, Size, Theme};

use super::badge::{BadgeTone, badge, filter_chip};
use super::button::Button;
use super::card::{card, stat_card};
use super::dialog::dialog;
use super::empty_state::{empty_state, progress_bar};
use super::icon::{Icon, icon};
use super::icon_button::IconButton;
use super::kbd::kbd;
use super::list_row::{ListRowProps, list_row};
use super::menu::{MenuEntry, menu};
use super::panel::{PanelSide, panel};
use super::segmented::{SegmentOption, segmented};
use super::split_handle::split_handle;
use super::status_bar::{SaveStatus, StatusBarProps, status_bar};
use super::tabs::{DocTabItem, TabStripEvents, UnderlineTab, tab_strip, underline_tabs};
use super::text_field::TextField;
use super::toast::{ToastKind, toast};
use super::toggle::{CheckState, checkbox, toggle_switch};
use super::toolbar::{toolbar_frame, toolbar_separator};
use crate::design::metrics::ControlSize;
use crate::design::{
    ActiveTheme, active_theme, iced_theme, interface_scale_percent, set_active_theme,
    set_interface_scale_percent, tokens,
};
use crate::tabs::{Tab, TabKind, TabView};

#[derive(Debug, Clone)]
enum GalleryMsg {
    Noop,
    Input(String),
}

fn build_gallery_sheet<'a>(sample_tab: &'a Tab) -> Element<'a, GalleryMsg, Theme, Renderer> {
    let t = tokens();

    let buttons = row![
        Button::primary("Primary")
            .icon(Icon::Check)
            .on_press(GalleryMsg::Noop),
        Button::secondary("Secondary").on_press(GalleryMsg::Noop),
        Button::ghost("Ghost").on_press(GalleryMsg::Noop),
        Button::danger("Danger")
            .icon(Icon::Trash)
            .on_press(GalleryMsg::Noop),
        Button::primary("Disabled"),
        Button::secondary("Loading")
            .loading(true)
            .on_press(GalleryMsg::Noop),
        IconButton::new(Icon::Gear, "Settings")
            .shortcut("Ctrl+,")
            .on_press(GalleryMsg::Noop),
        IconButton::new(Icon::Pen, "Pen")
            .selected(true)
            .on_press(GalleryMsg::Noop),
    ]
    .spacing(8.0);

    let inputs = row![
        TextField::new("Note title", "Architecture").on_input(GalleryMsg::Input),
        TextField::search("Filter files…", "notes")
            .on_input(GalleryMsg::Input)
            .on_clear(GalleryMsg::Noop),
        TextField::new("Invalid name", "bad/name")
            .error(Some("File name cannot contain `/`".to_string()))
            .on_input(GalleryMsg::Input),
    ]
    .spacing(8.0);

    let controls = row![
        checkbox("Match case", CheckState::On, Some(GalleryMsg::Noop), false),
        checkbox("Mixed", CheckState::Mixed, Some(GalleryMsg::Noop), true),
        toggle_switch("Reduce motion", true, Some(GalleryMsg::Noop), false),
        segmented(
            vec![
                SegmentOption::new("System", true, GalleryMsg::Noop).icon(Icon::Desktop),
                SegmentOption::new("Dark", false, GalleryMsg::Noop).icon(Icon::Moon),
                SegmentOption::new("Light", false, GalleryMsg::Noop).icon(Icon::Sun),
            ],
            ControlSize::Md,
        ),
        kbd("Ctrl+Shift+P"),
    ]
    .spacing(12.0);

    let badges = row![
        badge("Saved", BadgeTone::Success, Some(Icon::Check)),
        badge("Unsaved", BadgeTone::Warning, Some(Icon::Warning)),
        badge("Failed", BadgeTone::Danger, Some(Icon::WarningCircle)),
        badge("Indexed", BadgeTone::Info, Some(Icon::Info)),
        filter_chip("PDFs", true, Some(Icon::FilePdf), GalleryMsg::Noop),
    ]
    .spacing(8.0);

    let strip = tab_strip(
        vec![DocTabItem {
            tab: sample_tab,
            label: "Index".to_string(),
            dirty: true,
            is_active: true,
            is_split_secondary: false,
        }],
        None,
        false,
        TabStripEvents {
            on_activate: Box::new(|_| GalleryMsg::Noop),
            on_close: Box::new(|_| GalleryMsg::Noop),
            on_drag_start: Box::new(|_| GalleryMsg::Noop),
            on_drag_over: Box::new(|_| GalleryMsg::Noop),
            on_drag_end: GalleryMsg::Noop,
            on_context_menu: Box::new(|_| GalleryMsg::Noop),
            on_new_tab: GalleryMsg::Noop,
            on_toggle_overflow: GalleryMsg::Noop,
        },
    );

    let tabs_and_bars = column![
        toolbar_frame(
            vec![
                IconButton::new(Icon::LayoutPanelLeft, "Sidebar")
                    .selected(true)
                    .on_press(GalleryMsg::Noop)
                    .into(),
                toolbar_separator(),
                underline_tabs(vec![
                    UnderlineTab {
                        label: "Dashboard".to_string(),
                        selected: true,
                        on_select: GalleryMsg::Noop,
                    },
                    UnderlineTab {
                        label: "Log".to_string(),
                        selected: false,
                        on_select: GalleryMsg::Noop,
                    },
                ]),
            ],
            false,
        ),
        strip,
    ];

    let rows_and_menus = row![
        panel(
            "Outline",
            Some("3"),
            vec![],
            column![
                list_row(ListRowProps {
                    title: "1. Very long section heading that truncates cleanly with ellipsis"
                        .into(),
                    subtitle: Some("p. 12".into()),
                    leading_icon: Some(Icon::FileText),
                    shortcut: Some("Enter"),
                    selected: true,
                    active_doc: true,
                    focused: false,
                    drag_over: false,
                    max_title_chars: 28,
                    on_press: Some(GalleryMsg::Noop),
                }),
                progress_bar(0.68, true),
            ]
            .spacing(8.0),
            PanelSide::Right,
            false,
        ),
        split_handle(false, GalleryMsg::Noop, GalleryMsg::Noop, GalleryMsg::Noop),
        menu(
            vec![
                MenuEntry::item("Rename", Some(GalleryMsg::Noop))
                    .with_icon(Icon::Pencil)
                    .with_shortcut("F2"),
                MenuEntry::Separator,
                MenuEntry::item("Delete", Some(GalleryMsg::Noop))
                    .with_icon(Icon::Trash)
                    .with_shortcut("Delete")
                    .danger(),
            ],
            200.0,
        ),
        column![
            stat_card(
                "This week",
                "12h 30m",
                Some("+2h vs last week"),
                Some(Icon::Clock)
            ),
            card(icon(Icon::Network, 20.0, t.accent)),
            toast(
                "Save failed: read-only file",
                ToastKind::Error,
                Some(("Retry", GalleryMsg::Noop)),
                GalleryMsg::Noop,
                1.0,
            ),
        ]
        .spacing(8.0),
    ]
    .spacing(12.0)
    .height(Length::Fixed(260.0));

    let bottom = status_bar(StatusBarProps {
        save_status: SaveStatus::Saved,
        cursor_line_col: Some((11, 4)),
        word_count: Some(420),
        selection_chars: Some(12),
        indexing: false,
        vault_name: Some("research-vault"),
        focused: false,
    });

    column![
        buttons,
        inputs,
        controls,
        badges,
        tabs_and_bars,
        rows_and_menus,
        bottom
    ]
    .spacing(10.0)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[test]
fn component_gallery_renders_in_dark_and_light_at_100_and_130_percent() {
    use iced_test::core::renderer::Headless as _;
    use iced_test::core::{Event, clipboard, renderer, window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};

    let _exclusive = crate::e2e::exclusive();
    crate::test_support::isolate_data_dir();
    crate::test_support::load_bundled_fonts();
    let (tx, _rx) = iced_test::futures::futures::channel::mpsc::channel(8);
    let program = crate::program(iced::window::Settings::default(), None);
    let _ = iced_test::Emulator::new(
        tx,
        &program,
        iced_test::emulator::Mode::Zen,
        Size::new(64.0, 64.0),
    );

    let prev_theme = active_theme();
    let prev_scale = interface_scale_percent();

    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("target/e2e");
    let _ = std::fs::create_dir_all(&out_dir);

    let sample_tab = Tab {
        id: 1,
        kind: TabKind::Note,
        path: "notes/Index.md".to_string(),
        view: TabView::Note,
        missing: false,
    };

    for (theme, theme_label) in [(ActiveTheme::Dark, "dark"), (ActiveTheme::Light, "light")] {
        for scale_pct in [90_u32, 100, 130] {
            set_active_theme(theme);
            set_interface_scale_percent(scale_pct);

            let mut headless = iced_test::futures::futures::executor::block_on(Renderer::new(
                crate::design::fonts::UI_FONT,
                iced::Pixels(13.0),
                None,
            ))
            .expect("create headless renderer");

            let size = Size::new(1280.0, 860.0);
            let shot_size = Size::new(1280, 860);
            let theme_val = iced_theme();
            let mut cache = Cache::default();

            for pass in 0..2 {
                let root: Element<'_, GalleryMsg, Theme, Renderer> = if pass == 0 {
                    dialog(
                        "Confirm delete",
                        empty_state(
                            Icon::FolderOpen,
                            "Empty folder",
                            "Drop files here or create a note.",
                            None,
                        ),
                        vec![Button::danger("Delete").on_press(GalleryMsg::Noop).build()],
                        GalleryMsg::Noop,
                        None,
                        1.0,
                    )
                } else {
                    build_gallery_sheet(&sample_tab)
                };
                let mut ui = UserInterface::build(root, size, cache, &mut headless);
                let _ = ui.update(
                    &[Event::Window(window::Event::RedrawRequested(
                        std::time::Instant::now(),
                    ))],
                    iced::mouse::Cursor::Unavailable,
                    &mut headless,
                    &mut clipboard::Null,
                    &mut Vec::new(),
                );
                ui.draw(
                    &mut headless,
                    &theme_val,
                    &renderer::Style {
                        text_color: tokens().fg_primary,
                    },
                    iced::mouse::Cursor::Unavailable,
                );
                cache = Cache::default();
                let bytes = headless.screenshot(shot_size, 1.0, tokens().bg_canvas);
                assert_eq!(bytes.len(), 1280 * 860 * 4);
                if pass == 1 && (scale_pct == 100 || scale_pct == 130) {
                    let png_path = out_dir.join(format!("gallery-{theme_label}-{scale_pct}.png"));
                    if let Some(img) = image::RgbaImage::from_raw(1280, 860, bytes) {
                        let _ = img.save(&png_path);
                    }
                }
            }
        }
    }

    set_active_theme(prev_theme);
    set_interface_scale_percent(prev_scale);
}
