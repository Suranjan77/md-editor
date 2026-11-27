//! The study-tracker dashboard and its tabs. The config model is in `config`.

use iced::advanced::text::Wrapping;
use iced::widget::{Space, column, container, row, scrollable, text, text_editor};

mod config;
mod sections;

pub use config::*;
use iced::{Alignment, Background, Element, Length, Renderer, Theme};
use sections::*;

use crate::design::metrics::{
    ControlSize, RADIUS_MD, RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, ShellMessage, TrackerTab};
use crate::ui::{
    BadgeTone, Button, ButtonVariant, Icon, IconButton, TextField, TooltipPosition, UnderlineTab,
    badge, card, empty_state, stat_card, underline_tabs, with_tooltip,
};
use md_editor_core::tracker::StudySession;
use serde::{Deserialize, Serialize};

const BOLD: iced::Font = crate::design::fonts::UI_FONT_SEMIBOLD;

/// A stat card: value in `heading`, label in `caption` (`pages/tracker.md`).
fn kpi_card<'a>(
    label: &'static str,
    value: String,
    detail: &'static str,
) -> Element<'a, Message, Theme, Renderer> {
    container(stat_card(label, &value, Some(detail), None))
        .width(Length::FillPortion(1))
        .into()
}

/// `1h 05m`, `45m` or `0m`: whole minutes, never a negative zero.
pub fn format_duration(hours: f32) -> String {
    let minutes = (hours * 60.0).round().max(0.0) as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, m) => format!("{h}h {m:02}m"),
    }
}

fn activity_chart<'a>(sessions: &[StudySession]) -> Element<'a, Message, Theme, Renderer> {
    let today = chrono::Local::now().date_naive();
    let mut days = Vec::new();

    for offset in (0..14).rev() {
        let day = today - chrono::Duration::days(offset);
        let key = day.format("%Y-%m-%d").to_string();
        let hours = sessions
            .iter()
            .filter(|session| session.date.starts_with(&key))
            .map(|session| session.hours)
            .sum::<f32>();
        days.push((day, hours));
    }

    if days.iter().all(|(_, hours)| *hours <= 0.0) {
        return card(empty_state(
            Icon::Clock,
            "No sessions in the last two weeks",
            "Start the timer to log one, or add a session in the Log tab.",
            None,
        ));
    }
    let max_hours = days.iter().map(|(_, hours)| *hours).fold(4.0_f32, f32::max);
    let mut bars = row![]
        .spacing(SPACE_1)
        .align_y(Alignment::End)
        .height(Length::Fixed(120.0));

    for (day, hours) in days {
        let bar_height = ((hours / max_hours) * 88.0).max(4.0);
        let is_today = day == today;
        let bar = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(bar_height))
            .style(move |_| container::Style {
                background: Some(Background::Color(if hours > 0.0 {
                    tokens().accent
                } else {
                    tokens().border_subtle
                })),
                border: iced::Border {
                    radius: RADIUS_SM.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        // The exact value is on the bar, not only implied by its height.
        let bar = with_tooltip(
            bar,
            &format!("{} — {}", day.format("%a %-d %b"), format_duration(hours)),
            None,
            TooltipPosition::Top,
        );
        bars = bars.push(
            column![
                Space::new().height(Length::Fill),
                bar,
                text(
                    day.format("%a")
                        .to_string()
                        .chars()
                        .next()
                        .unwrap_or(' ')
                        .to_string()
                )
                .size(TextRole::Caption.size())
                .color(if is_today {
                    tokens().accent
                } else {
                    tokens().fg_tertiary
                }),
            ]
            .spacing(SPACE_1)
            .align_x(Alignment::Center)
            .width(Length::FillPortion(1)),
        );
    }

    container(
        column![
            text("Last 14 days")
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            bars,
        ]
        .spacing(SPACE_3),
    )
    .padding(SPACE_3)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(tokens().bg_inset)),
        border: iced::Border {
            color: tokens().border_subtle,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..Default::default()
    })
    .into()
}

