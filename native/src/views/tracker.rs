use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Background, Element, Length, Renderer, Theme};

use crate::messages::{Message, TrackerTab};
use crate::theme;
use md_editor_core::tracker::StudySession;

const BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

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

const READING_SECTIONS: &[(&str, &str)] = &[
    ("Mathematics", "Axler, Matrix Cookbook, MacKay, MML, Boyd"),
    ("Systems", "CS:APP, K&R, ROCm/HIP guide, PMPP"),
    ("Deep Learning", "Goodfellow, Dive into Deep Learning"),
    ("Efficient AI", "Quantization, pruning, distillation, serving papers"),
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
    ("1A", "Gate 1A - Mathematics", &[
        "Derive SVD from eigendecomposition",
        "Explain KL divergence asymmetry",
        "Derive Adam update rule",
        "Derive softmax cross-entropy gradients",
    ]),
    ("1B", "Gate 1B - Systems", &[
        "Explain cache hierarchy and tiled matmul",
        "Write a HIP kernel from memory",
        "Explain GPU occupancy and coalescing",
    ]),
    ("1C", "Gate 1C - Deep Learning", &[
        "Explain transformer forward/backward pass",
        "Read profiler FLOP and bandwidth data",
        "Explain roofline model",
    ]),
    ("2A", "Gate 2A - Quantization", &[
        "Symmetric vs asymmetric quantization",
        "Explain per-channel quantization",
        "Derive STE gradient",
        "Explain Hessian role in GPTQ",
    ]),
    ("3D", "Gate 3D - Attention", &[
        "Explain FlashAttention IO bottleneck",
        "Implement KV cache + GQA",
        "Explain speculative decoding guarantee",
    ]),
    ("4C", "Gate 4C - Original Research", &[
        "Write complete paper draft",
        "Release reproducible code",
        "Present and defend work",
    ]),
];

const READING_ITEMS: &[(&str, &[(&str, &str)])] = &[
    ("Textbooks - Mathematics", &[
        ("critical", "Linear Algebra Done Right - Axler"),
        ("critical", "Information Theory, Inference, and Learning Algorithms - MacKay"),
        ("important", "Convex Optimization - Boyd & Vandenberghe"),
    ]),
    ("Textbooks - Systems", &[
        ("critical", "Computer Systems: A Programmer's Perspective"),
        ("critical", "C Programming Language - K&R"),
        ("critical", "AMD ROCm & HIP Programming Guide"),
    ]),
    ("Deep Learning", &[
        ("important", "Deep Learning - Goodfellow et al."),
        ("important", "Dive into Deep Learning"),
    ]),
    ("Efficient AI Papers", &[
        ("critical", "Quantization and GPTQ"),
        ("critical", "Pruning and SparseGPT"),
        ("critical", "FlashAttention and PagedAttention"),
    ]),
];

pub fn default_config_json() -> String {
    "{\n  \"PHASES\": \"See src/tracker-data.js for the full editable schema\",\n  \"PROJECTS\": \"Project status is stored in tracker_kv as proj_<id>\",\n  \"GATES\": \"Gate checkboxes are stored as gate_<phase>_<index>\",\n  \"READING\": \"Reading checkboxes are stored as read_<section>_<index>\"\n}".to_string()
}

