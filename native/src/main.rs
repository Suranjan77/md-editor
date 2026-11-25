//! Entry point: CLI arguments, window settings and the iced daemon.

// Release builds on Windows run as a GUI app so launching the exe does not
// open a console window. Debug builds keep the console for stdout/stderr.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autocomplete;
mod design;
#[cfg(test)]
mod e2e;
mod editor;
mod editor_state;
mod fuzzy;
mod graph_state;
mod ink;
mod logging;
mod messages;
mod pdf_notes;
mod pdf_pane;
mod platform;
mod raster;
mod search;
mod search_state;
mod shortcuts;
#[cfg(test)]
mod size_lint;
mod tabs;
#[cfg(test)]
mod test_support;
mod tracker_state;
mod ui;
mod ui_state;
mod vault_state;
mod views;

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliAction {
    /// A file or directory passed by the OS file handler (the desktop entry
    /// declares `Exec=md-editor %F`) or on the command line.
    OpenPath(String),
    RunApp,
}

fn parse_cli_args(args: &[String]) -> CliAction {
    match args.get(1) {
        Some(arg) if !arg.starts_with('-') => CliAction::OpenPath(arg.clone()),
        _ => CliAction::RunApp,
    }
}

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();
    let startup_path = match parse_cli_args(&args) {
        CliAction::OpenPath(path) => Some(std::path::PathBuf::from(path)),
        CliAction::RunApp => None,
    };
    logging::init();

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

    let main_window = iced::window::Settings {
        size: restore_window_size(),
        icon,
        platform_specific,
        ..Default::default()
    };

    program(main_window, startup_path).run()
}

/// The app as an iced program: everything `main` runs, minus the event loop.
///
/// A daemon rather than an application: the handwriting canvas lives in its
/// own window so it can sit on a second monitor beside the notes. Daemons own
/// no window by default, so boot opens the main one. The headless end-to-end
/// tests (`e2e/`) drive this same program in `iced_test`'s emulator.
pub(crate) fn program(
    main_window: iced::window::Settings,
    startup_path: Option<std::path::PathBuf>,
) -> iced::Daemon<impl iced_program::Program<State = app::MdEditor, Message = messages::Message>> {
    let mut d = iced::daemon(
        move || app::MdEditor::boot(main_window.clone(), startup_path.clone()),
        app::MdEditor::update,
        app::MdEditor::view,
    )
    .default_font(design::fonts::UI_FONT);
    for bytes in design::fonts::BUNDLED_FONTS {
        d = d.font(*bytes);
    }
    d.title(app::MdEditor::title)
        .theme(|state: &app::MdEditor, _window| state.theme())
        .subscription(app::MdEditor::subscription)
}

/// Settings for the detached handwriting window.
///
/// Sized for writing rather than reading, and given the same icon so it is
/// recognisable when the two windows sit on different monitors.
pub fn ink_window_settings() -> iced::window::Settings {
    let icon = iced::window::icon::from_file_data(
        include_bytes!("../../md-editor.png"),
        Some(image::ImageFormat::Png),
    )
    .ok();

    iced::window::Settings {
        size: iced::Size::new(1000.0, 760.0),
        icon,
        ..Default::default()
    }
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

        // Flags (including the removed `--install`) fall through to a normal
        // app run; packages own desktop integration now.
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--install".to_string()]),
            CliAction::RunApp
        );
        assert_eq!(
            parse_cli_args(&["md-editor".to_string(), "--verbose".to_string()]),
            CliAction::RunApp
        );
    }
}
