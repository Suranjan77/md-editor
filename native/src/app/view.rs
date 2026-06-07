use iced::widget::{Space, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Task, Theme};

use crate::app_shell::AppShellMode;

use crate::features::pdf::annotations::{build_linked_pdf_note_content, slug_fragment};
use crate::messages::Message;
use crate::theme as app_theme;
use crate::views;
use std::collections::HashSet;

use super::model::*;
use crate::app::*;

impl MdEditor {
    pub(crate) fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let shell_state = self.app_shell_state();
        let _command_groups = shell_state.command_groups();
        let shell_status = self.app_shell_status(shell_state);

        if matches!(shell_state.mode, AppShellMode::NoVault) {
            return views::welcome::view(&[]);
        }

        let toolbar = views::toolbar::view(
            self.workspace.active_path.as_deref(),
            self.active_pdf_path
                .as_deref()
                .or(self.active_image_path.as_deref()),
            self.sidebar_visible,
            self.tracker.visible,
            self.toc_visible,
            self.workspace.active_path.is_some() || self.active_pdf_path.is_some(),
            self.split_view_active,
            self.workspace.active_path.is_some(),
            self.pdf_annotations_visible,
            self.active_pdf_path.is_some(),
        );

        let sidebar = views::sidebar::view(
            &self.workspace.vault_entries,
            self.workspace.selected_path.as_deref(),
            self.workspace
                .active_path
                .as_deref()
                .or(self.active_pdf_path.as_deref())
                .or(self.active_image_path.as_deref()),
            &self.workspace.expanded_folders,
            !self.sidebar_visible,
            shell_state.persistence.sidebar_width,
        );

        let editor_search_active = self.editor_search_is_active();
        let pdf_search_active = self.pdf_search_is_active();

