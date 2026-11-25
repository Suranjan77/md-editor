//! Search surfaces: the in-note find bar (with replace row) and the vault-wide search overlay.

use iced::widget::{Column, Space, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, Elevation, PALETTE_MAX_LIST_HEIGHT, PALETTE_WIDTH, RADIUS_LG,
    SPACE_1, SPACE_2, SPACE_3,
};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, PdfMessage, SearchMessage, Shortcut};
use crate::shortcuts;
use crate::ui::{
    Button, ButtonVariant, Icon, IconButton, ListRowProps, TextField, empty_state, list_row,
    toolbar_frame,
};

pub const FILE_SEARCH_INPUT_ID: &str = "file_search_input";
pub const GLOBAL_SEARCH_INPUT_ID: &str = "global_search_input";

/// What the in-note find bar shows.
pub struct FileBarProps<'a> {
    pub query: &'a str,
    pub replace: &'a str,
    pub replace_visible: bool,
    pub regex: bool,
    pub match_case: bool,
    pub match_count: usize,
    pub active_match: Option<usize>,
}

fn caption<'a>(content: String) -> Element<'a, Message, Theme, Renderer> {
    let role = TextRole::MonoCaption;
    text(content)
        .size(role.size())
        .font(role.font())
        .color(tokens().fg_secondary)
        .into()
}

fn match_counter<'a>(
    query: &str,
    count: usize,
    active: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    caption(match active {
        Some(index) if count > 0 => format!("{} of {count}", index + 1),
        _ if query.is_empty() => String::new(),
        _ if count == 0 => "No matches".to_string(),
        _ => format!("{count} matches"),
    })
}

fn search_toggles<'a>(regex: bool, match_case: bool) -> [Element<'a, Message, Theme, Renderer>; 2] {
    [
        IconButton::new(Icon::TextAa, "Match case")
            .size(ControlSize::Sm)
            .selected(match_case)
            .on_press(Message::Search(SearchMessage::MatchCaseToggled(
                !match_case,
            )))
            .build(),
        IconButton::new(Icon::Regex, "Regular expression")
            .size(ControlSize::Sm)
            .selected(regex)
            .on_press(Message::Search(SearchMessage::RegexToggled(!regex)))
            .build(),
    ]
}

/// The find bar above a note: query, toggles, counter, previous / next, and
/// an expandable replace row.
pub fn file_bar<'a>(props: FileBarProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    let [case, regex] = search_toggles(props.regex, props.match_case);
    let expander = IconButton::new(
        if props.replace_visible {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        "Toggle replace",
    )
    .size(ControlSize::Sm)
    .selected(props.replace_visible)
    .on_press(Message::Search(SearchMessage::ToggleReplaceRow))
    .build();

    let find_row = toolbar_frame(
        vec![
            expander,
            TextField::search("Find in note", props.query)
                .id(iced::advanced::widget::Id::new(FILE_SEARCH_INPUT_ID))
                .size(ControlSize::Sm)
                .on_input(|value| Message::Search(SearchMessage::QueryChanged(value)))
                .on_submit(Message::Search(SearchMessage::Next))
                .on_clear(Message::Search(SearchMessage::QueryChanged(String::new())))
                .build(),
            case,
            regex,
            match_counter(props.query, props.match_count, props.active_match),
            IconButton::new(Icon::ChevronUp, "Previous match")
                .size(ControlSize::Sm)
                .shortcut("Shift Enter")
                .on_press(Message::Search(SearchMessage::Previous))
                .build(),
            IconButton::new(Icon::ChevronDown, "Next match")
                .size(ControlSize::Sm)
                .shortcut("Enter")
                .on_press(Message::Search(SearchMessage::Next))
                .build(),
            IconButton::new(Icon::X, "Close find")
                .size(ControlSize::Sm)
                .shortcut("Esc")
                .on_press(Message::Search(SearchMessage::Close))
                .build(),
        ],
        false,
    );
    if !props.replace_visible {
        return find_row;
    }

    let has_match = props.match_count > 0;
    let replace_row = toolbar_frame(
        vec![
            Space::new()
                .width(Length::Fixed(ControlSize::Sm.height()))
                .into(),
            TextField::new("Replace with", props.replace)
                .size(ControlSize::Sm)
                .on_input(|value| Message::Search(SearchMessage::ReplaceChanged(value)))
                .on_submit(Message::Search(SearchMessage::ReplaceOne))
                .build(),
            Button::new("Replace")
                .size(ControlSize::Sm)
                .on_press_maybe(has_match.then_some(Message::Search(SearchMessage::ReplaceOne)))
                .tooltip("Replace this match and go to the next", Some("Enter"))
                .build(),
            Button::new("Replace all")
                .size(ControlSize::Sm)
                .variant(ButtonVariant::Ghost)
                .on_press_maybe(has_match.then_some(Message::Search(SearchMessage::ReplaceAll)))
                .build(),
        ],
        false,
    );
    column![find_row, replace_row].into()
}

