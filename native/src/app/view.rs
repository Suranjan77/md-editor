//! Building the UI: decomposed regional builders for the shell window and detached handwriting window.

use super::*;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, Elevation, SHEET_GUTTER, SHEET_RADIUS, SPACE_0, SPACE_2, SPACE_5,
};
use crate::design::tokens;
use crate::messages::Shortcut;
use crate::messages::{
    ContextMenuTarget, EditorMessage, GraphMessage, Message, PdfMessage, ShellMessage, TabsMessage,
    VaultMessage,
};
use crate::shortcuts;
use crate::tabs::TabKind;
use crate::ui::{
    Button, ButtonVariant, DocTabItem, FocusRegion, Icon, IconButton, SaveStatus, StatusBarProps,
    TabStripEvents, empty_state, split_handle, status_bar, tab_strip,
};
use iced::widget::column;

impl MdEditor {
    pub fn view(&self, window: iced::window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        if Some(window) == self.ink_window {
            return self.ink_view();
        }
        self.shell_view()
    }

    pub(super) fn ink_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let t = tokens();
        let (live_options, live_color) = self.ink.live_style();

        column![
            views::ink::toolbar(views::ink::ToolbarState {
                tool: self.ink.tool,
                eraser_mode: self.ink.eraser_mode,
                paper: self.ink.document.paper,
                size: if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_size
                } else {
                    self.ink.options.size
                },
                zoom: self.ink.camera.zoom,
                can_undo: self.ink.document.can_undo(),
                can_redo: self.ink.document.can_redo(),
                has_strokes: !self.ink.document.is_empty(),
                has_selection: self.ink.has_selection(),
                dirty: self.ink.dirty,
                path: self.ink.path.as_deref(),
                pen_available: crate::ink::pen::is_available(),
            }),
            row![
                container(crate::ink::canvas::view(crate::ink::canvas::InkSurface {
                    document: &self.ink.document,
                    live: self.ink.live_points(),
                    lasso: self.ink.lasso(),
                    selection: self.ink.selection(),
                    selection_bounds: self.ink.selection_bounds(),
                    cache: &self.ink.cache,
                    camera: self.ink.camera,
                    options: self.ink.options,
                    live_options,
                    color: live_color,
                    tool: self.ink.tool,
                    pen_erases: self.ink.pen_erases(),
                    eraser_radius: self.ink.eraser_screen_radius(),
                    drawing: self.ink.is_drawing(),
                    hover: self.ink.hover(),
                    radial: self.ink.radial_menu(),
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.bg_canvas)),
                    ..Default::default()
                }),
                views::ink::color_palette(if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_color
                } else {
                    self.ink.color
                }),
            ]
            .height(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    pub(super) fn shell_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.vault.root.is_none() {
            return views::welcome::view(&self.ui.recent_vaults);
        }

        let t = tokens();
        let rail = self.build_rail();
        let tab_bar = self.build_tab_strip();
        let sidebar = self.build_sidebar();
        let main_content = self.build_main_pane();
        let right_panels = self.build_right_panels();
        let bottom_bar = self.build_status_bar();

        // Folio shell: the document lives on a rounded "sheet" that floats on the chrome
        // ground; tabs sit on the chrome above it, the active one sharing its colour.
        let sheet = container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_canvas)),
                border: iced::Border {
                    color: t.border_subtle,
                    width: BORDER_WIDTH,
                    radius: SHEET_RADIUS.into(),
                },
                shadow: Elevation::Sheet.shadow(),
                ..Default::default()
            });

        let has_tabs = !self.tabs.is_empty() && !self.graph.visible && !self.ui.settings_visible;
        let center_column = container(
            column![tab_bar, sheet]
                .spacing(SPACE_0)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .padding(iced::Padding {
            top: if has_tabs { SPACE_0 } else { SHEET_GUTTER },
            right: if self.right_panels_open() {
                SPACE_0
            } else {
                SHEET_GUTTER
            },
            bottom: SPACE_0,
            left: SPACE_0,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let body_row = row![rail, sidebar, center_column, right_panels]
            .spacing(SPACE_0)
            .height(Length::Fill);

        let shell_frame = container(
            column![body_row, bottom_bar]
                .spacing(SPACE_0)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.bg_surface)),
            ..Default::default()
        });

        let mut layers = vec![shell_frame.into()];
        self.append_overlays(&mut layers);
        crate::ui::context_anchor(stack(layers), super::overlays::anchor_context_menu).into()
    }

    fn build_rail(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        views::rail::view(views::rail::RailProps {
            sidebar_visible: self.vault.sidebar_visible,
            graph_visible: self.graph.visible,
            tracker_visible: self.tracker.visible,
            settings_visible: self.ui.settings_visible,
            ink_visible: self.ink.is_visible(),
            region_focused: self.ui.focused_region == FocusRegion::Toolbar,
        })
    }

    fn right_panels_open(&self) -> bool {
        !self.graph.visible
            && !self.ui.settings_visible
            && (self.motion.toc_width() >= 1.0 || self.motion.backlinks_width() >= 1.0)
    }

    /// Document-scoped toggles at the trailing end of the tab strip.
    fn document_actions(&self) -> Vec<Element<'_, Message, Theme, iced::Renderer>> {
        let mut actions = Vec::new();
        let split_available = self.tabs.mru_of_kind(TabKind::Note).is_some()
            && self.tabs.mru_of_kind(TabKind::Pdf).is_some();
        if split_available {
            actions.push(
                IconButton::new(Icon::Split, "Split view")
                    .shortcut(shortcuts::chord(Shortcut::SplitView))
                    .size(ControlSize::Sm)
                    .selected(self.ui.split_view_active)
                    .on_press(Message::Shell(ShellMessage::SplitViewToggle))
                    .build(),
            );
        }
        if self.active_path.is_some() || self.pdf.active_path.is_some() {
            actions.push(
                IconButton::new(Icon::ListTree, "Outline")
                    .shortcut(shortcuts::chord(Shortcut::TableOfContents))
                    .size(ControlSize::Sm)
                    .selected(self.editor.toc_visible)
                    .on_press(Message::Editor(EditorMessage::ToggleTOC))
                    .build(),
            );
        }
        actions.push(
            IconButton::new(Icon::Link, "Backlinks")
                .shortcut(shortcuts::chord(Shortcut::ToggleBacklinks))
                .size(ControlSize::Sm)
                .selected(self.vault.backlinks_visible)
                .on_press(Message::KeyboardShortcut(Shortcut::ToggleBacklinks))
                .build(),
        );
        actions
    }

    fn build_tab_strip(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.tabs.is_empty() || self.graph.visible || self.ui.settings_visible {
            return container(Space::new())
                .height(Length::Fixed(SPACE_0))
                .into();
        }
        let active_id = self.tabs.active_id();
        let split_note = self
            .ui
            .split_view_active
            .then(|| self.tabs.mru_of_kind(TabKind::Note).map(|t| t.id))
            .flatten();
        let split_pdf = self
            .ui
            .split_view_active
            .then(|| self.tabs.mru_of_kind(TabKind::Pdf).map(|t| t.id))
            .flatten();

        let items: Vec<DocTabItem> = self
            .tabs
            .list()
            .iter()
            .map(|tab| {
                let is_active = active_id == Some(tab.id);
                DocTabItem {
                    tab,
                    label: self.tabs.label_for(tab),
                    dirty: tab.kind == TabKind::Note
                        && self.active_path.as_deref() == Some(tab.path.as_str())
                        && self.editor.buffer.dirty,
                    is_active,
                    is_split_secondary: !is_active
                        && (Some(tab.id) == split_note || Some(tab.id) == split_pdf),
                }
            })
            .collect();

        let cursor_pos = self.ui.cursor_pos;
        tab_strip(
            items,
            self.ui.dragging_tab,
            self.ui.tab_overflow_open,
            TabStripEvents {
                on_activate: Box::new(|id| Message::Tabs(TabsMessage::Activate(id))),
                on_close: Box::new(|id| Message::Tabs(TabsMessage::Close(id))),
                on_drag_start: Box::new(|id| Message::Tabs(TabsMessage::DragStart(id))),
                on_drag_over: Box::new(|idx| Message::Tabs(TabsMessage::DragOver(idx))),
                on_drag_end: Message::Tabs(TabsMessage::DragEnd),
                on_context_menu: Box::new(move |id| {
                    Message::Shell(ShellMessage::OpenContextMenu(
                        ContextMenuTarget::Tab(id),
                        cursor_pos.0,
                        cursor_pos.1,
                    ))
                }),
                on_new_tab: Message::Vault(VaultMessage::CreateFileDialog),
                on_toggle_overflow: Message::Tabs(TabsMessage::ToggleOverflowMenu),
            },
            self.document_actions(),
        )
    }

    fn build_sidebar(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let sidebar_width = self.motion.sidebar_width();
        if sidebar_width < 1.0 {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        }
        let panel_el = container(views::sidebar::view(views::sidebar::SidebarProps {
            entries: &self.vault.entries,
            selected_path: self.vault.selected_path.as_deref(),
            active_path: self
                .tabs
                .active_tab()
                .map(|t| t.path.as_str())
                .or(self.active_path.as_deref())
                .or(self.pdf.active_path.as_deref()),
            expanded_folders: &self.vault.expanded_folders,
            filter_query: &self.vault.filter_query,
            width: self.ui.sidebar_width,
            collapsed: false,
            cursor_pos: self.ui.cursor_pos,
            focused: self.ui.focused_region == FocusRegion::Sidebar,
        }))
        .width(Length::Fixed(sidebar_width))
        .clip(true);

        row![
            panel_el,
            split_handle(
                self.ui.resizing_panel == Some(crate::ui_state::ResizingPanel::Sidebar),
                Message::Shell(ShellMessage::SidebarResizeStart),
                Message::Shell(ShellMessage::PanelResizeEnd),
                Message::Shell(ShellMessage::ResetSidebarWidth),
            ),
        ]
        .spacing(SPACE_0)
        .height(Length::Fill)
        .into()
    }

    fn build_main_pane(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.ui.settings_visible {
            return views::settings::view(&self.ui, self.vault.root.as_deref());
        }
        if self.graph.visible {
            return views::graph::view(&self.graph);
        }
        if self.tracker.visible {
            return self.tracker.view();
        }
        if self.ui.split_view_active && self.active_path.is_some() && self.pdf.active_path.is_some()
        {
            let left_portion = (self.ui.split_ratio * 1000.0) as u16;
            let right_portion = ((1.0 - self.ui.split_ratio) * 1000.0) as u16;
            return row![
                container(self.build_editor_view()).width(Length::FillPortion(left_portion)),
                split_handle(
                    self.ui.is_resizing_split,
                    Message::Shell(ShellMessage::SplitViewDragStart),
                    Message::Shell(ShellMessage::SplitViewDragEnd),
                    Message::Shell(ShellMessage::SplitViewReset),
                ),
                container(self.build_pdf_view()).width(Length::FillPortion(right_portion)),
            ]
            .spacing(SPACE_0)
            .height(Length::Fill)
            .into();
        }
        if self.showing_pdf && self.pdf.active_path.is_some() {
            return self.build_pdf_view();
        }
        if self.active_image.is_some() {
            return self.build_image_view();
        }
        if self.active_path.is_some() {
            return self.build_editor_view();
        }

        // Empty tab state (all tabs closed or fresh vault before opening a file).
        empty_state(
            Icon::FileText,
            "No document open",
            "Select a file from the sidebar, open the Command Palette, or create a new note.",
            Some(
                row![
                    Button::new("New Note")
                        .variant(ButtonVariant::Primary)
                        .icon(Icon::FilePlus)
                        .on_press(Message::Vault(VaultMessage::CreateFileDialog))
                        .build(),
                    Button::new("Command Palette")
                        .variant(ButtonVariant::Secondary)
                        .icon(Icon::Command)
                        .on_press(Message::Shell(ShellMessage::CommandPaletteOpen))
                        .build(),
                    Button::new("Knowledge Graph")
                        .variant(ButtonVariant::Secondary)
                        .icon(Icon::Network)
                        .on_press(Message::Graph(GraphMessage::Toggle))
                        .build(),
                ]
                .spacing(SPACE_2)
                .into(),
            ),
        )
    }

    fn build_editor_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let editor_search_active = self.editor_search_is_active();
        let active_search_match = editor_search_active
            .then(|| self.search.active_match_position())
            .flatten();
        let editor_search_query = if editor_search_active {
            self.search.query.as_str()
        } else {
            ""
        };
        let cursor_pos = self.ui.cursor_pos;

        let editor_widget = mouse_area(
            container(
                crate::editor::renderer::Editor::new(
                    &self.editor.buffer,
                    &self.editor.highlight.lines,
                    &self.editor.image_cache,
                    &self.editor.math_cache,
                    |value| Message::Editor(EditorMessage::Command(value)),
                    |value| Message::Editor(EditorMessage::CommandNoScroll(value)),
                    |value| Message::Vault(VaultMessage::SidebarFileClicked(value)),
                    |value| Message::Editor(EditorMessage::CheckboxToggle(value)),
                )
                .search(
                    editor_search_query,
                    self.search.regex,
                    self.search.match_case,
                    active_search_match,
                )
                .scale_factor(self.ui.scale_factor)
                .on_paste(|| Message::Editor(EditorMessage::Paste))
                .reveal_caret(self.editor.reveal_request, |view| {
                    Message::Editor(EditorMessage::CaretView(view))
                })
                .layout_revision(self.editor.layout_revision())
                .completion_open(self.editor.completion.is_some()),
            )
            .padding(EDITOR_PADDING)
            .width(Length::Fill),
        )
        .on_right_press(Message::Editor(EditorMessage::OpenContextMenu(
            cursor_pos.0,
            cursor_pos.1,
        )));

        let editor_scroll = scrollable(editor_widget)
            .id(iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID))
            .on_scroll(|vp| {
                Message::Editor(EditorMessage::Scrolled {
                    y: vp.absolute_offset().y,
                    viewport_width: vp.bounds().width,
                    viewport_height: vp.bounds().height,
                })
            })
            .height(Length::Fill);
        let editor_scroll: Element<'_, Message, Theme, iced::Renderer> =
            match (&self.editor.completion, &self.editor.caret_view) {
                (Some(completion), Some(caret)) => iced::widget::stack![
                    editor_scroll,
                    views::completion::view(
                        completion,
                        caret,
                        EDITOR_PADDING,
                        self.editor.viewport_width,
                    )
                ]
                .height(Length::Fill)
                .into(),
                _ => editor_scroll.into(),
            };

        let file_bar: Element<'_, Message, Theme, iced::Renderer> = if editor_search_active {
            views::search::file_bar(views::search::FileBarProps {
                query: &self.search.query,
                replace: &self.search.replace,
                replace_visible: self.search.replace_visible,
                regex: self.search.regex,
                match_case: self.search.match_case,
                match_count: self.search.match_count(),
                active_match: self.search.match_index,
            })
        } else {
            container(Space::new())
                .height(Length::Fixed(SPACE_0))
                .width(Length::Fill)
                .into()
        };

        column![file_bar, editor_scroll]
            .spacing(SPACE_0)
            .height(Length::Fill)
            .into()
    }

    fn build_pdf_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.pdf.active_path.is_none() {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        }
        let pdf_search_active = self.pdf_search_is_active();
        let focused_ann = self.pdf.focused_annotation_id.as_ref().and_then(|ann_id| {
            self.pdf
                .annotations
                .values()
                .flatten()
                .find(|a| &a.id == ann_id)
        });
        let pdf_toolbar = views::pdf_viewer::toolbar(views::pdf_viewer::PdfToolbarProps {
            current_page: self.pdf.current_page,
            total_pages: self.pdf.total_pages,
            zoom: self.pdf.zoom,
            fit_to_width: self.pdf.fit_to_width,
            page_input: self.ui.pdf_page_input.as_deref(),
            zoom_input: self.ui.pdf_zoom_input.as_deref(),
            toc_visible: self.editor.toc_visible,
            has_selection: self.pdf.selection.is_some(),
            has_annotations: self.pdf.annotations.values().any(|v| !v.is_empty()),
            focused_annotation: focused_ann,
        });

        let pdf_pages = scrollable(views::pdf_viewer::view_continuous(
            views::pdf_viewer::ContinuousProps {
                pages: &self.pdf.pages,
                zoom: self.pdf.zoom,
                dimensions: &self.pdf.dimensions,
                page_sizes: &self.pdf.page_sizes,
                placeholder_page_size: self.pdf.placeholder_page_size,
                search_matches: if pdf_search_active
                    || self.search.visible
                    || self.search.file_visible
                {
                    &self.search.pdf_results
                } else {
                    &[]
                },
                search_by_page: &self.search.pdf_indices_by_page,
                active_search_index: self.search.match_index,
                page_texts: &self.pdf.page_text,
                annotations: &self.pdf.annotations,
                references: &self.pdf.references,
                active_selection: self.pdf.selection,
                focused_annotation_id: self.pdf.focused_annotation_id.as_deref(),
                load_error: self.pdf.load_error.as_deref(),
                scroll_y: self.pdf.scroll_y,
                viewport_height: self.pdf.viewport_height,
            },
        ))
        .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
        .on_scroll(|vp| {
            Message::Pdf(PdfMessage::Scrolled {
                y: vp.absolute_offset().y,
                viewport_height: vp.bounds().height,
            })
        })
        .height(Length::Fill);
        // Reports the pane's real size for fit-to-width (see `PdfPane::viewport_width`).
        let pdf_pages = iced::widget::sensor(pdf_pages)
            .on_show(|size| Message::Pdf(PdfMessage::ViewportMeasured(size)))
            .on_resize(|size| Message::Pdf(PdfMessage::ViewportMeasured(size)));

        let search_bar: Element<'_, Message, Theme, iced::Renderer> = if pdf_search_active {
            views::pdf_viewer::search_bar(
                &self.search.query,
                self.search.regex,
                self.search.match_case,
                self.search.loose,
                self.search.pdf_results.len(),
                self.search.match_index,
            )
        } else {
            container(Space::new())
                .height(Length::Fixed(SPACE_0))
                .width(Length::Fill)
                .into()
        };

        // Top-docked shared viewer toolbar -> optional find bar -> continuous page scroll.
        column![pdf_toolbar, search_bar, pdf_pages]
            .spacing(SPACE_0)
            .height(Length::Fill)
            .into()
    }

    fn build_image_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let Some((handle, width, height)) = &self.active_image else {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        };
        let t = tokens();
        let label = self.active_image_path.as_deref().unwrap_or("Image");
        let zoom = self.ui.image_zoom;
        let actual = Button::new("100%")
            .size(crate::design::metrics::ControlSize::Sm)
            .variant(if zoom == Some(1.0) {
                ButtonVariant::Secondary
            } else {
                ButtonVariant::Ghost
            })
            .on_press(Message::Shell(ShellMessage::ImageZoomActual))
            .tooltip("Actual size", None)
            .build();
        let top_bar = views::viewer_toolbar::view(views::viewer_toolbar::ViewerToolbar {
            outline: None,
            position: None,
            zoom: Some(views::viewer_toolbar::ZoomControls {
                percent: zoom.map_or(100, |z| (z * 100.0).round() as u32),
                draft: None,
                on_input: None,
                on_submit: None,
                zoom_out: Message::Shell(ShellMessage::ImageZoomOut),
                zoom_in: Message::Shell(ShellMessage::ImageZoomIn),
                fit_width: None,
                fit_page: Some((zoom.is_none(), Message::Shell(ShellMessage::ImageZoomFit))),
            }),
            find: None,
            context: vec![actual],
            title: Some(format!("{label}  ·  {width:.0} × {height:.0} px")),
        });

        let img_el: Element<'_, Message, Theme, iced::Renderer> = if let Some(scale) = zoom {
            scrollable(
                container(
                    iced::widget::image(handle.clone())
                        .width(Length::Fixed(width * scale))
                        .height(Length::Fixed(height * scale)),
                )
                .padding(SPACE_5)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
            )
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            container(
                iced::widget::image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(iced::ContentFit::Contain),
            )
            .padding(SPACE_5)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        column![
            top_bar,
            container(img_el)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.viewer_ground)),
                    ..Default::default()
                }),
        ]
        .spacing(SPACE_0)
        .height(Length::Fill)
        .into()
    }

    fn build_right_panels(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.graph.visible || self.ui.settings_visible {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        }
        let toc_width = self.motion.toc_width();
        let backlinks_width = self.motion.backlinks_width();
        if toc_width < 1.0 && backlinks_width < 1.0 {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        }

        let pdf_toc_active = self.pdf.active_path.is_some()
            && (self.showing_pdf
                || (self.ui.split_view_active && self.active_panel == ActivePanel::Pdf));

        let toc_el: Element<'_, Message, Theme, iced::Renderer> = if toc_width >= 1.0 {
            let (entries, is_synth, current_pos) = if pdf_toc_active {
                (
                    &self.pdf.toc_entries[..],
                    self.pdf.toc_is_synthetic,
                    self.pdf.current_page as usize,
                )
            } else {
                (
                    &self.editor.toc_entries[..],
                    false,
                    self.editor.buffer.cursor_line,
                )
            };
            container(views::toc::view(
                entries,
                is_synth,
                current_pos,
                self.ui.side_panel_width,
                self.ui.focused_region == FocusRegion::SidePanel,
            ))
            .width(Length::Fixed(toc_width))
            .clip(true)
            .into()
        } else {
            container(Space::new()).width(Length::Fixed(SPACE_0)).into()
        };

        let backlinks_el: Element<'_, Message, Theme, iced::Renderer> = if backlinks_width >= 1.0 {
            container(views::backlinks::view(
                &self.vault.backlinks,
                true,
                self.ui.side_panel_width,
                self.ui.focused_region == FocusRegion::SidePanel,
            ))
            .width(Length::Fixed(backlinks_width))
            .clip(true)
            .into()
        } else {
            container(Space::new()).width(Length::Fixed(SPACE_0)).into()
        };

        row![
            split_handle(
                self.ui.resizing_panel == Some(crate::ui_state::ResizingPanel::SidePanel),
                Message::Shell(ShellMessage::SidePanelResizeStart),
                Message::Shell(ShellMessage::PanelResizeEnd),
                Message::Shell(ShellMessage::ResetSidePanelWidth),
            ),
            backlinks_el,
            toc_el,
        ]
        .spacing(SPACE_0)
        .height(Length::Fill)
        .into()
    }

    fn build_status_bar(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        // Hidden in focus mode (sidebar + side panels all hidden).
        let in_focus_mode = !self.vault.sidebar_visible
            && !self.vault.backlinks_visible
            && !self.editor.toc_visible
            && !self.tracker.visible
            && !self.graph.visible
            && !self.ui.settings_visible;
        if in_focus_mode {
            return container(Space::new())
                .height(Length::Fixed(SPACE_0))
                .into();
        }

        let note_active =
            self.active_path.is_some() && (!self.showing_pdf || self.ui.split_view_active);
        let save_status = if self.ui.last_save_failed {
            SaveStatus::Failed
        } else if self.editor.autosave_pending_since.is_some() {
            SaveStatus::Saving
        } else if self.editor.buffer.dirty {
            SaveStatus::Unsaved
        } else if note_active {
            SaveStatus::Saved
        } else {
            SaveStatus::Idle
        };

        let cursor_line_col = note_active.then(|| {
            let (l, c) = (
                self.editor.buffer.cursor_line,
                self.editor.buffer.cursor_col,
            );
            (l + 1, c + 1)
        });
        let word_count = note_active.then(|| self.editor.buffer.word_count());
        let selection_chars = note_active
            .then(|| {
                self.editor
                    .buffer
                    .selected_text()
                    .map(|s| s.chars().count())
            })
            .flatten();
        let vault_name = self
            .vault
            .root
            .as_deref()
            .and_then(|r| r.rsplit(['/', '\\']).find(|s| !s.is_empty()));

        status_bar(StatusBarProps {
            save_status,
            cursor_line_col,
            word_count,
            selection_chars,
            indexing: self.vault.indexing,
            leading: Some(views::breadcrumbs::view(
                vault_name,
                self.active_path
                    .as_deref()
                    .or(self.pdf.active_path.as_deref())
                    .or(self.active_image_path.as_deref()),
            )),
            focused: self.ui.focused_region == FocusRegion::StatusBar,
        })
    }
}