        let active_search_match = if editor_search_active {
            self.active_search_match_position()
        } else {
            None
        };
        let editor_search_query = if editor_search_active {
            self.search.editor.query.as_str()
        } else {
            ""
        };
        let existing_files: HashSet<String> = self
            .workspace
            .vault_entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.path.clone())
            .collect();
        let editor_scroll = scrollable(
            container(
                crate::editor::renderer::Editor::new(
                    &self.buffer,
                    &self.highlighted_lines,
                    &self.image_cache,
                    &self.math_cache,
                    Message::EditorCommand,
                    Message::EditorCommandNoScroll,
                    Message::SidebarFileClicked,
                    Message::EditorCheckboxToggle,
                )
                .on_block_context_menu(|line_idx, absolute_pos| Message::EditorBlockContextMenu {
                    line_idx,
                    absolute_pos,
                })
                .on_context_menu(|line_idx, col, absolute_pos| Message::EditorContextMenu {
                    line_idx,
                    col,
                    absolute_pos,
                })
                .vault_context(
                    self.workspace.vault_root.as_deref(),
                    self.workspace.active_path.as_deref(),
                    &existing_files,
                )
                .search(
                    editor_search_query,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                    active_search_match,
                )
                .modifiers(self.keyboard_modifiers),
            )
            .padding(20)
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
                    &self.search.editor.query,
                    &self.search.editor.replace,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                    self.current_document_match_count(),
                    self.search.editor.active_index,
                    self.search.editor.wrap_status,
                )
                .into()
            } else {
                container(Space::new())
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };
            column![file_bar, editor_scroll].height(Length::Fill).into()
        };

        let pdf_view: Element<Message, Theme, iced::Renderer> = if self.active_pdf_path.is_some() {
            let focused_ann = self.focused_annotation_id.as_ref().and_then(|ann_id| {
                self.pdf_annotations
                    .values()
                    .flatten()
                    .find(|a| &a.id == ann_id)
            });
            let pdf_toolbar = views::pdf_viewer::toolbar_with_companion_note(
                self.pdf_current_page,
                self.pdf_total_pages,
                self.pdf_state.zoom,
                self.pdf_fit_to_width,
                self.pdf_fit_to_page,
                self.pdf_selection.is_some(),
                focused_ann,
                self.workspace.active_path.is_some(),
                None,
            );
            // B5: pdf_toolbar now at TOP of the pdf pane.
            // search_bar or zero-height space appears between toolbar and content.
            let pdf_search_bar: Element<_, _, iced::Renderer> = if pdf_search_active {
                views::pdf_viewer::search_bar(
                    &self.pdf_state.search.query,
                    self.pdf_state.search.regex,
                    self.pdf_state.search.match_case,
                    self.pdf_state.search.matches.len(),
                    self.pdf_state.search.active_index,
                    self.pdf_state.search.searching,
                )
            } else {
                container(Space::new())
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };
            let left_panel: Element<_, _, iced::Renderer> = container(column![
                pdf_toolbar,
                pdf_search_bar,
                scrollable(views::pdf_viewer::view_continuous(
                    &self.pdf_pages,
                    self.pdf_state.zoom,
                    self.pdf_rotation,
                    &self.pdf_dimensions,
                    &self.pdf_state.page_sizes,
                    self.pdf_placeholder_page_size,
                    if pdf_search_active
                        || self.search.visible
                        || self.search.editor.visible
                        || self.pdf_state.search.visible
                    {
                        &self.pdf_state.search.matches
                    } else {
                        &[]
                    },
                    &self.pdf_state.search.page_index,
                    self.pdf_state.search.active_index,
                    &self.pdf_page_text,
                    &self.pdf_annotations,
                    &self.pdf_page_links,
                    self.pdf_selection,
                    self.focused_annotation_id.as_deref(),
                    self.pdf_scroll_y,
                    self.pdf_viewport_height,
                    0,
                ))
                .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
                .on_scroll(|vp| Message::PdfScrolled {
                    y: vp.absolute_offset().y,
                    viewport_height: vp.bounds().height,
                })
                .height(Length::Fill),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

            // pdf_toolbar is at TOP of pane (B5); no second column wrap needed.
            left_panel
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let md_toc: &[views::toc::TocEntry] = if self.workspace.active_path.is_some() {
            &self.md_toc_entries
        } else {
            &[]
        };
        let pdf_toc: &[views::toc::TocEntry] =
            if self.active_pdf_path.is_some() && (self.showing_pdf || self.split_view_active) {
                self.pdf_toc_entries_flat.as_deref().unwrap_or(&[])
            } else {
                &[]
            };

        let active_md_line = if self.workspace.active_path.is_some() {
            self.md_toc_entries
                .iter()
                .rev()
                .find(|e| e.line <= self.buffer.cursor_line)
                .map(|e| e.line)
        } else {
            None
        };

        let active_pdf_page = if self.active_pdf_path.is_some() {
            let current_page = self.pdf_current_page as usize;
            self.pdf_toc_entries_flat
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .rev()
                .find(|e| e.line <= current_page)
                .map(|e| e.line)
        } else {
            None
        };

        let toc_view: Element<Message, Theme, iced::Renderer> = if self.toc_visible {
            views::toc::view(
                md_toc,
                pdf_toc,
                shell_state.persistence.workflow_width,
                active_md_line,
                active_pdf_page,
            )
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let image_view: Element<Message, Theme, iced::Renderer> =
            if let Some((handle, width, height)) = &self.active_image {
                let label = self.active_image_path.as_deref().unwrap_or("Image");
                container(
                    column![
                        text(label).size(13).color(app_theme::text_muted()),
                        iced::widget::image(handle.clone())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .content_fit(iced::ContentFit::Contain),
                        text(format!("{:.0} x {:.0}", width, height))
                            .size(11)
                            .color(app_theme::text_muted()),
                    ]
                    .spacing(12)
                    .align_x(Alignment::Center)
                    .padding(24),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::bg_primary())),
                    ..Default::default()
                })
                .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let main_content: Element<Message, Theme, iced::Renderer> =
            if shell_state.uses_split_research_layout() {
                let left_portion = (self.split_ratio * 1000.0) as u16;
                let right_portion = ((1.0 - self.split_ratio) * 1000.0) as u16;

                let divider = mouse_area(
                    container(text("⋮").size(14).color(app_theme::text_muted()))
                        .width(Length::Fixed(10.0))
                        .height(Length::Fill)
                        .center_x(Length::Fixed(10.0))
                        .center_y(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(app_theme::bg_tertiary())),
                            ..Default::default()
                        }),
                )
                .on_press(Message::SplitViewDragStart)
                .on_release(Message::SplitViewDragEnd)
                .interaction(iced::mouse::Interaction::ResizingHorizontally);

                row![
                    container(pdf_view)
                        .width(Length::FillPortion(left_portion))
                        .style(|_| container::Style {
                            border: iced::Border {
                                color: app_theme::border(),
                                width: 1.0,
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    divider,
                    container(editor_view).width(Length::FillPortion(right_portion))
                ]
                .into()
            } else if shell_state.shows_pdf_document()
                && self.showing_pdf
                && self.active_pdf_path.is_some()
            {
                pdf_view
            } else if shell_state.shows_image_document() && self.active_image.is_some() {
                image_view
            } else {
                editor_view.into()
            };

        let content = column![toolbar, main_content].height(Length::Fill);

        let backlinks_view: Element<Message, Theme, iced::Renderer> = views::backlinks::view(
            &self.workspace.backlinks,
            self.workspace.backlinks_visible,
            shell_state.persistence.workflow_width,
            false,
        );

        let pdf_annotations_view: Element<Message, Theme, iced::Renderer> =
            if self.pdf_annotations_visible && self.active_pdf_path.is_some() {
                views::pdf_annotations::view(
                    &self.pdf_annotations,
                    self.pdf_annotations_filter_color,
                    self.pdf_annotations_filter_page,
                    self.pdf_annotations_filter_tag.as_deref(),
                    self.pdf_annotations_filter_unresolved,
                    self.pdf_annotations_filter_linked,
                    self.focused_annotation_id.as_deref(),
                    self.workspace.active_path.is_some(),
                    shell_state.persistence.workflow_width,
                )
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let status_bar = views::status_bar::view(shell_status);

        let layout = column![
            row![
                sidebar,
                content,
                pdf_annotations_view,
                backlinks_view,
                toc_view
            ]
            .height(Length::Fill),
            status_bar
        ]
        .height(Length::Fill);

        let mut layers = vec![
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::bg_primary())),
                    ..Default::default()
                })
                .into(),
        ];

        if self.search.visible {
            layers.push(
                container(views::search::view(
                    &self.search.editor.query,
                    &self.search.editor.replace,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                    self.current_document_match_count(),
                    &self.search.global.results,
                    self.search.global.searching,
                    self.search.global.error.as_deref(),
                    true,
                    &self.search.global.sources,
                    self.search.global.pdf_status.as_deref(),
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

        if self.overlays.command_palette_visible {
            layers.push(
                container(views::command_palette::view(
                    &self.overlays.command_palette_query,
                    self.command_palette_commands(),
                    self.window_width,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.58,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        if self.overlays.citation_palette_visible {
            layers.push(
                container(views::citation_palette::view(
                    &self.overlays.citation_palette_query,
                    self.citation_palette_items(),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.58,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        if let Some(modal_type) = &self.overlays.active_modal {
            layers.push(views::modals::view(
                modal_type,
                &self.overlays.modal_input,
                &self.overlays.link_note_picker_search,
                &self.workspace.vault_entries,
            ));
        }

        if self.tracker.visible {
            layers.push(
                container(views::tracker::view(
                    true,
                    self.tracker.running,
                    &self.tracker.sessions,
                    &self.tracker.kv,
                    self.tracker.tab,
                    &self.tracker.config_content,
                    &self.tracker.manual_date,
                    &self.tracker.manual_hours,
                    &self.tracker.manual_notes,
                ))
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

        if let Some(preview_handle) = &self.pdf_link_preview {
            let img = iced::widget::image(preview_handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Contain);

            let modal = container(
                iced::widget::column![
                    iced::widget::row![
                        Space::new().width(Length::Fill),
                        iced::widget::button("✕")
                            .on_press(Message::ClosePdfLinkPreview)
                            .padding(10)
                            .style(iced::widget::button::text)
                    ],
                    container(img)
                        .width(Length::Fixed(1160.0))
                        .height(Length::Fixed(820.0))
                        .padding(16)
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(iced::Color::WHITE)),
                            border: iced::Border {
                                radius: 6.0.into(),
                                ..Default::default()
                            },
                            shadow: iced::Shadow {
                                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                                offset: iced::Vector::new(0.0, 8.0),
                                blur_radius: 24.0,
                            },
                            ..Default::default()
                        })
                ]
                .spacing(8)
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

        if let Some(msg) = &self.overlays.toast {
            layers.push(
                container(views::toast::view(msg))
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

    pub(crate) fn render_pdf_page(&self, page: u16) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = self.pdf_state.zoom * PDF_RENDER_SUPERSAMPLE;
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer()?;
                let res = renderer.render_page(&path_str, page, zoom);
                Some((page, res))
            },
            move |res| {
                if let Some((p, Ok(img))) = res {
                    Message::PdfRendered(generation, p, img)
                } else if let Some((p, Err(err))) = res {
                    if err == "Skipped" {
                        Message::PdfRenderSkipped(generation, p)
                    } else {
                        Message::PdfRenderFailed(generation, p)
                    }
                } else {
                    Message::PdfRenderFailed(generation, page)
                }
            },
        )
    }

    pub(crate) fn render_pdf_page_direct(&mut self, page: u16) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = self.pdf_state.zoom * PDF_RENDER_SUPERSAMPLE;
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();
        if self
            .pdf_pages
            .get(page as usize)
            .is_none_or(|p| p.is_none() || self.pdf_stale_pages.contains(&page))
        {
            self.pdf_pending_pages.insert(page);
        }

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer()?;
                renderer
                    .render_page_priority(&path_str, page, zoom)
                    .map_err(|e| println!("PDF PRIORITY RENDER ERROR (Page {}): {}", page, e))
                    .ok()
                    .map(|img| (page, img))
            },
            move |res| {
                if let Some((p, img)) = res {
                    Message::PdfRendered(generation, p, img)
                } else {
                    Message::PdfRenderFailed(generation, page)
                }
            },
        )
    }

    pub(crate) fn render_all_pdf_pages(&mut self) -> Task<Message> {
        self.render_visible_pdf_pages()
    }

    pub(crate) fn render_visible_pdf_pages(&mut self) -> Task<Message> {
        if self.pdf_total_pages == 0 {
            return Task::none();
        }
        // Estimate visible range using viewport height and page height
        let page_h = self.estimated_pdf_page_height().max(100.0);
        let viewport_h = self.window_height.max(400.0);
        let pages_in_view = (viewport_h / page_h).ceil() as u16;
        let first_visible = self.pdf_current_page;
        let last_visible =
            (first_visible + pages_in_view).min(self.pdf_total_pages.saturating_sub(1));

        if let Some(path) = &self.active_pdf_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(first_visible, last_visible, &path_str);
                }
            }
        }

        let start = self
            .pdf_current_page
            .saturating_sub(PDF_RENDER_PRELOAD_PAGES);
        let end = (self.pdf_current_page + pages_in_view + PDF_RENDER_PRELOAD_PAGES)
            .min(self.pdf_total_pages.saturating_sub(1));
        self.render_pdf_page_range(start, end)
    }

    pub(crate) fn render_pdf_pages_for_viewport(
        &mut self,
        scroll_y: f32,
        viewport_height: f32,
    ) -> Task<Message> {
        if self.pdf_total_pages == 0 {
            return Task::none();
        }

        let first_visible = self.pdf_page_at_scroll(scroll_y);
        let last_visible = self.pdf_page_at_scroll(scroll_y + viewport_height);

        if let Some(path) = &self.active_pdf_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(first_visible, last_visible, &path_str);
                }
            }
        }

        let Some((start, end)) = self.pdf_render_range_for_viewport(scroll_y, viewport_height)
        else {
            return Task::none();
        };
        self.render_pdf_page_range(start, end)
    }

    pub(crate) fn render_pdf_page_range(&mut self, start: u16, end: u16) -> Task<Message> {
        let Some((start, end)) = self.bounded_pdf_page_range(start, end) else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        for page_idx in start..=end {
            if self
                .pdf_pages
                .get(page_idx as usize)
                .is_none_or(|p| p.is_none() || self.pdf_stale_pages.contains(&page_idx))
                && !self.pdf_pending_pages.contains(&page_idx)
            {
                self.pdf_pending_pages.insert(page_idx);
                tasks.push(self.render_pdf_page(page_idx));
            }
            if !self.pdf_page_text.contains_key(&page_idx)
                && !self.pdf_pending_text.contains(&page_idx)
            {
                tasks.push(self.load_pdf_page_text(page_idx));
            }
        }

        Task::batch(tasks)
    }

    pub(crate) fn pdf_render_range_for_viewport(
        &self,
        scroll_y: f32,
        viewport_height: f32,
    ) -> Option<(u16, u16)> {
        if self.pdf_total_pages == 0 {
            return None;
        }

        let range = self.pdf_state.layout.visible_range(
            scroll_y,
            viewport_height,
            PDF_RENDER_PRELOAD_PAGES,
        );
        if !range.is_empty() {
            return Some((range.start, range.end.saturating_sub(1)));
        }

        let page_h = self.estimated_pdf_page_height().max(100.0);
        let pages_in_view = (viewport_height.max(0.0) / page_h).ceil() as u16;
        let first = self.pdf_page_at_scroll(scroll_y);
        let last = (first + pages_in_view).min(self.pdf_total_pages.saturating_sub(1));
        Some((
            first.saturating_sub(PDF_RENDER_PRELOAD_PAGES),
            last.saturating_add(PDF_RENDER_PRELOAD_PAGES)
                .min(self.pdf_total_pages.saturating_sub(1)),
        ))
    }

    pub(crate) fn bounded_pdf_page_range(&self, start: u16, end: u16) -> Option<(u16, u16)> {
        if self.pdf_total_pages == 0 || start > end || start >= self.pdf_total_pages {
            return None;
        }

        let doc_last = self.pdf_total_pages.saturating_sub(1);
        let end = end.min(doc_last);
        let capped_end = end.min(start.saturating_add(PDF_RENDER_MAX_SCHEDULED_PAGES - 1));
        Some((start, capped_end))
    }

    pub(crate) fn estimated_pdf_page_height(&self) -> f32 {
        self.pdf_placeholder_display_size().1
    }

    pub(crate) fn first_pdf_page_size(&self) -> Option<(f32, f32)> {
        self.pdf_state
            .page_sizes
            .first()
            .and_then(|s| *s)
            .or_else(|| {
                self.pdf_dimensions.first().and_then(|d| {
                    d.map(|(w, h)| {
                        (
                            w as f32 / self.pdf_state.zoom,
                            h as f32 / self.pdf_state.zoom,
                        )
                    })
                })
            })
    }

    pub(crate) fn pdf_placeholder_display_size(&self) -> (f32, f32) {
        let size = pdf_placeholder_display_size_from(
            self.pdf_placeholder_page_size,
            self.pdf_state.page_sizes.first().and_then(|s| *s),
            self.pdf_dimensions.first().and_then(|d| *d),
            self.pdf_state.zoom,
        );
        if self.pdf_rotation == 90 || self.pdf_rotation == 270 {
            (size.1, size.0)
        } else {
            size
        }
    }

    pub(crate) fn pdf_page_display_size(&self, page: u16) -> (f32, f32) {
        let size = if let Some(Some((w, h))) = self.pdf_state.page_sizes.get(page as usize) {
            (*w * self.pdf_state.zoom, *h * self.pdf_state.zoom)
        } else {
            self.pdf_placeholder_display_size()
        };
        if self.pdf_rotation == 90 || self.pdf_rotation == 270 {
            (size.1, size.0)
        } else {
            size
        }
    }

    pub(crate) fn pdf_available_width(&self) -> f32 {
        let sidebar_width = if self.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.workspace.backlinks_visible {
            260.0
        } else {
            0.0
        };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.window_width - chrome_width).max(320.0);

        if self.split_view_active
            && self.workspace.active_path.is_some()
            && self.active_pdf_path.is_some()
        {
            (content_width * self.split_ratio).max(280.0)
        } else {
            content_width
        }
    }

    pub(crate) fn pdf_page_height(&self, page: u16) -> f32 {
        if (page as usize) < self.pdf_total_pages as usize {
            self.pdf_page_display_size(page).1
        } else {
            self.estimated_pdf_page_height()
        }
    }

    pub(crate) fn pdf_page_offset(&self, page: u16) -> f32 {
        self.pdf_state.layout.page_offset(page)
    }

    pub(crate) fn pdf_total_height(&self) -> f32 {
        self.pdf_state.layout.total_height()
    }

    pub(crate) fn pdf_page_at_scroll(&self, scroll_y: f32) -> u16 {
        self.pdf_state.layout.page_at_scroll(scroll_y)
    }

    pub(crate) fn pdf_search_match_scroll_y(
        &self,
        result: &md_editor_core::application::pdf_service::PdfSearchMatch,
    ) -> f32 {
        let rect = result.rects.first();
        let page_height = self
            .pdf_state
            .page_sizes
            .get(result.page_index as usize)
            .and_then(|size| *size)
            .map(|(_, h)| h)
            .unwrap_or_else(|| {
                self.pdf_page_height(result.page_index) / self.pdf_state.zoom.max(0.01)
            });
        pdf_search_match_scroll_y_from(
            self.pdf_page_offset(result.page_index),
            rect.map(|rect| rect.y),
            rect.map(|rect| rect.height).unwrap_or(0.0),
            page_height,
            self.pdf_state.zoom,
            self.pdf_total_height(),
        )
    }

    pub(crate) fn pdf_link_at(
        &self,
        page_idx: u16,
        x: f32,
        y: f32,
    ) -> Option<md_editor_core::domain::pdf::LinkInfo> {
        let links = self.pdf_page_links.get(&page_idx)?;
        let dim = self
            .pdf_dimensions
            .get(page_idx as usize)
            .and_then(|d| *d)?;
        let real_x = (x * dim.0 as f32) / self.pdf_state.zoom;
        let real_y = (y * dim.1 as f32) / self.pdf_state.zoom;

        links
            .iter()
            .find(|link| {
                let lx = link.bbox.x;
                let ly = link.bbox.y;
                let lw = link.bbox.width;
                let lh = link.bbox.height;
                real_x >= lx && real_x <= lx + lw && real_y >= ly && real_y <= ly + lh
            })
            .cloned()
    }

    pub(crate) fn find_pdf_annotation(
        &self,
        id: &str,
    ) -> Option<(u16, md_editor_core::domain::pdf::PdfAnnotation)> {
        self.pdf_annotations
            .iter()
            .find_map(|(page_idx, page_anns)| {
                page_anns
                    .iter()
                    .find(|ann| ann.id == id)
                    .cloned()
                    .map(|ann| (*page_idx, ann))
            })
    }

    pub(crate) fn pdf_paths_match(&self, active_path: Option<&str>, target_path: &str) -> bool {
        let Some(active_path) = active_path else {
            return false;
        };
        if active_path == target_path {
            return true;
        }

        let Some(vault_root) = self.workspace.vault_root.as_deref() else {
            return false;
        };
        let active_abs = md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(vault_root),
            active_path,
        );
        let target_abs = md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(vault_root),
            target_path,
        );
        normalize_path(&active_abs) == normalize_path(&target_abs)
    }

    pub(crate) fn pdf_selection_contains_point(&self, page_idx: u16, x: f32, y: f32) -> bool {
        let Some(sel) = &self.pdf_selection else {
            return false;
        };
        if sel.page_index != page_idx {
            return false;
        }
        let Some(page_text) = self.pdf_page_text.get(&sel.page_index) else {
            return false;
        };
        let start = sel.anchor_idx.min(sel.focus_idx);
        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
        let selected_chars = page_text
            .chars
            .iter()
            .filter(|c| c.text_index >= start && c.text_index < end)
            .cloned()
            .collect::<Vec<_>>();
        let px = x * page_text.page_width;
        let py = y * page_text.page_height;

        md_editor_core::domain::pdf::merge_char_rects(&selected_chars)
            .iter()
            .any(|rect| {
                let view_y = page_text.page_height - rect.y - rect.height;
                let pad = 4.0;
                px >= rect.x - pad
                    && px <= rect.x + rect.width + pad
                    && py >= view_y - pad
                    && py <= view_y + rect.height + pad
            })
    }

    pub(crate) fn annotation_at(
        &self,
        page_idx: u16,
        x: f32,
        y: f32,
    ) -> Option<md_editor_core::domain::pdf::PdfAnnotation> {
        let page_text = self.pdf_page_text.get(&page_idx)?;
        let px = x * page_text.page_width;
        let py = y * page_text.page_height;

        let page_anns = self.pdf_annotations.get(&page_idx)?;
        for ann in page_anns {
            for rect in &ann.rects {
                let view_y = page_text.page_height - rect.y - rect.height;
                if px >= rect.x
                    && px <= rect.x + rect.width
                    && py >= view_y
                    && py <= view_y + rect.height
                {
                    return Some(ann.clone());
                }
            }
        }
        None
    }

    pub(crate) fn resolve_active_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let root = self.workspace.vault_root.as_deref()?;
        Some(md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(root),
            path,
        ))
    }

    pub(crate) fn default_pdf_note_path(
        &self,
        ann: &md_editor_core::domain::pdf::PdfAnnotation,
    ) -> String {
        let pdf_filename = self
            .active_pdf_path
            .as_deref()
            .and_then(|pdf_path| std::path::Path::new(pdf_path).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let clean_pdf_name = slug_fragment(pdf_filename);
        format!(
            "pdf-notes/{}-p{}-{}.md",
            clean_pdf_name,
            ann.page_index + 1,
            &ann.id[..8.min(ann.id.len())]
        )
    }

    pub(crate) fn linked_pdf_note_file_content(
        &self,
        note_path: &str,
        pdf_path: &str,
        ann: &md_editor_core::domain::pdf::PdfAnnotation,
    ) -> String {
        match md_editor_core::vault::open_file(&self.state, note_path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        {
            Some(existing) => {
                build_linked_pdf_note_content(Some(&existing), note_path, pdf_path, ann).content
            }
            None => build_linked_pdf_note_content(None, note_path, pdf_path, ann).content,
        }
    }

    pub(crate) fn estimated_editor_viewport_width(&self) -> f32 {
        if self.editor_viewport_width > 0.0 {
            return self.editor_viewport_width;
        }
        let sidebar_width = if self.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.workspace.backlinks_visible {
            260.0
        } else {
            0.0
        };
        let pdf_ann_width = if self.pdf_annotations_visible && self.active_pdf_path.is_some() {
            270.0
        } else {
            0.0
        };
        let chrome_width = sidebar_width + toc_width + backlinks_width + pdf_ann_width;
        let content_width = (self.window_width - chrome_width).max(320.0);

        if self.split_view_active
            && self.workspace.active_path.is_some()
            && self.active_pdf_path.is_some()
        {
            (content_width * (1.0 - self.split_ratio)).max(280.0)
        } else {
            content_width
        }
    }

    pub(crate) fn estimated_editor_viewport_height(&self) -> f32 {
        if self.editor_viewport_height > 0.0 {
            return self.editor_viewport_height;
        }
        let mut height = self.window_height - 48.0; // toolbar ~48px
        if self.search.editor.visible && self.workspace.active_path.is_some() {
            height -= 40.0; // search bar ~40px
        }
        height.max(200.0)
    }

    pub(crate) fn estimated_editor_line_y(&self, target_line: usize) -> f32 {
        crate::editor::renderer::line_visual_y::<iced::Renderer>(
            &self.highlighted_lines,
            &self.image_cache,
            &self.math_cache,
            self.estimated_editor_viewport_width().max(240.0),
            self.buffer.cursor_line,
            self.buffer.cursor_col,
            target_line,
            true,
        ) + 20.0
    }
}
