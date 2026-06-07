#![allow(
    clippy::bool_assert_comparison,
    clippy::collapsible_if,
    clippy::derivable_impls,
    clippy::extra_unused_type_parameters,
    clippy::if_same_then_else,
    clippy::items_after_test_module,
    clippy::let_and_return,
    clippy::manual_strip,
    clippy::manual_map,
    clippy::needless_range_loop,
    clippy::needless_lifetimes,
    clippy::nonminimal_bool,
    clippy::redundant_closure,
    clippy::redundant_guards,
    clippy::single_match,
    clippy::too_many_arguments,
    clippy::unnecessary_cast,
    clippy::useless_conversion,
    clippy::unnecessary_map_or,
    clippy::unnecessary_min_or_max,
    clippy::double_ended_iterator_last
)]

mod app;
mod app_shell;
mod command_registry;
mod editor;
mod features;
mod integrity;
mod messages;
#[cfg(target_os = "linux")]
mod platform;
mod search;
mod theme;
mod views;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliAction {
    Install,
    Uninstall,
    RunApp,
}

#[allow(dead_code)]
fn parse_cli_args(args: &[String]) -> CliAction {
    if args.len() > 1 {
        let cmd = args[1].as_str();
        if cmd == "--install" || cmd == "--install-desktop" {
            return CliAction::Install;
        } else if cmd == "--uninstall" || cmd == "--uninstall-desktop" {
            return CliAction::Uninstall;
        }
    }
    CliAction::RunApp
}

fn main() -> iced::Result {
    #[cfg(target_os = "linux")]
    {
        let args: Vec<String> = std::env::args().collect();
        match parse_cli_args(&args) {
            CliAction::Install => {
                if platform::desktop_integration::install_linux_desktop_entry() {
                    println!("MD Editor desktop entry and icons installed successfully.");
                } else {
                    eprintln!("Failed to install MD Editor desktop entry and icons.");
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
            CliAction::Uninstall => {
                if platform::desktop_integration::uninstall_linux_desktop_entry() {
                    println!("MD Editor desktop entry and icons uninstalled successfully.");
                } else {
                    eprintln!("Failed to uninstall MD Editor desktop entry and icons.");
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
            CliAction::RunApp => {}
        }
    }

    let icon = iced::window::icon::from_file_data(
        include_bytes!("../../md-editor.png"),
        Some(image::ImageFormat::Png),
    )
    .ok();

    #[cfg(target_os = "linux")]
    let platform_specific = iced::window::settings::PlatformSpecific {
        application_id: String::from("md-editor"),
        ..Default::default()
    };

    #[cfg(not(target_os = "linux"))]
    let platform_specific = iced::window::settings::PlatformSpecific::default();

    iced::application(
        app::MdEditor::new,
        app::MdEditor::update,
        app::MdEditor::view,
    )
    .title(app::MdEditor::title)
    .theme(|state: &app::MdEditor| state.theme())
    .subscription(app::MdEditor::subscription)
    .window(iced::window::Settings {
        size: iced::Size::new(1200.0, 800.0),
        icon,
        platform_specific,
        ..Default::default()
    })
    .run()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_load_icon() {
        let res = iced::window::icon::from_file_data(
            include_bytes!("../../md-editor.png"),
            Some(image::ImageFormat::Png),
        );
        assert!(res.is_ok(), "Failed to load icon: {:?}", res.err());
    }

    #[test]
    fn test_parse_cli_args() {
        use super::{CliAction, parse_cli_args};

        // Empty args list (just binary path) -> RunApp
        assert_eq!(
            parse_cli_args(&["md-editor".to_string()]),
            CliAction::RunApp
        );

        // Standard flags for installation
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--install".to_string()]),
            CliAction::Install
        );
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--install-desktop".to_string()]),
            CliAction::Install
        );

        // Standard flags for uninstallation
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--uninstall".to_string()]),
            CliAction::Uninstall
        );
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--uninstall-desktop".to_string()]),
            CliAction::Uninstall
        );

        // Arbitrary args (like opening a file or directory path) -> RunApp
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "notes.md".to_string()]),
            CliAction::RunApp
        );
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "/home/user/vault".to_string()]),
            CliAction::RunApp
        );
    }
}
