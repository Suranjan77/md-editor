//! Building the UI: the shell window and the handwriting window.

use super::*;
use iced::widget::column;

impl MdEditor {
    pub fn view(&self, window: iced::window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        if Some(window) == self.ink_window {
            return self.ink_view();
        }
        self.shell_view()
    }

    /// The handwriting window: toolbar over a canvas that fills the rest.
    pub(super) fn ink_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
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
                    eraser_radius: self.ink.eraser_screen_radius(),
                    drawing: self.ink.is_drawing(),
                    hover: self.ink.hover(),
                    radial: self.ink.radial_menu(),
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                }),
                // Its own column rather than an overlay: the canvas claims all
                // pen input within its bounds, so a panel drawn on top of it
                // could never be pressed with the pen.
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
            return views::welcome::view();
        }

        let toolbar = views::toolbar::view(
            self.active_path.as_deref(),
            self.pdf
                .active_path
                .as_deref()
                .or(self.active_image_path.as_deref())
                .or(self.pptx.active_path.as_deref()),
            self.editor.buffer.dirty,
            self.vault.sidebar_visible,
            self.vault.backlinks_visible,
            self.graph.visible,
            self.tracker.visible,
            self.editor.toc_visible,
            self.pdf.active_path.is_some()
                && (self.showing_pdf || (self.ui.split_view_active && self.active_path.is_some())),
            self.ui.split_view_active,
            self.active_path.is_some(),
            self.ink.is_visible(),
        );

        // Panels are clipped to an animated width rather than swapped between
        // "full" and "absent", so opening and closing them reads as movement.
        // The content keeps its natural width throughout; only the window onto
        // it changes, which avoids re-laying-out the tree on every frame.
        let sidebar_width = self.motion.sidebar_width();
        let sidebar: Element<Message, Theme, iced::Renderer> = container(views::sidebar::view(
            &self.vault.entries,
            self.vault.selected_path.as_deref(),
            self.active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref())
                .or(self.pptx.active_path.as_deref()),
            &self.vault.expanded_folders,
            sidebar_width < 1.0,
        ))
        .width(Length::Fixed(sidebar_width))
        .clip(true)
        .into();

        let editor_search_active = self.editor_search_is_active();
        let pdf_search_active = self.pdf_search_is_active();

        let active_search_match = if editor_search_active {
            self.search.active_match_position()
        } else {
            None
        };
        let editor_search_query = if editor_search_active {
            self.search.query.as_str()
        } else {
            ""
        };
        let editor_scroll = scrollable(
            container(
                crate::editor::renderer::Editor::new(
                    &self.editor.buffer,
                    &self.editor.highlight.lines,
                    &self.editor.image_cache,
                    &self.editor.math_cache,
                    Message::EditorCommand,
                    Message::EditorCommandNoScroll,
                    Message::SidebarFileClicked,
                    Message::EditorCheckboxToggle,
                )
                .search(
                    editor_search_query,
                    self.search.regex,
                    self.search.match_case,
                    active_search_match,
                )
                .scale_factor(self.ui.scale_factor)
                .reveal_caret(self.editor.reveal_request, Message::EditorCaretView)
                .layout_revision(self.editor.layout_revision()),
            )
            .padding(EDITOR_PADDING)
            .width(Length::Fill),
        )
        .id(iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID))
        .on_scroll(|vp| Message::EditorScrolled {
            y: vp.absolute_offset().y,
            viewport_width: vp.bounds().width,
            viewport_height: vp.bounds().height,
        })
        .height(Length::Fill);

        let editor_view: Element<Message, Theme, iced::Renderer> = {
            let file_bar: Element<'_, Message, Theme, iced::Renderer> = if editor_search_active {
                views::search::file_bar(
                    &self.search.query,
                    &self.search.replace,
                    self.search.regex,
                    self.search.match_case,
                    self.search.match_count(),
                    self.search.match_index,
                )
            } else {
                container(Space::new())
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };
            column![file_bar, editor_scroll].height(Length::Fill).into()
        };

        let pdf_view: Element<Message, Theme, iced::Renderer> = if self.pdf.active_path.is_some() {
            let focused_ann = self.pdf.focused_annotation_id.as_ref().and_then(|ann_id| {
                self.pdf
                    .annotations
                    .values()
                    .flatten()
                    .find(|a| &a.id == ann_id)
            });
            let pdf_toolbar = views::pdf_viewer::toolbar(
                self.pdf.current_page,
                self.pdf.total_pages,
                self.pdf.zoom,
                self.editor.toc_visible,
                self.pdf.selection.is_some(),
                self.pdf.annotations.values().any(|v| !v.is_empty()),
                focused_ann,
            );
            let pdf_pages = scrollable(views::pdf_viewer::view_continuous(
                &self.pdf.pages,
                self.pdf.zoom,
                &self.pdf.dimensions,
                &self.pdf.page_sizes,
                self.pdf.placeholder_page_size,
                if pdf_search_active || self.search.visible || self.search.file_visible {
                    &self.search.pdf_results
                } else {
                    &[]
                },
                self.search.match_index,
                &self.pdf.page_text,
                &self.pdf.annotations,
                &self.pdf.references,
                self.pdf.selection,
                self.pdf.focused_annotation_id.as_deref(),
                self.pdf.load_error.as_deref(),
            ))
            .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
            .on_scroll(|vp| Message::PdfScrolled {
                y: vp.absolute_offset().y,
                viewport_height: vp.bounds().height,
            })
            .height(Length::Fill);

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
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };

            column![search_bar, pdf_pages, pdf_toolbar]
                .height(Length::Fill)
                .into()
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let pdf_toc_available = self.pdf.active_path.is_some()
            && (self.showing_pdf || (self.ui.split_view_active && self.active_path.is_some()));
        let toc_width = self.motion.toc_width();
        let toc_view: Element<Message, Theme, iced::Renderer> =
            if toc_width >= 1.0 && pdf_toc_available {
                container(views::toc::view(
                    &self.pdf.toc_entries,
                    self.pdf.toc_is_synthetic,
                ))
                .width(Length::Fixed(toc_width))
                .clip(true)
                .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let image_view: Element<Message, Theme, iced::Renderer> =
            if let Some((handle, width, height)) = &self.active_image {
                let label = self.active_image_path.as_deref().unwrap_or("Image");
                container(
                    column![
                        text(label)
                            .size(app_theme::TEXT_BASE)
                            .color(app_theme::TEXT_MUTED),
                        iced::widget::image(handle.clone())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .content_fit(iced::ContentFit::Contain),
                        text(format!("{:.0} x {:.0}", width, height))
                            .size(app_theme::TEXT_SM)
                            .color(app_theme::TEXT_MUTED),
                    ]
                    .spacing(app_theme::SPACE_4)
                    .align_x(Alignment::Center)
                    .padding(24),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                })
                .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let main_content: Element<Message, Theme, iced::Renderer> = if self.graph.visible {
            views::graph::view(&self.graph)
        } else if self.ui.split_view_active
            && self.active_path.is_some()
            && self.pdf.active_path.is_some()
        {
            let left_portion = (self.ui.split_ratio * 1000.0) as u16;
            let right_portion = ((1.0 - self.ui.split_ratio) * 1000.0) as u16;

            let divider = mouse_area(
                container(
                    text("⋮")
                        .size(app_theme::TEXT_BASE)
                        .color(app_theme::TEXT_MUTED),
                )
                .width(Length::Fixed(10.0))
                .height(Length::Fill)
                .center_x(Length::Fixed(10.0))
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_TERTIARY)),
                    ..Default::default()
                }),
            )
            .on_press(Message::SplitViewDragStart)
            .on_release(Message::SplitViewDragEnd)
            .interaction(iced::mouse::Interaction::ResizingHorizontally);

            row![
                container(editor_view).width(Length::FillPortion(left_portion)),
                divider,
                container(pdf_view)
                    .width(Length::FillPortion(right_portion))
                    .style(|_| container::Style {
                        border: iced::Border {
                            color: app_theme::BORDER,
                            width: 1.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
            ]
            .into()
        } else if self.showing_pdf && self.pdf.active_path.is_some() {
            pdf_view
        } else if self.pptx.active_path.is_some() {
            views::pptx_viewer::view(&self.pptx, self.pdf_available_width())
        } else if self.active_image.is_some() {
            image_view
        } else {
            editor_view
        };

        let content = column![toolbar, main_content].height(Length::Fill);

        let backlinks_width = self.motion.backlinks_width();
        let backlinks_view: Element<Message, Theme, iced::Renderer> =
            container(views::backlinks::view(
                &self.vault.backlinks,
                backlinks_width >= 1.0 && !self.graph.visible,
            ))
            .width(Length::Fixed(backlinks_width))
            .clip(true)
            .into();

        let layout = row![sidebar, content, backlinks_view, toc_view].height(Length::Fill);

        let mut layers = vec![
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                })
                .into(),
        ];

        if self.search.visible {
            layers.push(
                container(views::search::view(
                    &self.search.query,
                    &self.search.replace,
                    self.search.regex,
                    self.search.match_case,
                    self.search.match_count(),
                    &self.search.results,
                    self.search.results_truncated,
                    &self.search.pdf_results,
                    self.search.pdf_error.as_deref(),
                    true,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.6,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        // Kept mounted through the fade-out, so dismissing the palette is a
        // transition rather than a cut.
        let palette_opacity = self.motion.palette_opacity();
        if palette_opacity > 0.01 {
            layers.push(
                container(views::command_palette::view(
                    &self.ui.command_palette_query,
                    &self.ui.commands,
                    &self.vault.entries,
                    self.ui.palette_selected,
                    palette_opacity,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0,
                        0.0,
                        0.0,
                        0.58 * palette_opacity,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        if let Some(modal_type) = &self.ui.active_modal {
            layers.push(views::modals::view(
                modal_type,
                &self.ui.modal_input,
                &self.ui.link_note_picker_search,
                &self.vault.entries,
            ));
        }

        if self.tracker.visible {
            layers.push(
                container(self.tracker.view())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(28)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgba(
                            0.0, 0.0, 0.0, 0.55,
                        ))),
                        ..Default::default()
                    })
                    .into(),
            );
        }

        if let Some(preview_handle) = &self.pdf.link_preview {
            // Size the box to the preview's aspect ratio, as large as fits in
            // ~85% of the window. Matching the aspect means the image fills the
            // box (no letterboxing) and the destination — centered in the
            // crop — sits centered in the modal, as large as the window allows.
            let (img_w, img_h) = self.pdf.link_preview_size.unwrap_or((800.0, 600.0));
            let aspect = (img_w / img_h).max(0.1);
            let max_w = self.preview_max_box_width();
            let max_h = (self.ui.window_height * 0.85).clamp(360.0, 1000.0);
            let mut preview_box_w = max_w;
            let mut preview_box_h = preview_box_w / aspect;
            if preview_box_h > max_h {
                preview_box_h = max_h;
                preview_box_w = preview_box_h * aspect;
            }

            let img = iced::widget::image(preview_handle.clone())
                .filter_method(iced::widget::image::FilterMethod::Linear)
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Contain);

            let modal = container(
                iced::widget::column![
                    iced::widget::row![
                        Space::new().width(Length::Fill),
                        iced::widget::button("✕")
                            .on_press(Message::ClosePdfLinkPreview)
                            .padding(app_theme::SPACE_3)
                    ],
                    container(img)
                        .width(Length::Fixed(preview_box_w))
                        .height(Length::Fixed(preview_box_h))
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(iced::Color::WHITE)),
                            border: iced::Border {
                                radius: app_theme::RADIUS_MD.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                ]
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.8,
                ))),
                ..Default::default()
            });
            layers.push(modal.into());
        }

        // Driven by the animation, not by `ui.toast` directly, so the toast
        // survives long enough to fade out after the message is cleared.
        let toast_opacity = self.motion.toast_opacity();
        if toast_opacity > 0.01 {
            layers.push(
                container(views::toast::view(&self.motion.toast_text, toast_opacity))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(40)
                    .into(),
            );
        }

        stack(layers).into()
    }
}
