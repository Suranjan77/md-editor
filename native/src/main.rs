mod app;
mod editor;
mod editor_state;
mod fuzzy;
mod messages;
mod motion;
mod pdf_notes;
mod pdf_pane;
mod search;
mod search_state;
mod theme;
mod tracker_state;
mod ui_state;
mod vault_state;
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliAction {
    Install,
    Uninstall,
    /// A file or directory passed by the OS file handler (the desktop entry
    /// declares `Exec=... %F`) or on the command line.
    OpenPath(String),
    RunApp,
}

fn parse_cli_args(args: &[String]) -> CliAction {
    if args.len() > 1 {
        let cmd = args[1].as_str();
        if cmd == "--install" || cmd == "--install-desktop" {
            return CliAction::Install;
        } else if cmd == "--uninstall" || cmd == "--uninstall-desktop" {
            return CliAction::Uninstall;
        } else if !cmd.starts_with('-') {
            return CliAction::OpenPath(cmd.to_string());
        }
    }
    CliAction::RunApp
}

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();
    let mut startup_path: Option<std::path::PathBuf> = None;
    match parse_cli_args(&args) {
        CliAction::Install => {
            #[cfg(target_os = "linux")]
            {
                if install_linux_desktop_entry() {
                    println!("MD Editor desktop entry and icons installed successfully.");
                    std::process::exit(0);
                }
                eprintln!("Failed to install MD Editor desktop entry and icons.");
            }
            #[cfg(not(target_os = "linux"))]
            eprintln!("--install is only supported on Linux.");
            std::process::exit(1);
        }
        CliAction::Uninstall => {
            #[cfg(target_os = "linux")]
            {
                if uninstall_linux_desktop_entry() {
                    println!("MD Editor desktop entry and icons uninstalled successfully.");
                    std::process::exit(0);
                }
                eprintln!("Failed to uninstall MD Editor desktop entry and icons.");
            }
            #[cfg(not(target_os = "linux"))]
            eprintln!("--uninstall is only supported on Linux.");
            std::process::exit(1);
        }
        CliAction::OpenPath(path) => startup_path = Some(path.into()),
        CliAction::RunApp => {}
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
        move || app::MdEditor::new_with_startup_file(startup_path.clone()),
        app::MdEditor::update,
        app::MdEditor::view,
    )
    .title(app::MdEditor::title)
    .theme(|state: &app::MdEditor| state.theme())
    .subscription(app::MdEditor::subscription)
    .window(iced::window::Settings {
        size: restore_window_size(),
        icon,
        platform_specific,
        ..Default::default()
    })
    .run()
}

/// Default window size for a first run, or when the stored geometry is missing
/// or unusable.
const DEFAULT_WINDOW_SIZE: iced::Size = iced::Size {
    width: 1200.0,
    height: 800.0,
};

/// The window size from the previous session, clamped to something sane.
///
/// A stored size is rejected rather than trusted blindly: a monitor that went
/// away, or a stray write, should not open the app at 20×8 pixels or larger
/// than any display the user still owns.
fn restore_window_size() -> iced::Size {
    restore_window_size_from(md_editor_core::config::read_startup_value("window_size"))
}

/// The parsing and clamping half of [`restore_window_size`], split out so it
/// can be exercised without a settings database.
fn restore_window_size_from(stored: Option<String>) -> iced::Size {
    let Some(raw) = stored else {
        return DEFAULT_WINDOW_SIZE;
    };

    let Some((w, h)) = raw.split_once('x') else {
        return DEFAULT_WINDOW_SIZE;
    };

    match (w.trim().parse::<f32>(), h.trim().parse::<f32>()) {
        (Ok(width), Ok(height)) if width.is_finite() && height.is_finite() => iced::Size {
            width: width.clamp(640.0, 16_384.0),
            height: height.clamp(480.0, 16_384.0),
        },
        _ => DEFAULT_WINDOW_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_WINDOW_SIZE, restore_window_size_from};

    #[test]
    fn window_size_round_trips_and_rejects_nonsense() {
        // A normal stored value comes back unchanged.
        let restored = restore_window_size_from(Some("1440x900".to_string()));
        assert_eq!((restored.width, restored.height), (1440.0, 900.0));

        // Absent, malformed, or non-numeric values fall back rather than
        // opening the app at some unusable size.
        for raw in [
            None,
            Some(String::new()),
            Some("wide x tall".to_string()),
            Some("1200".to_string()),
            Some("NaNxNaN".to_string()),
        ] {
            let restored = restore_window_size_from(raw.clone());
            assert_eq!(
                (restored.width, restored.height),
                (DEFAULT_WINDOW_SIZE.width, DEFAULT_WINDOW_SIZE.height),
                "expected fallback for {raw:?}"
            );
        }

        // A monitor that went away must not leave the window unusably small,
        // or larger than any display the user still owns.
        let tiny = restore_window_size_from(Some("20x8".to_string()));
        assert_eq!((tiny.width, tiny.height), (640.0, 480.0));
        let huge = restore_window_size_from(Some("999999x999999".to_string()));
        assert_eq!((huge.width, huge.height), (16_384.0, 16_384.0));
    }

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

        // File or directory paths are handed to the app to open (the desktop
        // entry registers the binary as a markdown/PDF handler with `%F`).
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "notes.md".to_string()]),
            CliAction::OpenPath("notes.md".to_string())
        );
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "/home/user/vault".to_string()]),
            CliAction::OpenPath("/home/user/vault".to_string())
        );

        // Unknown flags fall through to a normal app run.
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--verbose".to_string()]),
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
