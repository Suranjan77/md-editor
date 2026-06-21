//! Study-tracker sub-state.
//!
//! Owns every field that used to be a `tracker_*` member of `MdEditor` and the
//! handling of the `Tracker*` messages. The shell forwards those messages to
//! [`TrackerState::update`] and renders [`TrackerState::view`]. Side effects
//! that belong to the global UI (toasts) are surfaced as
//! [`Message::ShowToast`] tasks rather than reaching back into the shell.
//!
//! This is the first extraction described in
//! `docs/refactor-mdeditor-decomposition.md`.

use std::collections::HashMap;
use std::time::Instant;

use iced::widget::text_editor;
use iced::{Element, Task, Theme};

use md_editor_core::state::AppState;
use md_editor_core::tracker::StudySession;

use crate::messages::{Message, TrackerTab};
use crate::views;

pub struct TrackerState {
    pub visible: bool,
    running: bool,
    started_at: Option<Instant>,
    sessions: Vec<StudySession>,
    kv: HashMap<String, String>,
    tab: TrackerTab,
    config_json: String,
    config_content: text_editor::Content,
    manual_date: String,
    manual_hours: String,
    manual_notes: String,
}

impl TrackerState {
    pub fn new(state: &AppState) -> Self {
        let sessions = md_editor_core::tracker::get_sessions(state).unwrap_or_default();
        let config_json = md_editor_core::config::get_sys_config(state, "tracker_config")
            .ok()
            .flatten()
            .filter(|json| views::tracker::parse_config(json).is_ok())
            .unwrap_or_else(views::tracker::default_config_json);
        let kv = md_editor_core::tracker::get_kv(state)
            .unwrap_or_default()
            .into_iter()
            .map(|item| (item.key, item.value))
            .collect();

        Self {
            visible: false,
            running: false,
            started_at: None,
            sessions,
            kv,
            tab: TrackerTab::Dashboard,
            config_content: text_editor::Content::with_text(&config_json),
            config_json,
            manual_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            manual_hours: String::new(),
            manual_notes: String::new(),
        }
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Handle a `Tracker*` message. Returns a task; toasts are emitted as
    /// [`Message::ShowToast`] so the shell owns the actual toast field.
    pub fn update(&mut self, message: Message, state: &AppState) -> Task<Message> {
        match message {
            Message::TrackerToggle => {
                self.visible = !self.visible;
                if self.visible {
                    self.reload_from_disk(state);
                }
                Task::none()
            }
            Message::TrackerStart => {
                self.running = true;
                self.started_at = Some(Instant::now());
                toast("Study timer started")
            }
            Message::TrackerStop => {
                let mut task = Task::none();
                if let Some(started_at) = self.started_at.take() {
                    let elapsed = started_at.elapsed();
                    let hours = (elapsed.as_secs_f32() / 3600.0).max(0.01);
                    let session = StudySession {
                        id: 0,
                        date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                        hours,
                        activity_type: "Study".to_string(),
                        phase: "Focus".to_string(),
                        notes: None,
                    };
                    if md_editor_core::tracker::save_session(state, session).is_ok() {
                        self.refresh_sessions(state);
                        task = toast("Study session saved");
                    }
                }
                self.running = false;
                task
            }
            Message::TrackerTabSelected(tab) => {
                self.tab = tab;
                Task::none()
            }
            Message::TrackerProjectStatusChanged(id, status) => {
                let key = format!("proj_{}", id);
                if md_editor_core::tracker::set_kv(state, &key, &status).is_ok() {
                    self.kv.insert(key, status);
                }
                Task::none()
            }
            Message::TrackerGateToggled(gate_id, item_idx) => {
                let key = format!("gate_{}_{}", gate_id, item_idx);
                self.toggle_bool_kv(state, key);
                Task::none()
            }
            Message::TrackerReadingToggled(section, item_idx) => {
                let key = format!("read_{}_{}", section, item_idx);
                self.toggle_bool_kv(state, key);
                Task::none()
            }
            Message::TrackerConfigEdited(action) => {
                self.config_content.perform(action);
                self.config_json = self.config_content.text();
                Task::none()
            }
            Message::TrackerConfigSave => match views::tracker::parse_config(&self.config_json) {
                Ok(_) => {
                    if md_editor_core::config::set_sys_config(
                        state,
                        "tracker_config",
                        &self.config_json,
                    )
                    .is_ok()
                    {
                        toast("Tracker configuration saved")
                    } else {
                        Task::none()
                    }
                }
                Err(err) => toast(format!("Invalid tracker JSON: {}", err)),
            },
            Message::TrackerManualDateChanged(value) => {
                self.manual_date = value;
                Task::none()
            }
            Message::TrackerManualHoursChanged(value) => {
                self.manual_hours = value;
                Task::none()
            }
            Message::TrackerManualNotesChanged(value) => {
                self.manual_notes = value;
                Task::none()
            }
            Message::TrackerManualAdd => self.add_manual_session(state),
            Message::TrackerSessionDelete(id) => {
                match md_editor_core::tracker::delete_session(state, id) {
                    Ok(()) => {
                        self.refresh_sessions(state);
                        toast("Session deleted")
                    }
                    Err(err) => toast(err),
                }
            }
            _ => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        views::tracker::view(
            true,
            self.running,
            &self.sessions,
            &self.kv,
            self.tab,
            &self.config_content,
            &self.manual_date,
            &self.manual_hours,
            &self.manual_notes,
        )
    }

    // ── internals ───────────────────────────────────────────────────

    fn reload_from_disk(&mut self, state: &AppState) {
        self.kv = md_editor_core::tracker::get_kv(state)
            .unwrap_or_default()
            .into_iter()
            .map(|item| (item.key, item.value))
            .collect();
        self.config_json = md_editor_core::config::get_sys_config(state, "tracker_config")
            .ok()
            .flatten()
            .filter(|json| views::tracker::parse_config(json).is_ok())
            .unwrap_or_else(views::tracker::default_config_json);
        self.config_content = text_editor::Content::with_text(&self.config_json);
    }

    fn refresh_sessions(&mut self, state: &AppState) {
        self.sessions = md_editor_core::tracker::get_sessions(state).unwrap_or_default();
    }

    fn toggle_bool_kv(&mut self, state: &AppState, key: String) {
        let next = if self.kv.get(&key).map(|v| v == "true").unwrap_or(false) {
            "false"
        } else {
            "true"
        };
        if md_editor_core::tracker::set_kv(state, &key, next).is_ok() {
            self.kv.insert(key, next.to_string());
        }
    }

    fn add_manual_session(&mut self, state: &AppState) -> Task<Message> {
        match self.manual_hours.trim().parse::<f32>() {
            Ok(hours) if hours > 0.0 => {
                let session = StudySession {
                    id: 0,
                    date: self.manual_date.trim().to_string(),
                    hours,
                    activity_type: "Manual".to_string(),
                    phase: "Manual".to_string(),
                    notes: (!self.manual_notes.trim().is_empty())
                        .then(|| self.manual_notes.trim().to_string()),
                };
                match md_editor_core::tracker::save_session(state, session) {
                    Ok(()) => {
                        self.refresh_sessions(state);
                        self.manual_hours.clear();
                        self.manual_notes.clear();
                        toast("Manual study session added")
                    }
                    Err(err) => toast(err),
                }
            }
            _ => toast("Enter a positive hour value"),
        }
    }
}

fn toast(message: impl Into<String>) -> Task<Message> {
    Task::done(Message::ShowToast(message.into()))
}
