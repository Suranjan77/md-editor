use iced::advanced::text::Wrapping;
use iced::widget::{
    Space, button, checkbox, column, container, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Background, Element, Length, Renderer, Theme};

use crate::messages::{Message, TrackerTab};
use crate::theme;
use md_editor_core::tracker::StudySession;
use serde::{Deserialize, Serialize};

const BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

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
    let config: TrackerConfig = serde_json::from_str(json).map_err(|err| err.to_string())?;
    if config.phases.is_empty() {
        return Err("PHASES must contain at least one phase".to_string());
    }
    if config.projects.is_empty() {
        return Err("PROJECTS must contain at least one project".to_string());
    }
    Ok(config)
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

fn kpi_card<'a>(
    title: &'static str,
    value: String,
    sub: &'static str,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        column![
            text(title).size(9).color(theme::text_muted()).font(BOLD),
            text(value).size(18).color(theme::accent()).font(BOLD),
            text(sub).size(8).color(theme::text_muted()),
        ]
        .spacing(2),
    )
    .padding(10)
    .width(Length::FillPortion(1))
    .style(|_| container::Style {
        background: Some(Background::Color(theme::bg_secondary())),
        border: iced::Border {
            color: theme::border_subtle(),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
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

    let max_hours = days.iter().map(|(_, hours)| *hours).fold(4.0_f32, f32::max);
    let mut bars = row![]
        .spacing(4)
        .align_y(Alignment::End)
        .height(Length::Fixed(120.0));

    for (day, hours) in days {
        let bar_height = ((hours / max_hours) * 88.0).max(4.0);
        let is_today = day == today;
        let color = if is_today {
            theme::accent_secondary()
        } else {
            theme::accent()
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
                    theme::accent_secondary()
                } else {
                    theme::text_muted()
                }),
            ]
            .spacing(5)
            .align_x(Alignment::Center)
            .width(Length::FillPortion(1)),
        );
    }

    container(
        column![
            text("Weekly Activity")
                .size(13)
                .color(theme::text_primary())
                .font(BOLD),
            bars,
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(theme::bg_secondary())),
        border: iced::Border {
            color: theme::border_subtle(),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn curriculum_panel<'a>(phases: Vec<PhaseConfig>) -> Element<'a, Message, Theme, Renderer> {
    let mut phase_list = column![].spacing(6);
    for phase in phases {
        phase_list = phase_list.push(
            container(
                row![
                    text(phase.id).size(12).color(theme::accent()).font(BOLD),
                    column![
                        text(phase.title)
                            .size(13)
                            .color(theme::text_primary())
                            .font(BOLD),
                        text(format!("{} - {}", phase.year, phase.months))
                            .size(10)
                            .color(theme::text_muted()),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(8)
            .style(|_| container::Style {
                background: Some(Background::Color(theme::bg_secondary())),
                border: iced::Border {
                    color: theme::border_subtle(),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }),
        );
    }

    container(
        column![
            text("Curriculum Roadmap")
                .size(14)
                .color(theme::text_primary())
                .font(BOLD),
            scrollable(phase_list).height(Length::Fill),
        ]
        .spacing(10),
    )
    .padding(14)
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
            .fold(column![].spacing(8), |col, section| {
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
                            .size(12)
                            .color(theme::accent())
                            .font(BOLD),
                        text(body).size(11).color(theme::text_muted()),
                    ]
                    .spacing(2),
                )
            });

    container(
        column![
            text("Projects, Gates, Reading")
                .size(14)
                .color(theme::text_primary())
                .font(BOLD),
            row![
                kpi_card(
                    "PROJECTS",
                    project_count.to_string(),
                    "Implementation milestones"
                ),
                kpi_card("GATES", gate_count.to_string(), "Checkpoint gates"),
            ]
            .spacing(8),
            text("Reading Tracks")
                .size(13)
                .color(theme::text_primary())
                .font(BOLD),
            reading,
        ]
        .spacing(12),
    )
    .padding(14)
    .height(Length::Fill)
    .style(|_| panel_style())
    .into()
}

fn panel_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::bg_primary())),
        border: iced::Border {
            color: theme::border(),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub fn view<'a>(
    visible: bool,
    running: bool,
    sessions: &'a [StudySession],
    kv: &'a std::collections::HashMap<String, String>,
    active_tab: TrackerTab,
    config_json: &'a text_editor::Content,
    manual_date: &'a str,
    manual_hours: &'a str,
    manual_notes: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let title = text("Study Tracker")
        .size(18)
        .color(theme::accent())
        .font(BOLD);

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
        kpi_card("TOTAL TIME", format!("{:.1}h", total_hours), "Accumulated"),
        kpi_card("SESSIONS", format!("{}", session_count), "Total sessions"),
        kpi_card("AVERAGE", format!("{:.1}h", avg_hours), "Per session"),
        kpi_card(
            "CURRICULUM",
            format!("{} phases", tracker_config.phases.len()),
            "Configured roadmap"
        ),
    ]
    .spacing(6)
    .width(Length::Fill);

    let controls = row![if running {
        button(
            container(text("Stop Timer").size(13).font(BOLD))
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .on_press(Message::TrackerStop)
        .padding(10)
        .width(Length::Fill)
        .style(button::secondary)
    } else {
        button(
            container(text("Start Timer").size(13).font(BOLD))
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .on_press(Message::TrackerStart)
        .padding(10)
        .width(Length::Fill)
        .style(button::primary)
    },]
    .spacing(10)
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let running_status: Element<'a, Message, Theme, Renderer> = if running {
        container(
            row![
                text("Timer running")
                    .size(12)
                    .color(theme::accent_secondary())
                    .font(BOLD),
                Space::new().width(Length::Fill),
                text("Focus session").size(10).color(theme::text_muted()),
            ]
            .align_y(Alignment::Center),
        )
        .padding(10)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(theme::bg_surface())),
            border: iced::Border {
                color: theme::accent(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        container(
            text("Ready to log focused study time")
                .size(12)
                .color(theme::text_muted()),
        )
        .padding(10)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(theme::bg_secondary())),
            border: iced::Border {
                color: theme::border_subtle(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
    };

    let tab_bar = row![
        tab_button("Dashboard", TrackerTab::Dashboard, active_tab),
        tab_button("Log", TrackerTab::Log, active_tab),
        tab_button("Projects", TrackerTab::Projects, active_tab),
        tab_button("Gates", TrackerTab::Gates, active_tab),
        tab_button("Reading", TrackerTab::Reading, active_tab),
        tab_button("Config", TrackerTab::Config, active_tab),
    ]
    .spacing(6);

    let body = match active_tab {
        TrackerTab::Dashboard => dashboard_body(sessions, running_status, controls, tracker_config),
        TrackerTab::Log => log_body(sessions, manual_date, manual_hours, manual_notes),
        TrackerTab::Projects => projects_body(kv, tracker_config.projects),
        TrackerTab::Gates => gates_body(kv, tracker_config.gates),
        TrackerTab::Reading => reading_body(kv, tracker_config.reading),
        TrackerTab::Config => config_body(config_json),
    };

    let dashboard = column![
        row![
            title,
            Space::new().width(Length::Fill),
            button(text("✕").size(16).font(BOLD))
                .on_press(Message::TrackerToggle)
                .style(button::text),
        ]
        .align_y(Alignment::Center),
        tab_bar,
        kpis,
        body,
    ]
    .spacing(16)
    .padding(18);

    container(dashboard)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| panel_style())
        .into()
}

fn tab_button<'a>(
    label: &'static str,
    tab: TrackerTab,
    active: TrackerTab,
) -> Element<'a, Message, Theme, Renderer> {
    button(
        text(label)
            .size(12)
            .color(if tab == active {
                theme::accent()
            } else {
                theme::text_muted()
            })
            .font(BOLD),
    )
    .on_press(Message::TrackerTabSelected(tab))
    .padding([8, 12])
    .style(button::text)
    .into()
}

fn dashboard_body<'a>(
    sessions: &'a [StudySession],
    running_status: Element<'a, Message, Theme, Renderer>,
    controls: iced::widget::Row<'a, Message, Theme, Renderer>,
    config: TrackerConfig,
) -> Element<'a, Message, Theme, Renderer> {
    let sessions_list = sessions_list(sessions);
    let project_count = config.projects.len();
    let gate_count = config.gates.len();
    row![
        column![
            controls,
            running_status,
            activity_chart(sessions),
            text("Recent Sessions")
                .size(14)
                .color(theme::text_primary())
                .font(BOLD),
            sessions_list,
        ]
        .spacing(12)
        .width(Length::FillPortion(2)),
        curriculum_panel(config.phases),
        milestones_panel(project_count, gate_count, config.reading),
    ]
    .spacing(14)
    .height(Length::Fill)
    .into()
}