fn curriculum_panel<'a>(phases: Vec<PhaseConfig>) -> Element<'a, Message, Theme, Renderer> {
    let mut phase_list = column![].spacing(SPACE_2);
    for phase in phases {
        phase_list = phase_list.push(
            container(
                row![
                    text(phase.id)
                        .size(TextRole::Caption.size())
                        .color(tokens().accent)
                        .font(BOLD),
                    column![
                        text(phase.title)
                            .size(TextRole::Body.size())
                            .color(tokens().fg_primary)
                            .font(BOLD),
                        text(format!("{} - {}", phase.year, phase.months))
                            .size(TextRole::Caption.size())
                            .color(tokens().fg_tertiary),
                    ]
                    .spacing(SPACE_1)
                    .width(Length::Fill),
                ]
                .spacing(SPACE_3)
                .align_y(Alignment::Center),
            )
            .padding(SPACE_2)
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

    container(
        column![
            text("Curriculum Roadmap")
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            scrollable(phase_list).height(Length::Fill),
        ]
        .spacing(SPACE_3),
    )
    .padding(SPACE_3)
    .height(Length::Fill)
    .style(|_| panel_style())
    .into()
}

fn milestones_panel<'a>(
    project_count: usize,
    gate_count: usize,
    reading_sections: Vec<ReadingSectionConfig>,
) -> Element<'a, Message, Theme, Renderer> {
    let reading =
        reading_sections
            .into_iter()
            .take(4)
            .fold(column![].spacing(SPACE_2), |col, section| {
                let body = section
                    .items
                    .into_iter()
                    .take(4)
                    .map(|item| item.title)
                    .collect::<Vec<_>>()
                    .join(", ");
                col.push(
                    column![
                        text(section.section)
                            .size(TextRole::Caption.size())
                            .color(tokens().accent)
                            .font(BOLD),
                        text(body)
                            .size(TextRole::Caption.size())
                            .color(tokens().fg_tertiary),
                    ]
                    .spacing(SPACE_1),
                )
            });

    container(
        column![
            text("Projects, Gates, Reading")
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            row![
                kpi_card(
                    "PROJECTS",
                    project_count.to_string(),
                    "Implementation milestones"
                ),
                kpi_card("GATES", gate_count.to_string(), "Checkpoint gates"),
            ]
            .spacing(SPACE_2),
            text("Reading Tracks")
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            reading,
        ]
        .spacing(SPACE_3),
    )
    .padding(SPACE_3)
    .height(Length::Fill)
    .style(|_| panel_style())
    .into()
}

fn panel_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens().bg_canvas)),
        border: iced::Border {
            color: tokens().border_default,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..Default::default()
    }
}

/// Everything the study tracker renders from.
pub struct TrackerProps<'a> {
    pub visible: bool,
    pub running: bool,
    pub sessions: &'a [StudySession],
    pub kv: &'a std::collections::HashMap<String, String>,
    pub active_tab: TrackerTab,
    pub config_json: &'a text_editor::Content,
    pub manual_date: &'a str,
    pub manual_hours: &'a str,
    pub manual_notes: &'a str,
}

