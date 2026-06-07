use iced::Subscription;

use crate::messages::{Message, Shortcut};

use super::model::*;

impl MdEditor {
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let keyboard = iced::keyboard::listen().map(|event| {
            match event {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    // Escape key — close overlays
                    if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                        return Message::KeyboardShortcut(Shortcut::Escape);
                    }
                    if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) {
                        return Message::KeyboardShortcut(Shortcut::Submit);
                    }
                    if modifiers.alt() && (modifiers.command() || modifiers.control()) {
                        match key {
                            iced::keyboard::Key::Character(c) if c == "b" => {
                                return Message::KeyboardShortcut(Shortcut::ToggleBacklinks);
                            }
                            iced::keyboard::Key::Character(c) if c == "s" => {
                                return Message::KeyboardShortcut(Shortcut::StudyTracker);
                            }
                            _ => {}
                        }
                    }
                    if modifiers.alt() {
                        match key {
                            iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => {
                                return Message::PdfNavBack;
                            }
                            iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => {
                                return Message::PdfNavForward;
                            }
                            iced::keyboard::Key::Character(c) if c == "p" => {
                                return Message::KeyboardShortcut(Shortcut::SwitchPane);
                            }
                            iced::keyboard::Key::Character(c) if c == "g" => {
                                return Message::KeyboardShortcut(Shortcut::FollowCitation);
                            }
                            iced::keyboard::Key::Character(c) if c == "u" => {
                                return Message::KeyboardShortcut(Shortcut::ShowUsages);
                            }
                            iced::keyboard::Key::Character(c) if c == "c" => {
                                return Message::KeyboardShortcut(Shortcut::CitationPalette);
                            }
                            iced::keyboard::Key::Character(c) if c == "e" => {
                                return Message::KeyboardShortcut(Shortcut::ExcerptModeToggle);
                            }
                            iced::keyboard::Key::Character(c) if c == "i" => {
                                return Message::KeyboardShortcut(Shortcut::ExcerptInsertBatch);
                            }
                            _ => {}
                        }
                    }
                    if modifiers.command() || modifiers.control() {
                        match key {
                            iced::keyboard::Key::Character(c) if c == "s" => {
                                return Message::KeyboardShortcut(Shortcut::Save);
                            }
                            iced::keyboard::Key::Character(c) if c == "o" => {
                                return Message::KeyboardShortcut(Shortcut::OpenVault);
                            }
                            iced::keyboard::Key::Character(c) if c == "n" => {
                                return Message::KeyboardShortcut(Shortcut::NewFile);
                            }
                            iced::keyboard::Key::Character(c) if c == "f" => {
                                return Message::KeyboardShortcut(Shortcut::Search);
                            }
                            iced::keyboard::Key::Character(c) if c == "c" => {
                                return Message::PdfCopySelection;
                            }
                            iced::keyboard::Key::Character(c) if c == "p" => {
                                return Message::KeyboardShortcut(Shortcut::CommandPalette);
                            }
                            iced::keyboard::Key::Character(c) if c == "b" => {
                                return Message::KeyboardShortcut(Shortcut::ToggleSidebar);
                            }
                            iced::keyboard::Key::Character(c) if c == "t" => {
                                return Message::KeyboardShortcut(Shortcut::TableOfContents);
                            }
                            iced::keyboard::Key::Character(c) if c == "=" || c == "+" => {
                                return Message::KeyboardShortcut(Shortcut::ZoomIn);
                            }
                            iced::keyboard::Key::Character(c) if c == "-" => {
                                return Message::KeyboardShortcut(Shortcut::ZoomOut);
                            }
                            iced::keyboard::Key::Character(c) if c == "0" => {
                                return Message::KeyboardShortcut(Shortcut::ZoomFit);
                            }
                            iced::keyboard::Key::Character(c) if c == "g" => {
                                return Message::KeyboardShortcut(Shortcut::GoToPage);
                            }
                            iced::keyboard::Key::Character(c) if c == "r" => {
                                return Message::KeyboardShortcut(Shortcut::PdfSearch);
                            }
                            iced::keyboard::Key::Character(c) if c == "h" => {
                                return Message::KeyboardShortcut(Shortcut::PdfHighlight);
                            }
                            iced::keyboard::Key::Character(c) if c == "z" => {
                                return Message::KeyboardShortcut(Shortcut::PdfZoomInput);
                            }
                            _ => {}
                        }
                    }
                    match key {
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Home) => {
                            return Message::KeyboardShortcut(Shortcut::PdfFirstPage);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::End) => {
                            return Message::KeyboardShortcut(Shortcut::PdfLastPage);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) => {
                            return Message::PdfScrollBy(64.0);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) => {
                            return Message::PdfScrollBy(-64.0);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::PageDown) => {
                            return Message::PdfScrollBy(520.0);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::PageUp) => {
                            return Message::PdfScrollBy(-520.0);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            Message::Tick
        });

        let pdf_ctrl_scroll = iced::event::listen_with(|event, _status, _window_id| match event {
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::KeyboardModifiersChanged(modifiers))
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                let zoom_delta = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y * 0.1,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => y * 0.001,
                };
                Some(Message::PdfWheelScrolledForZoom(zoom_delta))
            }
            _ => None,
        });

        let toast = if self.overlays.toast.is_some() {
            iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastHide)
        } else {
            Subscription::none()
        };

        let highlight_debounce = if self.pending_highlight_generation.is_some() {
            iced::time::every(HIGHLIGHT_DEBOUNCE).map(|_| Message::HighlightDebounceElapsed)
        } else {
            Subscription::none()
        };

        let editor_autosave_timer = if self.pending_editor_save.is_some() {
            iced::time::every(std::time::Duration::from_millis(500))
                .map(|_| Message::EditorAutosaveElapsed)
        } else {
            Subscription::none()
        };

        let mouse_drag = if self.is_resizing_split {
            iced::event::listen_with(|event, _status, _window_id| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::SplitViewDragging(position.x))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::SplitViewDragEnd),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let window_events = iced::event::listen_with(|event, _status, _window_id| {
            if let iced::Event::Window(iced::window::Event::Resized(size)) = event {
                Some(Message::WindowResized(
                    size.width as f32,
                    size.height as f32,
                ))
            } else {
                None
            }
        });

        Subscription::batch(vec![
            keyboard,
            pdf_ctrl_scroll,
            toast,
            highlight_debounce,
            mouse_drag,
            editor_autosave_timer,
            window_events,
        ])
    }
}
