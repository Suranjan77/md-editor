use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::link_note_picker;

#[derive(Debug, Clone, PartialEq)]
pub enum PdfContextMenuItem {
    Copy,
    CopyAsQuote,
    CopyWithSourceLink,
    HighlightYellow,
    HighlightGreen,
    HighlightBlue,
    HighlightPink,
    HighlightOrange,
    UnderlineBlue,
    StrikeRed,
    SearchSelectedText,
    InsertQuoteLink,
    InsertAnnotationLink { id: String, page: u16 },
    EditNote { id: String, page: u16 },
    LinkToNote { id: String, page: u16 },
    OpenLinkedNote(String),
    DeleteHighlight(String),
    OpenLink(md_editor_core::pdf::LinkInfo),
    CopyLink(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfContextMenuState {
    pub absolute_pos: iced::Point,
    pub items: Vec<PdfContextMenuItem>,
}

pub fn pdf_annotation_context_menu_items(
    ann: &md_editor_core::pdf::PdfAnnotation,
    markdown_open: bool,
) -> Vec<PdfContextMenuItem> {
    let mut items = Vec::new();
    if markdown_open {
        items.push(PdfContextMenuItem::InsertAnnotationLink {
            id: ann.id.clone(),
            page: ann.page_index,
        });
    }
    items.push(PdfContextMenuItem::EditNote {
        id: ann.id.clone(),
        page: ann.page_index,
    });
    if let Some(path) = ann.linked_note_path.as_deref().filter(|p| !p.is_empty()) {
        items.push(PdfContextMenuItem::OpenLinkedNote(path.to_string()));
    } else {
        items.push(PdfContextMenuItem::LinkToNote {
            id: ann.id.clone(),
            page: ann.page_index,
        });
    }
    items.push(PdfContextMenuItem::DeleteHighlight(ann.id.clone()));
    items
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModalType {
    PdfContextMenu(PdfContextMenuState),
    CreateFile,
    CreateFolder,
    Delete(String),         // path
    QuickNote(String),      // annotation ID
    LinkNote(String),       // annotation ID
    AnnotationTags(String), // annotation ID
    GoToPage { total: u16, error: Option<String> },
}

/// Go-to-page dialog — separate inline render so the
/// text_input sits at the top level and can receive focus.
fn view_go_to_page<'a>(
    total_pages: u16,
    error: Option<&'a str>,
    input_value: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    let input_box = container(
        text_input("", input_value)
            .on_input(Message::NameModalInputChanged)
            .on_submit(Message::NameModalSubmitCurrent)
            .padding(10)
            .width(Length::Fixed(120.0)),
    );

    let input_box = if error.is_some() {
        input_box.style(|_| container::Style {
            border: iced::Border {
                color: theme::DANGER,
                width: 1.5,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
    } else {
        input_box
    };

    container(
        container(
            column![
                text("Go to Page").size(18).color(theme::TEXT_PRIMARY),
                input_box,
                text(format!("/ {}", total_pages))
                    .size(16)
                    .color(theme::TEXT_MUTED),
                if let Some(err) = error {
                    text(err).size(12).color(theme::DANGER)
                } else {
                    text("Press Enter to navigate, Esc to cancel.")
                        .size(11)
                        .color(theme::TEXT_MUTED)
                },
                row![
                    button(text("Cancel").size(14))
                        .on_press(Message::NameModalCancel)
                        .padding([8, 20])
                        .style(button::text),
                    button(text("Go").size(14))
                        .on_press(Message::NameModalSubmitCurrent)
                        .padding([8, 20])
                        .style(button::primary),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .padding(24),
        )
        .width(Length::Fixed(320.0))
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_SECONDARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.7,
        ))),
        ..Default::default()
    })
    .into()
}

pub(crate) fn view_context_menu<'a>(
    state: &PdfContextMenuState,
) -> Element<'a, Message, Theme, Renderer> {
    use iced::widget::Space;
    let mut menu_col = column![].spacing(1);

    for item in &state.items {
        let label = match item {
            PdfContextMenuItem::Copy => "Copy",
            PdfContextMenuItem::CopyAsQuote => "Copy as quote",
            PdfContextMenuItem::CopyWithSourceLink => "Copy quote with PDF link",
            PdfContextMenuItem::HighlightYellow => "Highlight: Yellow",
            PdfContextMenuItem::HighlightGreen => "Highlight: Green",
            PdfContextMenuItem::HighlightBlue => "Highlight: Blue",
            PdfContextMenuItem::HighlightPink => "Highlight: Pink",
            PdfContextMenuItem::HighlightOrange => "Highlight: Orange",
            PdfContextMenuItem::UnderlineBlue => "Underline: Blue",
            PdfContextMenuItem::StrikeRed => "Strikeout: Red",
            PdfContextMenuItem::SearchSelectedText => "Search selected text",
            PdfContextMenuItem::InsertQuoteLink => "Insert quote in note",
            PdfContextMenuItem::InsertAnnotationLink { .. } => "Insert highlight in note",
            PdfContextMenuItem::EditNote { .. } => "Edit short note",
            PdfContextMenuItem::LinkToNote { .. } => "Link markdown note",
            PdfContextMenuItem::OpenLinkedNote(_) => "Open markdown note",
            PdfContextMenuItem::DeleteHighlight(_) => "Delete highlight",
            PdfContextMenuItem::OpenLink(_) => "Open Link",
            PdfContextMenuItem::CopyLink(_) => "Copy Link",
        };

        menu_col = menu_col.push(
            button(text(label).size(12).color(theme::TEXT_PRIMARY))
                .on_press(Message::PdfContextMenuAction(item.clone()))
                .padding([6, 12])
                .style(button::text)
                .width(Length::Fill),
        );
    }

    let menu_card = container(menu_col)
        .width(Length::Fixed(220.0))
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_SECONDARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        });

    let x = state.absolute_pos.x;
    let y = state.absolute_pos.y;

    let content = row![
        Space::new().width(Length::Fixed(x.max(0.0))),
        column![Space::new().height(Length::Fixed(y.max(0.0))), menu_card,]
    ];

    container(
        button(content)
            .on_press(Message::NameModalCancel)
            .style(button::text)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(0),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

pub fn view<'a>(
    modal_type: &'a ModalType,
    input_value: &'a str,
    picker_search: &str,
    vault_entries: &'a [md_editor_core::types::FileEntry],
) -> Element<'a, Message, Theme, Renderer> {
    if let ModalType::PdfContextMenu(state) = modal_type {
        return view_context_menu(state);
    }

    // GoToPage renders its own full-screen overlay with focused input.
    if let ModalType::GoToPage { total, error } = modal_type {
        return view_go_to_page(*total, error.as_deref(), input_value);
    }

    let title = match modal_type {
        ModalType::GoToPage { .. } => "", // unreachable — handled above
        ModalType::PdfContextMenu(_) => "",
        ModalType::CreateFile => "Create New File",
        ModalType::CreateFolder => "Create New Folder",
        ModalType::Delete(_) => "Delete Confirmation",
        ModalType::QuickNote(_) => "Short Note",
        ModalType::LinkNote(_) => "Link Markdown Note",
        ModalType::AnnotationTags(_) => "Edit Annotation Tags",
    };

    let content: Element<'a, Message, Theme, Renderer> = match modal_type {
        ModalType::Delete(path) => column![
            text(format!("Are you sure you want to delete '{}'?", path)).color(theme::TEXT_PRIMARY),
            text("This action cannot be undone.")
                .size(12)
                .color(theme::TEXT_MUTED),
            row![
                button(text("Cancel").size(14))
                    .on_press(Message::NameModalCancel)
                    .padding([8, 20])
                    .style(button::text),
                button(text("Delete").size(14))
                    .on_press(Message::DeleteFile(path.clone()))
                    .padding([8, 20])
                    .style(button::secondary),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(20)
        .into(),
        ModalType::LinkNote(_) => link_note_picker::view(input_value, picker_search, vault_entries),
        _ => {
            let (placeholder, confirm_label) = match modal_type {
                ModalType::QuickNote(_) => ("Write a short note...", "Save Note"),
                ModalType::AnnotationTags(_) => ("tag1, tag2, ...", "Save Tags"),
                ModalType::CreateFile => ("File name...", "Create File"),
                ModalType::CreateFolder => ("Folder name...", "Create Folder"),
                _ => ("Enter name...", "Confirm"),
            };
            column![
                text(title).size(18).color(theme::TEXT_PRIMARY),
                text_input(placeholder, input_value)
                    .on_input(Message::NameModalInputChanged)
                    .padding(10),
                row![
                    button(text("Cancel").size(14))
                        .on_press(Message::NameModalCancel)
                        .padding([8, 20])
                        .style(button::text),
                    button(text(confirm_label).size(14))
                        .on_press(Message::NameModalSubmit(input_value.to_string()))
                        .padding([8, 20])
                        .style(button::primary),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
            ]
            .spacing(20)
            .into()
        }
    };

    container(
        container(content)
            .width(Length::Fixed(match modal_type {
                ModalType::LinkNote(_) => 560.0,
                _ => 400.0,
            }))
            .padding(30)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_SECONDARY)),
                border: iced::Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.7,
        ))),
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind};

    fn annotation(linked_note_path: Option<&str>) -> PdfAnnotation {
        PdfAnnotation {
            id: "ann-1".to_string(),
            document_id: "doc".to_string(),
            page_index: 4,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: "Important highlight".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: linked_note_path.map(str::to_string),
            markdown_anchor: None,
            tags: Vec::new(),
            status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn annotation_context_menu_includes_insert_only_when_markdown_open() {
        let ann = annotation(None);

        assert!(
            pdf_annotation_context_menu_items(&ann, true)
                .iter()
                .any(|item| matches!(item, PdfContextMenuItem::InsertAnnotationLink { id, page } if id == "ann-1" && *page == 4))
        );

        assert!(
            !pdf_annotation_context_menu_items(&ann, false)
                .iter()
                .any(|item| matches!(item, PdfContextMenuItem::InsertAnnotationLink { .. }))
        );
    }

    #[test]
    fn annotation_context_menu_prefers_open_linked_note_when_present() {
        let ann = annotation(Some("notes/highlight.md"));
        let items = pdf_annotation_context_menu_items(&ann, true);

        assert!(items.iter().any(|item| {
            matches!(item, PdfContextMenuItem::OpenLinkedNote(path) if path == "notes/highlight.md")
        }));
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, PdfContextMenuItem::LinkToNote { .. }))
        );
    }

    #[test]
    fn annotation_context_menu_insert_click_emits_action_message() {
        let state = PdfContextMenuState {
            absolute_pos: iced::Point::ORIGIN,
            items: pdf_annotation_context_menu_items(&annotation(None), true),
        };
        let mut ui = iced_test::simulator(view_context_menu(&state));

        ui.click("Insert highlight in note")
            .expect("annotation insert item should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::PdfContextMenuAction(PdfContextMenuItem::InsertAnnotationLink { id, page })]
                if id == "ann-1" && *page == 4
        ));
    }
}