fn log_body<'a>(
    sessions: &'a [StudySession],
    manual_date: &'a str,
    manual_hours: &'a str,
    manual_notes: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        column![
            text("Session Log")
                .size(15)
                .color(theme::text_primary())
                .font(BOLD),
            row![
                text_input("YYYY-MM-DD", manual_date)
                    .on_input(Message::TrackerManualDateChanged)
                    .padding(8)
                    .width(Length::FillPortion(2)),
                text_input("Hours", manual_hours)
                    .on_input(Message::TrackerManualHoursChanged)
                    .padding(8)
                    .width(Length::FillPortion(1)),
                text_input("Notes", manual_notes)
                    .on_input(Message::TrackerManualNotesChanged)
                    .padding(8)
                    .width(Length::FillPortion(3)),
                button(text("Add").size(12).font(BOLD))
                    .on_press(Message::TrackerManualAdd)
                    .padding([8, 12])
                    .style(button::primary),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            sessions_list(sessions),
        ]
        .spacing(12),
    )
    .padding(14)
    .height(Length::Fill)
    .style(|_| panel_style())
    .into()
}

fn sessions_list<'a>(sessions: &'a [StudySession]) -> Element<'a, Message, Theme, Renderer> {
    if sessions.is_empty() {
        return container(
            text("No sessions yet. Start studying!")
                .color(theme::text_muted())
                .size(13),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    let mut col = column![].spacing(8);
    for session in sessions {
        col = col.push(
            container(
                row![
                    column![
                        text(&session.date).size(10).color(theme::text_muted()),
                        text(format!("{:.1} hours", session.hours))
                            .size(14)
                            .color(theme::text_primary())
                            .font(BOLD),
                        text(session.notes.as_deref().unwrap_or(&session.phase))
                            .size(11)
                            .color(theme::text_muted()),
                    ]
                    .width(Length::Fill),
                    text(&session.activity_type)
                        .size(11)
                        .color(theme::accent())
                        .font(BOLD),
                    button(text("Delete").size(11).color(theme::text_muted()))
                        .on_press(Message::TrackerSessionDelete(session.id))
                        .padding([5, 8])
                        .style(button::text),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding(8),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(theme::bg_secondary())),
                border: iced::Border {
                    color: theme::border_subtle(),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }),
        );
    }
    scrollable(col).height(Length::Fill).into()
}

fn projects_body<'a>(
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
    .spacing(10);

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
                            .size(13)
                            .color(theme::text_primary())
                            .font(BOLD),
                        text(format!(
                            "Phase {} - {}",
                            project.phase,
                            status_label(status)
                        ))
                        .size(10)
                        .color(theme::text_muted()),
                    ]
                    .width(Length::Fill),
                    status_button(project_id.clone(), "not_started", "Todo", status),
                    status_button(project_id.clone(), "in_progress", "Doing", status),
                    status_button(project_id, "complete", "Done", status),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(10)
            .style(|_| panel_style()),
        );
    }
    scrollable(col).height(Length::Fill).into()
}

