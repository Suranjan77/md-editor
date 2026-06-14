use super::*;

impl Shell {
    pub(super) fn handle_settings_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SettingsThemeChanged(theme) => {
                if let Some(Overlay::Settings { theme: t, .. }) = &mut self.overlay {
                    *t = theme;
                }
                Task::none()
            }
            Message::SettingsReduceMotionChanged(reduce_motion) => {
                if let Some(Overlay::Settings {
                    reduce_motion: setting,
                    ..
                }) = &mut self.overlay
                {
                    *setting = reduce_motion;
                }
                Task::none()
            }
            Message::SettingsScopeChanged(idx, val) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && let Some(row) = keymap.bindings.get_mut(idx)
                {
                    row.scope = val;
                }
                Task::none()
            }
            Message::SettingsChordChanged(idx, val) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && let Some(row) = keymap.bindings.get_mut(idx)
                {
                    row.chord = val;
                }
                Task::none()
            }
            Message::SettingsCommandChanged(idx, val) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && let Some(row) = keymap.bindings.get_mut(idx)
                {
                    row.command = if val.trim().is_empty() {
                        None
                    } else {
                        Some(val)
                    };
                }
                Task::none()
            }
            Message::SettingsAddRow => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay {
                    keymap.bindings.push(crate::settings::BindingRow {
                        scope: "workspace".to_string(),
                        chord: String::new(),
                        command: None,
                    });
                }
                Task::none()
            }
            Message::SettingsRemoveRow(idx) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && idx < keymap.bindings.len()
                {
                    keymap.bindings.remove(idx);
                }
                Task::none()
            }
            Message::SettingsSave => {
                if let Some(Overlay::Settings {
                    theme,
                    reduce_motion,
                    keymap,
                    error: _,
                }) = self.overlay.clone()
                {
                    match crate::settings::validate_overrides(&self.registry, &keymap) {
                        Ok(()) => {
                            if let Err(e) =
                                crate::settings::save_keymap_overrides(&self.vault_root, &keymap)
                            {
                                if let Some(Overlay::Settings {
                                    error: err_field, ..
                                }) = &mut self.overlay
                                {
                                    *err_field = Some(e);
                                }
                            } else {
                                self.theme_name = theme;
                                self.reduce_motion = reduce_motion;
                                if self.reduce_motion {
                                    for session in self.sessions.md.values_mut() {
                                        session.finish_motion();
                                    }
                                }
                                self.reload_keymap();
                                self.close_overlay();
                                self.save_session();
                                return self.success("Settings saved successfully");
                            }
                        }
                        Err(e) => {
                            if let Some(Overlay::Settings {
                                error: err_field, ..
                            }) = &mut self.overlay
                            {
                                *err_field = Some(e);
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::SettingsCancel => {
                self.close_overlay();
                Task::none()
            }

            _ => Task::none(),
        }
    }
}
