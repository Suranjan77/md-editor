//! Study tracker view and messages for v3 (Phase 4). Wires the global
//! tracker SQLite connection to an interactive right panel.

use iced::widget::{
    Space, button, checkbox, column, container, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Background, Element, Fill, Length};
use md_vault::tracker::StudySession;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::tokens;
use super::tracker_widgets::{kpi_card, panel_style};
use crate::gui::Message;

const BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackerTab {
    Dashboard,
    Log,
    Projects,
    Gates,
    Reading,
    Config,
}

#[derive(Debug, Clone)]
pub enum TrackerMessage {
    Toggle,
    Start,
    Stop,
    TabSelected(TrackerTab),
    ProjectStatusChanged(String, String),
    GateToggled(String, usize),
    ReadingToggled(String, usize),
    ConfigEdited(iced::widget::text_editor::Action),
    ConfigSave,
    ManualDateChanged(String),
    ManualHoursChanged(String),
    ManualNotesChanged(String),
    ManualAdd,
    SessionDelete(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerConfig {
    #[serde(rename = "PHASES")]
    pub phases: Vec<PhaseConfig>,
    #[serde(rename = "PROJECTS")]
    pub projects: Vec<ProjectConfig>,
    #[serde(rename = "GATES")]
    pub gates: Vec<GateConfig>,
    #[serde(rename = "READING")]
    pub reading: Vec<ReadingSectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseConfig {
    pub id: String,
    pub title: String,
    pub year: String,
    pub months: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub phase: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    pub id: String,
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingSectionConfig {
    pub section: String,
    pub items: Vec<ReadingItemConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingItemConfig {
    pub priority: String,
    pub title: String,
}

const PHASES: &[(&str, &str, &str, &str)] = &[
    ("1A", "Mathematics", "Year 1", "Months 1-4"),
    ("1B", "Systems: C/C++ & Hardware", "Year 1", "Months 4-8"),
    ("1C", "Deep Learning Foundations", "Year 1", "Months 8-12"),
    ("2A", "Quantization", "Year 2", "Months 13-18"),
    ("2B", "Pruning", "Year 2", "Months 16-20"),
    ("2C", "Knowledge Distillation", "Year 2", "Months 19-22"),
    ("2D", "Efficient Training", "Year 2", "Months 21-23"),
    ("2E", "Deployment", "Year 2", "Months 23-24"),
    ("3A", "Low-Rank & PEFT", "Year 3", "Months 25-29"),
    ("3B", "Mixture of Experts", "Year 3", "Months 28-32"),
    ("3C", "State Space Models", "Year 3", "Months 31-34"),
    ("3D", "Attention & Spec. Decoding", "Year 3", "Months 33-37"),
    ("3E", "Training at Scale", "Year 3", "Months 35-38"),
    ("3F", "TinyML & Edge", "Year 3", "Months 37-40"),
    ("4A", "Research Methodology", "Year 4", "Months 41-44"),
    ("4B", "Advanced Topics", "Year 4", "Months 43-48"),
    ("4C", "Original Research", "Year 4", "Months 46-48+"),
];

const PROJECTS: &[(&str, &str, &str)] = &[
    ("1.1", "1A", "SVD from scratch (NumPy eig() only)"),
    ("1.2", "1A", "KL divergence explorer"),
    ("1.3", "1A", "Scalar autograd engine"),
    ("1.5", "1B", "Cache-aware matrix multiplication"),
    ("1.7", "1B", "First HIP kernel"),
    ("1.10", "1C", "Transformer from scratch"),
    ("2.2", "2A", "PTQ from scratch"),
    ("2.3", "2A", "QAT implementation"),
    ("2.6", "2B", "Unstructured magnitude pruning"),
    ("2.10", "2C", "Classic temperature distillation"),
    ("2.13", "2D", "Gradient checkpointing"),
    ("2.16", "2E", "Full export pipeline"),
    ("3.1", "3A", "LoRA from scratch"),
    ("3.5", "3B", "MoE FFN layer"),
    ("3.8", "3C", "Mamba block implementation"),
    ("3.11", "3D", "FlashAttention tiling"),
    ("3.15", "3E", "FSDP training"),
    ("3.18", "3F", "MobileNetV2 + TFLite"),
    ("4.1", "4A", "Reproducibility study"),
    ("4.8", "4C", "Research proposal"),
];

const GATES: &[(&str, &str, &[&str])] = &[
    (
        "1A",
        "Gate 1A - Mathematics",
        &[
            "Derive SVD from eigendecomposition",
            "Explain KL divergence asymmetry",
            "Derive Adam update rule",
            "Derive softmax cross-entropy gradients",
        ],
    ),
    (
        "1B",
        "Gate 1B - Systems",
        &[
            "Explain cache hierarchy and tiled matmul",
            "Write a HIP kernel from memory",
            "Explain GPU occupancy and coalescing",
        ],
    ),
    (
        "1C",
        "Gate 1C - Deep Learning",
        &[
            "Explain transformer forward/backward pass",
            "Read profiler FLOP and bandwidth data",
            "Explain roofline model",
        ],
    ),
    (
        "2A",
        "Gate 2A - Quantization",
        &[
            "Symmetric vs asymmetric quantization",
            "Explain per-channel quantization",
            "Derive STE gradient",
            "Explain Hessian role in GPTQ",
        ],
    ),
    (
        "3D",
        "Gate 3D - Attention",
        &[
            "Explain FlashAttention IO bottleneck",
            "Implement KV cache + GQA",
            "Explain speculative decoding guarantee",
        ],
    ),
    (
        "4C",
        "Gate 4C - Original Research",
        &[
            "Write complete paper draft",
            "Release reproducible code",
            "Present and defend work",
        ],
    ),
];

const READING_ITEMS: &[(&str, &[(&str, &str)])] = &[
    (
        "Textbooks - Mathematics",
        &[
            ("critical", "Linear Algebra Done Right - Axler"),
            (
                "critical",
                "Information Theory, Inference, and Learning Algorithms - MacKay",
            ),
            ("important", "Convex Optimization - Boyd & Vandenberghe"),
        ],
    ),
    (
        "Textbooks - Systems",
        &[
            ("critical", "Computer Systems: A Programmer's Perspective"),
            ("critical", "C Programming Language - K&R"),
            ("critical", "AMD ROCm & HIP Programming Guide"),
        ],
    ),
    (
        "Deep Learning",
        &[
            ("important", "Deep Learning - Goodfellow et al."),
            ("important", "Dive into Deep Learning"),
        ],
    ),
    (
        "Efficient AI Papers",
        &[
            ("critical", "Quantization and GPTQ"),
            ("critical", "Pruning and SparseGPT"),
            ("critical", "FlashAttention and PagedAttention"),
        ],
    ),
];

pub fn default_config_json() -> String {
    serde_json::to_string_pretty(&default_config()).unwrap_or_else(|_| "{}".to_string())
}

pub fn parse_config(json: &str) -> Result<TrackerConfig, String> {
    serde_json::from_str(json).map_err(|err| err.to_string())
}

pub fn config_or_default(json: &str) -> TrackerConfig {
    parse_config(json).unwrap_or_else(|_| default_config())
}

fn default_config() -> TrackerConfig {
    TrackerConfig {
        phases: PHASES
            .iter()
            .map(|(id, title, year, months)| PhaseConfig {
                id: (*id).to_string(),
                title: (*title).to_string(),
                year: (*year).to_string(),
                months: (*months).to_string(),
            })
            .collect(),
        projects: PROJECTS
            .iter()
            .map(|(id, phase, name)| ProjectConfig {
                id: (*id).to_string(),
                phase: (*phase).to_string(),
                name: (*name).to_string(),
            })
            .collect(),
        gates: GATES
            .iter()
            .map(|(id, title, items)| GateConfig {
                id: (*id).to_string(),
                title: (*title).to_string(),
                items: items.iter().map(|item| (*item).to_string()).collect(),
            })
            .collect(),
        reading: READING_ITEMS
            .iter()
            .map(|(section, items)| ReadingSectionConfig {
                section: (*section).to_string(),
                items: items
                    .iter()
                    .map(|(priority, title)| ReadingItemConfig {
                        priority: (*priority).to_string(),
                        title: (*title).to_string(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn activity_chart<'a>(
    t: &'static tokens::Tokens,
    sessions: &[StudySession],
) -> Element<'a, Message> {
    let today = chrono::Local::now().date_naive();
    let mut days = Vec::new();

    for offset in (0..7).rev() {
        let day = today - chrono::Duration::days(offset);
        let key = day.format("%Y-%m-%d").to_string();
        let hours = sessions
            .iter()
            .filter(|session| session.date.starts_with(&key))
            .map(|session| session.hours)
            .sum::<f32>();
        days.push((day, hours));
    }

    let max_hours = days.iter().map(|(_, hours)| *hours).fold(4.0_f32, f32::max);
    let mut bars = row![]
        .spacing(4)
        .align_y(Alignment::End)
        .height(Length::Fixed(80.0));

    for (day, hours) in days {
        let bar_height = ((hours / max_hours) * 60.0).max(4.0);
        let is_today = day == today;
        let color = if is_today {
            t.accent_secondary
        } else {
            t.accent
        };
        bars = bars.push(
            column![
                Space::new().height(Length::Fill),
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fixed(bar_height))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(color)),
                        border: iced::Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                text(
                    day.format("%a")
                        .to_string()
                        .chars()
                        .next()
                        .unwrap_or(' ')
                        .to_string()
                )
                .size(9)
                .color(if is_today {
                    t.accent_secondary
                } else {
                    t.text_muted
                }),
            ]
            .spacing(3)
            .align_x(Alignment::Center)
            .width(Length::FillPortion(1)),
        );
    }

    container(
        column![
            text("Weekly Activity")
                .size(12)
                .color(t.text_primary)
                .font(BOLD),
            bars,
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.bg_secondary)),
        border: iced::Border {
            color: t.border_subtle,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn curriculum_panel<'a>(
    t: &'static tokens::Tokens,
    phases: Vec<PhaseConfig>,
) -> Element<'a, Message> {
    let mut phase_list = column![].spacing(6);
    for phase in phases {
        phase_list = phase_list.push(
            container(
                row![
                    text(phase.id).size(11).color(t.accent).font(BOLD),
                    column![
                        text(phase.title).size(11).color(t.text_primary).font(BOLD),
                        text(format!("{} - {}", phase.year, phase.months))
                            .size(9)
                            .color(t.text_muted),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding(6)
            .style(move |_| container::Style {
                background: Some(Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border_subtle,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
        );
    }

    container(
        column![
            text("Curriculum Roadmap")
                .size(12)
                .color(t.text_primary)
                .font(BOLD),
            scrollable(phase_list).height(Length::Fixed(180.0)),
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Length::Fill)
    .style(move |_| panel_style(t))
    .into()
}

fn milestones_panel<'a>(
    t: &'static tokens::Tokens,
    project_count: usize,
    gate_count: usize,
    reading_sections: Vec<ReadingSectionConfig>,
) -> Element<'a, Message> {
    let reading =
        reading_sections
            .into_iter()
            .take(3)
            .fold(column![].spacing(6), |col, section| {
                let body = section
                    .items
                    .into_iter()
                    .take(3)
                    .map(|item| item.title)
                    .collect::<Vec<_>>()
                    .join(", ");
                col.push(
                    column![
                        text(section.section).size(10).color(t.accent).font(BOLD),
                        text(body).size(9).color(t.text_muted),
                    ]
                    .spacing(1),
                )
            });

    container(
        column![
            text("Milestones").size(12).color(t.text_primary).font(BOLD),
            row![
                kpi_card(t, "PROJECTS", project_count.to_string(), "Milestones"),
                kpi_card(t, "GATES", gate_count.to_string(), "Checkpoints"),
            ]
            .spacing(6),
            text("Reading Tracks")
                .size(11)
                .color(t.text_primary)
                .font(BOLD),
            reading,
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Length::Fill)
    .style(move |_| panel_style(t))
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    t: &'static tokens::Tokens,
    visible: bool,
    running: bool,
    sessions: &'a [StudySession],
    kv: &'a HashMap<String, String>,
    active_tab: TrackerTab,
    config_json: &'a text_editor::Content,
    manual_date: &'a str,
    manual_hours: &'a str,
    manual_notes: &'a str,
) -> Element<'a, Message> {
    if !visible {
        return container(Space::new())
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into();
    }

    let title = row![
        text("Study Tracker").size(16).color(t.accent).font(BOLD),
        Space::new().width(Length::Fill),
        button(text("✕").size(14).font(BOLD))
            .on_press(Message::Tracker(TrackerMessage::Toggle))
            .style(button::text),
    ]
    .align_y(Alignment::Center);

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
        kpi_card(t, "TOTAL", format!("{:.1}h", total_hours), "Accumulated"),
        kpi_card(t, "COUNT", format!("{}", session_count), "Sessions"),
        kpi_card(t, "AVG", format!("{:.1}h", avg_hours), "Hours"),
    ]
    .spacing(4)
    .width(Length::Fill);

    let controls = row![if running {
        button(
            container(text("Stop Timer").size(12).font(BOLD))
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .on_press(Message::Tracker(TrackerMessage::Stop))
        .padding(8)
        .width(Length::Fill)
        .style(button::secondary)
    } else {
        button(
            container(text("Start Timer").size(12).font(BOLD))
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .on_press(Message::Tracker(TrackerMessage::Start))
        .padding(8)
        .width(Length::Fill)
        .style(button::primary)
    },]
    .spacing(8)
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let running_status: Element<'a, Message> = if running {
        container(
            row![
                text("Timer running")
                    .size(11)
                    .color(t.accent_secondary)
                    .font(BOLD),
                Space::new().width(Length::Fill),
                text("Focus session").size(9).color(t.text_muted),
            ]
            .align_y(Alignment::Center),
        )
        .padding(8)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(t.bg_surface)),
            border: iced::Border {
                color: t.accent,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        container(text("Ready to log study time").size(11).color(t.text_muted))
            .padding(8)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border_subtle,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .into()
    };

    let tab_bar = scrollable(
        row![
            tab_button(t, "Dashboard", TrackerTab::Dashboard, active_tab),
            tab_button(t, "Log", TrackerTab::Log, active_tab),
            tab_button(t, "Projects", TrackerTab::Projects, active_tab),
            tab_button(t, "Gates", TrackerTab::Gates, active_tab),
            tab_button(t, "Reading", TrackerTab::Reading, active_tab),
            tab_button(t, "Config", TrackerTab::Config, active_tab),
        ]
        .spacing(4),
    )
    .direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::default(),
    ));

    let body = match active_tab {
        TrackerTab::Dashboard => {
            dashboard_body(t, sessions, running_status, controls, tracker_config)
        }
        TrackerTab::Log => log_body(t, sessions, manual_date, manual_hours, manual_notes),
        TrackerTab::Projects => projects_body(t, kv, tracker_config.projects),
        TrackerTab::Gates => gates_body(t, kv, tracker_config.gates),
        TrackerTab::Reading => reading_body(t, kv, tracker_config.reading),
        TrackerTab::Config => config_body(t, config_json),
    };

    let dashboard = column![title, tab_bar, kpis, body,].spacing(10).padding(10);

    container(dashboard)
        .width(360)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.bg_primary)),
            border: iced::Border {
                color: t.border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn tab_button<'a>(
    t: &'static tokens::Tokens,
    label: &'static str,
    tab: TrackerTab,
    active: TrackerTab,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(11)
            .color(if tab == active {
                t.accent
            } else {
                t.text_muted
            })
            .font(BOLD),
    )
    .on_press(Message::Tracker(TrackerMessage::TabSelected(tab)))
    .padding([4, 6])
    .style(button::text)
    .into()
}

fn dashboard_body<'a>(
    t: &'static tokens::Tokens,
    sessions: &'a [StudySession],
    running_status: Element<'a, Message>,
    controls: iced::widget::Row<'a, Message>,
    config: TrackerConfig,
) -> Element<'a, Message> {
    let sessions_list = sessions_list(t, sessions);
    let project_count = config.projects.len();
    let gate_count = config.gates.len();

    scrollable(
        column![
            controls,
            running_status,
            activity_chart(t, sessions),
            curriculum_panel(t, config.phases),
            milestones_panel(t, project_count, gate_count, config.reading),
            text("Recent Sessions")
                .size(12)
                .color(t.text_primary)
                .font(BOLD),
            sessions_list,
        ]
        .spacing(10),
    )
    .height(Fill)
    .into()
}

fn log_body<'a>(
    t: &'static tokens::Tokens,
    sessions: &'a [StudySession],
    manual_date: &'a str,
    manual_hours: &'a str,
    manual_notes: &'a str,
) -> Element<'a, Message> {
    container(
        column![
            text("Session Log")
                .size(12)
                .color(t.text_primary)
                .font(BOLD),
            column![
                text_input("YYYY-MM-DD", manual_date)
                    .on_input(|value| Message::Tracker(TrackerMessage::ManualDateChanged(value)))
                    .padding(6)
                    .width(Length::Fill),
                row![
                    text_input("Hours", manual_hours)
                        .on_input(|value| Message::Tracker(TrackerMessage::ManualHoursChanged(
                            value
                        )))
                        .padding(6)
                        .width(Length::FillPortion(1)),
                    button(text("Add").size(11).font(BOLD))
                        .on_press(Message::Tracker(TrackerMessage::ManualAdd))
                        .padding([6, 12])
                        .style(button::primary),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                text_input("Notes", manual_notes)
                    .on_input(|value| Message::Tracker(TrackerMessage::ManualNotesChanged(value)))
                    .padding(6)
                    .width(Length::Fill),
            ]
            .spacing(6),
            sessions_list(t, sessions),
        ]
        .spacing(10),
    )
    .padding(6)
    .height(Fill)
    .style(move |_| panel_style(t))
    .into()
}

fn sessions_list<'a>(
    t: &'static tokens::Tokens,
    sessions: &'a [StudySession],
) -> Element<'a, Message> {
    if sessions.is_empty() {
        return container(
            text("No sessions yet. Start studying!")
                .color(t.text_muted)
                .size(11),
        )
        .width(Length::Fill)
        .height(Length::Fixed(80.0))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    let mut col = column![].spacing(6);
    for session in sessions {
        col = col.push(
            container(
                row![
                    column![
                        text(&session.date).size(9).color(t.text_muted),
                        text(format!("{:.1} hours", session.hours))
                            .size(12)
                            .color(t.text_primary)
                            .font(BOLD),
                        text(session.notes.as_deref().unwrap_or(&session.phase))
                            .size(10)
                            .color(t.text_muted),
                    ]
                    .width(Length::Fill),
                    column![
                        text(&session.activity_type)
                            .size(9)
                            .color(t.accent)
                            .font(BOLD),
                        button(text("Delete").size(9).color(t.text_muted))
                            .on_press(Message::Tracker(TrackerMessage::SessionDelete(session.id)))
                            .padding([3, 6])
                            .style(button::text),
                    ]
                    .align_x(Alignment::End)
                    .spacing(2),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .padding(6),
            )
            .style(move |_| container::Style {
                background: Some(Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border_subtle,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
        );
    }
    scrollable(col).height(Length::Fixed(180.0)).into()
}

fn projects_body<'a>(
    t: &'static tokens::Tokens,
    kv: &'a HashMap<String, String>,
    projects: Vec<ProjectConfig>,
) -> Element<'a, Message> {
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
        t,
        "Project Milestones",
        format!(
            "{} items - {} done - {} active",
            projects.len(),
            complete,
            in_progress
        ),
        complete,
        projects.len(),
    )]
    .spacing(8);

    let mut list = column![].spacing(6);
    for project in projects {
        let status = kv
            .get(&format!("proj_{}", project.id))
            .map(String::as_str)
            .unwrap_or("not_started");
        let project_id = project.id.clone();
        list = list.push(
            container(
                row![
                    status_dot(t, status),
                    column![
                        text(format!("{} - {}", project.id, project.name))
                            .size(11)
                            .color(t.text_primary)
                            .font(BOLD),
                        text(format!(
                            "Phase {} - {}",
                            project.phase,
                            status_label(status)
                        ))
                        .size(9)
                        .color(t.text_muted),
                    ]
                    .width(Length::Fill),
                    row![
                        status_button(t, project_id.clone(), "not_started", "Todo", status),
                        status_button(t, project_id.clone(), "in_progress", "Doing", status),
                        status_button(t, project_id, "complete", "Done", status),
                    ]
                    .spacing(2)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(6)
            .style(move |_| panel_style(t)),
        );
    }
    col = col.push(scrollable(list).height(Fill));
    col.into()
}

fn status_button<'a>(
    t: &'static tokens::Tokens,
    id: String,
    value: &'static str,
    label: &'static str,
    current: &str,
) -> Element<'a, Message> {
    let active = current == value;
    button(
        text(label)
            .size(10)
            .color(if active { t.bg_primary } else { t.text_muted }),
    )
    .on_press(Message::Tracker(TrackerMessage::ProjectStatusChanged(
        id,
        value.to_string(),
    )))
    .padding([4, 6])
    .style(if active {
        button::primary
    } else {
        button::secondary
    })
    .into()
}

fn status_label(status: &str) -> &'static str {
    match status {
        "in_progress" => "Doing",
        "complete" => "Done",
        _ => "Todo",
    }
}

fn gates_body<'a>(
    t: &'static tokens::Tokens,
    kv: &'a HashMap<String, String>,
    gates: Vec<GateConfig>,
) -> Element<'a, Message> {
    let total_items = gates.iter().map(|gate| gate.items.len()).sum::<usize>();
    let completed_items = gates
        .iter()
        .map(|gate| {
            gate.items
                .iter()
                .enumerate()
                .filter(|(idx, _)| {
                    kv.get(&format!("gate_{}_{}", gate.id, idx))
                        .map(|v| v == "true")
                        .unwrap_or(false)
                })
                .count()
        })
        .sum::<usize>();

    let mut grid = column![section_summary(
        t,
        "Gate Checkpoints",
        format!("{} gates configured", gates.len()),
        completed_items,
        total_items,
    )]
    .spacing(8);

    let mut list = column![].spacing(6);
    for gate in gates {
        let completed = gate
            .items
            .iter()
            .enumerate()
            .filter(|(idx, _)| {
                kv.get(&format!("gate_{}_{}", gate.id, idx))
                    .map(|v| v == "true")
                    .unwrap_or(false)
            })
            .count();
        let mut item_col = column![
            text(gate.title).size(12).color(t.text_primary).font(BOLD),
            progress_bar(t, completed, gate.items.len())
        ]
        .spacing(6);
        for (idx, item) in gate.items.into_iter().enumerate() {
            let checked = kv
                .get(&format!("gate_{}_{}", gate.id, idx))
                .map(|v| v == "true")
                .unwrap_or(false);
            let gate_id = gate.id.clone();
            item_col = item_col.push(
                checkbox(checked)
                    .label(item)
                    .on_toggle(move |_| {
                        Message::Tracker(TrackerMessage::GateToggled(gate_id.clone(), idx))
                    })
                    .size(13),
            );
        }
        list = list.push(
            container(item_col)
                .padding(8)
                .style(move |_| panel_style(t)),
        );
    }
    grid = grid.push(scrollable(list).height(Fill));
    grid.into()
}

fn reading_body<'a>(
    t: &'static tokens::Tokens,
    kv: &'a HashMap<String, String>,
    sections: Vec<ReadingSectionConfig>,
) -> Element<'a, Message> {
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
                .filter(|(idx, _)| {
                    kv.get(&format!("read_{key_section}_{idx}"))
                        .map(|v| v == "true")
                        .unwrap_or(false)
                })
                .count()
        })
        .sum::<usize>();

    let mut grid = column![section_summary(
        t,
        "Reading Queue",
        format!("{} sections configured", sections.len()),
        completed_items,
        total_items,
    )]
    .spacing(8);

    let mut list = column![].spacing(6);
    for section in sections {
        let key_section = section.section.replace(' ', "");
        let completed = section
            .items
            .iter()
            .enumerate()
            .filter(|(idx, _)| {
                kv.get(&format!("read_{key_section}_{idx}"))
                    .map(|v| v == "true")
                    .unwrap_or(false)
            })
            .count();
        let mut item_col = column![
            text(section.section)
                .size(12)
                .color(t.text_primary)
                .font(BOLD),
            progress_bar(t, completed, section.items.len())
        ]
        .spacing(6);
        for (idx, item) in section.items.into_iter().enumerate() {
            let checked = kv
                .get(&format!("read_{key_section}_{idx}"))
                .map(|v| v == "true")
                .unwrap_or(false);
            let label = format!("{}  {}", item.priority.to_uppercase(), item.title);
            let section_clone = key_section.clone();
            item_col = item_col.push(
                checkbox(checked)
                    .label(label)
                    .on_toggle(move |_| {
                        Message::Tracker(TrackerMessage::ReadingToggled(section_clone.clone(), idx))
                    })
                    .size(13),
            );
        }
        list = list.push(
            container(item_col)
                .padding(8)
                .style(move |_| panel_style(t)),
        );
    }
    grid = grid.push(scrollable(list).height(Fill));
    grid.into()
}

