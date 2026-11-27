//! Tracker tab bodies other than the dashboard: session log, projects, gates, reading lists and the JSON settings.

use super::*;
use iced::widget::{column, row};

pub(super) fn log_body<'a>(
    sessions: &'a [StudySession],
    manual_date: &'a str,
    manual_hours: &'a str,
    manual_notes: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        column![
            text("Session Log")
                .size(TextRole::TitleSm.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            row![
                TextField::new("YYYY-MM-DD", manual_date)
                    .monospace(true)
                    .width(Length::FillPortion(2))
                    .on_input(|value| Message::Shell(ShellMessage::TrackerManualDateChanged(value)))
                    .build(),
                TextField::new("Hours", manual_hours)
                    .monospace(true)
                    .width(Length::FillPortion(1))
                    .on_input(
                        |value| Message::Shell(ShellMessage::TrackerManualHoursChanged(value))
                    )
                    .build(),
                TextField::new("Notes", manual_notes)
                    .width(Length::FillPortion(3))
                    .on_input(
                        |value| Message::Shell(ShellMessage::TrackerManualNotesChanged(value))
                    )
                    .on_submit(Message::Shell(ShellMessage::TrackerManualAdd))
                    .build(),
                Button::primary("Add session")
                    .icon(Icon::Plus)
                    .on_press(Message::Shell(ShellMessage::TrackerManualAdd))
                    .build(),
            ]
            .spacing(SPACE_2)
            .align_y(Alignment::Center),
            sessions_list(sessions),
        ]
        .spacing(SPACE_3),
    )
    .padding(SPACE_3)
    .height(Length::Fill)
    .style(|_| panel_style())
    .into()
}

pub(super) fn sessions_list<'a>(
    sessions: &'a [StudySession],
) -> Element<'a, Message, Theme, Renderer> {
    if sessions.is_empty() {
        return container(
            text("No sessions yet. Start studying!")
                .color(tokens().fg_tertiary)
                .size(TextRole::Body.size()),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    let mut col = column![].spacing(SPACE_2);
    for session in sessions {
        col = col.push(
            container(
                row![
                    column![
                        text(&session.date)
                            .size(TextRole::Caption.size())
                            .color(tokens().fg_tertiary),
                        text(format_duration(session.hours))
                            .size(TextRole::Body.size())
                            .color(tokens().fg_primary)
                            .font(BOLD),
                        text(session.notes.as_deref().unwrap_or(&session.phase))
                            .size(TextRole::Caption.size())
                            .color(tokens().fg_tertiary),
                    ]
                    .width(Length::Fill),
                    text(&session.activity_type)
                        .size(TextRole::Caption.size())
                        .color(tokens().accent)
                        .font(BOLD),
                    IconButton::new(Icon::Trash, "Delete session")
                        .size(ControlSize::Sm)
                        .danger(true)
                        .on_press(Message::Shell(ShellMessage::TrackerSessionDelete(
                            session.id
                        )))
                        .build(),
                ]
                .spacing(SPACE_2)
                .align_y(Alignment::Center)
                .padding(SPACE_2),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(tokens().bg_inset)),
                border: iced::Border {
                    color: tokens().border_subtle,
                    width: 1.0,
                    radius: RADIUS_MD.into(),
                },
                ..Default::default()
            }),
        );
    }
    scrollable(col).height(Length::Fill).into()
}

