//! Settings view handlers and persistence for theme, scale, motion, editor typography and attachments.

use super::*;
use crate::design::{
    ActiveTheme, ContentMeasure, ReadingFont, ThemeMode, motion::ReducedMotionPref,
};
use crate::messages::{AttachmentsMode, SettingsMessage};

const KEY_THEME_MODE: &str = "ui_theme_mode";
const KEY_INTERFACE_SCALE: &str = "ui_interface_scale";
const KEY_REDUCED_MOTION: &str = "ui_reduced_motion";
const KEY_READING_FONT: &str = "editor_reading_font";
const KEY_EDITOR_FONT_SIZE: &str = "editor_font_size";
const KEY_CONTENT_MEASURE: &str = "editor_content_measure";
const KEY_LINE_NUMBERS: &str = "editor_line_numbers";
const KEY_EXCLUDED_FOLDERS: &str = "vault_excluded_folders";
const KEY_RECENT_VAULTS: &str = "recent_vaults";

impl MdEditor {
    /// Route `SettingsMessage` updates.
    pub(super) fn update_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        match message {
            SettingsMessage::Toggle => {
                self.ui.settings_visible = !self.ui.settings_visible;
                self.ui.context_menu = None;
                Task::none()
            }
            SettingsMessage::Close => {
                self.ui.settings_visible = false;
                Task::none()
            }
            SettingsMessage::SelectSection(section) => {
                self.ui.settings_section = section;
                Task::none()
            }
            SettingsMessage::ThemeModeSelected(mode) => {
                crate::design::set_theme_mode(mode);
                let active = match mode {
                    ThemeMode::Dark => ActiveTheme::Dark,
                    ThemeMode::Light => ActiveTheme::Light,
                    ThemeMode::System => self.system_theme_fallback(),
                };
                self.apply_active_theme(active)
            }
            SettingsMessage::SystemThemeChanged(os_mode) => {
                let active = match os_mode {
                    iced::theme::Mode::Light => ActiveTheme::Light,
                    iced::theme::Mode::Dark | iced::theme::Mode::None => ActiveTheme::Dark,
                };
                self.last_os_theme = active;
                if crate::design::theme_mode() == ThemeMode::System {
                    return self.apply_active_theme(active);
                }
                Task::none()
            }
            SettingsMessage::InterfaceScaleSelected(pct) => {
                crate::design::set_interface_scale_percent(pct);
                self.persist_design_settings();
                self.refit_pdf_if_needed()
            }
            SettingsMessage::ReducedMotionSelected(pref) => {
                crate::design::motion::set_reduced_motion_pref(pref);
                crate::design::motion::apply_reduced_motion(
                    crate::platform::os_prefers_reduced_motion(),
                );
                self.persist_design_settings();
                Task::none()
            }
            SettingsMessage::ReadingFontSelected(font) => {
                crate::design::set_reading_font(font);
                self.persist_design_settings();
                Task::none()
            }
            SettingsMessage::EditorFontSizeChanged(px) => {
                crate::design::set_editor_font_size(px);
                self.persist_design_settings();
                Task::none()
            }
            SettingsMessage::ContentMeasureSelected(measure) => {
                crate::design::set_content_measure(measure);
                self.persist_design_settings();
                Task::none()
            }
            SettingsMessage::LineNumbersToggled(on) => {
                crate::design::set_line_numbers(on);
                self.persist_design_settings();
                Task::none()
            }
            SettingsMessage::AttachmentsModeSelected(mode) => {
                self.ui.attachments_mode = mode;
                self.persist_attachments_setting();
                Task::none()
            }
            SettingsMessage::AttachmentsCustomChanged(folder) => {
                self.ui.attachments_custom = folder;
                if self.ui.attachments_mode == AttachmentsMode::Custom {
                    self.persist_attachments_setting();
                }
                Task::none()
            }
            SettingsMessage::PickAttachmentsFolder => {
                let root = self.vault.root.clone();
                Task::perform(
                    async move {
                        let mut dialog = rfd::AsyncFileDialog::new()
                            .set_title("Select Vault Attachments Folder");
                        if let Some(ref r) = root {
                            dialog = dialog.set_directory(r);
                        }
                        let picked = dialog.pick_folder().await?;
                        let path = picked.path();
                        if let Some(r) = root
                            && let Ok(rel) = path.strip_prefix(&r)
                        {
                            return Some(rel.to_string_lossy().replace('\\', "/"));
                        }
                        Some(path.file_name()?.to_string_lossy().to_string())
                    },
                    |picked| Message::Settings(SettingsMessage::AttachmentsFolderPicked(picked)),
                )
            }
            SettingsMessage::AttachmentsFolderPicked(Some(folder)) => {
                let clean = folder.trim().trim_matches('/').to_string();
                if !clean.is_empty() {
                    self.ui.attachments_mode = AttachmentsMode::Custom;
                    self.ui.attachments_custom = clean;
                    self.persist_attachments_setting();
                }
                Task::none()
            }
            SettingsMessage::AttachmentsFolderPicked(None) => Task::none(),
            SettingsMessage::ExcludedFoldersChanged(list) => {
                self.ui.excluded_folders = list;
                self.persist_design_settings();
                Task::none()
            }
            SettingsMessage::ExcludedFoldersApply => {
                let names = md_editor_core::vault::parse_excluded_dirs(&self.ui.excluded_folders);
                if md_editor_core::vault::set_user_excluded_dirs(names) {
                    self.ui.notify(
                        crate::ui::ToastKind::Success,
                        "Excluded folders updated".to_string(),
                        false,
                    );
                    return self.reindex_vault();
                }
                Task::none()
            }
        }
    }

    fn system_theme_fallback(&self) -> ActiveTheme {
        self.last_os_theme
    }

    /// Apply `active` theme, flushing cached math bitmaps so equations re-render in the new ink colour.
    pub(super) fn apply_active_theme(&mut self, active: ActiveTheme) -> Task<Message> {
        let changed = crate::design::active_theme() != active;
        crate::design::set_active_theme(active);
        self.persist_design_settings();
        if changed {
            // Span colours come from the theme: re-highlight the open note.
            self.editor.refresh_highlighting();
            self.editor.forget_math();
            return self.editor.load_math(self.ui.scale_factor);
        }
        Task::none()
    }

    fn persist_attachments_setting(&self) {
        let raw = match self.ui.attachments_mode {
            AttachmentsMode::VaultRoot => "root".to_string(),
            AttachmentsMode::BesideNote => "beside-note".to_string(),
            AttachmentsMode::Custom => {
                let trimmed = self.ui.attachments_custom.trim().trim_matches('/');
                if trimmed.is_empty() {
                    "attachments".to_string()
                } else {
                    trimmed.to_string()
                }
            }
        };
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            crate::app::paste::ATTACHMENTS_FOLDER_KEY,
            &raw,
        );
    }

    /// Restore appearance, typography, attachments and recent-vault settings on boot.
    pub(super) fn restore_design_settings(&mut self) {
        let get = |key| {
            md_editor_core::config::get_sys_config(&self.state, key)
                .ok()
                .flatten()
        };

        // Fresh settings start Dark, so visuals are deterministic before any OS theme event.
        let mode = get(KEY_THEME_MODE).map_or(ThemeMode::Dark, |raw| ThemeMode::parse(&raw));
        crate::design::set_theme_mode(mode);
        let active = match mode {
            ThemeMode::Light => ActiveTheme::Light,
            ThemeMode::Dark | ThemeMode::System => ActiveTheme::Dark,
        };
        crate::design::set_active_theme(active);

        let scale = get(KEY_INTERFACE_SCALE)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(100);
        crate::design::set_interface_scale_percent(scale);

        let rm_pref = ReducedMotionPref::parse(get(KEY_REDUCED_MOTION).as_deref().unwrap_or(""));
        crate::design::motion::set_reduced_motion_pref(rm_pref);
        crate::design::motion::apply_reduced_motion(crate::platform::os_prefers_reduced_motion());

        let font = ReadingFont::parse(get(KEY_READING_FONT).as_deref().unwrap_or(""));
        crate::design::set_reading_font(font);

        if let Some(font_px) = get(KEY_EDITOR_FONT_SIZE).and_then(|s| s.parse::<f32>().ok()) {
            crate::design::set_editor_font_size(font_px);
        }

        let measure = ContentMeasure::parse(get(KEY_CONTENT_MEASURE).as_deref().unwrap_or(""));
        crate::design::set_content_measure(measure);
        crate::design::set_line_numbers(get(KEY_LINE_NUMBERS).as_deref() == Some("on"));

        if let Some(att) = get(crate::app::paste::ATTACHMENTS_FOLDER_KEY) {
            match att.trim() {
                "" | "root" => self.ui.attachments_mode = AttachmentsMode::VaultRoot,
                "beside-note" => self.ui.attachments_mode = AttachmentsMode::BesideNote,
                custom => {
                    self.ui.attachments_mode = AttachmentsMode::Custom;
                    self.ui.attachments_custom = custom.to_string();
                }
            }
        }

        if let Some(excl) = get(KEY_EXCLUDED_FOLDERS) {
            md_editor_core::vault::set_user_excluded_dirs(
                md_editor_core::vault::parse_excluded_dirs(&excl),
            );
            self.ui.excluded_folders = excl;
        }

        if let Some(raw_recent) = get(KEY_RECENT_VAULTS)
            && let Ok(list) = serde_json::from_str::<Vec<String>>(&raw_recent)
        {
            self.ui.recent_vaults = list;
        }
    }

    pub(super) fn persist_design_settings(&self) {
        let pairs = [
            (
                KEY_THEME_MODE,
                crate::design::theme_mode().as_str().to_string(),
            ),
            (
                KEY_INTERFACE_SCALE,
                crate::design::interface_scale_percent().to_string(),
            ),
            (
                KEY_REDUCED_MOTION,
                crate::design::motion::reduced_motion_pref()
                    .as_str()
                    .to_string(),
            ),
            (
                KEY_READING_FONT,
                crate::design::reading_font().as_str().to_string(),
            ),
            (
                KEY_EDITOR_FONT_SIZE,
                format!("{:.0}", crate::design::editor_font_size()),
            ),
            (
                KEY_CONTENT_MEASURE,
                crate::design::content_measure().as_str().to_string(),
            ),
            (
                KEY_LINE_NUMBERS,
                if crate::design::line_numbers() {
                    "on"
                } else {
                    "off"
                }
                .to_string(),
            ),
            (KEY_EXCLUDED_FOLDERS, self.ui.excluded_folders.clone()),
        ];
        for (k, v) in pairs {
            let _ = md_editor_core::config::set_sys_config(&self.state, k, &v);
        }
    }

    /// Record `path` at the front of `recent_vaults` (capped at 6).
    pub(super) fn record_recent_vault(&mut self, path: &str) {
        self.ui.recent_vaults.retain(|p| p != path);
        self.ui.recent_vaults.insert(0, path.to_string());
        self.ui.recent_vaults.truncate(6);
        if let Ok(json) = serde_json::to_string(&self.ui.recent_vaults) {
            let _ = md_editor_core::config::set_sys_config(&self.state, KEY_RECENT_VAULTS, &json);
        }
    }
}