fn config_body<'a>(
    t: &'static tokens::Tokens,
    config_json: &'a text_editor::Content,
) -> Element<'a, Message> {
    container(
        column![
            text("Tracker Configuration")
                .size(12)
                .color(t.text_primary)
                .font(BOLD),
            text("Phases, projects, gates, and reading lists configured below:")
                .size(10)
                .color(t.text_muted),
            text_editor(config_json)
                .placeholder("Tracker JSON")
                .on_action(|action| Message::Tracker(TrackerMessage::ConfigEdited(action)))
                .padding(8)
                .size(11)
                .height(Length::Fixed(200.0))
                .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
                .font(iced::Font::MONOSPACE),
            button(text("Save Configuration").size(11).font(BOLD))
                .on_press(Message::Tracker(TrackerMessage::ConfigSave))
                .padding([6, 12])
                .style(button::primary),
        ]
        .spacing(8),
    )
    .padding(8)
    .height(Fill)
    .style(move |_| panel_style(t))
    .into()
}

fn section_summary<'a>(
    t: &'static tokens::Tokens,
    title: &'static str,
    subtitle: String,
    done: usize,
    total: usize,
) -> Element<'a, Message> {
    container(
        column![
            row![
                column![
                    text(title).size(12).color(t.text_primary).font(BOLD),
                    text(subtitle).size(10).color(t.text_muted),
                ]
                .spacing(1)
                .width(Length::Fill),
                text(format!("{}/{}", done, total))
                    .size(12)
                    .color(t.accent)
                    .font(BOLD),
            ]
            .align_y(Alignment::Center),
            progress_bar(t, done, total),
        ]
        .spacing(6),
    )
    .padding(8)
    .style(move |_| panel_style(t))
    .into()
}

fn progress_bar<'a>(t: &'static tokens::Tokens, done: usize, total: usize) -> Element<'a, Message> {
    let ratio = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    if ratio <= 0.0 {
        return container(Space::new())
            .height(Length::Fixed(4.0))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(t.bg_tertiary)),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into();
    }

    let fill = (ratio * 1000.0).round().max(1.0) as u16;
    let rest = (1000_u16).saturating_sub(fill);

    container(
        row![
            container(Space::new())
                .height(Length::Fixed(4.0))
                .width(Length::FillPortion(fill))
                .style(move |_| container::Style {
                    background: Some(Background::Color(t.accent)),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            container(Space::new())
                .height(Length::Fixed(4.0))
                .width(Length::FillPortion(rest.max(1)))
                .style(move |_| container::Style {
                    background: Some(Background::Color(t.bg_tertiary)),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        ]
        .spacing(0),
    )
    .height(Length::Fixed(4.0))
    .width(Length::Fill)
    .into()
}

fn status_dot<'a>(t: &'static tokens::Tokens, status: &str) -> Element<'a, Message> {
    let color = match status {
        "complete" => t.success,
        "in_progress" => t.accent,
        _ => t.text_muted,
    };

    container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fixed(6.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
