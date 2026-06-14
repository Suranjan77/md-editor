//! Desktop entry installer (plan Phase 3.2).

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("HOME environment variable is not set")]
    HomeUnavailable,
    #[error("Failed to locate current executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("Executable path is not valid UTF-8")]
    NonUtf8Executable,
    #[error("IO error during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("Embedded icon is invalid: {0}")]
    InvalidIcon(#[source] image::ImageError),
}

fn io_result<T>(operation: &'static str, result: std::io::Result<T>) -> Result<T, DesktopError> {
    result.map_err(|source| DesktopError::Io { operation, source })
}

fn desktop_exec_arg(path: &str) -> String {
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

pub fn install_with_home(home: &Path) -> Result<(), DesktopError> {
    let exe_path = std::env::current_exe().map_err(DesktopError::CurrentExecutable)?;
    let exe_str = exe_path.to_str().ok_or(DesktopError::NonUtf8Executable)?;

    let local_share = home.join(".local").join("share");
    let app_dir = local_share.join("applications");
    let icons_dir = local_share.join("icons");
    let hicolor_dir = icons_dir.join("hicolor");

    io_result(
        "creating desktop applications directory",
        std::fs::create_dir_all(&app_dir),
    )?;
    io_result(
        "creating icons directory",
        std::fs::create_dir_all(&icons_dir),
    )?;
    io_result(
        "creating hicolor icon directory",
        std::fs::create_dir_all(&hicolor_dir),
    )?;

    let index_theme_path = hicolor_dir.join("index.theme");
    if !index_theme_path.exists() {
        let _ = std::fs::copy("/usr/share/icons/hicolor/index.theme", &index_theme_path);
    }

    let icon_bytes = include_bytes!("../../md-editor.png");

    let primary_icon_path = icons_dir.join("md-editor.png");
    io_result(
        "writing primary application icon",
        std::fs::write(&primary_icon_path, icon_bytes),
    )?;

    let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
    io_result(
        "creating scalable icon directory",
        std::fs::create_dir_all(&scalable_apps_dir),
    )?;
    io_result(
        "writing scalable application icon",
        std::fs::write(scalable_apps_dir.join("md-editor.png"), icon_bytes),
    )?;

    let image = image::load_from_memory(icon_bytes).map_err(DesktopError::InvalidIcon)?;
    let sizes = [16, 32, 48, 64, 128, 256, 512];
    for &size in &sizes {
        let size_dir = hicolor_dir.join(format!("{}x{}", size, size)).join("apps");
        io_result(
            "creating sized icon directory",
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
            .map_err(DesktopError::InvalidIcon)?;
        io_result(
            "writing sized application icon",
            std::fs::write(target_path, bytes),
        )?;
    }

    let desktop_content = format!(
        r#"[Desktop Entry]
Name=MD Editor V3
Comment=Native desktop markdown workspace (V3)
Exec={} %f
Icon=md-editor
Terminal=false
Type=Application
MimeType=text/markdown;application/pdf;
Categories=Office;WordProcessor;Utility;
StartupWMClass=md3-shell
"#,
        desktop_exec_arg(exe_str)
    );

    let desktop_file_path = app_dir.join("md3.desktop");
    io_result(
        "writing desktop entry",
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

pub fn uninstall_with_home(home: &Path) -> Result<(), DesktopError> {
    let local_share = home.join(".local").join("share");
    let app_dir = local_share.join("applications");
    let icons_dir = local_share.join("icons");
    let hicolor_dir = icons_dir.join("hicolor");

    // Remove desktop file
    let desktop_file_path = app_dir.join("md3.desktop");
    if desktop_file_path.exists() {
        io_result(
            "removing desktop entry",
            std::fs::remove_file(desktop_file_path),
        )?;
    }

    // Remove primary icon
    let primary_icon_path = icons_dir.join("md-editor.png");
    if primary_icon_path.exists() {
        io_result(
            "removing primary application icon",
            std::fs::remove_file(primary_icon_path),
        )?;
    }

    // Remove scalable icon
    let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
    let scalable_icon = scalable_apps_dir.join("md-editor.png");
    if scalable_icon.exists() {
        io_result(
            "removing scalable application icon",
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
                "removing sized application icon",
                std::fs::remove_file(size_icon),
            )?;
        }
    }

    let _ = std::process::Command::new("update-desktop-database")
        .arg(&app_dir)
        .status();

    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("-f")
        .arg(&hicolor_dir)
        .status();

    Ok(())
}

pub fn install() -> Result<(), DesktopError> {
    let home = std::env::var_os("HOME").ok_or(DesktopError::HomeUnavailable)?;
    install_with_home(Path::new(&home))
}

pub fn uninstall() -> Result<(), DesktopError> {
    let home = std::env::var_os("HOME").ok_or(DesktopError::HomeUnavailable)?;
    uninstall_with_home(Path::new(&home))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_installation_roundtrip() {
        let test_home_dir = tempfile::TempDir::new().unwrap();
        let test_home = test_home_dir.path();

        // Install
        install_with_home(test_home).expect("install should succeed");

        let local_share = test_home.join(".local").join("share");
        let app_file = local_share.join("applications").join("md3.desktop");
        let primary_icon = local_share.join("icons").join("md-editor.png");
        let scalable_icon = local_share
            .join("icons")
            .join("hicolor")
            .join("scalable")
            .join("apps")
            .join("md-editor.png");

        assert!(app_file.exists());
        assert!(primary_icon.exists());
        assert!(scalable_icon.exists());

        let desktop_content = std::fs::read_to_string(&app_file).unwrap();
        assert!(desktop_content.contains("Exec="));
        assert!(desktop_content.contains("Icon=md-editor"));

        // Verify specific sized icons
        let sizes = [16, 32, 48, 64, 128, 256, 512];
        for &size in &sizes {
            let size_icon = local_share
                .join("icons")
                .join("hicolor")
                .join(format!("{}x{}", size, size))
                .join("apps")
                .join("md-editor.png");
            assert!(size_icon.exists());
        }

        // Uninstall
        uninstall_with_home(test_home).expect("uninstall should succeed");

        assert!(!app_file.exists());
        assert!(!primary_icon.exists());
        assert!(!scalable_icon.exists());
        for &size in &sizes {
            let size_icon = local_share
                .join("icons")
                .join("hicolor")
                .join(format!("{}x{}", size, size))
                .join("apps")
                .join("md-editor.png");
            assert!(!size_icon.exists());
        }
    }

    #[test]
    fn test_desktop_exec_arg_escaping() {
        assert_eq!(
            desktop_exec_arg("/tmp/MD Editor/`build`/$app\""),
            "\"/tmp/MD Editor/\\`build\\`/\\$app\\\"\""
        );
    }

    #[test]
    fn test_desktop_install_idempotent() {
        let test_home_dir = tempfile::TempDir::new().unwrap();
        let test_home = test_home_dir.path();

        install_with_home(test_home).expect("first install should succeed");
        install_with_home(test_home).expect("second install should succeed");

        let local_share = test_home.join(".local").join("share");
        let app_file = local_share.join("applications").join("md3.desktop");
        assert!(app_file.exists());

        let sizes = [16, 32, 48, 64, 128, 256, 512];
        for &size in &sizes {
            let size_icon = local_share
                .join("icons")
                .join("hicolor")
                .join(format!("{}x{}", size, size))
                .join("apps")
                .join("md-editor.png");
            assert!(size_icon.exists());
        }
    }
}
