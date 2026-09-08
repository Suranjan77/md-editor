use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::fuzzy;
use crate::messages::{Message, Shortcut};
use crate::theme;

/// Widget id of the palette's query field, so the shell can put the caret in
/// it the moment the palette opens. Without this the palette can only be
/// clicked, which defeats the point of a command palette.
pub const PALETTE_INPUT_ID: &str = "command_palette_input";

/// Most results shown at once. Beyond this the list stops being scannable and
/// the answer is a longer query, not more scrolling.
const MAX_RESULTS: usize = 40;

pub struct Command {
    pub name: String,
    pub shortcut: Shortcut,
    pub icon: String,
}

/// The command registry.
///
/// Every action the palette can run is declared here rather than being
/// discovered from the toolbar or the keymap, so a command exists in exactly
/// one place and the palette and the shortcut list cannot drift apart.
pub fn get_commands() -> Vec<Command> {
    let entry = |name: &str, shortcut, icon: &str| Command {
        name: name.to_string(),
        shortcut,
        icon: icon.to_string(),
    };

    vec![
        entry("New File", Shortcut::NewFile, "+"),
        entry("Open Vault", Shortcut::OpenVault, "O"),
        entry("Save File", Shortcut::Save, "S"),
        entry("Search Vault", Shortcut::Search, "/"),
        entry("Toggle Sidebar", Shortcut::ToggleSidebar, "S"),
        entry("Toggle Backlinks", Shortcut::ToggleBacklinks, "B"),
        entry("Table of Contents", Shortcut::TableOfContents, "T"),
        entry("Study Tracker", Shortcut::StudyTracker, "R"),
        entry("Split View", Shortcut::SplitView, "|"),
        entry("Focus Mode", Shortcut::FocusMode, "F"),
    ]
}

/// What activating a palette row does. Returned to the shell so keyboard
/// selection can run the same thing a click would, without the shell needing
/// to know how results are ranked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    Command(Shortcut),
    OpenFile(String),
}

/// The action for the row at `index`, if there is one.
pub fn action_at(
    query: &str,
    commands: &[Command],
    entries: &[md_editor_core::types::FileEntry],
    index: usize,
) -> Option<PaletteAction> {
    rank(query, commands, entries)
        .get(index)
        .map(|hit| match hit {
            Hit::Command(command) => PaletteAction::Command(command.shortcut),
            Hit::File { path, .. } => PaletteAction::OpenFile((*path).to_string()),
        })
}

/// How many rows the current query produces, so the shell can clamp the
/// selection without duplicating the ranking rules.
pub fn result_count(
    query: &str,
    commands: &[Command],
    entries: &[md_editor_core::types::FileEntry],
) -> usize {
    rank(query, commands, entries).len()
}

/// One ranked row in the palette: either a command to run or a vault file to
/// open. Both are scored by the same matcher so they can be interleaved by
/// relevance rather than segregated into fixed sections.
enum Hit<'a> {
    Command(&'a Command),
    File { path: &'a str, is_pdf: bool },
}

fn rank<'a>(
    query: &str,
    commands: &'a [Command],
    entries: &'a [md_editor_core::types::FileEntry],
) -> Vec<Hit<'a>> {
    // With no query the palette is a menu of what the app can do; files would
    // just be an arbitrary slice of the vault.
    if query.trim().is_empty() {
        return commands.iter().map(Hit::Command).collect();
    }

    let query = query.trim();
    let mut scored: Vec<(i32, Hit<'a>)> = Vec::new();

    for command in commands {
        if let Some(score) = fuzzy::score(&command.name, query) {
            // Commands edge out files at equal relevance: the palette is
            // primarily a way to act, and a file is one keystroke further
            // away in the sidebar anyway.
            scored.push((score + 8, Hit::Command(command)));
        }
    }

    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let is_pdf = entry.path.to_lowercase().ends_with(".pdf");
        if let Some(score) = fuzzy::score_path(&entry.path, query) {
            scored.push((
                score,
                Hit::File {
                    path: &entry.path,
                    is_pdf,
                },
            ));
        }
    }

    // Sort by descending score. `sort_by` is stable, so equal scores keep
    // registry order for commands and vault order for files.
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(MAX_RESULTS);
    scored.into_iter().map(|(_, hit)| hit).collect()
}