/// Everything the vault search overlay renders from.
pub struct SearchProps<'a> {
    pub query: &'a str,
    pub regex: bool,
    pub match_case: bool,
    /// Matches in the open note, for the summary line.
    pub current_match_count: usize,
    /// Keyboard selection across PDF results, then note results.
    pub selected: usize,
    pub results: &'a [md_editor_core::types::SearchResult],
    pub results_truncated: bool,
    pub pdf_results: &'a [md_editor_core::pdf::PdfSearchMatch],
    pub pdf_error: Option<&'a str>,
}

fn section_label<'a>(label: String) -> Element<'a, Message, Theme, Renderer> {
    let role = TextRole::Label;
    container(
        text(label)
            .size(role.size())
            .font(role.font())
            .color(tokens().fg_tertiary),
    )
    .padding([SPACE_1, SPACE_3])
    .into()
}

/// The vault search overlay (`Ctrl+Shift+F`): query and toggles, then PDF
/// matches in the open document and note matches across the vault. ↑/↓ move
/// the selection, Enter opens it.
pub fn view<'a>(props: SearchProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let [case, regex] = search_toggles(props.regex, props.match_case);
    let header = row![
        TextField::search("Search notes and the open PDF", props.query)
            .id(iced::advanced::widget::Id::new(GLOBAL_SEARCH_INPUT_ID))
            .on_input(|value| Message::Search(SearchMessage::QueryChanged(value)))
            .on_submit(Message::Search(SearchMessage::GlobalSearchSubmit))
            .on_clear(Message::Search(SearchMessage::QueryChanged(String::new())))
            .build(),
        case,
        regex,
        IconButton::new(Icon::X, "Close search")
            .shortcut(shortcuts::chord(Shortcut::Escape))
            .on_press(Message::Search(SearchMessage::Close))
            .build(),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);

    let mut list: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if !props.pdf_results.is_empty() {
        list.push(section_label(format!(
            "In this PDF · {}",
            props.pdf_results.len()
        )));
        for (i, result) in props.pdf_results.iter().enumerate() {
            list.push(list_row(ListRowProps {
                title: format!("p. {} — {}", result.page_index + 1, result.context.trim()).into(),
                subtitle: None,
                leading_icon: Some(Icon::FilePdf),
                shortcut: None,
                selected: props.selected == i,
                active_doc: false,
                focused: false,
                drag_over: false,
                max_title_chars: 64,
                on_press: Some(Message::Pdf(PdfMessage::SearchResultClicked(
                    result.page_index,
                ))),
            }));
        }
    }
    if !props.results.is_empty() {
        list.push(section_label(format!("Notes · {}", props.results.len())));
        let offset = props.pdf_results.len();
        for (i, result) in props.results.iter().enumerate() {
            list.push(list_row(ListRowProps {
                title: result.path.as_str().into(),
                subtitle: Some(result.context.trim().into()),
                leading_icon: Some(Icon::FileText),
                shortcut: None,
                selected: props.selected == offset + i,
                active_doc: false,
                focused: false,
                drag_over: false,
                max_title_chars: 64,
                on_press: Some(Message::Search(SearchMessage::ResultClicked(
                    result.path.clone(),
                ))),
            }));
        }
    }
    if props.results_truncated {
        list.push(section_label(
            "Showing the first 100 notes — add words to narrow the search".to_string(),
        ));
    }
    if let Some(error) = props.pdf_error {
        list.push(section_label(error.to_string()));
    }

    let body: Element<'a, Message, Theme, Renderer> = if list.is_empty() {
        if props.query.trim().is_empty() {
            empty_state(
                Icon::Search,
                "Search the vault",
                "Type to search every note, and the open PDF.",
                None,
            )
        } else {
            empty_state(
                Icon::Search,
                "No results",
                "Try fewer words, or turn off Match case and Regex.",
                None,
            )
        }
    } else {
        scrollable(Column::with_children(list).spacing(SPACE_1))
            .height(Length::Shrink)
            .into()
    };

    let summary = if props.current_match_count > 0 {
        caption(format!("{} in the open note", props.current_match_count))
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    container(
        column![
            header,
            summary,
            container(body).max_height(PALETTE_MAX_LIST_HEIGHT)
        ]
        .spacing(SPACE_2),
    )
    .width(Length::Fixed(PALETTE_WIDTH))
    .padding(SPACE_3)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(t.bg_popover)),
        border: iced::Border {
            color: t.border_default,
            width: BORDER_WIDTH,
            radius: RADIUS_LG.into(),
        },
        shadow: Elevation::Dialog.shadow(),
        ..Default::default()
    })
    .into()
}
