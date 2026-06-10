use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::widget::text_editor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackerTab {
    Dashboard,
    Log,
    Projects,
    Gates,
    Reading,
    Config,
}

#[derive(Debug, Clone)]
pub(crate) enum TrackerMessage {
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

#[derive(Debug, PartialEq)]
pub(crate) enum TrackerEffect {
    None,
    PersistShellAndReload,
    ShowToast(&'static str),
    SaveElapsed(Duration),
    PersistKv {
        key: String,
        value: String,
    },
    SaveConfig,
    AddManualSession {
        date: String,
        hours: f32,
        notes: Option<String>,
    },
    InvalidManualHours,
    DeleteSession(i64),
}

#[derive(Debug)]
pub(crate) struct TrackerState {
    pub(crate) visible: bool,
    pub(crate) running: bool,
    pub(crate) started_at: Option<Instant>,
    pub(crate) sessions: Vec<md_editor_core::domain::StudySession>,
    pub(crate) kv: HashMap<String, String>,
    pub(crate) tab: TrackerTab,
    pub(crate) config_json: String,
    pub(crate) config_content: text_editor::Content,
    pub(crate) manual_date: String,
    pub(crate) manual_hours: String,
    pub(crate) manual_notes: String,
}

impl TrackerState {
    pub(crate) fn new(
        sessions: Vec<md_editor_core::domain::StudySession>,
        kv: HashMap<String, String>,
        config_json: String,
        manual_date: String,
    ) -> Self {
        let config_content = text_editor::Content::with_text(&config_json);

        Self {
            visible: false,
            running: false,
            started_at: None,
            sessions,
            kv,
            tab: TrackerTab::Dashboard,
            config_json,
            config_content,
            manual_date,
            manual_hours: String::new(),
            manual_notes: String::new(),
        }
    }

    pub(crate) fn toggle_visibility(&mut self) {
        self.visible = !self.visible;
    }

    pub(crate) fn start(&mut self, started_at: Instant) {
        self.running = true;
        self.started_at = Some(started_at);
    }

    pub(crate) fn stop(&mut self) -> Option<Instant> {
        self.running = false;
        self.started_at.take()
    }

    pub(crate) fn replace_config(&mut self, config_json: String) {
        self.config_content = text_editor::Content::with_text(&config_json);
        self.config_json = config_json;
    }

    pub(crate) fn edit_config(&mut self, action: text_editor::Action) {
        self.config_content.perform(action);
        self.config_json = self.config_content.text();
    }

    pub(crate) fn update(&mut self, message: TrackerMessage, now: Instant) -> TrackerEffect {
        match message {
            TrackerMessage::Toggle => {
                self.toggle_visibility();
                TrackerEffect::PersistShellAndReload
            }
            TrackerMessage::Start => {
                self.start(now);
                TrackerEffect::ShowToast("Study timer started")
            }
            TrackerMessage::Stop => self
                .stop()
                .map(|started_at| {
                    TrackerEffect::SaveElapsed(now.saturating_duration_since(started_at))
                })
                .unwrap_or(TrackerEffect::None),
            TrackerMessage::TabSelected(tab) => {
                self.tab = tab;
                TrackerEffect::None
            }
            TrackerMessage::ProjectStatusChanged(id, status) => TrackerEffect::PersistKv {
                key: format!("proj_{id}"),
                value: status,
            },
            TrackerMessage::GateToggled(gate_id, item_idx) => {
                self.toggle_kv_effect(format!("gate_{gate_id}_{item_idx}"))
            }
            TrackerMessage::ReadingToggled(section, item_idx) => {
                self.toggle_kv_effect(format!("read_{section}_{item_idx}"))
            }
            TrackerMessage::ConfigEdited(action) => {
                self.edit_config(action);
                TrackerEffect::None
            }
            TrackerMessage::ConfigSave => TrackerEffect::SaveConfig,
            TrackerMessage::ManualDateChanged(value) => {
                self.manual_date = value;
                TrackerEffect::None
            }
            TrackerMessage::ManualHoursChanged(value) => {
                self.manual_hours = value;
                TrackerEffect::None
            }
            TrackerMessage::ManualNotesChanged(value) => {
                self.manual_notes = value;
                TrackerEffect::None
            }
            TrackerMessage::ManualAdd => {
                let Ok(hours) = self.manual_hours.trim().parse::<f32>() else {
                    return TrackerEffect::InvalidManualHours;
                };
                if hours <= 0.0 {
                    return TrackerEffect::InvalidManualHours;
                }
                TrackerEffect::AddManualSession {
                    date: self.manual_date.trim().to_string(),
                    hours,
                    notes: (!self.manual_notes.trim().is_empty())
                        .then(|| self.manual_notes.trim().to_string()),
                }
            }
            TrackerMessage::SessionDelete(id) => TrackerEffect::DeleteSession(id),
        }
    }

