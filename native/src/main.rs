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
mod editor;
mod messages;
mod pdf_layout;
mod pdf_links;
mod pdf_notes;
mod pdf_page_cache;
mod search;
mod theme;
mod views;

#[cfg(target_os = "linux")]
fn install_linux_desktop_entry_with_home(home: &str) -> bool {
    let run = || -> Option<()> {
        let exe_path = std::env::current_exe().ok()?;
        let exe_str = exe_path.to_str()?;

        // Paths
        let local_share = std::path::PathBuf::from(home).join(".local").join("share");
        let app_dir = local_share.join("applications");
        let icons_dir = local_share.join("icons");
        let hicolor_dir = icons_dir.join("hicolor");

        // Create directories if they do not exist
        let _ = std::fs::create_dir_all(&app_dir);
        let _ = std::fs::create_dir_all(&icons_dir);
        let _ = std::fs::create_dir_all(&hicolor_dir);

        // Copy system hicolor index.theme if missing locally
        let index_theme_path = hicolor_dir.join("index.theme");
        if !index_theme_path.exists() {
            let _ = std::fs::copy("/usr/share/icons/hicolor/index.theme", &index_theme_path);
        }

        // Icon bytes
        let icon_bytes = include_bytes!("../../md-editor.png");

        // Write primary icon (1024x1024) directly to ~/.local/share/icons/md-editor.png
        let primary_icon_path = icons_dir.join("md-editor.png");
        let _ = std::fs::write(&primary_icon_path, icon_bytes);

        // Write scalable icon (1024x1024) to ~/.local/share/icons/hicolor/scalable/apps/md-editor.png
        let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
        let _ = std::fs::create_dir_all(&scalable_apps_dir);
        let _ = std::fs::write(scalable_apps_dir.join("md-editor.png"), icon_bytes);

        // Resize and write to specific sizes
        if let Ok(img) = image::load_from_memory(icon_bytes) {
            let sizes = [16, 32, 48, 64, 128, 256, 512];
            for &size in &sizes {
                let size_dir = hicolor_dir.join(format!("{}x{}", size, size)).join("apps");
                let _ = std::fs::create_dir_all(&size_dir);
                let target_path = size_dir.join("md-editor.png");

                let resized = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
                let mut bytes = Vec::new();
                if resized
                    .write_to(
                        &mut std::io::Cursor::new(&mut bytes),
                        image::ImageFormat::Png,
                    )
                    .is_ok()
                {
                    let _ = std::fs::write(target_path, bytes);
                }
            }
        }

        // Desktop entry content using the absolute path to the primary icon
        let primary_icon_str = primary_icon_path.to_str()?;
        let desktop_content = format!(
            r#"[Desktop Entry]
Name=MD Editor
Comment=Native desktop markdown workspace
Exec={} %F
Icon={}
Terminal=false
Type=Application
MimeType=text/markdown;application/pdf;
Categories=Office;WordProcessor;Utility;
StartupWMClass=md-editor
"#,
            exe_str, primary_icon_str
        );

        let desktop_file_path = app_dir.join("md-editor.desktop");
        let _ = std::fs::write(desktop_file_path, desktop_content);

        // Update desktop database and icon cache
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&app_dir)
            .status();

        let _ = std::process::Command::new("gtk-update-icon-cache")
            .arg("-f")
            .arg(&hicolor_dir)
            .status();

        Some(())
    };

    run().is_some()
}

