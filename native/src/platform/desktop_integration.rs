fn desktop_exec_arg(path: &str) -> String {
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

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
        std::fs::create_dir_all(&app_dir).ok()?;
        std::fs::create_dir_all(&icons_dir).ok()?;
        std::fs::create_dir_all(&hicolor_dir).ok()?;

        // Copy system hicolor index.theme if missing locally
        let index_theme_path = hicolor_dir.join("index.theme");
        if !index_theme_path.exists() {
            let _ = std::fs::copy("/usr/share/icons/hicolor/index.theme", &index_theme_path);
        }

        // Icon bytes
        let icon_bytes = include_bytes!("../../../md-editor.png");

        // Write primary icon (1024x1024) directly to ~/.local/share/icons/md-editor.png
        let primary_icon_path = icons_dir.join("md-editor.png");
        std::fs::write(&primary_icon_path, icon_bytes).ok()?;

        // Write scalable icon (1024x1024) to ~/.local/share/icons/hicolor/scalable/apps/md-editor.png
        let scalable_apps_dir = hicolor_dir.join("scalable").join("apps");
        std::fs::create_dir_all(&scalable_apps_dir).ok()?;
        std::fs::write(scalable_apps_dir.join("md-editor.png"), icon_bytes).ok()?;

        // Resize and write to specific sizes
        if let Ok(img) = image::load_from_memory(icon_bytes) {
            let sizes = [16, 32, 48, 64, 128, 256, 512];
            for &size in &sizes {
                let size_dir = hicolor_dir.join(format!("{}x{}", size, size)).join("apps");
                std::fs::create_dir_all(&size_dir).ok()?;
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
                    std::fs::write(target_path, bytes).ok()?;
                }
            }
        } else {
            return None;
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
            desktop_exec_arg(exe_str),
            primary_icon_str
        );

        let desktop_file_path = app_dir.join("md-editor.desktop");
        std::fs::write(desktop_file_path, desktop_content).ok()?;

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

pub(crate) fn install_linux_desktop_entry() -> bool {
    if let Ok(home) = std::env::var("HOME") {
        install_linux_desktop_entry_with_home(&home)
    } else {
        false
    }
}

pub(crate) fn uninstall_linux_desktop_entry() -> bool {
    if let Ok(home) = std::env::var("HOME") {
        uninstall_linux_desktop_entry_with_home(&home)
    } else {
        false
    }
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
        let test_home_str = test_home.to_str().unwrap();

        // 1. Run the installation with the test home directory
        let installed = install_linux_desktop_entry_with_home(test_home_str);
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
        let uninstalled = uninstall_linux_desktop_entry_with_home(test_home_str);
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

    #[test]
    fn desktop_exec_arg_escapes_reserved_characters() {
        assert_eq!(
            desktop_exec_arg("/tmp/MD Editor/`build`/$app\""),
            "\"/tmp/MD Editor/\\`build\\`/\\$app\\\"\""
        );
    }
}