fn kpi_card<'a>(
    title: &'static str,
    value: String,
    sub: &'static str,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        column![
            text(title).size(9).color(theme::TEXT_MUTED).font(BOLD),
            text(value).size(18).color(theme::ACCENT).font(BOLD),
            text(sub).size(8).color(theme::TEXT_MUTED),
        ]
        .spacing(2),
    )
    .padding(10)
    .width(Length::FillPortion(1))
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER_SUBTLE,
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
            theme::ACCENT_SECONDARY
        } else {
            theme::ACCENT
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
                    theme::ACCENT_SECONDARY
                } else {
                    theme::TEXT_MUTED
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
                .color(theme::TEXT_PRIMARY)
                .font(BOLD),
            bars,
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER_SUBTLE,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn curriculum_panel<'a>() -> Element<'a, Message, Theme, Renderer> {
    let mut phases = column![].spacing(6);
    for (id, title, year, months) in PHASES {
        phases = phases.push(
            container(
                row![
                    text(*id).size(12).color(theme::ACCENT).font(BOLD),
                    column![
                        text(*title).size(13).color(theme::TEXT_PRIMARY).font(BOLD),
                        text(format!("{year} - {months}")).size(10).color(theme::TEXT_MUTED),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(8)
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_SECONDARY)),
                border: iced::Border {
                    color: theme::BORDER_SUBTLE,
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
                .color(theme::TEXT_PRIMARY)
                .font(BOLD),
            scrollable(phases).height(Length::Fill),
        ]
        .spacing(10),
    )
    .padding(14)
    .height(Length::Fill)
    .style(|_| panel_style())
    .into()
}

fn milestones_panel<'a>() -> Element<'a, Message, Theme, Renderer> {
    let reading = READING_SECTIONS.iter().fold(column![].spacing(8), |col, (title, body)| {
        col.push(
            column![
                text(*title).size(12).color(theme::ACCENT).font(BOLD),
                text(*body).size(11).color(theme::TEXT_MUTED),
            ]
            .spacing(2),
        )
    });

    container(
        column![
            text("Projects, Gates, Reading")
                .size(14)
                .color(theme::TEXT_PRIMARY)
                .font(BOLD),
            row![
                kpi_card("PROJECTS", "51".to_string(), "Implementation milestones"),
                kpi_card("GATES", "17".to_string(), "Oral/checkpoint gates"),
            ]
            .spacing(8),
            text("Reading Tracks")
                .size(13)
                .color(theme::TEXT_PRIMARY)
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
        background: Some(Background::Color(theme::BG_PRIMARY)),
        border: iced::Border {
            color: theme::BORDER,
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
    config_json: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let title = text("Study Tracker")
        .size(18)
        .color(theme::ACCENT)
        .font(BOLD);

    let total_hours = sessions.iter().map(|s| s.hours).sum::<f32>();
    let session_count = sessions.len();
    let avg_hours = if session_count > 0 {
        total_hours / session_count as f32
    } else {
        0.0
    };

    let kpis = row![
        kpi_card("TOTAL TIME", format!("{:.1}h", total_hours), "Accumulated"),
        kpi_card("SESSIONS", format!("{}", session_count), "Total sessions"),
        kpi_card("AVERAGE", format!("{:.1}h", avg_hours), "Per session"),
        kpi_card("CURRICULUM", "4 years".to_string(), "17 phases"),
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
                    .color(theme::ACCENT_SECONDARY)
                    .font(BOLD),
                Space::new().width(Length::Fill),
                text("Focus session").size(10).color(theme::TEXT_MUTED),
            ]
            .align_y(Alignment::Center),
        )
        .padding(10)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(theme::BG_SURFACE)),
            border: iced::Border {
                color: theme::ACCENT,
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
                .color(theme::TEXT_MUTED),
        )
        .padding(10)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(theme::BG_SECONDARY)),
            border: iced::Border {
                color: theme::BORDER_SUBTLE,
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
        TrackerTab::Dashboard => dashboard_body(sessions, running_status, controls),
        TrackerTab::Log => log_body(sessions),
        TrackerTab::Projects => projects_body(kv),
        TrackerTab::Gates => gates_body(kv),
        TrackerTab::Reading => reading_body(kv),
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

fn tab_button<'a>(label: &'static str, tab: TrackerTab, active: TrackerTab) -> Element<'a, Message, Theme, Renderer> {
    button(text(label).size(12).color(if tab == active { theme::ACCENT } else { theme::TEXT_MUTED }).font(BOLD))
        .on_press(Message::TrackerTabSelected(tab))
        .padding([8, 12])
        .style(button::text)
        .into()
}

fn dashboard_body<'a>(
    sessions: &'a [StudySession],
    running_status: Element<'a, Message, Theme, Renderer>,
    controls: iced::widget::Row<'a, Message, Theme, Renderer>,
) -> Element<'a, Message, Theme, Renderer> {
    let sessions_list = sessions_list(sessions);
    row![
        column![
            controls,
            running_status,
            activity_chart(sessions),
            text("Recent Sessions").size(14).color(theme::TEXT_PRIMARY).font(BOLD),
            sessions_list,
        ].spacing(12).width(Length::FillPortion(2)),
        curriculum_panel(),
        milestones_panel(),
    ].spacing(14).height(Length::Fill).into()
}

fn log_body<'a>(sessions: &'a [StudySession]) -> Element<'a, Message, Theme, Renderer> {
    container(column![
        text("Session Log").size(15).color(theme::TEXT_PRIMARY).font(BOLD),
        sessions_list(sessions),
    ].spacing(12))
        .padding(14)
        .height(Length::Fill)
        .style(|_| panel_style())
        .into()
}

