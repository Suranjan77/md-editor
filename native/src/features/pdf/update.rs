//! PDF update handlers for the PDF feature.
//!
//! This module contains all the update arms for PDF messages that were previously
//! in the app-level update coordinator. The coordinator delegates to this module
//! via the `update_pdf` function.

use crate::app::model::MdEditor;
use crate::app::model::{PDF_RENDER_SUPERSAMPLE, PDF_SCROLLABLE_ID, PDF_TEXT_PAGE_CACHE_LIMIT};
use crate::app::{
    focus_pdf_search_input, pdf_companion_note_key, reindex_markdown_file_with_parser_targets,
    resolve_relative_link_path, save_markdown_file_with_parser_targets, text_by_char_range,
};
use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::features::pdf::annotations::{normalize_note_path, note_filename_from_path};
use crate::features::pdf::message::PdfMessage;
use crate::features::pdf::navigation::NavigationTarget;
use crate::features::pdf::view_model::PdfLayout;
use crate::features::shell::ActivePanel;
use crate::messages::Message;
use crate::views;
use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};
use iced::Task;
use iced::widget::operation::{self, AbsoluteOffset};

/// Update the PDF feature state in response to a PDF message.
///
/// This is the feature-level update function called by the app coordinator.
#[allow(unreachable_patterns)]
pub fn update_pdf(editor: &mut MdEditor, message: PdfMessage) -> Task<Message> {
    match message {
        PdfMessage::LinkNoteFolderSelected(folder) => {
            if matches!(
                editor.overlays.active_modal,
                Some(views::modals::ModalType::LinkNote(_))
            ) {
                let filename = note_filename_from_path(&editor.overlays.modal_input);
                editor.overlays.modal_input = if folder.is_empty() {
                    filename
                } else {
                    format!("{}/{}", folder.trim_end_matches('/'), filename)
                };
            }
            Task::none()
        }
        PdfMessage::LinkNoteFileSelected(path) => {
            if matches!(
                editor.overlays.active_modal,
                Some(views::modals::ModalType::LinkNote(_))
            ) {
                editor.overlays.modal_input = normalize_note_path(&path);
            }
            Task::none()
        }
        PdfMessage::LinkNotePickerSearchChanged(query) => {
            if matches!(
                editor.overlays.active_modal,
                Some(views::modals::ModalType::LinkNote(_))
            ) {
                editor.overlays.link_note_picker_search = query;
            }
            Task::none()
        }
        PdfMessage::Loaded(generation, pages) => {
            if generation != editor.pdf.render_generation {
                return Task::none();
            }
            editor.pdf.total_pages = pages;
            editor.pdf.pages = vec![None; pages as usize];
            editor.pdf.dimensions = vec![None; pages as usize];
            if editor.pdf.view.page_sizes.len() != pages as usize {
                editor.pdf.view.page_sizes = vec![None; pages as usize];
            }
            editor.pdf.view.layout = PdfLayout::rebuild(
                &editor.pdf.view.page_sizes,
                editor.pdf.view.zoom,
                editor.pdf_placeholder_display_size(),
                PDF_PAGE_SPACING,
                PDF_PAGE_LIST_PADDING,
                editor.pdf.rotation,
            );
            editor.pdf.pending_pages.clear();
            editor.pdf.stale_pages.clear();
            editor.pdf.pending_links.clear();
            editor.pdf.programmatic_scroll = false;
            editor.pdf.toc_target_page = None;

            // Eagerly generate page-level TOC entries so the panel isn't
            // blank even if the bookmark extraction hasn't finished yet.
            let page_entries: Vec<views::toc::TocEntry> = (0..pages)
                .map(|p| views::toc::TocEntry {
                    level: 1,
                    text: format!("Page {}", p + 1),
                    line: p as usize,
                })
                .collect();
            if editor.pdf.toc_entries_flat.is_none() {
                editor.pdf.toc_entries_flat = Some(page_entries);
            }

            if pages == 0 {
                editor.overlays.toast =
                    Some("PDF renderer is unavailable or the PDF could not be opened".to_string());
            }
            if editor.pdf.fit_to_width
                && editor
                    .pdf
                    .view
                    .page_sizes
                    .iter()
                    .take(pages as usize)
                    .any(Option::is_some)
            {
                Task::done(Message::Pdf(PdfMessage::FitToWidth))
            } else if editor.pdf.fit_to_page
                && editor
                    .pdf
                    .view
                    .page_sizes
                    .iter()
                    .take(pages as usize)
                    .any(Option::is_some)
            {
                Task::done(Message::Pdf(PdfMessage::FitToPage))
            } else if editor.pdf.fit_to_width || editor.pdf.fit_to_page {
                Task::none()
            } else {
                editor.render_all_pdf_pages()
            }
        }
        PdfMessage::ZoomChanged(zoom) => {
            let current_page = editor.pdf_page_at_scroll(editor.pdf.scroll_y);
            let page_start_offset = editor.pdf_page_offset(current_page);
            let relative_ratio = if editor.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                0.0
            } else {
                let page_height_old = editor.pdf_page_height(current_page);
                if page_height_old > 0.0 {
                    ((editor.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                } else {
                    0.0
                }
            };

            editor.pdf.fit_to_width = false;
            editor.pdf.fit_to_page = false;
            editor.pdf.view.zoom = zoom.clamp(0.5, 4.0);
            editor.pdf.stale_pages = editor
                .pdf
                .pages
                .iter()
                .enumerate()
                .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                .collect();
            editor.pdf.placeholder_page_size = editor.first_pdf_page_size();
            editor.pdf.pending_pages.clear();
            editor.pdf.pending_links.clear();
            editor.pdf.toc_target_page = Some(current_page);
            editor.pdf.programmatic_scroll = true;
            editor.pdf.render_generation = editor.pdf.render_generation.wrapping_add(1);

            editor.pdf.view.layout = PdfLayout::rebuild(
                &editor.pdf.view.page_sizes,
                editor.pdf.view.zoom,
                editor.pdf_placeholder_display_size(),
                PDF_PAGE_SPACING,
                PDF_PAGE_LIST_PADDING,
                editor.pdf.rotation,
            );
            editor.update_pdf_page_cache();

            let new_scroll_y = if editor.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                editor.pdf.scroll_y
            } else {
                editor.pdf_page_offset(current_page)
                    + relative_ratio * editor.pdf_page_height(current_page)
            };
            editor.pdf.scroll_y = new_scroll_y;

            Task::batch(vec![
                editor.render_visible_pdf_pages(),
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: new_scroll_y,
                    },
                ),
            ])
        }
        PdfMessage::WheelScrolledForZoom(delta) => {
            if editor.pdf.active_path.is_some()
                && editor.pdf.showing_pdf
                && (editor.shell.keyboard_modifiers.control()
                    || editor.shell.keyboard_modifiers.command())
                && delta.abs() > f32::EPSILON
            {
                let next_zoom = (editor.pdf.view.zoom + delta).clamp(0.5, 4.0);
                if (next_zoom - editor.pdf.view.zoom).abs() > f32::EPSILON {
                    Task::done(Message::Pdf(PdfMessage::ZoomChanged(next_zoom)))
                } else {
                    Task::none()
                }
            } else {
                Task::none()
            }
        }
        PdfMessage::FitToWidth => {
            let is_initial = editor.pdf.initial_target_page.is_some();
            let current_page = if let Some(target_page) = editor.pdf.initial_target_page.take() {
                target_page.min(editor.pdf.total_pages.saturating_sub(1))
            } else {
                editor.pdf_page_at_scroll(editor.pdf.scroll_y)
            };
            let page_start_offset = editor.pdf_page_offset(current_page);
            let relative_ratio = if is_initial {
                0.0
            } else if editor.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                0.0
            } else {
                let page_height_old = editor.pdf_page_height(current_page);
                if page_height_old > 0.0 {
                    ((editor.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                } else {
                    0.0
                }
            };

            editor.pdf.fit_to_width = true;
            editor.pdf.fit_to_page = false;
            let available_width = editor.pdf_available_width();
            let page_width = editor
                .pdf
                .view
                .page_sizes
                .iter()
                .flatten()
                .next()
                .map(|(w, _)| (*w).max(1.0))
                .or_else(|| {
                    editor
                        .pdf
                        .dimensions
                        .iter()
                        .flatten()
                        .next()
                        .map(|(w, _)| (*w as f32 / editor.pdf.view.zoom).max(1.0))
                })
                .unwrap_or(612.0);
            let next_zoom = ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, 4.0);
            editor.pdf.view.zoom = ((next_zoom * 100.0).round() / 100.0).clamp(0.5, 4.0);
            editor.pdf.stale_pages = editor
                .pdf
                .pages
                .iter()
                .enumerate()
                .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                .collect();
            editor.pdf.placeholder_page_size = editor.first_pdf_page_size();
            editor.pdf.pending_pages.clear();
            editor.pdf.pending_links.clear();
            editor.pdf.toc_target_page = Some(current_page);
            editor.pdf.programmatic_scroll = true;
            editor.pdf.render_generation = editor.pdf.render_generation.wrapping_add(1);

            editor.pdf.view.layout = PdfLayout::rebuild(
                &editor.pdf.view.page_sizes,
                editor.pdf.view.zoom,
                editor.pdf_placeholder_display_size(),
                PDF_PAGE_SPACING,
                PDF_PAGE_LIST_PADDING,
                editor.pdf.rotation,
            );
            editor.update_pdf_page_cache();

            let new_scroll_y = if is_initial {
                editor.pdf_page_offset(current_page)
            } else if editor.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                editor.pdf.scroll_y
            } else {
                editor.pdf_page_offset(current_page)
                    + relative_ratio * editor.pdf_page_height(current_page)
            };
            editor.pdf.scroll_y = new_scroll_y;
            if is_initial {
                editor.pdf.current_page = current_page;
            }

            Task::batch(vec![
                editor.render_visible_pdf_pages(),
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: new_scroll_y,
                    },
                ),
            ])
        }
        PdfMessage::FitToPage => {
            let is_initial = editor.pdf.initial_target_page.is_some();
            let current_page = if let Some(target_page) = editor.pdf.initial_target_page.take() {
                target_page.min(editor.pdf.total_pages.saturating_sub(1))
            } else {
                editor.pdf_page_at_scroll(editor.pdf.scroll_y)
            };
            let page_start_offset = editor.pdf_page_offset(current_page);
            let relative_ratio = if is_initial {
                0.0
            } else if editor.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                0.0
            } else {
                let page_height_old = editor.pdf_page_height(current_page);
                if page_height_old > 0.0 {
                    ((editor.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                } else {
                    0.0
                }
            };

            editor.pdf.fit_to_page = true;
            editor.pdf.fit_to_width = false;
            let available_width = editor.pdf_available_width();
            let viewport_height = if editor.pdf.viewport_height > 0.0 {
                editor.pdf.viewport_height
            } else {
                editor.estimated_editor_viewport_height()
            };

            let (page_width, page_height) = editor
                .pdf
                .view
                .page_sizes
                .iter()
                .flatten()
                .next()
                .map(|(w, h)| ((*w).max(1.0), (*h).max(1.0)))
                .or_else(|| {
                    editor.pdf.dimensions.iter().flatten().next().map(|(w, h)| {
                        (
                            (*w as f32 / editor.pdf.view.zoom).max(1.0),
                            (*h as f32 / editor.pdf.view.zoom).max(1.0),
                        )
                    })
                })
                .unwrap_or((612.0, 792.0));

            let w_zoom = (available_width - 48.0).max(240.0) / page_width;
            let h_zoom = (viewport_height - 40.0).max(200.0) / page_height;
            let next_zoom = w_zoom.min(h_zoom).clamp(0.5, 4.0);
            editor.pdf.view.zoom = ((next_zoom * 100.0).round() / 100.0).clamp(0.5, 4.0);
            editor.pdf.stale_pages = editor
                .pdf
                .pages
                .iter()
                .enumerate()
                .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                .collect();
            editor.pdf.placeholder_page_size = editor.first_pdf_page_size();
            editor.pdf.pending_pages.clear();
            editor.pdf.pending_links.clear();
            editor.pdf.toc_target_page = Some(current_page);
            editor.pdf.programmatic_scroll = true;
            editor.pdf.render_generation = editor.pdf.render_generation.wrapping_add(1);

            editor.pdf.view.layout = PdfLayout::rebuild(
                &editor.pdf.view.page_sizes,
                editor.pdf.view.zoom,
                editor.pdf_placeholder_display_size(),
                PDF_PAGE_SPACING,
                PDF_PAGE_LIST_PADDING,
                editor.pdf.rotation,
            );
            editor.update_pdf_page_cache();

            let new_scroll_y = if is_initial {
                editor.pdf_page_offset(current_page)
            } else if editor.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                editor.pdf.scroll_y
            } else {
                editor.pdf_page_offset(current_page)
                    + relative_ratio * editor.pdf_page_height(current_page)
            };
            editor.pdf.scroll_y = new_scroll_y;
            if is_initial {
                editor.pdf.current_page = current_page;
            }

            Task::batch(vec![
                editor.render_visible_pdf_pages(),
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: new_scroll_y,
                    },
                ),
            ])
        }
        PdfMessage::RotateClockwise => {
            if editor.pdf.active_path.is_some() && editor.pdf.showing_pdf {
                editor.pdf.rotation = (editor.pdf.rotation + 90) % 360;
                editor.pdf.view.page_cache.clear();
                editor.pdf.pages.fill(None);
                editor.pdf.dimensions.fill(None);
                editor.pdf.stale_pages.clear();
                editor.pdf.pending_pages.clear();
                editor.pdf.pending_links.clear();
                editor.pdf.view.layout = PdfLayout::rebuild(
                    &editor.pdf.view.page_sizes,
                    editor.pdf.view.zoom,
                    editor.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    editor.pdf.rotation,
                );
                editor.pdf.render_generation = editor.pdf.render_generation.wrapping_add(1);

                if editor.pdf.fit_to_width {
                    Task::done(Message::Pdf(PdfMessage::FitToWidth))
                } else if editor.pdf.fit_to_page {
                    Task::done(Message::Pdf(PdfMessage::FitToPage))
                } else {
                    editor.render_visible_pdf_pages()
                }
            } else {
                Task::none()
            }
        }
        PdfMessage::PageSizesLoaded(generation, path, sizes) => {
            if generation != editor.pdf.render_generation
                || editor.pdf.active_path.as_deref() != Some(path.as_str())
            {
                return Task::none();
            }
            editor.pdf.view.page_sizes = sizes.into_iter().map(Some).collect();
            if editor.pdf.view.page_sizes.len() < editor.pdf.total_pages as usize {
                editor
                    .pdf
                    .view
                    .page_sizes
                    .resize(editor.pdf.total_pages as usize, None);
            }
            if editor.pdf.placeholder_page_size.is_none() {
                editor.pdf.placeholder_page_size = editor.first_pdf_page_size();
            }
            editor.pdf.view.layout = PdfLayout::rebuild(
                &editor.pdf.view.page_sizes,
                editor.pdf.view.zoom,
                editor.pdf_placeholder_display_size(),
                PDF_PAGE_SPACING,
                PDF_PAGE_LIST_PADDING,
                editor.pdf.rotation,
            );
            if editor.pdf.fit_to_width && editor.pdf.total_pages > 0 {
                Task::done(Message::Pdf(PdfMessage::FitToWidth))
            } else if editor.pdf.fit_to_page && editor.pdf.total_pages > 0 {
                Task::done(Message::Pdf(PdfMessage::FitToPage))
            } else if let Some(page) = editor.pdf.initial_target_page.take() {
                editor.navigate_pdf_page(page)
            } else {
                Task::none()
            }
        }
        PdfMessage::Rendered(generation, page, img) => {
            editor.pdf.pending_pages.remove(&page);
            if generation != editor.pdf.render_generation {
                return Task::none();
            }
            let img = match editor.pdf.rotation {
                90 => img.rotate90(),
                180 => img.rotate180(),
                270 => img.rotate270(),
                _ => img,
            };
            let width = img.width();
            let height = img.height();
            let handle =
                iced::widget::image::Handle::from_rgba(width, height, img.into_rgba8().into_raw());
            let logical_width = (width as f32 / PDF_RENDER_SUPERSAMPLE).round() as u32;
            let logical_height = (height as f32 / PDF_RENDER_SUPERSAMPLE).round() as u32;
            if (page as usize) < editor.pdf.pages.len() {
                editor.pdf.pages[page as usize] = Some(handle.clone());
                editor.pdf.dimensions[page as usize] = Some((logical_width, logical_height));
                editor.pdf.stale_pages.remove(&page);

                // Insert into the LRU cache for bounded memory.
                let byte_size = width as usize * height as usize * 4; // RGBA
                editor.pdf.view.page_cache.insert(
                    page,
                    handle,
                    (logical_width, logical_height),
                    byte_size,
                );
                editor.sync_pdf_pages_to_cache();
            }
            if editor.pdf.placeholder_page_size.is_none() || page == 0 {
                editor.pdf.placeholder_page_size = Some((
                    logical_width as f32 / editor.pdf.view.zoom,
                    logical_height as f32 / editor.pdf.view.zoom,
                ));
            }
            let mut tasks = vec![editor.load_pdf_page_links(page)];
            if !editor.pdf.page_text.contains_key(&page) && !editor.pdf.pending_text.contains(&page)
            {
                tasks.push(editor.load_pdf_page_text(page));
            }
            if editor.pdf.toc_target_page == Some(page) {
                let scroll_y = editor.pdf_page_offset(page);
                let current_scroll_y = editor.pdf.scroll_y;
                if (current_scroll_y - scroll_y).abs() < 5.0 {
                    editor.pdf.toc_target_page = None;
                    editor.pdf.programmatic_scroll = false;
                    editor.pdf.current_page = page.min(editor.pdf.total_pages.saturating_sub(1));
                    let start = page.saturating_sub(2);
                    let end = (page + 2).min(editor.pdf.total_pages.saturating_sub(1));
                    editor.update_pdf_page_cache();
                    tasks.push(editor.render_pdf_page_range(start, end));
                } else {
                    editor.pdf.programmatic_scroll = true;
                    tasks.push(operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: scroll_y,
                        },
                    ));
                }
            }
            Task::batch(tasks)
        }
        PdfMessage::RenderFailed(generation, page) => {
            editor.pdf.pending_pages.remove(&page);
            if generation != editor.pdf.render_generation {
                return Task::none();
            }
            if editor.pdf.toc_target_page == Some(page) {
                editor.pdf.toc_target_page = None;
                editor.pdf.programmatic_scroll = false;
            }
            editor.overlays.toast = Some(format!("Could not render PDF page {}", page + 1));
            Task::none()
        }
        PdfMessage::RenderSkipped(generation, page) => {
            editor.pdf.pending_pages.remove(&page);
            if generation != editor.pdf.render_generation {
                return Task::none();
            }
            if editor.pdf.toc_target_page == Some(page) {
                editor.pdf.toc_target_page = None;
                editor.pdf.programmatic_scroll = false;
            }
            Task::none()
        }
        PdfMessage::TocClicked(index) => {
            if editor.pdf.active_path.is_some() {
                if editor.pdf.showing_pdf && editor.pdf.active_path.is_some() {
                    editor.push_pdf_navigation_history();
                } else if editor.workspace.active_path.is_some() {
                    editor.push_markdown_navigation_history();
                }
                let target_page = index
                    .min(editor.pdf.total_pages.saturating_sub(1) as usize)
                    .max(0) as u16;
                editor.set_active_panel(ActivePanel::Pdf);
                editor.navigate_pdf_page(target_page)
            } else {
                Task::none()
            }
        }
        PdfMessage::Scrolled { y, viewport_height } => {
            if (editor.shell.keyboard_modifiers.control()
                || editor.shell.keyboard_modifiers.command())
                && !editor.pdf.programmatic_scroll
            {
                return operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: editor.pdf.scroll_y,
                    },
                );
            }
            editor.pdf.viewport_height = viewport_height;
            editor.set_active_panel(ActivePanel::Pdf);
            editor.pdf.scroll_y = y;
            let new_page = editor.pdf_page_at_scroll(y + viewport_height * 0.33);

            let target_page_ready = if let Some(target_page) = editor.pdf.toc_target_page {
                editor
                    .pdf
                    .pages
                    .get(target_page as usize)
                    .is_some_and(|page| page.is_some())
            } else {
                false
            };

            if editor.pdf.programmatic_scroll {
                if let Some(target_page) = editor.pdf.toc_target_page {
                    let target_y = editor.pdf_page_offset(target_page);
                    let max_scroll_y = (editor.pdf_total_height() - viewport_height).max(0.0);
                    let expected_y = target_y.min(max_scroll_y);
                    if ((y - expected_y).abs() < 5.0 || new_page == target_page)
                        && target_page_ready
                    {
                        editor.pdf.programmatic_scroll = false;
                    }
                } else {
                    editor.pdf.programmatic_scroll = false;
                }
            } else {
                editor.pdf.toc_target_page = None;
            }

            if let Some(target_page) = editor.pdf.toc_target_page {
                let target_y = editor.pdf_page_offset(target_page);
                let max_scroll_y = (editor.pdf_total_height() - viewport_height).max(0.0);
                let expected_y = target_y.min(max_scroll_y);
                if ((y - expected_y).abs() < 5.0 || new_page == target_page) && target_page_ready {
                    // Arrived! Clear programmatic scroll flags and render.
                    editor.pdf.toc_target_page = None;
                    editor.pdf.programmatic_scroll = false;
                    editor.pdf.current_page =
                        target_page.min(editor.pdf.total_pages.saturating_sub(1));
                    let start = editor.pdf.current_page.saturating_sub(2);
                    let end =
                        (editor.pdf.current_page + 2).min(editor.pdf.total_pages.saturating_sub(1));
                    editor.update_pdf_page_cache();
                    return editor.render_pdf_page_range(start, end);
                } else {
                    // Still scrolling programmatically to target. Skip rendering intermediate pages.
                    editor.update_pdf_page_cache();
                    return Task::none();
                }
            }

            if new_page != editor.pdf.current_page && new_page < editor.pdf.total_pages {
                if new_page.abs_diff(editor.pdf.current_page) > 8 {
                    editor.pdf.pending_pages.clear();
                    editor.pdf.pending_links.clear();
                }
                editor.pdf.current_page = new_page;
                let task = editor.render_pdf_pages_for_viewport(y, viewport_height);
                editor.update_pdf_page_cache();
                task
            } else {
                let task = editor.render_pdf_pages_for_viewport(y, viewport_height);
                editor.update_pdf_page_cache();
                task
            }
        }
        PdfMessage::LeftClicked(page_idx, x, y, modifiers) => {
            editor.set_active_panel(ActivePanel::Pdf);
            if let Some(link) = editor.pdf_link_at(page_idx, x, y) {
                if let Some(dest_page) = link.dest_page {
                    editor.push_pdf_navigation_history();
                    editor.pdf.current_page =
                        dest_page.min(u32::from(editor.pdf.total_pages.saturating_sub(1))) as u16;
                    editor.navigate_pdf_page(editor.pdf.current_page)
                } else if let Some(uri) = link.uri {
                    if modifiers.control() || modifiers.command() {
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "", &uri])
                            .spawn();
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg(&uri).spawn();
                        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                        let _ = std::process::Command::new("xdg-open").arg(&uri).spawn();
                        editor.overlays.toast = Some(format!("Opening: {}", uri));
                    } else {
                        editor.overlays.toast =
                            Some(format!("External link (Ctrl+click to open): {}", uri));
                    }
                    Task::none()
                } else {
                    Task::none()
                }
            } else if let Some(ann) = editor.annotation_at(page_idx, x, y) {
                editor.pdf.focused_annotation_id = Some(ann.id.clone());
                if let Some(ref path) = ann.linked_note_path {
                    if !path.is_empty() {
                        Task::done(Message::Pdf(PdfMessage::OpenLinkedNote(path.clone())))
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            } else {
                editor.pdf.focused_annotation_id = None;
                Task::none()
            }
        }
        PdfMessage::ContextMenuAction(action) => match action {
            views::modals::PdfContextMenuItem::Copy => {
                if let Some(sel) = &editor.pdf.selection {
                    if let Some(page_text) = editor.pdf.page_text.get(&sel.page_index) {
                        let start = sel.anchor_idx.min(sel.focus_idx);
                        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                        let selected = text_by_char_range(&page_text.text, start, end);
                        if !selected.is_empty() {
                            editor.overlays.active_modal = None;
                            return iced::clipboard::write(selected);
                        }
                    }
                }
                Task::none()
            }
            views::modals::PdfContextMenuItem::CopyAsQuote => {
                if let Some(sel) = &editor.pdf.selection {
                    if let Some(page_text) = editor.pdf.page_text.get(&sel.page_index) {
                        let start = sel.anchor_idx.min(sel.focus_idx);
                        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                        let selected = text_by_char_range(&page_text.text, start, end);
                        if !selected.is_empty() {
                            let quote = selected
                                .lines()
                                .map(|l| format!("> {}", l))
                                .collect::<Vec<_>>()
                                .join("\n");
                            editor.overlays.active_modal = None;
                            return iced::clipboard::write(quote);
                        }
                    }
                }
                Task::none()
            }
            views::modals::PdfContextMenuItem::CopyWithSourceLink => {
                if let Some(command) = editor.pdf_selection_quote_link_command() {
                    let EditorCommand::InsertPdfQuoteLink {
                        selected_text,
                        page_number: _,
                        link,
                    } = command
                    else {
                        return Task::none();
                    };
                    let markdown = format!("{selected_text}\n[label]({link})");
                    editor.overlays.active_modal = None;
                    return iced::clipboard::write(markdown);
                }
                Task::none()
            }
            views::modals::PdfContextMenuItem::HighlightYellow
            | views::modals::PdfContextMenuItem::HighlightGreen
            | views::modals::PdfContextMenuItem::HighlightBlue
            | views::modals::PdfContextMenuItem::HighlightPink
            | views::modals::PdfContextMenuItem::HighlightOrange => {
                let color = match action {
                    views::modals::PdfContextMenuItem::HighlightYellow => {
                        md_editor_core::domain::pdf::PdfAnnotationColor::Yellow
                    }
                    views::modals::PdfContextMenuItem::HighlightGreen => {
                        md_editor_core::domain::pdf::PdfAnnotationColor::Green
                    }
                    views::modals::PdfContextMenuItem::HighlightBlue => {
                        md_editor_core::domain::pdf::PdfAnnotationColor::Blue
                    }
                    views::modals::PdfContextMenuItem::HighlightPink => {
                        md_editor_core::domain::pdf::PdfAnnotationColor::Pink
                    }
                    _ => md_editor_core::domain::pdf::PdfAnnotationColor::Orange,
                };
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::CreateHighlight(color)))
            }
            views::modals::PdfContextMenuItem::UnderlineBlue => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::CreateAnnotation(
                    md_editor_core::domain::pdf::PdfAnnotationKind::Underline,
                    md_editor_core::domain::pdf::PdfAnnotationColor::Blue,
                )))
            }
            views::modals::PdfContextMenuItem::StrikeRed => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::CreateAnnotation(
                    md_editor_core::domain::pdf::PdfAnnotationKind::Strike,
                    md_editor_core::domain::pdf::PdfAnnotationColor::Red,
                )))
            }
            views::modals::PdfContextMenuItem::SearchSelectedText => {
                if let Some(sel) = &editor.pdf.selection {
                    if let Some(page_text) = editor.pdf.page_text.get(&sel.page_index) {
                        let start = sel.anchor_idx.min(sel.focus_idx);
                        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                        let selected = text_by_char_range(&page_text.text, start, end);
                        if !selected.trim().is_empty() {
                            editor.pdf.view.search.query = selected.trim().to_string();
                            editor.pdf.selection = None;
                            editor.overlays.active_modal = None;
                            editor.pdf.view.search.visible = true;
                            editor.search.visible = false;
                            return Task::batch(vec![
                                editor.search_pdf(),
                                focus_pdf_search_input(),
                                editor.restore_scroll_positions(),
                            ]);
                        }
                    }
                }
                Task::none()
            }
            views::modals::PdfContextMenuItem::InsertQuoteLink => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::InsertQuoteLink))
            }
            views::modals::PdfContextMenuItem::InsertAnnotationLink { id, page: _ } => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::InsertAnnotationLink(id)))
            }
            views::modals::PdfContextMenuItem::EditNote { id, page } => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::EditAnnotationNote(id, page)))
            }
            views::modals::PdfContextMenuItem::LinkToNote { id, page: _ } => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::LinkNote(id, String::new())))
            }
            views::modals::PdfContextMenuItem::OpenLinkedNote(path) => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::OpenLinkedNote(path)))
            }
            views::modals::PdfContextMenuItem::DeleteHighlight(id) => {
                editor.overlays.active_modal = None;
                Task::done(Message::Pdf(PdfMessage::DeleteHighlight(id)))
            }
            views::modals::PdfContextMenuItem::OpenLink(link) => {
                editor.overlays.active_modal = None;
                if let Some(dest_page) = link.dest_page {
                    editor.push_pdf_navigation_history();
                    editor.pdf.current_page =
                        dest_page.min(u32::from(editor.pdf.total_pages.saturating_sub(1))) as u16;
                    editor.navigate_pdf_page(editor.pdf.current_page)
                } else if let Some(uri) = link.uri {
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("cmd")
                        .args(["/C", "start", "", &uri])
                        .spawn();
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&uri).spawn();
                    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                    let _ = std::process::Command::new("xdg-open").arg(&uri).spawn();
                    editor.overlays.toast = Some(format!("Opening: {}", uri));
                    Task::none()
                } else {
                    Task::none()
                }
            }
            views::modals::PdfContextMenuItem::CopyLink(uri) => {
                editor.overlays.active_modal = None;
                iced::clipboard::write(uri)
            }
        },
        PdfMessage::LinkPreviewResult(Ok(res)) => {
            if let Ok(img) = image::load_from_memory(&res.image_data) {
                let width = img.width();
                let height = img.height();
                editor.pdf.link_preview = Some(iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                ));
            }
            Task::none()
        }
        PdfMessage::LinkPreviewResult(Err(e)) => {
            editor.overlays.toast = Some(format!("Preview Error: {}", e));
            Task::none()
        }
        PdfMessage::CloseLinkPreview => {
            editor.pdf.link_preview = None;
            editor.overlays.active_modal = None;
            Task::none()
        }
        PdfMessage::TocLoaded(generation, entries) => {
            if generation != editor.pdf.render_generation {
                return Task::none();
            }
            pub(crate) fn flatten_pdf_toc(
                entries: &[md_editor_core::application::pdf_service::TocEntry],
                level: u8,
                out: &mut Vec<views::toc::TocEntry>,
            ) {
                for entry in entries {
                    if let Some(page_index) = entry.page_index {
                        out.push(views::toc::TocEntry {
                            level,
                            text: entry.title.clone(),
                            line: page_index as usize,
                        });
                    }
                    flatten_pdf_toc(&entry.children, (level + 1).min(6), out);
                }
            }

            // Check if the PDF has any bookmarks at all
            let has_bookmarks = !entries.is_empty();
            let mut mapped = Vec::new();
            flatten_pdf_toc(&entries, 1, &mut mapped);

            // If the PDF has no bookmarks, fill with page entries (the eager
            // fallback in PdfLoaded already did this, but we may have had a
            // zero-page race earlier).
            if !has_bookmarks {
                let current = editor.pdf.toc_entries_flat.get_or_insert_with(Vec::new);
                if current.is_empty() {
                    for p in 0..editor.pdf.total_pages {
                        current.push(views::toc::TocEntry {
                            level: 1,
                            text: format!("Page {}", p + 1),
                            line: p as usize,
                        });
                    }
                }
            } else if !mapped.is_empty() {
                // PDF has bookmarks — replace page entries with real TOC.
                editor.pdf.toc_entries_flat = Some(mapped);
            }
            // else: PDF has bookmark structure but no valid page refs; keep
            // the eager page entries as fallback.
            Task::none()
        }
        PdfMessage::PageLinksLoaded(generation, page, links) => {
            editor.pdf.pending_links.remove(&page);
            if generation != editor.pdf.render_generation {
                return Task::none();
            }
            editor.pdf.page_links.insert(page, links);
            Task::none()
        }
        PdfMessage::SearchMatchesFound(search_id, matches) => {
            if search_id == editor.search.pdf_active_id {
                if editor.search.visible && editor.search.global.pdf_search_id == Some(search_id) {
                    if let Some(pdf_path) = &editor.pdf.active_path {
                        let query_lower = editor.search.editor.query.to_lowercase();
                        let query_trimmed = editor.search.editor.query.trim();

                        let is_linked =
                            |p1: &str, p2: &str| editor.state.vault_paths_are_linked(p1, p2);

                        let match_index_base = editor.pdf.view.search.matches.len();
                        for (match_offset, m) in matches.iter().enumerate() {
                            let mut score = 4.0;
                            score *= 1.5;
                            if m.context.to_lowercase().contains(&query_lower) {
                                if m.context.trim().to_lowercase() == query_trimmed.to_lowercase() {
                                    score *= 2.0;
                                }
                            }
                            if let Some(ref active) = editor.workspace.active_path {
                                if is_linked(pdf_path, active) {
                                    score *= 1.3;
                                }
                            }

                            editor.search.global.results.push(
                                md_editor_core::types::UnifiedSearchResult {
                                    group: md_editor_core::types::SearchResultGroup::PdfContent,
                                    path: pdf_path.clone(),
                                    line: (m.page_index + 1) as usize,
                                    context: format!(
                                        "PDF text ({} areas): {}",
                                        m.rects.len(),
                                        md_editor_core::vault::search_result_preview(
                                            &m.context,
                                            query_trimmed,
                                            None,
                                        )
                                    ),
                                    score,
                                    page_index: Some(m.page_index),
                                    annotation_id: Some(
                                        (match_index_base + match_offset).to_string(),
                                    ),
                                },
                            );
                        }

                        editor.search.global.results.sort_by(|a, b| {
                            b.score
                                .partial_cmp(&a.score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                .then_with(|| a.group.cmp(&b.group))
                                .then_with(|| a.path.cmp(&b.path))
                                .then_with(|| a.line.cmp(&b.line))
                        });
                    }
                }

                editor.pdf.view.search.matches.extend(matches);
                editor.rebuild_pdf_search_page_index();
                if editor.pdf.view.search.active_index.is_none()
                    && !editor.pdf.view.search.matches.is_empty()
                    && !editor.search.visible
                {
                    editor.pdf.view.search.active_index = Some(0);
                    editor.navigate_pdf_search_to_index(0)
                } else {
                    Task::none()
                }
            } else {
                Task::none()
            }
        }
        PdfMessage::SearchFinished(search_id, result) => {
            if search_id == editor.search.pdf_active_id {
                editor.pdf.view.search.searching = false;
                if editor.search.global.pdf_search_id == Some(search_id) {
                    editor.search.global.pending_pdf = false;
                    editor.search.global.pdf_search_id = None;
                    editor.update_global_search_searching();
                }
                match result {
                    Ok(()) => Task::none(),
                    Err(err) => {
                        editor.search.pdf_error = Some(err);
                        editor.pdf.view.search.matches.clear();
                        editor.pdf.view.search.page_index.clear();
                        Task::none()
                    }
                }
            } else {
                Task::none()
            }
        }
        PdfMessage::SearchResultClicked(page) => {
            editor.search.visible = false;
            editor.pdf.view.search.visible = true;
            editor.set_active_panel(ActivePanel::Pdf);
            editor.pdf.view.search.active_index = editor
                .pdf
                .view
                .search
                .matches
                .iter()
                .position(|result| result.page_index == page);
            if let Some(index) = editor.pdf.view.search.active_index {
                editor.navigate_pdf_search_to_index(index)
            } else {
                editor.pdf.current_page = page.min(editor.pdf.total_pages.saturating_sub(1));
                editor.navigate_pdf_page(editor.pdf.current_page)
            }
        }
        PdfMessage::ScrollBy(delta) => {
            if editor.pdf.active_path.is_none()
                || (!editor.pdf.showing_pdf
                    && !(editor.shell.split_view_active && editor.workspace.active_path.is_some()))
                || (editor.shell.split_view_active
                    && editor.workspace.active_path.is_some()
                    && editor.shell.active_panel != ActivePanel::Pdf)
                || editor.search.visible
                || editor.search.editor.visible
                || editor.pdf.view.search.visible
                || editor.overlays.active_modal.is_some()
                || editor.overlays.command_palette_visible
            {
                return Task::none();
            }
            let max_y = editor.pdf_total_height().max(0.0);
            let y = (editor.pdf.scroll_y + delta).clamp(0.0, max_y);
            operation::scroll_to(
                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                AbsoluteOffset { x: 0.0, y },
            )
        }
        PdfMessage::FirstPage => {
            if editor.pdf.showing_pdf && editor.pdf.total_pages > 0 {
                editor.navigate_pdf_page(0)
            } else {
                Task::none()
            }
        }
        PdfMessage::LastPage => {
            if editor.pdf.showing_pdf && editor.pdf.total_pages > 0 {
                editor.navigate_pdf_page(editor.pdf.total_pages.saturating_sub(1))
            } else {
                Task::none()
            }
        }
        PdfMessage::NavBack => {
            let current_target = if editor.pdf.showing_pdf && editor.pdf.active_path.is_some() {
                Some(NavigationTarget::Pdf {
                    path: editor.pdf.active_path.clone().unwrap(),
                    page: editor.pdf.current_page,
                    scroll_offset: editor.pdf.scroll_y,
                    zoom: editor.pdf.view.zoom,
                })
            } else {
                editor
                    .workspace
                    .active_path
                    .as_ref()
                    .map(|path| NavigationTarget::Markdown {
                        path: path.clone(),
                        line: editor.editor.buffer.cursor_line,
                        column: editor.editor.buffer.cursor_col,
                    })
            };

            if let Some(target) = current_target {
                if !editor.workspace.navigation_history.entries.is_empty() {
                    if editor.workspace.navigation_history.current_index
                        == editor.workspace.navigation_history.entries.len() - 1
                        && editor.workspace.navigation_history.entries
                            [editor.workspace.navigation_history.current_index]
                            .target
                            != target
                    {
                        editor.workspace.navigation_history.push(target);
                    }
                }
            }

            if let Some(target) = editor.workspace.navigation_history.go_back() {
                editor.navigate_to_target(target)
            } else {
                Task::none()
            }
        }
        PdfMessage::NavForward => {
            if let Some(target) = editor.workspace.navigation_history.go_forward() {
                editor.navigate_to_target(target)
            } else {
                Task::none()
            }
        }
        PdfMessage::SearchToggle => {
            if editor.pdf.showing_pdf {
                if editor.pdf.view.search.visible {
                    editor.pdf.view.search.visible = false;
                    editor.pdf.view.search.matches.clear();
                    editor.pdf.view.search.page_index.clear();
                } else {
                    editor.pdf.view.search.visible = true;
                    editor.search.visible = false;
                }
                Task::none()
            } else {
                Task::none()
            }
        }
        PdfMessage::GoToPage => {
            if editor.pdf.active_path.is_some()
                && editor.pdf.showing_pdf
                && editor.pdf.total_pages > 0
            {
                editor.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                    total: editor.pdf.total_pages,
                    error: None,
                });
                editor.overlays.modal_input.clear();
                Task::none()
            } else {
                Task::none()
            }
        }
        PdfMessage::DocumentIdComputed(Some((path, hash, len, mtime))) => {
            let _ = editor.state.save_pdf_document(&hash, &path, len, mtime);
            editor.pdf.document_id = Some(hash.clone());

            let annotations = editor
                .state
                .get_pdf_annotations(&hash, None)
                .unwrap_or_default();
            editor.pdf.annotations.clear();
            for ann in annotations {
                editor
                    .pdf
                    .annotations
                    .entry(ann.page_index)
                    .or_default()
                    .push(ann);
            }

            let mut target_page = None;
            if let Some(ref target_id) = editor.pdf.initial_target_annotation {
                for (page_idx, page_anns) in &editor.pdf.annotations {
                    if page_anns.iter().any(|a| &a.id == target_id) {
                        target_page = Some(*page_idx);
                        editor.pdf.focused_annotation_id = Some(target_id.clone());
                        break;
                    }
                }
            }

            let scroll_task = if editor.pdf.total_pages > 0 {
                if let Some(page) = target_page {
                    editor.pdf.initial_target_page = None;
                    editor.pdf.initial_target_annotation = None;
                    editor.navigate_pdf_page(page)
                } else if let Some(page) = editor.pdf.initial_target_page {
                    editor.pdf.initial_target_page = None;
                    editor.navigate_pdf_page(page)
                } else {
                    Task::none()
                }
            } else {
                if let Some(page) = target_page {
                    editor.pdf.initial_target_page = Some(page);
                    editor.pdf.initial_target_annotation = None;
                }
                Task::none()
            };

            scroll_task
        }
        PdfMessage::DocumentIdComputed(None) => Task::none(),
        PdfMessage::PageTextLoaded(generation, page, res) => {
            editor.pdf.pending_text.remove(&page);
            if generation == editor.pdf.render_generation {
                if let Ok(page_text) = res {
                    if let Some(ref path) = editor.pdf.active_path {
                        let _ = editor.state.save_pdf_page_text(path, page, &page_text.text);
                    }
                    editor.pdf.page_text.insert(page, page_text);
                    editor.pdf.text_lru.push_back(page);
                    if editor.pdf.text_lru.len() > PDF_TEXT_PAGE_CACHE_LIMIT {
                        if let Some(oldest) = editor.pdf.text_lru.pop_front() {
                            editor.pdf.page_text.remove(&oldest);
                        }
                    }
                }
            }
            Task::none()
        }
        PdfMessage::SelectionChanged(page, anchor, focus) => {
            editor.set_active_panel(ActivePanel::Pdf);
            editor.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                page_index: page,
                anchor_idx: anchor,
                focus_idx: focus,
            });
            Task::none()
        }
        PdfMessage::SelectionCleared => {
            editor.pdf.selection = None;
            Task::none()
        }
        PdfMessage::SelectionFinished(page, anchor, focus) => {
            editor.set_active_panel(ActivePanel::Pdf);
            editor.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                page_index: page,
                anchor_idx: anchor,
                focus_idx: focus,
            });
            Task::none()
        }
        PdfMessage::CopySelection => {
            if !editor.pdf_copy_shortcut_is_active() {
                return Task::none();
            }
            if let Some(sel) = &editor.pdf.selection {
                if let Some(page_text) = editor.pdf.page_text.get(&sel.page_index) {
                    let start = sel.anchor_idx.min(sel.focus_idx);
                    let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                    let selected = text_by_char_range(&page_text.text, start, end);
                    if !selected.is_empty() {
                        return iced::clipboard::write(selected);
                    }
                }
            }
            Task::none()
        }
        PdfMessage::InsertQuoteLink => {
            if editor.workspace.active_path.is_none() {
                editor.overlays.toast =
                    Some("Open a markdown file before inserting a quote link".to_string());
                return Task::none();
            }
            if editor.overlays.excerpt_mode_active {
                if let Some(sel) = &editor.pdf.selection {
                    if let Some(page_text) = editor.pdf.page_text.get(&sel.page_index) {
                        let start = sel.anchor_idx.min(sel.focus_idx);
                        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                        let selected = text_by_char_range(&page_text.text, start, end);
                        if !selected.trim().is_empty() {
                            editor.overlays.excerpts_queue.push(
                                crate::messages::CitationItem::Selection {
                                    text: selected,
                                    page_index: sel.page_index,
                                },
                            );
                            editor.overlays.toast = Some("Quote queued to excerpts".to_string());
                        }
                    }
                }
                return Task::none();
            }
            let Some(command) = editor.pdf_selection_quote_link_command() else {
                editor.overlays.toast =
                    Some("Select PDF text before inserting a quote link".to_string());
                return Task::none();
            };
            editor.set_active_panel(ActivePanel::Markdown);
            editor.run_editor_command(command)
        }
        PdfMessage::InsertAnnotationLink(annotation_id) => {
            if editor.workspace.active_path.is_none() {
                editor.overlays.toast =
                    Some("Open a markdown file before inserting a highlight".to_string());
                return Task::none();
            }
            if editor.overlays.excerpt_mode_active {
                if let Some((_, ann)) = editor.find_pdf_annotation(&annotation_id) {
                    editor.overlays.excerpts_queue.push(
                        crate::messages::CitationItem::Annotation {
                            id: ann.id.clone(),
                            text: ann.selected_text.clone(),
                            page_index: ann.page_index,
                        },
                    );
                    editor.overlays.toast = Some("Annotation queued to excerpts".to_string());
                }
                return Task::none();
            }
            let Some(command) = editor.pdf_annotation_link_command(&annotation_id) else {
                editor.overlays.toast =
                    Some("Select a PDF highlight before inserting it".to_string());
                return Task::none();
            };
            editor.set_active_panel(ActivePanel::Markdown);
            editor.run_editor_command(command)
        }
        PdfMessage::CreateHighlight(color) => {
            Task::done(Message::Pdf(PdfMessage::CreateAnnotation(
                md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
                color,
            )))
        }
        PdfMessage::CreateAnnotation(kind, color) => {
            if let (Some(sel), Some(doc_id)) = (&editor.pdf.selection, &editor.pdf.document_id) {
                if let Some(page_text) = editor.pdf.page_text.get(&sel.page_index) {
                    let start = sel.anchor_idx.min(sel.focus_idx);
                    let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);

                    let mut selected_chars = Vec::new();
                    for c in &page_text.chars {
                        if c.text_index >= start && c.text_index < end {
                            selected_chars.push(c.clone());
                        }
                    }

                    let selected_text = text_by_char_range(&page_text.text, start, end);

                    let rects = md_editor_core::domain::pdf::merge_char_rects(&selected_chars);

                    let id = uuid::Uuid::new_v4().to_string();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;

                    let ann = md_editor_core::domain::pdf::PdfAnnotation {
                        id: id.clone(),
                        document_id: doc_id.clone(),
                        page_index: sel.page_index,
                        kind,
                        color,
                        selected_text,
                        ranges: vec![md_editor_core::domain::pdf::PdfTextRange {
                            start_text_index: start,
                            end_text_index: end,
                        }],
                        rects,
                        note: None,
                        linked_note_path: None,
                        markdown_anchor: None,
                        tags: Vec::new(),
                        status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
                        created_at: now,
                        updated_at: now,
                    };

                    if let Err(e) = editor.state.save_pdf_annotation(&ann) {
                        editor.overlays.toast = Some(format!("Failed to save annotation: {}", e));
                    } else {
                        editor
                            .pdf
                            .annotations
                            .entry(sel.page_index)
                            .or_default()
                            .push(ann);
                        editor.pdf.selection = None;
                        if let Some(path) = &editor.pdf.active_path {
                            editor.workspace.backlinks =
                                md_editor_core::vault::get_mixed_backlinks(&editor.state, path)
                                    .unwrap_or_default();
                        }
                    }
                }
            }
            Task::none()
        }
        PdfMessage::DeleteHighlight(id) => {
            if let Err(e) = editor.state.delete_pdf_annotation(&id) {
                editor.overlays.toast = Some(format!("Failed to delete highlight: {}", e));
            } else {
                for page_anns in editor.pdf.annotations.values_mut() {
                    page_anns.retain(|a| a.id != id);
                }
                if editor.pdf.focused_annotation_id.as_ref() == Some(&id) {
                    editor.pdf.focused_annotation_id = None;
                }
                if let Some(views::modals::ModalType::QuickNote(ref mid)) =
                    editor.overlays.active_modal
                {
                    if mid == &id {
                        editor.overlays.active_modal = None;
                        editor.overlays.modal_input.clear();
                    }
                }
                if let Some(path) = &editor.pdf.active_path {
                    editor.workspace.backlinks =
                        md_editor_core::vault::get_mixed_backlinks(&editor.state, path)
                            .unwrap_or_default();
                }
            }
            Task::none()
        }
        PdfMessage::AddQuickNote(id, note_content) => {
            let mut found_ann = None;
            for page_anns in editor.pdf.annotations.values_mut() {
                if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                    let trimmed_note = note_content.trim().to_string();
                    ann.note = if trimmed_note.is_empty() {
                        None
                    } else {
                        Some(trimmed_note)
                    };
                    ann.updated_at = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    found_ann = Some(ann.clone());
                    break;
                }
            }
            let mut task = Task::none();
            if let Some(ann) = found_ann {
                if let Err(e) = editor.state.save_pdf_annotation(&ann) {
                    editor.overlays.toast = Some(format!("Failed to save note: {}", e));
                } else {
                    if let Some(path) = &editor.pdf.active_path {
                        editor.workspace.backlinks =
                            md_editor_core::vault::get_mixed_backlinks(&editor.state, path)
                                .unwrap_or_default();

                        if let Some(note_path) = ann.linked_note_path.as_deref() {
                            if let Ok(bytes) =
                                md_editor_core::vault::open_file(&editor.state, note_path)
                            {
                                if let Ok(existing_content) = String::from_utf8(bytes) {
                                    let updated_content =
                                            crate::features::pdf::annotations::sync_annotation_note_in_markdown(
                                                &existing_content,
                                                path,
                                                &ann,
                                            );
                                    if updated_content != existing_content {
                                        if let Err(e) = save_markdown_file_with_parser_targets(
                                            &editor.state,
                                            note_path,
                                            &updated_content,
                                        ) {
                                            editor.overlays.toast =
                                                Some(format!("Failed to sync linked note: {}", e));
                                        } else if editor.workspace.active_path.as_deref()
                                            == Some(note_path)
                                        {
                                            editor.editor.buffer =
                                                DocBuffer::from_text(&updated_content);
                                            let _ = reindex_markdown_file_with_parser_targets(
                                                &editor.state,
                                                note_path,
                                                &updated_content,
                                            );
                                            task = editor.highlight_all();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            task
        }
        PdfMessage::LinkNote(annotation_id, mut note_path) => {
            let mut annotation = None;
            for page_anns in editor.pdf.annotations.values() {
                if let Some(ann) = page_anns.iter().find(|a| a.id == annotation_id) {
                    annotation = Some(ann.clone());
                    break;
                }
            }
            if let Some(mut ann) = annotation {
                if note_path.is_empty() {
                    editor.overlays.modal_input = editor.default_pdf_note_path(&ann);
                    editor.overlays.link_note_picker_search.clear();
                    editor.overlays.active_modal =
                        Some(views::modals::ModalType::LinkNote(annotation_id));
                    return Task::none();
                }

                note_path = normalize_note_path(&note_path);
                if let Some(pdf_path) = &editor.pdf.active_path {
                    let content = editor.linked_pdf_note_file_content(&note_path, pdf_path, &ann);

                    if let Err(e) =
                        save_markdown_file_with_parser_targets(&editor.state, &note_path, &content)
                    {
                        editor.overlays.toast =
                            Some(format!("Failed to create linked note: {}", e));
                        return Task::none();
                    }
                } else {
                    return Task::none();
                }

                ann.linked_note_path = Some(note_path.clone());
                ann.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                if let Err(e) = editor.state.save_pdf_annotation(&ann) {
                    editor.overlays.toast = Some(format!("Failed to link note: {}", e));
                } else {
                    for page_anns in editor.pdf.annotations.values_mut() {
                        if let Some(a) = page_anns.iter_mut().find(|a| a.id == annotation_id) {
                            a.linked_note_path = Some(note_path.clone());
                            a.updated_at = ann.updated_at;
                            break;
                        }
                    }
                    editor.workspace.vault_entries =
                        md_editor_core::vault::list_vault(&editor.state).unwrap_or_default();
                    if let Some(pdf_path) = &editor.pdf.active_path {
                        let _ = md_editor_core::config::set_sys_config(
                            &editor.state,
                            &pdf_companion_note_key(pdf_path),
                            &note_path,
                        );
                    }
                    editor.overlays.toast = Some(format!("Linked note: {}", note_path));
                    return Task::done(Message::Pdf(PdfMessage::OpenLinkedNote(note_path)));
                }
            }
            Task::none()
        }
        PdfMessage::OpenLinkedNote(note_path) => {
            editor.shell.split_view_active = true;
            let open_task = editor.open_file_extended(&note_path, false);
            if editor.pdf.fit_to_width {
                Task::batch(vec![
                    open_task,
                    Task::done(Message::Pdf(PdfMessage::FitToWidth)),
                ])
            } else {
                Task::batch(vec![open_task, editor.restore_scroll_positions()])
            }
        }
        PdfMessage::OpenCompanionNote(note_path) => {
            editor.shell.split_view_active = true;
            let open_task = editor.open_file_extended(&note_path, false);
            if editor.pdf.fit_to_width {
                Task::batch(vec![
                    open_task,
                    Task::done(Message::Pdf(PdfMessage::FitToWidth)),
                ])
            } else {
                Task::batch(vec![open_task, editor.restore_scroll_positions()])
            }
        }
        PdfMessage::ToggleAnnotationsSidebar => {
            if editor.pdf.active_path.is_some() {
                editor.shell.pdf_annotations_visible = !editor.shell.pdf_annotations_visible;
                editor.persist_shell_state();
            }
            Task::none()
        }
        PdfMessage::FilterAnnotationsByColor(color) => {
            editor.pdf.annotations_filter_color = color;
            Task::none()
        }
        PdfMessage::FilterAnnotationsByPage(page) => {
            editor.pdf.annotations_filter_page = page;
            Task::none()
        }
        PdfMessage::FilterAnnotationsByTag(tag) => {
            editor.pdf.annotations_filter_tag = tag;
            Task::none()
        }
        PdfMessage::FilterAnnotationsByLinked(linked) => {
            editor.pdf.annotations_filter_linked = linked;
            Task::none()
        }
        PdfMessage::FilterAnnotationsByUnresolved(unresolved) => {
            editor.pdf.annotations_filter_unresolved = unresolved;
            Task::none()
        }
        PdfMessage::ToggleAnnotationStatus(id) => {
            let mut found_ann = None;
            for page_anns in editor.pdf.annotations.values_mut() {
                if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                    ann.status = match ann.status {
                        md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved => {
                            md_editor_core::domain::pdf::PdfAnnotationStatus::Resolved
                        }
                        md_editor_core::domain::pdf::PdfAnnotationStatus::Resolved => {
                            md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved
                        }
                    };
                    ann.updated_at = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    found_ann = Some(ann.clone());
                    break;
                }
            }
            if let Some(ann) = found_ann {
                if let Err(e) = editor.state.save_pdf_annotation(&ann) {
                    editor.overlays.toast =
                        Some(format!("Failed to toggle annotation status: {}", e));
                }
            }
            Task::none()
        }
        PdfMessage::EditAnnotationTags(id) => {
            editor.pdf.focused_annotation_id = Some(id.clone());
            let mut tags_str = String::new();
            for page_anns in editor.pdf.annotations.values() {
                if let Some(ann) = page_anns.iter().find(|a| a.id == id) {
                    tags_str = ann.tags.join(", ");
                    break;
                }
            }
            editor.overlays.active_modal = Some(views::modals::ModalType::AnnotationTags(id));
            editor.overlays.modal_input = tags_str;
            Task::none()
        }
        PdfMessage::UpdateAnnotationTags(id, input) => {
            let tags = input
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>();
            let mut found_ann = None;
            for page_anns in editor.pdf.annotations.values_mut() {
                if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                    ann.tags = tags.clone();
                    ann.updated_at = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    found_ann = Some(ann.clone());
                    break;
                }
            }
            if let Some(ann) = found_ann {
                if let Err(e) = editor.state.save_pdf_annotation(&ann) {
                    editor.overlays.toast = Some(format!("Failed to save annotation tags: {}", e));
                }
            }
            Task::none()
        }
        PdfMessage::NavigateToAnnotation { id, page } => {
            editor.pdf.focused_annotation_id = Some(id);
            editor.navigate_pdf_page(page)
        }
        PdfMessage::EditAnnotationNote(id, _page) => {
            editor.pdf.focused_annotation_id = Some(id.clone());
            let mut note = String::new();
            for page_anns in editor.pdf.annotations.values() {
                if let Some(ann) = page_anns.iter().find(|a| a.id == id) {
                    note = ann.note.clone().unwrap_or_default();
                    break;
                }
            }
            editor.overlays.active_modal = Some(views::modals::ModalType::QuickNote(id));
            editor.overlays.modal_input = note;
            Task::none()
        }
        PdfMessage::ExportAnnotations => {
            let Some(ref pdf_path) = editor.pdf.active_path else {
                return Task::none();
            };
            let path_str = pdf_path.clone();
            let pdf_filename = std::path::Path::new(&path_str)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("document.pdf")
                .to_string();
            let default_name = format!(
                "{}-annotations.md",
                std::path::Path::new(&pdf_filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("document")
            );

            let mut annotations_list = Vec::new();
            for page_anns in editor.pdf.annotations.values() {
                annotations_list.extend(page_anns.clone());
            }

            Task::perform(
                async move {
                    let file = rfd::AsyncFileDialog::new()
                        .set_title("Export Annotations")
                        .set_file_name(&default_name)
                        .add_filter("Markdown", &["md", "markdown"])
                        .save_file()
                        .await;

                    if let Some(f) = file {
                        let dest_path = f.path().to_path_buf();
                        let content =
                            crate::features::pdf::annotations::export_annotations_to_markdown(
                                &pdf_filename,
                                &path_str,
                                &annotations_list,
                            );
                        match std::fs::write(&dest_path, content) {
                            Ok(()) => Ok(dest_path.to_string_lossy().to_string()),
                            Err(e) => Err(format!("Failed to write file: {}", e)),
                        }
                    } else {
                        Err("Export cancelled".to_string())
                    }
                },
                |res| Message::Pdf(PdfMessage::AnnotationsExported(res)),
            )
        }
        PdfMessage::AnnotationsExported(res) => {
            match res {
                Ok(path) => {
                    editor.overlays.toast = Some(format!("Exported to {}", path));
                }
                Err(err) => {
                    if err != "Export cancelled" {
                        editor.overlays.toast = Some(err);
                    }
                }
            }
            Task::none()
        }
        PdfMessage::RightClicked {
            page_index,
            x,
            y,
            absolute_pos,
        } => {
            editor.set_active_panel(ActivePanel::Pdf);
            let mut items = Vec::new();

            // 1. Text selection context
            if editor.pdf_selection_contains_point(page_index, x, y) {
                items.push(views::modals::PdfContextMenuItem::Copy);
                items.push(views::modals::PdfContextMenuItem::CopyAsQuote);
                items.push(views::modals::PdfContextMenuItem::CopyWithSourceLink);
                items.push(views::modals::PdfContextMenuItem::HighlightYellow);
                items.push(views::modals::PdfContextMenuItem::HighlightGreen);
                items.push(views::modals::PdfContextMenuItem::HighlightBlue);
                items.push(views::modals::PdfContextMenuItem::HighlightPink);
                items.push(views::modals::PdfContextMenuItem::HighlightOrange);
                items.push(views::modals::PdfContextMenuItem::UnderlineBlue);
                items.push(views::modals::PdfContextMenuItem::StrikeRed);
                items.push(views::modals::PdfContextMenuItem::SearchSelectedText);
                if editor.workspace.active_path.is_some() {
                    items.push(views::modals::PdfContextMenuItem::InsertQuoteLink);
                }
            }

            // 2. Annotation context
            let mut target_ann = None;
            if x < 0.0 || y < 0.0 {
                if let Some(ref ann_id) = editor.pdf.focused_annotation_id {
                    for page_anns in editor.pdf.annotations.values() {
                        if let Some(ann) = page_anns.iter().find(|a| a.id == *ann_id) {
                            target_ann = Some(ann.clone());
                            break;
                        }
                    }
                }
            } else {
                target_ann = editor.annotation_at(page_index, x, y);
            }

            if let Some(ann) = target_ann {
                items.extend(views::modals::pdf_annotation_context_menu_items(
                    &ann,
                    editor.workspace.active_path.is_some(),
                ));
            }

            // 3. Link context and preview task
            let mut preview_task = Task::none();
            if x >= 0.0 && y >= 0.0 {
                if let Some(link) = editor.pdf_link_at(page_index, x, y) {
                    items.push(views::modals::PdfContextMenuItem::OpenLink(link.clone()));
                    if let Some(ref uri) = link.uri {
                        items.push(views::modals::PdfContextMenuItem::CopyLink(uri.clone()));
                    }

                    if let Some(dest_page) = link.dest_page {
                        let dest_y = link.dest_y;
                        if let Some(ref path) = editor.pdf.active_path {
                            if let Some(abs_path) = editor.resolve_active_path(path) {
                                let abs_path = abs_path.to_string_lossy().to_string();
                                let _state = editor.state.clone();
                                preview_task = Task::perform(
                                    async move {
                                        let renderer = _state.pdf_renderer()?;
                                        renderer
                                            .render_link_preview(
                                                &abs_path,
                                                dest_page.into(),
                                                dest_y,
                                            )
                                            .ok()
                                    },
                                    |res| {
                                        Message::Pdf(PdfMessage::LinkPreviewResult(
                                            res.ok_or_else(|| "Failed to preview".into()),
                                        ))
                                    },
                                );
                            }
                        }
                    }
                }
            }

            if !items.is_empty() {
                editor.overlays.active_modal = Some(views::modals::ModalType::PdfContextMenu(
                    views::modals::PdfContextMenuState {
                        absolute_pos,
                        items,
                    },
                ));
            }

            preview_task
        }
        PdfMessage::AnnotationFocused {
            document_path,
            annotation_id,
            page,
        } => {
            let resolved_pdf_path = resolve_relative_link_path(
                editor.workspace.vault_root.as_deref(),
                editor.workspace.active_path.as_deref(),
                &document_path,
            );

            editor.shell.split_view_active = true;
            editor.pdf.showing_pdf = true;

            if editor.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
                editor.pdf.focused_annotation_id = Some(annotation_id);
                editor.navigate_pdf_page(page.saturating_sub(1))
            } else {
                editor.pdf.initial_target_page = Some(page.saturating_sub(1));
                editor.pdf.initial_target_annotation = Some(annotation_id);
                editor.open_pdf(&resolved_pdf_path)
            }
        }
        _ => Task::none(),
    }
}