fn status_button<'a>(
    id: String,
    value: &'static str,
    label: &'static str,
    current: &str,
) -> Element<'a, Message, Theme, Renderer> {
    let active = current == value;
    button(text(label).size(11).color(if active {
        theme::bg_primary()
    } else {
        theme::text_muted()
    }))
    .on_press(Message::TrackerProjectStatusChanged(id, value.to_string()))
    .padding([6, 10])
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
                .filter(|(idx, _)| {
                    kv.get(&format!("gate_{}_{}", gate.id, idx))
                        .map(|v| v == "true")
                        .unwrap_or(false)
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
    .spacing(12);

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
            text(gate.title)
                .size(14)
                .color(theme::text_primary())
                .font(BOLD),
            progress_bar(completed, gate.items.len())
        ]
        .spacing(8);
        for (idx, item) in gate.items.into_iter().enumerate() {
            let checked = kv
                .get(&format!("gate_{}_{}", gate.id, idx))
                .map(|v| v == "true")
                .unwrap_or(false);
            let gate_id = gate.id.clone();
            item_col = item_col.push(
                checkbox(checked)
                    .label(item)
                    .on_toggle(move |_| Message::TrackerGateToggled(gate_id.clone(), idx))
                    .size(15),
            );
        }
        grid = grid.push(container(item_col).padding(12).style(|_| panel_style()));
    }
    scrollable(grid).height(Length::Fill).into()
}