pub fn view<'a>(props: TrackerProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    let TrackerProps {
        visible,
        running,
        sessions,
        kv,
        active_tab,
        config_json,
        manual_date,
        manual_hours,
        manual_notes,
    } = props;
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let total_hours = sessions.iter().map(|s| s.hours).sum::<f32>();
    let session_count = sessions.len();
    let avg_hours = if session_count > 0 {
        total_hours / session_count as f32
    } else {
        0.0
    };
    let config_text = config_json.text();
    let tracker_config = config_or_default(&config_text);

    let kpis = row![
        kpi_card("Total time", format_duration(total_hours), "Accumulated"),
        kpi_card("Sessions", session_count.to_string(), "Logged"),
        kpi_card("Average", format_duration(avg_hours), "Per session"),
        kpi_card(
            "Curriculum",
            format!("{} phases", tracker_config.phases.len()),
            "Configured roadmap"
        ),
    ]
    .spacing(SPACE_2)
    .width(Length::Fill);

    // The single primary action of the view.
    let timer = if running {
        Button::primary("Stop timer")
            .icon(Icon::Pause)
            .on_press(Message::Shell(ShellMessage::TrackerStop))
            .build()
    } else {
        Button::primary("Start timer")
            .icon(Icon::Play)
            .on_press(Message::Shell(ShellMessage::TrackerStart))
            .build()
    };
    let running_status: Element<'a, Message, Theme, Renderer> = if running {
        badge("Timer running", BadgeTone::Success, Some(Icon::Clock))
    } else {
        Space::new().width(Length::Fixed(SPACE_0)).into()
    };
    let controls = row![running_status, timer]
        .spacing(SPACE_2)
        .align_y(Alignment::Center);

    let tab = |label: &str, value: TrackerTab| UnderlineTab {
        label: label.to_string(),
        selected: active_tab == value,
        on_select: Message::Shell(ShellMessage::TrackerTabSelected(value)),
    };
    let tab_bar = underline_tabs(vec![
        tab("Dashboard", TrackerTab::Dashboard),
        tab("Log", TrackerTab::Log),
        tab("Projects", TrackerTab::Projects),
        tab("Gates", TrackerTab::Gates),
        tab("Reading", TrackerTab::Reading),
        tab("Settings", TrackerTab::Config),
    ]);

    let body = match active_tab {
        TrackerTab::Dashboard => dashboard_body(sessions, tracker_config),
        TrackerTab::Log => log_body(sessions, manual_date, manual_hours, manual_notes),
        TrackerTab::Projects => projects_body(kv, tracker_config.projects),
        TrackerTab::Gates => gates_body(kv, tracker_config.gates),
        TrackerTab::Reading => reading_body(kv, tracker_config.reading),
        TrackerTab::Config => config_body(config_json),
    };

    let title = TextRole::Title;
    let header = row![
        text("Study tracker")
            .size(title.size())
            .line_height(title.line_height())
            .font(title.font())
            .color(tokens().fg_primary),
        Space::new().width(Length::Fill),
        controls,
        IconButton::new(Icon::X, "Close tracker")
            .shortcut("Esc")
            .on_press(Message::Shell(ShellMessage::TrackerToggle))
            .build(),
    ]
    .spacing(SPACE_3)
    .align_y(Alignment::Center);

    container(
        column![header, tab_bar, kpis, body]
            .spacing(SPACE_4)
            .padding(SPACE_4),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(tokens().bg_canvas)),
        ..Default::default()
    })
    .into()
}

fn dashboard_body<'a>(
    sessions: &'a [StudySession],
    config: TrackerConfig,
) -> Element<'a, Message, Theme, Renderer> {
    let sessions_list = sessions_list(sessions);
    let project_count = config.projects.len();
    let gate_count = config.gates.len();
    row![
        column![
            activity_chart(sessions),
            text("Recent sessions")
                .size(TextRole::Body.size())
                .color(tokens().fg_primary)
                .font(BOLD),
            sessions_list,
        ]
        .spacing(SPACE_3)
        .width(Length::FillPortion(2)),
        curriculum_panel(config.phases),
        milestones_panel(project_count, gate_count, config.reading),
    ]
    .spacing(SPACE_3)
    .height(Length::Fill)
    .into()
}

fn section_summary<'a>(
    title: &'static str,
    subtitle: String,
    done: usize,
    total: usize,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        column![
            row![
                column![
                    text(title)
                        .size(TextRole::TitleSm.size())
                        .color(tokens().fg_primary)
                        .font(BOLD),
                    text(subtitle)
                        .size(TextRole::Caption.size())
                        .color(tokens().fg_tertiary),
                ]
                .spacing(SPACE_1)
                .width(Length::Fill),
                text(format!("{}/{}", done, total))
                    .size(TextRole::Body.size())
                    .color(tokens().accent)
                    .font(BOLD),
            ]
            .align_y(Alignment::Center),
            progress_bar(done, total),
        ]
        .spacing(SPACE_3),
    )
    .padding(SPACE_3)
    .style(|_| panel_style())
    .into()
}

/// Progress with its percentage beside it, so it never relies on colour alone.
fn progress_bar<'a>(done: usize, total: usize) -> Element<'a, Message, Theme, Renderer> {
    let fraction = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    crate::ui::progress_bar(fraction, true)
}

fn status_dot<'a>(status: &str) -> Element<'a, Message, Theme, Renderer> {
    let color = match status {
        "complete" => tokens().status_success,
        "in_progress" => tokens().accent,
        _ => tokens().fg_tertiary,
    };

    container(Space::new())
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: iced::Border {
                radius: RADIUS_SM.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod duration_tests {
    use super::format_duration;

    #[test]
    fn durations_read_as_hours_and_minutes_never_negative_zero() {
        assert_eq!(format_duration(0.0), "0m");
        assert_eq!(format_duration(-0.01), "0m");
        assert_eq!(format_duration(0.75), "45m");
        assert_eq!(format_duration(1.0833), "1h 05m");
        assert_eq!(format_duration(12.5), "12h 30m");
    }
}
