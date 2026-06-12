use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Background, Border, Element, Fill, Task};
use md3_kernel::{CommandId, CommandRegistry};

use super::tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeRow {
    pub label: String,
    pub command: CommandId,
    pub chord: Option<String>,
}

pub fn welcome_rows(registry: &CommandRegistry) -> Vec<WelcomeRow> {
    [
        ("Open File...", CommandId("file.quick-open")),
        ("Browse Vault", CommandId("workspace.toggle-files")),
        ("Command Palette", CommandId("palette.open")),
        ("Keyboard Shortcuts", CommandId("help.shortcuts")),
    ]
    .into_iter()
    .filter_map(|(label, command)| {
        let spec = registry.get(command)?;
        Some(WelcomeRow {
            label: label.to_string(),
            command,
            chord: spec
                .bindings
                .first()
                .map(|binding| binding.chord.to_string()),
        })
    })
    .collect()
}

#[derive(Debug, Clone)]
pub enum StartupMessage {
    OpenVault,
    CreateVault,
    OpenRecent(PathBuf),
    VaultPicked(Option<PathBuf>),
}

struct StartupWelcome {
    recent: Vec<PathBuf>,
    message: String,
}

pub fn run_startup(message: Option<String>) -> iced::Result {
    iced::application(
        move || StartupWelcome {
            recent: crate::vault_picker::recent_vaults(),
            message: message.clone().unwrap_or_default(),
        },
        StartupWelcome::update,
        StartupWelcome::view,
    )
    .title("md3")
    .theme(StartupWelcome::theme)
    .window(iced::window::Settings {
        size: iced::Size::new(760.0, 560.0),
        icon: iced::window::icon::from_file_data(include_bytes!("../../../../md-editor.png"), None)
            .ok(),
        ..Default::default()
    })
    .run()
}

impl StartupWelcome {
    fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    fn update(&mut self, message: StartupMessage) -> Task<StartupMessage> {
        match message {
            StartupMessage::OpenVault => Task::perform(
                crate::vault_picker::pick_vault_async_with_title("Open Vault Folder"),
                StartupMessage::VaultPicked,
            ),
            StartupMessage::CreateVault => Task::perform(
                crate::vault_picker::pick_vault_async_with_title("Create or Select Vault Folder"),
                StartupMessage::VaultPicked,
            ),
            StartupMessage::OpenRecent(path) => self.launch(path),
            StartupMessage::VaultPicked(Some(path)) => self.launch(path),
            StartupMessage::VaultPicked(None) => Task::none(),
        }
    }

    fn launch(&mut self, path: PathBuf) -> Task<StartupMessage> {
        match crate::vault_picker::launch_vault(&path) {
            Ok(()) => iced::exit(),
            Err(error) => {
                self.message = format!("Open {}: {error}", path.display());
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, StartupMessage> {
        let primary = button(text("Open Vault Folder").size(16))
            .padding([11, 22])
            .on_press(StartupMessage::OpenVault);
        let create = button(text("Create Vault").size(16))
            .padding([11, 22])
            .style(button::secondary)
            .on_press(StartupMessage::CreateVault);

        let mut recent = column![].spacing(4);
        if !self.recent.is_empty() {
            recent = recent.push(
                text("Recent Vaults")
                    .size(13)
                    .color(tokens::dark().text_muted),
            );
        }
        for path in &self.recent {
            recent = recent.push(
                button(text(path.display().to_string()).size(13))
                    .width(Fill)
                    .padding([7, 10])
                    .style(button::text)
                    .on_press(StartupMessage::OpenRecent(path.clone())),
            );
        }

        let message = (!self.message.is_empty())
            .then(|| {
                text(self.message.clone())
                    .size(13)
                    .color(tokens::dark().danger)
            })
            .map(Element::from)
            .unwrap_or_else(|| iced::widget::Space::new().height(0).into());
        let card = container(
            column![
                text("MD Editor").size(34),
                text("Choose a folder to use as a vault.")
                    .size(15)
                    .color(tokens::dark().text_muted),
                row![primary, create].spacing(12),
                message,
                scrollable(recent).height(220),
            ]
            .spacing(18)
            .width(520),
        )
        .padding(28)
        .style(|_| container::Style {
            background: Some(Background::Color(tokens::dark().bg_secondary)),
            border: Border {
                color: tokens::dark().border,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..container::Style::default()
        });
        container(card).center(Fill).into()
    }
}