    fn toggle_kv_effect(&self, key: String) -> TrackerEffect {
        let value = if self.kv.get(&key).is_some_and(|value| value == "true") {
            "false"
        } else {
            "true"
        };
        TrackerEffect::PersistKv {
            key,
            value: value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> TrackerState {
        TrackerState::new(
            Vec::new(),
            HashMap::new(),
            r#"{"projects":[]}"#.to_string(),
            "2026-06-07".to_string(),
        )
    }

    #[test]
    fn new_sets_stable_defaults_and_preserves_loaded_data() {
        let session = md_editor_core::domain::StudySession {
            id: 7,
            date: "2026-06-06".to_string(),
            hours: 1.5,
            activity_type: "Study".to_string(),
            phase: "Focus".to_string(),
            notes: Some("Chapter 1".to_string()),
        };
        let mut kv = HashMap::new();
        kv.insert("gate_1_0".to_string(), "true".to_string());

        let state = TrackerState::new(
            vec![session],
            kv,
            r#"{"projects":[]}"#.to_string(),
            "2026-06-07".to_string(),
        );

        assert!(!state.visible);
        assert!(!state.running);
        assert!(state.started_at.is_none());
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.kv.get("gate_1_0").map(String::as_str), Some("true"));
        assert_eq!(state.tab, TrackerTab::Dashboard);
        assert_eq!(state.config_content.text(), state.config_json);
        assert_eq!(state.manual_date, "2026-06-07");
        assert!(state.manual_hours.is_empty());
        assert!(state.manual_notes.is_empty());
    }

    #[test]
    fn timer_start_and_stop_keep_running_state_consistent() {
        let mut state = state();
        let started_at = Instant::now();

        state.start(started_at);

        assert!(state.running);
        assert_eq!(state.started_at, Some(started_at));
        assert_eq!(state.stop(), Some(started_at));
        assert!(!state.running);
        assert!(state.started_at.is_none());
    }

    #[test]
    fn replacing_config_updates_json_and_editor_content_together() {
        let mut state = state();
        let replacement = r#"{"projects":[{"id":"one"}]}"#.to_string();

        state.replace_config(replacement.clone());

        assert_eq!(state.config_json, replacement);
        assert_eq!(state.config_content.text(), replacement);
    }

    #[test]
    fn visibility_toggle_does_not_reset_tracker_data() {
        let mut state = state();
        state.manual_hours = "2".to_string();

        state.toggle_visibility();
        state.toggle_visibility();

        assert!(!state.visible);
        assert_eq!(state.manual_hours, "2");
    }

    #[test]
    fn reducer_emits_persistence_effect_for_kv_toggle() {
        let mut state = state();
        state
            .kv
            .insert("gate_review_2".to_string(), "true".to_string());

        let effect = state.update(
            TrackerMessage::GateToggled("review".to_string(), 2),
            Instant::now(),
        );

        assert_eq!(
            effect,
            TrackerEffect::PersistKv {
                key: "gate_review_2".to_string(),
                value: "false".to_string(),
            }
        );
        assert_eq!(
            state.kv.get("gate_review_2").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn reducer_validates_manual_hours_before_emitting_effect() {
        let mut state = state();
        state.manual_hours = "0".to_string();
        assert_eq!(
            state.update(TrackerMessage::ManualAdd, Instant::now()),
            TrackerEffect::InvalidManualHours
        );

        state.manual_hours = "1.25".to_string();
        state.manual_notes = "Review".to_string();
        assert_eq!(
            state.update(TrackerMessage::ManualAdd, Instant::now()),
            TrackerEffect::AddManualSession {
                date: "2026-06-07".to_string(),
                hours: 1.25,
                notes: Some("Review".to_string()),
            }
        );
    }
}