fn reading_body<'a>(
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
                .filter(|(idx, _)| {
                    kv.get(&format!("read_{key_section}_{idx}"))
                        .map(|v| v == "true")
                        .unwrap_or(false)
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
    .spacing(12);

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
                .size(14)
                .color(theme::text_primary())
                .font(BOLD),
            progress_bar(completed, section.items.len())
        ]
        .spacing(8);
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
                    .on_toggle(move |_| Message::TrackerReadingToggled(section_clone.clone(), idx))
                    .size(15),
            );
        }
        grid = grid.push(container(item_col).padding(12).style(|_| panel_style()));
    }
    scrollable(grid).height(Length::Fill).into()
}

fn config_body<'a>(config_json: &'a text_editor::Content) -> Element<'a, Message, Theme, Renderer> {
    container(column![
        text("Tracker JSON Configuration").size(15).color(theme::text_primary()).font(BOLD),
        text("Projects, gates, reading lists, and phases are read from this JSON. Save to apply it to every tracker tab.").size(12).color(theme::text_muted()),
        text_editor(config_json)
            .placeholder("Tracker JSON")
            .on_action(Message::TrackerConfigEdited)
            .padding(10)
            .size(12)
            .height(Length::Fixed(260.0))
            .wrapping(Wrapping::WordOrGlyph)
            .font(iced::Font::MONOSPACE),
        button(text("Save Configuration").size(12).font(BOLD))
            .on_press(Message::TrackerConfigSave)
            .padding([8, 12])
            .style(button::primary),
    ].spacing(12))
        .padding(14)
        .height(Length::Fill)
        .style(|_| panel_style())
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
                    text(title).size(15).color(theme::text_primary()).font(BOLD),
                    text(subtitle).size(11).color(theme::text_muted()),
                ]
                .spacing(2)
                .width(Length::Fill),
                text(format!("{}/{}", done, total))
                    .size(14)
                    .color(theme::accent())
                    .font(BOLD),
            ]
            .align_y(Alignment::Center),
            progress_bar(done, total),
        ]
        .spacing(10),
    )
    .padding(12)
    .style(|_| panel_style())
    .into()
}

fn progress_bar<'a>(done: usize, total: usize) -> Element<'a, Message, Theme, Renderer> {
    let ratio = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    if ratio <= 0.0 {
        return container(Space::new())
            .height(Length::Fixed(6.0))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(theme::bg_tertiary())),
                border: iced::Border {
                    radius: 3.0.into(),
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
                .height(Length::Fixed(6.0))
                .width(Length::FillPortion(fill))
                .style(|_| container::Style {
                    background: Some(Background::Color(theme::accent())),
                    border: iced::Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            container(Space::new())
                .height(Length::Fixed(6.0))
                .width(Length::FillPortion(rest.max(1)))
                .style(|_| container::Style {
                    background: Some(Background::Color(theme::bg_tertiary())),
                    border: iced::Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        ]
        .spacing(0),
    )
    .height(Length::Fixed(6.0))
    .width(Length::Fill)
    .into()
}

fn status_dot<'a>(status: &str) -> Element<'a, Message, Theme, Renderer> {
    let color = match status {
        "complete" => theme::success(),
        "in_progress" => theme::accent(),
        _ => theme::text_muted(),
    };

    container(Space::new())
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
