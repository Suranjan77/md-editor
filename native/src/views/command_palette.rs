//! The Ctrl+P Command Palette: unified command registry, fuzzy ranking, section headers and create-note fallback.

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    Elevation, IconSize, PALETTE_MAX_LIST_HEIGHT, PALETTE_WIDTH, RADIUS_LG, RADIUS_SM, SPACE_0,
    SPACE_1, SPACE_2, SPACE_3,
};
use crate::design::{TextRole, fade, tokens};
use crate::desks::Desk;
use crate::fuzzy;
use crate::messages::{DesksMessage, Message, ShellMessage, Shortcut, VaultMessage};
use crate::shortcuts;
use crate::ui::{Button, ButtonVariant, Icon, TextField, ellipsize_single_line, icon, kbd};

pub const PALETTE_INPUT_ID: &str = "command_palette_input";

const MAX_RESULTS: usize = 40;

pub struct Command {
    pub name: String,
    pub shortcut: Shortcut,
    pub icon: Icon,
}

/// Build the palette command list from the single-source-of-truth shortcut registry.
pub fn get_commands() -> Vec<Command> {
    shortcuts::REGISTRY
        .iter()
        .filter(|spec| spec.in_palette)
        .map(|spec| Command {
            name: spec.label.to_string(),
            shortcut: spec.shortcut,
            icon: spec.icon,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    Command(Shortcut),
    OpenFile(String),
    OpenDesk(String),
}

pub fn action_at(
    query: &str,
    commands: &[Command],
    entries: &[md_editor_core::types::FileEntry],
    desks: &[Desk],
    index: usize,
) -> Option<PaletteAction> {
    rank(query, commands, entries, desks)
        .get(index)
        .map(|hit| match hit {
            Hit::Command(command) => PaletteAction::Command(command.shortcut),
            Hit::File { path, .. } => PaletteAction::OpenFile((*path).to_string()),
            Hit::Desk(desk) => PaletteAction::OpenDesk(desk.name.clone()),
        })
}

pub fn result_count(
    query: &str,
    commands: &[Command],
    entries: &[md_editor_core::types::FileEntry],
    desks: &[Desk],
) -> usize {
    rank(query, commands, entries, desks).len()
}

enum Hit<'a> {
    Command(&'a Command),
    Desk(&'a Desk),
    File { path: &'a str, file_icon: Icon },
}

fn rank<'a>(
    query: &str,
    commands: &'a [Command],
    entries: &'a [md_editor_core::types::FileEntry],
    desks: &'a [Desk],
) -> Vec<Hit<'a>> {
    if query.trim().is_empty() {
        return commands.iter().map(Hit::Command).collect();
    }

    let query = query.trim();
    let mut scored: Vec<(i32, Hit<'a>)> = Vec::new();

    for command in commands {
        if let Some(score) = fuzzy::score(&command.name, query) {
            scored.push((score + 8, Hit::Command(command)));
        }
    }

    for desk in desks {
        let label = format!("Desk: {}", desk.name);
        let best = match (
            fuzzy::score(&label, query),
            fuzzy::score(&desk.name, query).map(|s| s + 12),
        ) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        if let Some(score) = best {
            scored.push((score, Hit::Desk(desk)));
        }
    }

    for entry in entries {
        if entry.is_dir {
            continue;
        }
        if let Some(score) = fuzzy::score_path(&entry.path, query) {
            scored.push((
                score,
                Hit::File {
                    path: &entry.path,
                    file_icon: file_icon(&entry.path),
                },
            ));
        }
    }

    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.truncate(MAX_RESULTS);
    scored.into_iter().map(|(_, hit)| hit).collect()
}

fn file_icon(path: &str) -> Icon {
    let lower = path.to_lowercase();
    if lower.ends_with(".pdf") {
        Icon::FilePdf
    } else if crate::ink::is_sketch_path(&lower) {
        Icon::Pen
    } else if crate::app::is_supported_image_path(&lower) {
        Icon::Image
    } else {
        Icon::FileText
    }
}

fn palette_row<'a>(
    lead_icon: Icon,
    label: String,
    trailing: Element<'a, Message, Theme, Renderer>,
    on_press: Message,
    is_selected: bool,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let icon_color = fade(
        if is_selected {
            t.accent
        } else {
            t.fg_secondary
        },
        opacity,
    );
    let fg = fade(t.fg_primary, opacity);

    button(
        row![
            icon(lead_icon, IconSize::Md.px(), icon_color),
            text(label)
                .size(TextRole::Caption.size())
                .color(fg)
                .font(if is_selected {
                    crate::design::fonts::UI_FONT_MEDIUM
                } else {
                    TextRole::Caption.font()
                }),
            Space::new().width(Length::Fill),
            trailing,
        ]
        .spacing(SPACE_3)
        .align_y(Alignment::Center)
        .padding([SPACE_2, SPACE_3]),
    )
    .width(Length::Fill)
    .on_press(on_press)
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let fill = if is_selected {
            Some(fade(t.bg_overlay, opacity))
        } else if hovered {
            Some(fade(t.bg_raised, opacity))
        } else {
            None
        };
        button::Style {
            background: fill.map(Background::Color),
            text_color: fg,
            border: Border {
                radius: RADIUS_SM.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

pub fn view<'a>(
    query: &'a str,
    commands: &'a [Command],
    entries: &'a [md_editor_core::types::FileEntry],
    desks: &'a [Desk],
    selected: usize,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    let input = container(
        TextField::search("Search commands, desks and files…", query)
            .id(PALETTE_INPUT_ID)
            .on_input(|value| Message::Shell(ShellMessage::CommandPaletteQueryChanged(value)))
            .build(),
    )
    .padding(SPACE_3);

    let hits = rank(query, commands, entries, desks);
    let mut list = column![].spacing(SPACE_1).padding([SPACE_1, SPACE_2]);

    if hits.is_empty() {
        let trimmed = query.trim();
        let mut fallback = column![
            text("No matching commands, desks or files")
                .size(TextRole::Caption.size())
                .color(fade(t.fg_secondary, opacity))
                .font(TextRole::Caption.font())
        ]
        .spacing(SPACE_2)
        .padding(SPACE_3);

        if !trimmed.is_empty() {
            fallback = fallback.push(
                Button::new(format!("No matches — create note '{trimmed}'"))
                    .variant(ButtonVariant::Secondary)
                    .icon(Icon::FilePlus)
                    .on_press(Message::Vault(VaultMessage::CreateNoteFromPalette(
                        trimmed.to_string(),
                    )))
                    .build(),
            );
        }
        list = list.push(fallback);
    } else if query.trim().is_empty() {
        list = list.push(
            container(
                text("Commands")
                    .size(TextRole::Caption.size())
                    .color(fade(t.fg_tertiary, opacity))
                    .font(crate::design::fonts::UI_FONT_SEMIBOLD),
            )
            .padding([SPACE_1, SPACE_3]),
        );
    }

    for (index, hit) in hits.into_iter().enumerate() {
        list = list.push(render_hit(hit, index == selected, opacity));
    }

    container(
        column![
            input,
            // Shrinks to the results, up to the cap, instead of reserving the full height.
            container(scrollable(list).height(Length::Shrink)).max_height(PALETTE_MAX_LIST_HEIGHT),
        ]
        .spacing(SPACE_0),
    )
    .width(Length::Fixed(PALETTE_WIDTH))
    .padding([SPACE_0, SPACE_0])
    .style(move |_| container::Style {
        background: Some(Background::Color(fade(t.bg_popover, opacity))),
        border: Border {
            color: fade(t.border_control, opacity),
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        shadow: Elevation::Popover.shadow(),
        ..Default::default()
    })
    .into()
}

fn render_hit<'a>(
    hit: Hit<'a>,
    is_selected: bool,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    match hit {
        Hit::Command(command) => {
            let chord = shortcuts::chord(command.shortcut);
            let trailing: Element<'a, Message, Theme, Renderer> = if chord.is_empty() {
                Space::new().width(Length::Fixed(SPACE_0)).into()
            } else {
                kbd(chord)
            };
            palette_row(
                command.icon,
                command.name.clone(),
                trailing,
                Message::Shell(ShellMessage::CommandPaletteCommandClicked(command.shortcut)),
                is_selected,
                opacity,
            )
        }
        Hit::Desk(desk) => {
            let trailing = text(desk.summary())
                .size(TextRole::Caption.size())
                .color(fade(t.fg_tertiary, opacity))
                .font(TextRole::Caption.font())
                .into();
            palette_row(
                Icon::Desktop,
                format!("Desk: {}", ellipsize_single_line(&desk.name, 36).0),
                trailing,
                Message::Desks(DesksMessage::OpenFromPalette(desk.name.clone())),
                is_selected,
                opacity,
            )
        }
        Hit::File { path, file_icon } => {
            let name = path.rsplit('/').next().unwrap_or(path);
            let folder = path.strip_suffix(name).unwrap_or("").trim_end_matches('/');
            let trailing = text(ellipsize_single_line(folder, 28).0)
                .size(TextRole::Caption.size())
                .color(fade(t.fg_tertiary, opacity))
                .font(TextRole::Caption.font())
                .into();
            palette_row(
                file_icon,
                ellipsize_single_line(name, 42).0,
                trailing,
                Message::Shell(ShellMessage::CommandPaletteFileClicked(path.to_string())),
                is_selected,
                opacity,
            )
        }
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
        let hits = rank("", &commands, &entries, &[]);
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
        let hits = rank("attention", &commands, &entries, &[]);
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
        let hits = rank("note", &commands, &entries, &[]);
        assert!(
            hits.iter()
                .all(|h| !matches!(h, Hit::File { path, .. } if *path == "notes")),
            "a folder is not something the palette can open"
        );
    }

    #[test]
    fn commands_are_still_matched_by_initials() {
        let commands = get_commands();
        let hits = rank("su", &commands, &[], &[]);
        assert!(
            matches!(hits.first(), Some(Hit::Command(c)) if c.name == "Split / Unsplit"),
            "initials should put the command first"
        );
    }

    #[test]
    fn enter_activates_the_highlighted_row() {
        let commands = get_commands();
        let entries = vec![entry("notes/attention.md", false)];

        assert_eq!(
            action_at("", &commands, &entries, &[], 0),
            Some(PaletteAction::Command(Shortcut::NewFile))
        );

        let hits = rank("attention", &commands, &entries, &[]);
        let file_row = hits
            .iter()
            .position(|h| matches!(h, Hit::File { .. }))
            .expect("the note should be among the results");
        assert_eq!(
            action_at("attention", &commands, &entries, &[], file_row),
            Some(PaletteAction::OpenFile("notes/attention.md".to_string()))
        );
    }

    #[test]
    fn selecting_past_the_end_activates_nothing() {
        let commands = get_commands();
        assert_eq!(action_at("", &commands, &[], &[], 9_999), None);
        assert_eq!(action_at("zzzzznomatch", &commands, &[], &[], 0), None);
    }

    #[test]
    fn result_count_matches_what_is_rendered() {
        let commands = get_commands();
        let entries = vec![entry("notes/attention.md", false)];
        for query in ["", "attention", "split", "zzzz"] {
            assert_eq!(
                result_count(query, &commands, &entries, &[]),
                rank(query, &commands, &entries, &[]).len(),
                "count and ranking disagreed for {query:?}"
            );
        }
    }

    #[test]
    fn knowledge_graph_is_discoverable() {
        let commands = get_commands();
        assert_eq!(
            action_at("knowledge graph", &commands, &[], &[], 0),
            Some(PaletteAction::Command(Shortcut::KnowledgeGraph))
        );
    }

    #[test]
    fn saved_desks_and_split_commands_are_discoverable_in_palette() {
        let commands = get_commands();
        let desks = vec![Desk {
            name: "Thesis Reading".to_string(),
            pane_a_tabs: crate::tabs::Tabs::new().to_json(),
            pane_b_tabs: None,
            axis: crate::app::workspace::SplitAxis::Vertical,
            split_ratio: 0.5,
            focused: crate::app::workspace::PaneId::A,
            sidebar_visible: true,
            backlinks_visible: false,
            toc_visible: false,
        }];
        let hits = rank("Thesis Reading", &commands, &[], &desks);
        let desk_idx = hits
            .iter()
            .position(|h| matches!(h, Hit::Desk(d) if d.name == "Thesis Reading"))
            .expect("saved desk should appear in palette results");
        assert_eq!(
            action_at("Thesis Reading", &commands, &[], &desks, desk_idx),
            Some(PaletteAction::OpenDesk("Thesis Reading".to_string()))
        );

        assert_eq!(
            action_at("Split Right", &commands, &[], &[], 0),
            Some(PaletteAction::Command(Shortcut::SplitRight))
        );
        assert_eq!(
            action_at("Split Down", &commands, &[], &[], 0),
            Some(PaletteAction::Command(Shortcut::SplitDown))
        );
        assert_eq!(
            action_at("Unsplit", &commands, &[], &[], 0),
            Some(PaletteAction::Command(Shortcut::Unsplit))
        );
    }

    #[test]
    fn results_are_capped() {
        let commands = get_commands();
        let entries: Vec<_> = (0..500)
            .map(|i| entry(&format!("notes/a{i}.md"), false))
            .collect();
        assert!(rank("a", &commands, &entries, &[]).len() <= MAX_RESULTS);
    }
}
