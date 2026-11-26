//! Sidebar clicks, internal/external/pdf:// link routing and sidebar keyboard navigation.

use super::*;
use crate::messages::{SidebarNavKey, VaultMessage};

impl MdEditor {
    /// Route clicks from the sidebar, backlinks, wikilinks and `pdf://` links.
    pub(super) fn on_sidebar_file_clicked(&mut self, path: String) -> Task<Message> {
        let path = path.trim().to_string();
        self.graph.visible = false;
        self.tracker.hide();
        self.ui.settings_visible = false;
        self.ui.context_menu = None;

        if let Some(url_str) = path.strip_prefix("pdf://") {
            return self.open_pdf_protocol_link(url_str);
        }

        let is_url =
            path.starts_with("http://") || path.starts_with("https://") || path.contains("://");
        if is_url {
            crate::platform::open_external(&path);
            return Task::none();
        }

        if let Some((file_part, anchor_part)) = path.split_once('#') {
            return self.open_anchored_vault_link(file_part.trim(), anchor_part.trim());
        }

        let resolved_path = self.resolve_internal_link_path(&path);
        self.vault.selected_path = Some(resolved_path.clone());
        self.open_vault_entry_by_kind(&resolved_path)
    }

    fn open_pdf_protocol_link(&mut self, url_str: &str) -> Task<Message> {
        let crate::pdf_notes::PdfLink {
            path: pdf_path,
            page,
            annotation: annotation_id,
            ..
        } = crate::pdf_notes::parse_pdf_link(url_str);

        let resolved_pdf_path = resolve_relative_link_path(
            self.vault.root.as_deref(),
            self.active_path.as_deref(),
            &pdf_path,
        );

        // Beside the note the link was followed from.
        let beside = self.active_path.is_some() && !self.showing_pdf;
        let (pane, focus) = self.focus_pane_for(&resolved_pdf_path, beside);
        let task = self.show_pdf_at(&resolved_pdf_path, page, annotation_id);
        workspace::route_to(pane, Task::batch(vec![focus, task]))
    }

    /// Show `path` in the pane swapped in, at `page` (1-based) or the
    /// highlight `annotation_id`: navigating if it is loaded, opening it if not.
    pub(super) fn show_pdf_at(
        &mut self,
        path: &str,
        page: Option<u16>,
        annotation_id: Option<String>,
    ) -> Task<Message> {
        self.showing_pdf = true;
        self.active_panel = ActivePanel::Pdf;
        if self.pdf.active_path.as_deref() == Some(path) {
            self.tabs.open(path, crate::tabs::TabKind::Pdf, false);
            self.persist_tabs();
            self.pdf.focused_annotation_id = annotation_id;
            if let Some(p) = page {
                self.navigate_pdf_page(p.saturating_sub(1))
            } else if let Some(ref ann_id) = self.pdf.focused_annotation_id {
                let target_page =
                    self.pdf.annotations.iter().find_map(|(idx, anns)| {
                        anns.iter().any(|a| &a.id == ann_id).then_some(*idx)
                    });
                target_page
                    .map(|tp| self.navigate_pdf_page(tp))
                    .unwrap_or_else(Task::none)
            } else {
                Task::none()
            }
        } else {
            self.pdf.initial_target_page = page.map(|p| p.saturating_sub(1));
            self.pdf.initial_target_annotation = annotation_id;
            let saved = self.tabs.find_by_path(path).map(|t| t.view.clone());
            self.open_pdf_restoring_view(path, saved)
        }
    }