pub(super) fn projects_body<'a>(
    kv: &'a std::collections::HashMap<String, String>,
    projects: Vec<ProjectConfig>,
) -> Element<'a, Message, Theme, Renderer> {
    let complete = projects
        .iter()
        .filter(|project| {
            kv.get(&format!("proj_{}", project.id))
                .map(|status| status == "complete")
                .unwrap_or(false)
        })
        .count();
    let in_progress = projects
        .iter()
        .filter(|project| {
            kv.get(&format!("proj_{}", project.id))
                .map(|status| status == "in_progress")
                .unwrap_or(false)
        })
        .count();

    let mut col = column![section_summary(
        "Project Milestones",
        format!(
            "{} configured from JSON - {} complete - {} active",
            projects.len(),
            complete,
            in_progress
        ),
        complete,
        projects.len(),
    )]
    .spacing(SPACE_3);

    for project in projects {
        let status = kv
            .get(&format!("proj_{}", project.id))
            .map(String::as_str)
            .unwrap_or("not_started");
        let project_id = project.id.clone();
        col = col.push(
            container(
                row![
                    status_dot(status),
                    column![
                        text(format!("{} - {}", project.id, project.name))
                            .size(TextRole::Body.size())
                            .color(tokens().fg_primary)
                            .font(BOLD),
                        text(format!(
                            "Phase {} - {}",
                            project.phase,
                            status_label(status)
                        ))
                        .size(TextRole::Caption.size())
                        .color(tokens().fg_tertiary),
                    ]
                    .width(Length::Fill),
                    status_button(project_id.clone(), "not_started", "Todo", status),
                    status_button(project_id.clone(), "in_progress", "Doing", status),
                    status_button(project_id, "complete", "Done", status),
                ]
                .spacing(SPACE_3)
                .align_y(Alignment::Center),
            )
            .padding(SPACE_3)
            .style(|_| panel_style()),
        );
    }
    scrollable(col).height(Length::Fill).into()
}

pub(super) fn status_button<'a>(
    id: String,
    value: &'static str,
    label: &'static str,
    current: &str,
) -> Element<'a, Message, Theme, Renderer> {
    Button::new(label)
        .size(ControlSize::Sm)
        .variant(if current == value {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Ghost
        })
        .on_press(Message::Shell(ShellMessage::TrackerProjectStatusChanged(
            id,
            value.to_string(),
        )))
        .build()
}

pub(super) fn status_label(status: &str) -> &'static str {
    match status {
        "in_progress" => "Doing",
        "complete" => "Done",
        _ => "Todo",
    }
}

pub(super) fn gates_body<'a>(
    kv: &'a std::collections::HashMap<String, String>,
    gates: Vec<GateConfig>,
) -> Element<'a, Message, Theme, Renderer> {
    let total_items = gates.iter().map(|gate| gate.items.len()).sum::<usize>();
    let completed_items = gates
        .iter()
        .map(|gate| {
            gate.items
                .iter()
                .enumerate()
                .filter(|(idx, item)| {
                    completion_checked(kv, &gate_completion_keys(&gate.id, item, *idx))
                })
                .count()
        })
        .sum::<usize>();

    let mut grid = column![section_summary(
        "Gate Checkpoints",
        format!("{} gates configured from JSON", gates.len()),
        completed_items,
        total_items,
    )]
    .spacing(SPACE_3);

    for gate in gates {
        let completed = gate
            .items
            .iter()
            .enumerate()
            .filter(|(idx, item)| {
                completion_checked(kv, &gate_completion_keys(&gate.id, item, *idx))
            })
            .count();
        let mut item_col = column![
            text(gate.title)
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            progress_bar(completed, gate.items.len())
        ]
        .spacing(SPACE_2);
        for (idx, item) in gate.items.into_iter().enumerate() {
            let checked = completion_checked(kv, &gate_completion_keys(&gate.id, &item, idx));
            let gate_id = gate.id.clone();
            let item_key = item.clone();
            item_col = item_col.push(crate::ui::checkbox(
                &item,
                checked,
                Some(Message::Shell(ShellMessage::TrackerGateToggled(
                    gate_id.clone(),
                    item_key.clone(),
                    idx,
                ))),
                false,
            ));
        }
        grid = grid.push(
            container(item_col)
                .padding(SPACE_3)
                .style(|_| panel_style()),
        );
    }
    scrollable(grid).height(Length::Fill).into()
}

