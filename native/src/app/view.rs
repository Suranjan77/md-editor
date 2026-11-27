//! Building the UI: decomposed regional builders for the shell window and detached handwriting window.

use super::pane_view::{NO_PAGE_INDEX, PaneView};
use super::*;
use crate::design::metrics::{SHEET_GUTTER, SPACE_0, SPACE_5};
use crate::design::tokens;
use crate::messages::{EditorMessage, Message, PdfMessage, ShellMessage, VaultMessage};
use crate::ui::{
    Button, ButtonVariant, FocusRegion, SaveStatus, StatusBarProps, split_handle, status_bar,
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
        let sidebar = self.build_sidebar();
        let right_panels = self.build_right_panels();
        let bottom_bar = self.build_status_bar();
        let center_column = container(self.build_workspace())
            .padding(iced::Padding {
                top: SPACE_0,
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
            desks_open: matches!(self.ui.active_modal, Some(views::modals::ModalType::Desks)),
            region_focused: self.ui.focused_region == FocusRegion::Toolbar,
        })
    }

    fn right_panels_open(&self) -> bool {
        !self.graph.visible
            && !self.ui.settings_visible
            && (self.motion.toc_width() >= 1.0 || self.motion.backlinks_width() >= 1.0)
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

    pub(super) fn build_editor_view<'a>(
        &'a self,
        pane: &PaneView<'a>,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        let editor_search_active = pane.focused && self.editor_search_is_active();
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
                    &pane.editor.buffer,
                    &pane.editor.highlight.lines,
                    &pane.editor.image_cache,
                    &pane.editor.math_cache,
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
                .reveal_caret(pane.editor.reveal_request, |view| {
                    Message::Editor(EditorMessage::CaretView(view))
                })
                .layout_revision(pane.editor.layout_revision())
                .completion_open(pane.editor.completion.is_some())
                .id(pane.id.editor_widget_id()),
            )
            .padding(EDITOR_PADDING)
            .width(Length::Fill),
        )
        .on_right_press(Message::Editor(EditorMessage::OpenContextMenu(
            cursor_pos.0,
            cursor_pos.1,
        )));

        let editor_scroll = scrollable(editor_widget)
            .id(pane.id.editor_scrollable_id())
            .on_scroll(|vp| {
                Message::Editor(EditorMessage::Scrolled {
                    y: vp.absolute_offset().y,
                    viewport_width: vp.bounds().width,
                    viewport_height: vp.bounds().height,
                })
            })
            .height(Length::Fill);
        let editor_scroll: Element<'_, Message, Theme, iced::Renderer> =
            match (&pane.editor.completion, &pane.editor.caret_view) {
                (Some(completion), Some(caret)) => iced::widget::stack![
                    editor_scroll,
                    views::completion::view(
                        completion,
                        caret,
                        EDITOR_PADDING,
                        pane.editor.viewport_width,
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

    pub(super) fn build_pdf_view<'a>(
        &'a self,
        pane: &PaneView<'a>,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if pane.pdf.active_path.is_none() {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        }
        let pdf_search_active = pane.focused && self.pdf_search_is_active();
        let focused_ann = pane.pdf.focused_annotation_id.as_ref().and_then(|ann_id| {
            pane.pdf
                .annotations
                .values()
                .flatten()
                .find(|a| &a.id == ann_id)
        });
        let pdf_toolbar = views::pdf_viewer::toolbar(views::pdf_viewer::PdfToolbarProps {
            current_page: pane.pdf.current_page,
            total_pages: pane.pdf.total_pages,
            zoom: pane.pdf.zoom,
            fit_to_width: pane.pdf.fit_to_width,
            page_input: self.ui.pdf_page_input.as_deref().filter(|_| pane.focused),
            zoom_input: self.ui.pdf_zoom_input.as_deref().filter(|_| pane.focused),
            toc_visible: pane.focused && self.editor.toc_visible,
            has_selection: pane.pdf.selection.is_some(),
            has_annotations: pane
                .pdf
                .annotations
                .values()
                .flatten()
                .any(|a| a.kind != md_editor_core::pdf::PdfAnnotationKind::Ink),
            has_ink: pane
                .pdf
                .annotations
                .values()
                .flatten()
                .any(|a| a.kind == md_editor_core::pdf::PdfAnnotationKind::Ink),
            ink_tool: pane.pdf.ink_tool,
            ink_color: pane.pdf.ink_color,
            focused_annotation: focused_ann,
        });

        let pdf_pages = scrollable(views::pdf_viewer::view_continuous(
            views::pdf_viewer::ContinuousProps {
                pages: &pane.pdf.pages,
                zoom: pane.pdf.zoom,
                dimensions: &pane.pdf.dimensions,
                page_sizes: &pane.pdf.page_sizes,
                placeholder_page_size: pane.pdf.placeholder_page_size,
                search_matches: if pane.focused
                    && (pdf_search_active || self.search.visible || self.search.file_visible)
                {
                    &self.search.pdf_results
                } else {
                    &[]
                },
                search_by_page: if pane.focused {
                    &self.search.pdf_indices_by_page
                } else {
                    &NO_PAGE_INDEX
                },
                active_search_index: self.search.match_index.filter(|_| pane.focused),
                page_texts: &pane.pdf.page_text,
                annotations: &pane.pdf.annotations,
                references: &pane.pdf.references,
                active_selection: pane.pdf.selection,
                focused_annotation_id: pane.pdf.focused_annotation_id.as_deref(),
                ink_tool: pane.pdf.ink_tool,
                ink_color: pane.pdf.ink_color,
                live_ink: pane.pdf.live_ink.as_ref(),
                load_error: pane.pdf.load_error.as_deref(),
                scroll_y: pane.pdf.scroll_y,
                viewport_height: pane.pdf.viewport_height,
            },
        ))
        .id(pane.id.pdf_scrollable_id())
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

    pub(super) fn build_image_view<'a>(
        &'a self,
        pane: &PaneView<'a>,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        let Some((handle, width, height)) = pane.active_image else {
            return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
        };
        let t = tokens();
        let label = pane.active_image_path.unwrap_or("Image");
        let zoom = pane.image_zoom;
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

        let pdf_toc_active = self.pdf.active_path.is_some() && self.showing_pdf;

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

        let note_active = self.active_path.is_some() && !self.showing_pdf;
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