    fn open_anchored_vault_link(&mut self, file_part: &str, anchor_part: &str) -> Task<Message> {
        let target_slug = slugify(anchor_part);
        if file_part.is_empty() {
            if let Some(line_idx) = find_heading_or_widget_line(
                &self.editor.buffer.text(),
                &self.editor.highlight.lines,
                &target_slug,
            ) {
                self.editor.request_reveal(Reveal::Center);
                return self.run_editor_command(EditorCommand::SetCursor {
                    line: line_idx,
                    col: 0,
                    affinity: Affinity::Downstream,
                });
            }
            self.ui.toast = Some(format!("Heading or widget not found: #{anchor_part}"));
            return Task::none();
        }

        let resolved_file = self.resolve_internal_link_path(file_part);
        self.vault.selected_path = Some(resolved_file.clone());
        let open_task = self.open_file_extended(&resolved_file, false);
        if let Some(line_idx) = find_heading_or_widget_line(
            &self.editor.buffer.text(),
            &self.editor.highlight.lines,
            &target_slug,
        ) {
            self.editor.request_reveal(Reveal::Center);
            let cmd_task = self.run_editor_command(EditorCommand::SetCursor {
                line: line_idx,
                col: 0,
                affinity: Affinity::Downstream,
            });
            Task::batch(vec![open_task, cmd_task])
        } else {
            self.editor.scroll_y = 0.0;
            let scroll_task = operation::scroll_to(
                workspace::current_pane().editor_scrollable_id(),
                AbsoluteOffset { x: 0.0, y: 0.0 },
            );
            Task::batch(vec![open_task, scroll_task])
        }
    }

    fn open_vault_entry_by_kind(&mut self, resolved_path: &str) -> Task<Message> {
        // Each file lives in one pane: open in the other pane, focus it there.
        if let Some(pane) = self
            .pane_holding(resolved_path)
            .filter(|pane| *pane != self.workspace.current)
        {
            return self.move_to_pane(resolved_path, pane, true);
        }
        let lower = resolved_path.to_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            self.showing_pdf = false;
            self.open_file(resolved_path)
        } else if lower.ends_with(".pdf") {
            self.pdf.active_path = Some(resolved_path.to_string());
            self.showing_pdf = true;
            let saved = self
                .tabs
                .find_by_path(resolved_path)
                .map(|t| t.view.clone());
            self.open_pdf_restoring_view(resolved_path, saved)
        } else if is_supported_image_path(&lower) {
            self.open_image(resolved_path)
        } else if lower.ends_with(".ink") {
            self.open_ink_page(resolved_path)
        } else {
            Task::none()
        }
    }

    /// Keyboard navigation in the file tree (`↑↓` move, `←→` collapse/expand, `Enter` open).
    pub(super) fn on_sidebar_key_nav(&mut self, nav: SidebarNavKey) -> Task<Message> {
        let rows = self.vault.visible_rows();
        if rows.is_empty() {
            return Task::none();
        }
        let current_idx = self
            .vault
            .selected_path
            .as_deref()
            .and_then(|sel| rows.iter().position(|(p, _)| p == sel));

        match nav {
            SidebarNavKey::Up => {
                let next = current_idx.map_or(0, |i| i.saturating_sub(1));
                self.vault.selected_path = Some(rows[next].0.clone());
                Task::none()
            }
            SidebarNavKey::Down => {
                let next = current_idx.map_or(0, |i| (i + 1).min(rows.len() - 1));
                self.vault.selected_path = Some(rows[next].0.clone());
                Task::none()
            }
            SidebarNavKey::Left => {
                if let Some(idx) = current_idx {
                    let (ref path, is_dir) = rows[idx];
                    if is_dir && self.vault.expanded_folders.contains(path) {
                        self.vault.expanded_folders.remove(path);
                    } else if let Some(parent_sep) = path.rfind('/') {
                        self.vault.selected_path = Some(path[..parent_sep].to_string());
                    }
                }
                Task::none()
            }
            SidebarNavKey::Right => {
                if let Some(idx) = current_idx {
                    let (ref path, is_dir) = rows[idx];
                    if is_dir {
                        self.vault.expanded_folders.insert(path.clone());
                    }
                }
                Task::none()
            }
            SidebarNavKey::Enter => {
                if let Some(idx) = current_idx {
                    let (path, is_dir) = rows[idx].clone();
                    if is_dir {
                        self.vault
                            .update(Message::Vault(VaultMessage::SidebarFolderToggled(path)))
                    } else {
                        self.on_sidebar_file_clicked(path)
                    }
                } else {
                    Task::none()
                }
            }
        }
    }
}