pub(super) fn reading_body<'a>(
    kv: &'a std::collections::HashMap<String, String>,
    sections: Vec<ReadingSectionConfig>,
) -> Element<'a, Message, Theme, Renderer> {
    let total_items = sections
        .iter()
        .map(|section| section.items.len())
        .sum::<usize>();
    let completed_items = sections
        .iter()
        .map(|section| {
            let key_section = section.section.replace(' ', "");
            section
                .items
                .iter()
                .enumerate()
                .filter(|(idx, item)| {
                    completion_checked(
                        kv,
                        &reading_completion_keys(&key_section, &item.title, *idx),
                    )
                })
                .count()
        })
        .sum::<usize>();

    let mut grid = column![section_summary(
        "Reading Queue",
        format!("{} sections configured from JSON", sections.len()),
        completed_items,
        total_items,
    )]
    .spacing(SPACE_3);

    for section in sections {
        let key_section = section.section.replace(' ', "");
        let completed = section
            .items
            .iter()
            .enumerate()
            .filter(|(idx, item)| {
                completion_checked(
                    kv,
                    &reading_completion_keys(&key_section, &item.title, *idx),
                )
            })
            .count();
        let mut item_col = column![
            text(section.section)
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            progress_bar(completed, section.items.len())
        ]
        .spacing(SPACE_2);
        for (idx, item) in section.items.into_iter().enumerate() {
            let checked =
                completion_checked(kv, &reading_completion_keys(&key_section, &item.title, idx));
            let label = format!("{}  {}", item.priority.to_uppercase(), item.title);
            let section_clone = key_section.clone();
            let item_key = item.title.clone();
            item_col = item_col.push(crate::ui::checkbox(
                &label,
                checked,
                Some(Message::Shell(ShellMessage::TrackerReadingToggled(
                    section_clone.clone(),
                    item_key.clone(),
                    idx,
                ))),
                false,
            ));
        }
        grid = grid.push(
            container(item_col)
                .padding(SPACE_3)
                .style(|_| panel_style()),
        );
    }
    scrollable(grid).height(Length::Fill).into()
}

pub(super) fn config_body<'a>(
    config_json: &'a text_editor::Content,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let mono = TextRole::MonoBody;
    // Parse errors show under the field as they are typed, with serde's line
    // and column; Save stays enabled and reports the same error if pressed.
    let error: Element<'a, Message, Theme, Renderer> = match parse_config(&config_json.text()) {
        Ok(_) => badge(
            "Valid configuration",
            BadgeTone::Success,
            Some(Icon::CheckCircle),
        ),
        Err(err) => row![
            crate::ui::icon(
                Icon::WarningCircle,
                crate::design::metrics::IconSize::Sm.px(),
                t.status_danger
            ),
            text(err)
                .size(TextRole::Caption.size())
                .font(TextRole::Caption.font())
                .color(t.status_danger),
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center)
        .into(),
    };
    card(
        column![
            text("Tracker configuration")
                .size(TextRole::TitleSm.size())
                .font(TextRole::TitleSm.font())
                .color(t.fg_primary),
            text("Projects, gates, reading lists and phases are read from this JSON. Save to apply it to every tab.")
                .size(TextRole::Caption.size())
                .font(TextRole::Caption.font())
                .color(t.fg_secondary),
            text_editor(config_json)
                .placeholder("Tracker JSON")
                .on_action(|value| Message::Shell(ShellMessage::TrackerConfigEdited(value)))
                .padding(SPACE_3)
                .size(mono.size())
                .font(mono.font())
                .height(Length::Fixed(CONFIG_EDITOR_HEIGHT))
                .wrapping(Wrapping::WordOrGlyph),
            error,
            Button::primary("Save configuration")
                .on_press(Message::Shell(ShellMessage::TrackerConfigSave))
                .build(),
        ]
        .spacing(SPACE_3),
    )
}

/// Height of the JSON editor on the Settings tab.
const CONFIG_EDITOR_HEIGHT: f32 = 320.0;
