//! Overlays above the shell: drop hint, tab overflow menu, context menus, vault search, command palette, modals, PDF link preview and toasts.

use super::*;
use crate::design::metrics::{
    DIALOG_WIDTH_LG, IconSize, RADIUS_MD, SPACE_2, SPACE_3, SPACE_4, SPACE_5,
};
use crate::design::{TextRole, fade, tokens};
use crate::messages::{
    ContextMenuTarget, EditorMessage, Message, PdfMessage, ShellMessage, Shortcut, TabsMessage,
    VaultMessage,
};
use crate::tabs::TabKind;
use crate::ui::{Icon, IconButton, MenuEntry, popover, positioned_context_menu};
use iced::widget::column;

impl MdEditor {
    pub(super) fn append_overlays<'a>(
        &'a self,
        layers: &mut Vec<Element<'a, Message, Theme, iced::Renderer>>,
    ) {
        let t = tokens();

        if !self.ui.hovered_drop_files.is_empty() {
            layers.push(
                container(popover(
                    row![
                        crate::ui::icon(Icon::ArrowSquareOut, IconSize::Md.px(), t.accent),
                        text("Drop files to import into vault or insert at cursor")
                            .size(TextRole::Caption.size())
                            .color(t.fg_primary)
                            .font(TextRole::Caption.font()),
                    ]
                    .spacing(SPACE_2)
                    .align_y(Alignment::Center),
                    None,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(SPACE_5)
                .align_x(Alignment::Center)
                .align_y(iced::alignment::Vertical::Top)
                .into(),
            );
        }

        if self.ui.tab_overflow_open {
            let entries: Vec<MenuEntry<Message>> = self
                .tabs
                .list()
                .iter()
                .map(|tab| MenuEntry::Item {
                    icon: Some(match tab.kind {
                        TabKind::Note => Icon::FileText,
                        TabKind::Pdf => Icon::FilePdf,
                        TabKind::Image => Icon::Image,
                    }),
                    label: tab.path.clone(),
                    shortcut: None,
                    danger: false,
                    on_select: Some(Message::Tabs(TabsMessage::Activate(tab.id))),
                })
                .collect();
            layers.push(positioned_context_menu(
                entries,
                (self.ui.window_width - 260.0).max(SPACE_4),
                84.0,
                self.ui.window_width,
                self.ui.window_height,
                Message::Shell(ShellMessage::CloseContextMenu),
            ));
        }

        if let Some(ref ctx) = self.ui.context_menu {
            layers.push(self.build_context_menu(ctx));
        }

        if self.search.visible {
            layers.push(
                container(views::search::view(views::search::SearchProps {
                    query: &self.search.query,
                    regex: self.search.regex,
                    match_case: self.search.match_case,
                    current_match_count: self.search.match_count(),
                    selected: self.search.global_selected,
                    results: &self.search.results,
                    results_truncated: self.search.results_truncated,
                    pdf_results: &self.search.pdf_results,
                    pdf_error: self.search.pdf_error.as_deref(),
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .padding(SPACE_5 * 3.0)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.bg_scrim)),
                    ..Default::default()
                })
                .into(),
            );
        }

        let palette_opacity = self.motion.palette_opacity();
        if palette_opacity > 0.01 {
            layers.push(
                mouse_area(
                    container(
                        mouse_area(views::command_palette::view(
                            &self.ui.command_palette_query,
                            &self.ui.commands,
                            &self.vault.entries,
                            self.ui.palette_selected,
                            palette_opacity,
                        ))
                        .on_press(Message::Shell(ShellMessage::CloseContextMenu)),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(fade(
                            t.bg_scrim,
                            palette_opacity,
                        ))),
                        ..Default::default()
                    }),
                )
                .on_press(Message::KeyboardShortcut(Shortcut::Escape))
                .into(),
            );
        }

        if let Some(modal_type) = &self.ui.active_modal {
            layers.push(views::modals::view(
                modal_type,
                &self.ui.modal_input,
                self.ui.modal_error.as_deref(),
                &self.ui.link_note_picker_search,
                &self.vault.entries,
            ));
        }

        if let Some(preview_handle) = &self.pdf.link_preview {
            layers.push(self.build_pdf_link_preview(preview_handle));
        }

        let has_persistent = self.ui.toast_queue.iter().any(|t| t.persistent);
        let toast_opacity = self.motion.toast_opacity();
        if toast_opacity > 0.01 || has_persistent {
            layers.push(
                container(views::toast::view_stack(
                    &self.ui.toast_queue,
                    &self.motion.toast_text,
                    toast_opacity.max(if has_persistent { 1.0 } else { 0.0 }),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(SPACE_4)
                .into(),
            );
        }
    }

    fn build_context_menu<'a>(
        &'a self,
        ctx: &'a crate::ui_state::ActiveContextMenu,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        let entries: Vec<MenuEntry<Message>> = match &ctx.target {
            ContextMenuTarget::SidebarEntry { path, is_dir } => {
                Self::sidebar_entry_menu(path, *is_dir)
            }
            ContextMenuTarget::SidebarEmpty => Self::sidebar_empty_menu(),
            ContextMenuTarget::Tab(id) => self.tab_menu(*id),
            ContextMenuTarget::Editor => Self::editor_menu(),
        };

        positioned_context_menu(
            entries,
            ctx.x,
            ctx.y,
            self.ui.window_width,
            self.ui.window_height,
            Message::Shell(ShellMessage::CloseContextMenu),
        )
    }

    fn build_pdf_link_preview<'a>(
        &'a self,
        preview_handle: &'a iced::widget::image::Handle,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        let t = tokens();
        let (img_w, img_h) = self.pdf.link_preview_size.unwrap_or((800.0, 600.0));
        let aspect = (img_w / img_h).max(0.1);
        let max_w = self.preview_max_box_width().min(DIALOG_WIDTH_LG);
        let max_h = (self.ui.window_height * 0.75).clamp(280.0, 720.0);
        let mut box_w = max_w;
        let mut box_h = box_w / aspect;
        if box_h > max_h {
            box_h = max_h;
            box_w = box_h * aspect;
        }

        let img = iced::widget::image(preview_handle.clone())
            .filter_method(iced::widget::image::FilterMethod::Linear)
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(iced::ContentFit::Contain);

        let card_box = popover(
            column![
                row![
                    text("Reference preview")
                        .size(TextRole::Caption.size())
                        .color(t.fg_secondary)
                        .font(TextRole::Caption.font()),
                    Space::new().width(Length::Fill),
                    IconButton::new(Icon::X, "Close preview")
                        .shortcut("Esc")
                        .on_press(Message::Pdf(PdfMessage::ClosePdfLinkPreview))
                        .build(),
                ]
                .align_y(Alignment::Center),
                container(img)
                    .width(Length::Fixed(box_w))
                    .height(Length::Fixed(box_h))
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(iced::Color::WHITE)),
                        border: iced::Border {
                            radius: RADIUS_MD.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            ]
            .spacing(SPACE_3),
            Some(DIALOG_WIDTH_LG),
        );

        mouse_area(
            container(
                mouse_area(card_box).on_press(Message::Shell(ShellMessage::CloseContextMenu)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_scrim)),
                ..Default::default()
            }),
        )
        .on_press(Message::Pdf(PdfMessage::ClosePdfLinkPreview))
        .into()
    }

    fn sidebar_entry_menu(path: &str, is_dir: bool) -> Vec<MenuEntry<Message>> {
        let path = path.to_string();
        let folder = if is_dir {
            path.clone()
        } else {
            path.rfind('/')
                .map(|i| path[..i].to_string())
                .unwrap_or_default()
        };
        let mut entries = Vec::new();
        if !is_dir && crate::tabs::TabKind::from_path(&path).is_some() {
            entries.push(MenuEntry::Item {
                icon: Some(Icon::Split),
                label: "Open to the side".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::OpenToSide(path.clone()))),
            });
            entries.push(MenuEntry::Separator);
        }
        entries.extend(vec![
            MenuEntry::Item {
                icon: Some(Icon::FilePlus),
                label: "New note here".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::NewNoteInFolder(
                    folder.clone(),
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::FolderPlus),
                label: "New folder here".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::NewFolderInFolder(folder))),
            },
            MenuEntry::Separator,
            MenuEntry::Item {
                icon: Some(Icon::Pencil),
                label: "Rename".into(),
                shortcut: Some("F2".into()),
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::RenameEntryDialog(
                    path.clone(),
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::FolderOpen),
                label: "Reveal in file manager".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::RevealInFileManager(
                    path.clone(),
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::Copy),
                label: "Copy relative path".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::CopyRelativePath(path.clone()))),
            },
            MenuEntry::Separator,
            MenuEntry::Item {
                icon: Some(Icon::Trash),
                label: "Delete".into(),
                shortcut: None,
                danger: true,
                on_select: Some(Message::Vault(VaultMessage::DeleteFileDialog(path.clone()))),
            },
        ]);
        entries
    }

    fn sidebar_empty_menu() -> Vec<MenuEntry<Message>> {
        vec![
            MenuEntry::Item {
                icon: Some(Icon::FilePlus),
                label: "New note".into(),
                shortcut: Some("Ctrl N".into()),
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::CreateFileDialog)),
            },
            MenuEntry::Item {
                icon: Some(Icon::FolderPlus),
                label: "New folder".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Vault(VaultMessage::CreateFolderDialog)),
            },
        ]
    }

    fn tab_menu(&self, id: crate::tabs::TabId) -> Vec<MenuEntry<Message>> {
        let tab_path = self
            .tabs
            .get(id)
            .map(|t| t.path.clone())
            .unwrap_or_default();
        vec![
            MenuEntry::Item {
                icon: Some(Icon::X),
                label: "Close".into(),
                shortcut: Some("Ctrl W".into()),
                danger: false,
                on_select: Some(Message::Tabs(TabsMessage::Close(id))),
            },
            MenuEntry::Item {
                icon: None,
                label: "Close others".into(),
                shortcut: None,
                danger: false,
                on_select: (self.tabs.len() > 1)
                    .then(|| Message::Tabs(TabsMessage::CloseOthers(id))),
            },
            MenuEntry::Item {
                icon: Some(Icon::Split),
                label: if self.workspace.is_split() {
                    "Move to other pane".into()
                } else {
                    "Move to new pane".into()
                },
                shortcut: Some("Ctrl Shift M".into()),
                danger: false,
                on_select: Some(Message::Tabs(TabsMessage::MoveToOtherPane(id))),
            },
            MenuEntry::Separator,
            MenuEntry::Item {
                icon: Some(Icon::FolderOpen),
                label: "Reveal in sidebar".into(),
                shortcut: None,
                danger: false,
                on_select: (!tab_path.is_empty())
                    .then(|| Message::Vault(VaultMessage::SidebarFileClicked(tab_path.clone()))),
            },
            MenuEntry::Item {
                icon: Some(Icon::Copy),
                label: "Copy relative path".into(),
                shortcut: None,
                danger: false,
                on_select: (!tab_path.is_empty())
                    .then(|| Message::Vault(VaultMessage::CopyRelativePath(tab_path))),
            },
        ]
    }

    fn editor_menu() -> Vec<MenuEntry<Message>> {
        vec![
            MenuEntry::Item {
                icon: Some(Icon::Clipboard),
                label: "Paste".into(),
                shortcut: Some("Ctrl V".into()),
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Paste)),
            },
            MenuEntry::Separator,
            MenuEntry::Item {
                icon: Some(Icon::TextB),
                label: "Bold".into(),
                shortcut: Some("Ctrl B".into()),
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::FormatBold,
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::TextItalic),
                label: "Italic".into(),
                shortcut: Some("Ctrl I".into()),
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::FormatItalic,
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::Code),
                label: "Inline Code".into(),
                shortcut: Some("Ctrl `".into()),
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::FormatInlineCode,
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::Link),
                label: "Insert Link".into(),
                shortcut: Some("Ctrl K".into()),
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::InsertLink,
                ))),
            },
            MenuEntry::Separator,
            MenuEntry::Item {
                icon: Some(Icon::Image),
                label: "Insert Image…".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::InsertImageDialog)),
            },
            MenuEntry::Item {
                icon: Some(Icon::Table),
                label: "Insert Table".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::InsertTable,
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::Math),
                label: "Insert Math Block".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::InsertMath,
                ))),
            },
            MenuEntry::Item {
                icon: Some(Icon::CheckSquare),
                label: "Insert Checkbox".into(),
                shortcut: None,
                danger: false,
                on_select: Some(Message::Editor(EditorMessage::Command(
                    EditorCommand::InsertCheckbox,
                ))),
            },
        ]
    }
}

/// Open a context menu where the right click happened: views build the
/// message without a position, `ui::context_anchor` supplies it.
pub(super) fn anchor_context_menu(message: Message, at: iced::Point) -> Message {
    match message {
        Message::Shell(ShellMessage::OpenContextMenu(target, _, _)) => {
            Message::Shell(ShellMessage::OpenContextMenu(target, at.x, at.y))
        }
        Message::Editor(EditorMessage::OpenContextMenu(_, _)) => {
            Message::Editor(EditorMessage::OpenContextMenu(at.x, at.y))
        }
        // From a pane's view: anchor what it carries.
        Message::FromPane(pane, inner) => {
            Message::FromPane(pane, Box::new(anchor_context_menu(*inner, at)))
        }
        other => other,
    }
}