fn badge<'a>(glyph: &'a str, opacity: f32) -> Element<'a, Message, Theme, Renderer> {
    container(
        text(glyph)
            .size(theme::TEXT_SM)
            .color(theme::fade(theme::TEXT_SECONDARY, opacity)),
    )
    .width(Length::Fixed(24.0))
    .height(Length::Fixed(24.0))
    .center_x(Length::Fixed(24.0))
    .center_y(Length::Fixed(24.0))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(theme::fade(
            theme::BG_TERTIARY,
            opacity,
        ))),
        border: iced::Border {
            color: theme::fade(theme::BORDER, opacity),
            width: 1.0,
            radius: theme::RADIUS_MD.into(),
        },
        ..Default::default()
    })
    .into()
}

fn row_button<'a>(
    icon: Element<'a, Message, Theme, Renderer>,
    label: Element<'a, Message, Theme, Renderer>,
    trailing: Element<'a, Message, Theme, Renderer>,
    on_press: Message,
    is_selected: bool,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    button(
        row![icon, label, Space::new().width(Length::Fill), trailing]
            .spacing(theme::SPACE_4)
            .align_y(Alignment::Center)
            .padding([theme::SPACE_3, theme::SPACE_4]),
    )
    .width(Length::Fill)
    .on_press(on_press)
    .style(move |_, _| button::Style {
        // The keyboard selection has to be visible, or arrow keys move an
        // invisible cursor and Enter is a guess.
        background: is_selected
            .then(|| iced::Background::Color(theme::fade(theme::BG_TERTIARY, opacity))),
        text_color: theme::fade(theme::TEXT_PRIMARY, opacity),
        border: iced::Border {
            radius: theme::RADIUS_SM.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

pub fn view<'a>(
    query: &str,
    commands: &'a [Command],
    entries: &'a [md_editor_core::types::FileEntry],
    selected: usize,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let input = iced::widget::text_input("Search commands and files…", query)
        .id(iced::advanced::widget::Id::new(PALETTE_INPUT_ID))
        .on_input(Message::CommandPaletteQueryChanged)
        .on_submit(Message::NameModalSubmitCurrent)
        .padding(theme::SPACE_4)
        .size(theme::TEXT_MD);

    let hits = rank(query, commands, entries);
    let mut list = column![].spacing(theme::SPACE_2);

    if hits.is_empty() {
        list = list.push(
            container(
                text("No matching commands or files")
                    .size(theme::TEXT_BASE)
                    .color(theme::fade(theme::TEXT_MUTED, opacity)),
            )
            .padding([theme::SPACE_4, theme::SPACE_4]),
        );
    }

    for (index, hit) in hits.into_iter().enumerate() {
        let is_selected = index == selected;
        list = list.push(match hit {
            Hit::Command(command) => row_button(
                badge(&command.icon, opacity),
                text(&command.name)
                    .size(theme::TEXT_BASE)
                    .color(theme::fade(theme::TEXT_PRIMARY, opacity))
                    .into(),
                text(shortcut_label(command.shortcut))
                    .size(theme::TEXT_SM)
                    .color(theme::fade(theme::TEXT_MUTED, opacity))
                    .into(),
                Message::CommandPaletteCommandClicked(command.shortcut),
                is_selected,
                opacity,
            ),
            Hit::File { path, is_pdf } => {
                let name = path.rsplit('/').next().unwrap_or(path);
                let folder = path.strip_suffix(name).unwrap_or("").trim_end_matches('/');

                row_button(
                    badge(if is_pdf { "P" } else { "M" }, opacity),
                    text(name)
                        .size(theme::TEXT_BASE)
                        .color(theme::fade(theme::TEXT_PRIMARY, opacity))
                        .into(),
                    text(folder)
                        .size(theme::TEXT_SM)
                        .color(theme::fade(theme::TEXT_MUTED, opacity))
                        .into(),
                    Message::CommandPaletteFileClicked(path.to_string()),
                    is_selected,
                    opacity,
                )
            }
        });
    }

    container(
        column![
            container(input).style(move |_| container::Style {
                border: iced::Border {
                    color: theme::fade(theme::BORDER, opacity),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
            scrollable(list).height(Length::Fixed(320.0)),
        ]
        .spacing(theme::SPACE_0),
    )
    .width(Length::Fixed(520.0))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(theme::fade(
            theme::BG_SECONDARY,
            opacity,
        ))),
        border: iced::Border {
            color: theme::fade(theme::BORDER, opacity),
            width: 1.0,
            radius: theme::RADIUS_MD.into(),
        },
        ..Default::default()
    })
    .into()
}

fn shortcut_label(shortcut: Shortcut) -> &'static str {
    match shortcut {
        Shortcut::Save => "Ctrl S",
        Shortcut::OpenVault => "Ctrl O",
        Shortcut::NewFile => "Ctrl N",
        Shortcut::Search => "Ctrl F",
        Shortcut::CommandPalette => "Ctrl P",
        Shortcut::ToggleSidebar => "Ctrl B",
        Shortcut::ToggleBacklinks => "Backlinks",
        Shortcut::FocusMode => "Focus",
        Shortcut::TableOfContents => "Ctrl T",
        Shortcut::StudyTracker => "Tracker",
        Shortcut::SplitView => "Split",
        Shortcut::Escape => "Esc",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, is_dir: bool) -> md_editor_core::types::FileEntry {
        md_editor_core::types::FileEntry {
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            is_dir,
        }
    }

    #[test]
    fn empty_query_lists_commands_only() {
        let commands = get_commands();
        let entries = vec![entry("notes/a.md", false)];
        let hits = rank("", &commands, &entries);
        assert_eq!(hits.len(), commands.len());
        assert!(hits.iter().all(|h| matches!(h, Hit::Command(_))));
    }

    #[test]
    fn a_query_reaches_vault_files() {
        let commands = get_commands();
        let entries = vec![
            entry("notes", true),
            entry("notes/attention-mechanisms.md", false),
            entry("papers/unrelated.pdf", false),
        ];
        let hits = rank("attention", &commands, &entries);
        assert!(
            hits.iter().any(
                |h| matches!(h, Hit::File { path, .. } if *path == "notes/attention-mechanisms.md")
            ),
            "the matching note should be reachable from the palette"
        );
    }

    #[test]
    fn directories_are_never_offered_as_results() {
        let commands = get_commands();
        let entries = vec![entry("notes", true), entry("notes/note.md", false)];
        let hits = rank("note", &commands, &entries);
        assert!(
            hits.iter()
                .all(|h| !matches!(h, Hit::File { path, .. } if *path == "notes")),
            "a folder is not something the palette can open"
        );
    }

    #[test]
    fn commands_are_still_matched_by_initials() {
        let commands = get_commands();
        let hits = rank("sv", &commands, &[]);
        assert!(
            matches!(hits.first(), Some(Hit::Command(c)) if c.name == "Split View"),
            "initials should put the command first"
        );
    }

    #[test]
    fn enter_activates_the_highlighted_row() {
        let commands = get_commands();
        let entries = vec![entry("notes/attention.md", false)];

        // Row 0 with an empty query is the first registered command.
        assert_eq!(
            action_at("", &commands, &entries, 0),
            Some(PaletteAction::Command(Shortcut::NewFile))
        );

        // A file query resolves to opening that file.
        let hits = rank("attention", &commands, &entries);
        let file_row = hits
            .iter()
            .position(|h| matches!(h, Hit::File { .. }))
            .expect("the note should be among the results");
        assert_eq!(
            action_at("attention", &commands, &entries, file_row),
            Some(PaletteAction::OpenFile("notes/attention.md".to_string()))
        );
    }

    #[test]
    fn selecting_past_the_end_activates_nothing() {
        let commands = get_commands();
        assert_eq!(action_at("", &commands, &[], 9_999), None);
        assert_eq!(action_at("zzzzznomatch", &commands, &[], 0), None);
    }

    #[test]
    fn result_count_matches_what_is_rendered() {
        let commands = get_commands();
        let entries = vec![entry("notes/attention.md", false)];
        for query in ["", "attention", "split", "zzzz"] {
            assert_eq!(
                result_count(query, &commands, &entries),
                rank(query, &commands, &entries).len(),
                "count and ranking disagreed for {query:?}"
            );
        }
    }

    #[test]
    fn results_are_capped() {
        let commands = get_commands();
        let entries: Vec<_> = (0..500)
            .map(|i| entry(&format!("notes/a{i}.md"), false))
            .collect();
        assert!(rank("a", &commands, &entries).len() <= MAX_RESULTS);
    }
}