fn sessions_list<'a>(sessions: &'a [StudySession]) -> Element<'a, Message, Theme, Renderer> {
    if sessions.is_empty() {
        return container(text("No sessions yet. Start studying!").color(theme::TEXT_MUTED).size(13))
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
                        text(&session.date).size(10).color(theme::TEXT_MUTED),
                        text(format!("{:.1} hours", session.hours)).size(14).color(theme::TEXT_PRIMARY).font(BOLD),
                        text(session.notes.as_deref().unwrap_or(&session.phase)).size(11).color(theme::TEXT_MUTED),
                    ].width(Length::Fill),
                    text(&session.activity_type).size(11).color(theme::ACCENT).font(BOLD),
                ].align_y(Alignment::Center).padding(8),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_SECONDARY)),
                border: iced::Border { color: theme::BORDER_SUBTLE, width: 1.0, radius: 6.0.into() },
                ..Default::default()
            }),
        );
    }
    scrollable(col).height(Length::Fill).into()
}

fn projects_body<'a>(kv: &'a std::collections::HashMap<String, String>) -> Element<'a, Message, Theme, Renderer> {
    let mut col = column![].spacing(8);
    for (id, phase, name) in PROJECTS {
        let status = kv.get(&format!("proj_{id}")).map(String::as_str).unwrap_or("not_started");
        col = col.push(container(row![
            column![
                text(format!("{id} - {name}")).size(13).color(theme::TEXT_PRIMARY).font(BOLD),
                text(format!("Phase {phase} - {status}")).size(10).color(theme::TEXT_MUTED),
            ].width(Length::Fill),
            status_button(*id, "not_started", "Todo", status),
            status_button(*id, "in_progress", "Doing", status),
            status_button(*id, "complete", "Done", status),
        ].spacing(8).align_y(Alignment::Center)).padding(10).style(|_| panel_style()));
    }
    scrollable(col).height(Length::Fill).into()
}

fn status_button<'a>(id: &'static str, value: &'static str, label: &'static str, current: &str) -> Element<'a, Message, Theme, Renderer> {
    button(text(label).size(11).color(if current == value { theme::ACCENT } else { theme::TEXT_MUTED }))
        .on_press(Message::TrackerProjectStatusChanged(id.to_string(), value.to_string()))
        .padding([6, 8])
        .style(button::text)
        .into()
}

fn gates_body<'a>(kv: &'a std::collections::HashMap<String, String>) -> Element<'a, Message, Theme, Renderer> {
    let mut grid = column![].spacing(12);
    for (gate_id, title, items) in GATES {
        let mut item_col = column![text(*title).size(14).color(theme::TEXT_PRIMARY).font(BOLD)].spacing(6);
        for (idx, item) in items.iter().enumerate() {
            let checked = kv.get(&format!("gate_{gate_id}_{idx}")).map(|v| v == "true").unwrap_or(false);
            item_col = item_col.push(
                checkbox(checked)
                    .label(*item)
                    .on_toggle(move |_| Message::TrackerGateToggled((*gate_id).to_string(), idx))
                    .size(15)
            );
        }
        grid = grid.push(container(item_col).padding(12).style(|_| panel_style()));
    }
    scrollable(grid).height(Length::Fill).into()
}

fn reading_body<'a>(kv: &'a std::collections::HashMap<String, String>) -> Element<'a, Message, Theme, Renderer> {
    let mut grid = column![].spacing(12);
    for (section, items) in READING_ITEMS {
        let key_section = section.replace(' ', "");
        let mut item_col = column![text(*section).size(14).color(theme::TEXT_PRIMARY).font(BOLD)].spacing(6);
        for (idx, (pri, item)) in items.iter().enumerate() {
            let checked = kv.get(&format!("read_{key_section}_{idx}")).map(|v| v == "true").unwrap_or(false);
            let label = format!("[{pri}] {item}");
            let section_clone = key_section.clone();
            item_col = item_col.push(
                checkbox(checked)
                    .label(label)
                    .on_toggle(move |_| Message::TrackerReadingToggled(section_clone.clone(), idx))
                    .size(15)
            );
        }
        grid = grid.push(container(item_col).padding(12).style(|_| panel_style()));
    }
    scrollable(grid).height(Length::Fill).into()
}

fn config_body<'a>(config_json: &'a str) -> Element<'a, Message, Theme, Renderer> {
    container(column![
        text("Tracker JSON Configuration").size(15).color(theme::TEXT_PRIMARY).font(BOLD),
        text("Stored in settings.tracker_config. Edit as compact JSON here; the web tracker uses the full schema.").size(12).color(theme::TEXT_MUTED),
        text_input("Tracker JSON", config_json)
            .on_input(Message::TrackerConfigChanged)
            .padding(10)
            .size(12),
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