#[cfg(target_os = "linux")]
fn uninstall_linux_desktop_entry_with_home(home: &str) -> bool {
    let run = || -> Option<()> {
        // Paths
        let local_share = std::path::PathBuf::from(home).join(".local").join("share");
        let app_dir = local_share.join("applications");
        let icons_dir = local_share.join("icons");
        let hicolor_dir = icons_dir.join("hicolor");

        // Remove desktop file
        let desktop_file_path = app_dir.join("md-editor.desktop");
        if desktop_file_path.exists() {
            let _ = std::fs::remove_file(desktop_file_path);
        }

        // Remove primary icon
        let primary_icon_path = icons_dir.join("md-editor.png");
        if primary_icon_path.exists() {
            let _ = std::fs::remove_file(primary_icon_path);
        }

        // Remove scalable icon
        let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
        let scalable_icon = scalable_apps_dir.join("md-editor.png");
        if scalable_icon.exists() {
            let _ = std::fs::remove_file(scalable_icon);
        }

        // Remove specific size icons
        let sizes = [16, 32, 48, 64, 128, 256, 512];
        for &size in &sizes {
            let size_icon = hicolor_dir
                .join(format!("{}x{}", size, size))
                .join("apps")
                .join("md-editor.png");
            if size_icon.exists() {
                let _ = std::fs::remove_file(size_icon);
            }
        }

        // Update desktop database and icon cache
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&app_dir)
            .status();

        let _ = std::process::Command::new("gtk-update-icon-cache")
            .arg("-f")
            .arg(&hicolor_dir)
            .status();

        Some(())
    };

    run().is_some()
}

#[cfg(target_os = "linux")]
fn install_linux_desktop_entry() -> bool {
    if let Ok(home) = std::env::var("HOME") {
        install_linux_desktop_entry_with_home(&home)
    } else {
        false
    }
}

#[cfg(target_os = "linux")]
fn uninstall_linux_desktop_entry() -> bool {
    if let Ok(home) = std::env::var("HOME") {
        uninstall_linux_desktop_entry_with_home(&home)
    } else {
        false
    }
}

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
                if install_linux_desktop_entry() {
                    println!("MD Editor desktop entry and icons installed successfully.");
                } else {
                    eprintln!("Failed to install MD Editor desktop entry and icons.");
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
            CliAction::Uninstall => {
                if uninstall_linux_desktop_entry() {
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

    #[test]
    #[cfg(target_os = "linux")]
    fn test_linux_desktop_installation_roundtrip() {
        // Create a temporary directory in the target folder to use as the home directory
        let mut test_home = std::env::temp_dir();
        test_home.push("md_editor_test_home");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).unwrap();
        let test_home_str = test_home.to_str().unwrap();

        // 1. Run the installation with the test home directory
        let installed = super::install_linux_desktop_entry_with_home(test_home_str);
        assert!(installed, "Installation should succeed");

        // Verify files were created
        let local_share = test_home.join(".local").join("share");
        let app_file = local_share.join("applications").join("md-editor.desktop");
        let primary_icon = local_share.join("icons").join("md-editor.png");
        let scalable_icon = local_share
            .join("icons")
            .join("hicolor")
            .join("scalable")
            .join("apps")
            .join("md-editor.png");

        assert!(app_file.exists(), "Desktop file must exist");
        assert!(primary_icon.exists(), "Primary icon must exist");
        assert!(scalable_icon.exists(), "Scalable icon must exist");

        // Verify specific sized icons
        let sizes = [16, 32, 48, 64, 128, 256, 512];
        for &size in &sizes {
            let size_icon = local_share
                .join("icons")
                .join("hicolor")
                .join(format!("{}x{}", size, size))
                .join("apps")
                .join("md-editor.png");
            assert!(size_icon.exists(), "Icon size {}x{} must exist", size, size);
        }

        // 2. Run the uninstallation
        let uninstalled = super::uninstall_linux_desktop_entry_with_home(test_home_str);
        assert!(uninstalled, "Uninstallation should succeed");

        // Verify files were cleaned up
        assert!(!app_file.exists(), "Desktop file must be deleted");
        assert!(!primary_icon.exists(), "Primary icon must be deleted");
        assert!(!scalable_icon.exists(), "Scalable icon must be deleted");
        for &size in &sizes {
            let size_icon = local_share
                .join("icons")
                .join("hicolor")
                .join(format!("{}x{}", size, size))
                .join("apps")
                .join("md-editor.png");
            assert!(
                !size_icon.exists(),
                "Icon size {}x{} must be deleted",
                size,
                size
            );
        }

        // Clean up the temporary folder
        let _ = std::fs::remove_dir_all(&test_home);
    }
}
