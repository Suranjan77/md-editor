//! Study-tracker sub-state.
//!
//! Owns every field that used to be a `tracker_*` member of `MdEditor` and the
//! handling of the `Tracker*` messages. The shell forwards those messages to
//! [`TrackerState::update`] and renders [`TrackerState::view`]. Side effects
//! that belong to the global UI (toasts) are surfaced as
//! [`ShellMessage::ShowToast`] tasks rather than reaching back into the shell.
//!
//! The first sub-state extracted from the `MdEditor` shell.

use std::collections::HashMap;
use std::time::Instant;

use iced::widget::text_editor;
use iced::{Element, Task, Theme};

use md_editor_core::state::AppState;
use md_editor_core::tracker::StudySession;

use crate::messages::{Message, ShellMessage, TrackerTab};
use crate::views;

pub struct TrackerState {
    pub visible: bool,
    running: bool,
    started_at: Option<Instant>,
    /// What the running session works on; summarized into its notes on stop.
    context: Option<crate::study_context::SessionContext>,
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

        let mut tracker = Self {
            visible: false,
            running: false,
            started_at: None,
            context: None,
            sessions,
            kv,
            tab: TrackerTab::Dashboard,
            config_content: text_editor::Content::with_text(&config_json),
            config_json,
            manual_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            manual_hours: String::new(),
            manual_notes: String::new(),
        };
        tracker.migrate_completion_keys(state);
        tracker
    }

    /// How many sessions are logged (e2e probes).
    #[cfg(test)]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// The notes of the session logged last (e2e probes).
    #[cfg(test)]
    pub fn last_session_notes(&self) -> Option<String> {
        self.sessions.iter().max_by_key(|s| s.id)?.notes.clone()
    }

    /// While the timer runs: the app's state after an update, for the
    /// session's summary.
    pub fn observe(&mut self, seen: &crate::study_context::Observation) {
        if let Some(context) = self.context.as_mut() {
            context.observe(Instant::now(), seen);
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Handle a `Tracker*` message. Returns a task; toasts are emitted as
    /// [`ShellMessage::ShowToast`] so the shell owns the actual toast field.
    pub fn update(&mut self, message: Message, state: &AppState) -> Task<Message> {
        match message {
            Message::Shell(ShellMessage::TrackerToggle) => {
                self.visible = !self.visible;
                if self.visible {
                    self.reload_from_disk(state);
                }
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerStart) => {
                let now = Instant::now();
                self.running = true;
                self.started_at = Some(now);
                // The shell's first observation follows this update.
                self.context = Some(crate::study_context::SessionContext::start(
                    now,
                    &Default::default(),
                ));
                toast("Study timer started")
            }
            Message::Shell(ShellMessage::TrackerStop) => {
                let mut task = Task::none();
                let summary = self
                    .context
                    .take()
                    .and_then(|context| context.finish(Instant::now()));
                if let Some(started_at) = self.started_at.take() {
                    let elapsed = started_at.elapsed();
                    let hours = (elapsed.as_secs_f32() / 3600.0).max(0.01);
                    let session = StudySession {
                        id: 0,
                        date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                        hours,
                        activity_type: "Study".to_string(),
                        phase: "Focus".to_string(),
                        notes: summary,
                    };
                    if md_editor_core::tracker::save_session(state, session).is_ok() {
                        self.refresh_sessions(state);
                        task = toast("Study session saved");
                    }
                }
                self.running = false;
                task
            }
            Message::Shell(ShellMessage::TrackerTabSelected(tab)) => {
                self.tab = tab;
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerProjectStatusChanged(id, status)) => {
                let key = format!("proj_{}", id);
                if md_editor_core::tracker::set_kv(state, &key, &status).is_ok() {
                    self.kv.insert(key, status);
                }
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerGateToggled(gate_id, item, item_idx)) => {
                let keys = views::tracker::gate_completion_keys(&gate_id, &item, item_idx);
                self.toggle_bool_kv_with_legacy(state, keys);
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerReadingToggled(section, item, item_idx)) => {
                let keys = views::tracker::reading_completion_keys(&section, &item, item_idx);
                self.toggle_bool_kv_with_legacy(state, keys);
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerConfigEdited(action)) => {
                self.config_content.perform(action);
                self.config_json = self.config_content.text();
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerConfigSave) => {
                match views::tracker::parse_config(&self.config_json) {
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
                }
            }
            Message::Shell(ShellMessage::TrackerManualDateChanged(value)) => {
                self.manual_date = value;
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerManualHoursChanged(value)) => {
                self.manual_hours = value;
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerManualNotesChanged(value)) => {
                self.manual_notes = value;
                Task::none()
            }
            Message::Shell(ShellMessage::TrackerManualAdd) => self.add_manual_session(state),
            Message::Shell(ShellMessage::TrackerSessionDelete(id)) => {
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
        views::tracker::view(views::tracker::TrackerProps {
            visible: true,
            running: self.running,
            sessions: &self.sessions,
            kv: &self.kv,
            active_tab: self.tab,
            config_json: &self.config_content,
            manual_date: &self.manual_date,
            manual_hours: &self.manual_hours,
            manual_notes: &self.manual_notes,
        })
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
        self.migrate_completion_keys(state);
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

    fn toggle_bool_kv_with_legacy(&mut self, state: &AppState, (key, legacy): (String, String)) {
        if !self.kv.contains_key(&key)
            && let Some(value) = self.kv.get(&legacy).cloned()
        {
            let _ = md_editor_core::tracker::set_kv(state, &key, &value);
            self.kv.insert(key.clone(), value);
        }
        self.toggle_bool_kv(state, key);
    }

    fn migrate_completion_keys(&mut self, state: &AppState) {
        let Ok(config) = views::tracker::parse_config(&self.config_json) else {
            return;
        };
        let mut keys = Vec::new();
        for gate in config.gates {
            for (index, item) in gate.items.iter().enumerate() {
                keys.push(views::tracker::gate_completion_keys(&gate.id, item, index));
            }
        }
        for section in config.reading {
            let section_key = section.section.replace(' ', "");
            for (index, item) in section.items.iter().enumerate() {
                keys.push(views::tracker::reading_completion_keys(
                    &section_key,
                    &item.title,
                    index,
                ));
            }
        }
        for (key, legacy) in keys {
            if !self.kv.contains_key(&key)
                && let Some(value) = self.kv.get(&legacy).cloned()
                && md_editor_core::tracker::set_kv(state, &key, &value).is_ok()
            {
                self.kv.insert(key, value);
            }
        }
    }

    fn add_manual_session(&mut self, state: &AppState) -> Task<Message> {
        let date = match chrono::NaiveDate::parse_from_str(self.manual_date.trim(), "%Y-%m-%d") {
            Ok(date) => date.format("%Y-%m-%d").to_string(),
            Err(_) => return toast("Enter a valid date in YYYY-MM-DD format"),
        };
        match self.manual_hours.trim().parse::<f32>() {
            Ok(hours) if hours.is_finite() && hours > 0.0 => {
                let session = StudySession {
                    id: 0,
                    date,
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
    Task::done(Message::Shell(ShellMessage::ShowToast(message.into())))
}
