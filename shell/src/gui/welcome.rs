use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Background, Border, Element, Fill, Task};
use md_kernel::{CommandId, CommandRegistry, Keymap};

use super::{Message, Shell, tokens};

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
    Shell(Message),
    ExitRequested,
}

struct StartupWelcome {
    recent: Vec<PathBuf>,
    message: String,
    registry: CommandRegistry,
    keymap: Keymap,
    shell: Option<Shell>,
}

pub fn run_startup(
    registry: CommandRegistry,
    keymap: Keymap,
    message: Option<String>,
) -> iced::Result {
    iced::application(
        move || StartupWelcome {
            recent: crate::vault_picker::recent_vaults(),
            message: message.clone().unwrap_or_default(),
            registry: registry.clone(),
            keymap: keymap.clone(),
            shell: None,
        },
        StartupWelcome::update,
        StartupWelcome::view,
    )
    .title("MD Editor")
    .theme(StartupWelcome::theme)
    .font(super::fonts::HANKEN_GROTESK_BYTES)
    .font(super::fonts::GEIST_MONO_BYTES)
    .default_font(super::fonts::SANS)
    .subscription(StartupWelcome::subscription)
    .window(iced::window::Settings {
        size: iced::Size::new(1200.0, 800.0),
        exit_on_close_request: false,
        icon: iced::window::icon::from_file_data(include_bytes!("../../../md-editor.png"), None)
            .ok(),
        ..Default::default()
    })
    .run()
}

impl StartupWelcome {
    fn theme(&self) -> iced::Theme {
        self.shell.as_ref().map_or(iced::Theme::Dark, Shell::theme)
    }

    fn subscription(&self) -> iced::Subscription<StartupMessage> {
        match &self.shell {
            Some(shell) => shell.subscription().map(StartupMessage::Shell),
            None => iced::window::close_requests().map(|_| StartupMessage::ExitRequested),
        }
    }

    fn update(&mut self, message: StartupMessage) -> Task<StartupMessage> {
        if let Some(shell) = &mut self.shell {
            return match message {
                StartupMessage::Shell(message) => shell.update(message).map(StartupMessage::Shell),
                StartupMessage::ExitRequested => iced::exit(),
                _ => Task::none(),
            };
        }
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
            StartupMessage::ExitRequested => iced::exit(),
            StartupMessage::Shell(_) => Task::none(),
        }
    }

    fn launch(&mut self, path: PathBuf) -> Task<StartupMessage> {
        let Ok(root) = path.canonicalize() else {
            self.message = format!("Vault folder is unavailable: {}", path.display());
            return Task::none();
        };
        if !root.is_dir() {
            self.message = format!("Vault folder is unavailable: {}", root.display());
            return Task::none();
        }
        crate::vault_picker::record_recent(&root);
        let mut keymap = self.keymap.clone();
        let report = crate::settings::apply_keymap_overrides(&root, &self.registry, &mut keymap);
        self.shell = Some(Shell::new(self.registry.clone(), keymap, root));
        self.message = report.warnings.join("\n");
        Task::none()
    }

    fn view(&self) -> Element<'_, StartupMessage> {
        if let Some(shell) = &self.shell {
            return shell.view().map(StartupMessage::Shell);
        }
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
                text("MD Editor")
                    .size(34)
                    .font(super::fonts::SANS_BOLD)
                    .color(tokens::dark().text_heading),
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
            background: Some(Background::Color(tokens::dark().surface_palette)),
            border: Border {
                color: tokens::dark().border_overlay,
                width: 1.0,
                radius: 14.0.into(),
            },
            ..container::Style::default()
        });
        container(card).center(Fill).into()
    }
}
