use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub(crate) enum PlatformError {
    HomeUnavailable,
    CurrentExecutable(std::io::Error),
    NonUtf8Executable,
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    InvalidIcon(image::ImageError),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeUnavailable => write!(formatter, "HOME is not set"),
            Self::CurrentExecutable(error) => {
                write!(formatter, "failed to locate current executable: {error}")
            }
            Self::NonUtf8Executable => write!(formatter, "executable path is not valid UTF-8"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::InvalidIcon(error) => {
                write!(formatter, "embedded application icon is invalid: {error}")
            }
        }
    }
}

impl std::error::Error for PlatformError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentExecutable(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::InvalidIcon(error) => Some(error),
            Self::HomeUnavailable | Self::NonUtf8Executable => None,
        }
    }
}

fn io_result<T>(operation: &'static str, result: std::io::Result<T>) -> Result<T, PlatformError> {
    result.map_err(|source| PlatformError::Io { operation, source })
}

fn desktop_exec_arg(path: &str) -> String {
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

fn install_linux_desktop_entry_with_home(home: &Path) -> Result<(), PlatformError> {
    let exe_path = std::env::current_exe().map_err(PlatformError::CurrentExecutable)?;
    let exe_str = exe_path.to_str().ok_or(PlatformError::NonUtf8Executable)?;

    let local_share = home.join(".local").join("share");
    let app_dir = local_share.join("applications");
    let icons_dir = local_share.join("icons");
    let hicolor_dir = icons_dir.join("hicolor");

    io_result(
        "failed to create desktop application directory",
        std::fs::create_dir_all(&app_dir),
    )?;
    io_result(
        "failed to create icon directory",
        std::fs::create_dir_all(&icons_dir),
    )?;
    io_result(
        "failed to create hicolor icon directory",
        std::fs::create_dir_all(&hicolor_dir),
    )?;

    let index_theme_path = hicolor_dir.join("index.theme");
    if !index_theme_path.exists() {
        let _ = std::fs::copy("/usr/share/icons/hicolor/index.theme", &index_theme_path);
    }

    let icon_bytes = include_bytes!("../../../md-editor.png");

    let primary_icon_path = icons_dir.join("md-editor.png");
    io_result(
        "failed to write primary application icon",
        std::fs::write(&primary_icon_path, icon_bytes),
    )?;

    let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
    io_result(
        "failed to create scalable icon directory",
        std::fs::create_dir_all(&scalable_apps_dir),
    )?;
    io_result(
        "failed to write scalable application icon",
        std::fs::write(scalable_apps_dir.join("md-editor.png"), icon_bytes),
    )?;

    let image = image::load_from_memory(icon_bytes).map_err(PlatformError::InvalidIcon)?;
    let sizes = [16, 32, 48, 64, 128, 256, 512];
    for &size in &sizes {
        let size_dir = hicolor_dir.join(format!("{}x{}", size, size)).join("apps");
        io_result(
            "failed to create sized icon directory",
            std::fs::create_dir_all(&size_dir),
        )?;
        let target_path = size_dir.join("md-editor.png");
        let resized = image.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let mut bytes = Vec::new();
        resized
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(PlatformError::InvalidIcon)?;
        io_result(
            "failed to write sized application icon",
            std::fs::write(target_path, bytes),
        )?;
    }

    let primary_icon_str = primary_icon_path
        .to_str()
        .ok_or(PlatformError::NonUtf8Executable)?;
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
        desktop_exec_arg(exe_str),
        primary_icon_str
    );

    let desktop_file_path = app_dir.join("md-editor.desktop");
    io_result(
        "failed to write desktop entry",
        std::fs::write(desktop_file_path, desktop_content),
    )?;

    let _ = std::process::Command::new("update-desktop-database")
        .arg(&app_dir)
        .status();

    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("-f")
        .arg(&hicolor_dir)
        .status();

    Ok(())
}

fn uninstall_linux_desktop_entry_with_home(home: &Path) -> Result<(), PlatformError> {
    let local_share = home.join(".local").join("share");
    let app_dir = local_share.join("applications");
    let icons_dir = local_share.join("icons");
    let hicolor_dir = icons_dir.join("hicolor");

    // Remove desktop file
    let desktop_file_path = app_dir.join("md-editor.desktop");
    if desktop_file_path.exists() {
        io_result(
            "failed to remove desktop entry",
            std::fs::remove_file(desktop_file_path),
        )?;
    }

    // Remove primary icon
    let primary_icon_path = icons_dir.join("md-editor.png");
    if primary_icon_path.exists() {
        io_result(
            "failed to remove primary application icon",
            std::fs::remove_file(primary_icon_path),
        )?;
    }

    // Remove scalable icon
    let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
    let scalable_icon = scalable_apps_dir.join("md-editor.png");
    if scalable_icon.exists() {
        io_result(
            "failed to remove scalable application icon",
            std::fs::remove_file(scalable_icon),
        )?;
    }

    // Remove specific size icons
    let sizes = [16, 32, 48, 64, 128, 256, 512];
    for &size in &sizes {
        let size_icon = hicolor_dir
            .join(format!("{}x{}", size, size))
            .join("apps")
            .join("md-editor.png");
        if size_icon.exists() {
            io_result(
                "failed to remove sized application icon",
                std::fs::remove_file(size_icon),
            )?;
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

    Ok(())
}

pub(crate) fn install_linux_desktop_entry() -> Result<(), PlatformError> {
    let home = std::env::var_os("HOME").ok_or(PlatformError::HomeUnavailable)?;
    install_linux_desktop_entry_with_home(Path::new(&home))
}

pub(crate) fn uninstall_linux_desktop_entry() -> Result<(), PlatformError> {
    let home = std::env::var_os("HOME").ok_or(PlatformError::HomeUnavailable)?;
    uninstall_linux_desktop_entry_with_home(Path::new(&home))
}

#[cfg(test)]
mod tests {
    use super::{
        desktop_exec_arg, install_linux_desktop_entry_with_home,
        uninstall_linux_desktop_entry_with_home,
    };

    #[test]
    fn test_linux_desktop_installation_roundtrip() {
        // Create a temporary directory in the target folder to use as the home directory
        let mut test_home = std::env::temp_dir();
        test_home.push("md_editor_test_home");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).unwrap();
        // 1. Run the installation with the test home directory
        install_linux_desktop_entry_with_home(&test_home).expect("installation should succeed");

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
        let desktop_content =
            std::fs::read_to_string(&app_file).expect("desktop entry should be readable");
        assert!(
            desktop_content.contains("Exec=\""),
            "desktop executable path should be quoted"
        );

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
        uninstall_linux_desktop_entry_with_home(&test_home).expect("uninstallation should succeed");

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

    #[test]
    fn desktop_exec_arg_escapes_reserved_characters() {
        assert_eq!(
            desktop_exec_arg("/tmp/MD Editor/`build`/$app\""),
            "\"/tmp/MD Editor/\\`build\\`/\\$app\\\"\""
        );
    }
}
