//! Configuration persistence: the settings key/value store and the
//! platform/portable config-directory resolution (P2.T3 — absorbed from the
//! legacy `config.rs` and `state.rs`).

use crate::database::settings_repository;
use crate::state::AppState;
use std::path::PathBuf;

/// Get a configuration value by key.
pub fn get_sys_config(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    settings_repository::get(&db, key)
}

/// Set a configuration value by key (upsert).
pub fn set_sys_config(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    settings_repository::set(&db, key, value)
}

pub(crate) fn settings_db_path() -> PathBuf {
    let mut dir = config_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create config directory {}: {err}", dir.display());
        return PathBuf::from("md_editor_settings.sqlite");
    }
    dir.push("md_editor_settings.sqlite");
    dir
}

fn config_dir() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let project_config = directories::ProjectDirs::from("com", "Suranjan77", "md-editor")
        .map(|dirs| dirs.config_dir().to_path_buf());

    config_dir_for(exe.as_deref(), project_config)
}

fn config_dir_for(exe_path: Option<&std::path::Path>, project_config: Option<PathBuf>) -> PathBuf {
    if let Some(exe_path) = exe_path {
        for portable_dir in portable_config_dirs(exe_path) {
            let flag = portable_dir.join("portable.flag");
            let db = portable_dir.join("md_editor_settings.sqlite");
            if flag.exists() || db.exists() {
                return portable_dir;
            }
        }
    }

    project_config
        .or_else(|| {
            exe_path
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf)
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

fn portable_config_dirs(exe_path: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Some(exe_dir) = exe_path.parent() else {
        return dirs;
    };

    dirs.push(exe_dir.to_path_buf());

    if exe_dir.file_name().is_some_and(|name| name == "MacOS")
        && exe_dir
            .parent()
            .and_then(std::path::Path::file_name)
            .is_some_and(|name| name == "Contents")
        && let Some(package_dir) = exe_dir
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::parent)
    {
        dirs.push(package_dir.to_path_buf());
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::{config_dir_for, portable_config_dirs};

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("md_editor_state_{name}_{nanos}"))
    }

    #[test]
    fn config_dir_uses_platform_directory_without_portable_marker() {
        let root = unique_temp_dir("platform");
        let exe_dir = root.join("bin");
        let platform_dir = root.join("config");
        std::fs::create_dir_all(&exe_dir).expect("test executable directory should exist");
        let exe = exe_dir.join("md-editor");

        assert_eq!(
            config_dir_for(Some(&exe), Some(platform_dir.clone())),
            platform_dir
        );

        std::fs::remove_dir_all(root).expect("test directory should be removable");
    }

    #[test]
    fn config_dir_uses_executable_directory_for_portable_flag_or_existing_db() {
        for marker in ["portable.flag", "md_editor_settings.sqlite"] {
            let root = unique_temp_dir(marker);
            let exe_dir = root.join("portable");
            std::fs::create_dir_all(&exe_dir).expect("test executable directory should exist");
            std::fs::write(exe_dir.join(marker), []).expect("portable marker should be writable");
            let exe = exe_dir.join("md-editor");

            assert_eq!(
                config_dir_for(Some(&exe), Some(root.join("config"))),
                exe_dir
            );

            std::fs::remove_dir_all(root).expect("test directory should be removable");
        }
    }

    #[test]
    fn macos_bundle_uses_marker_beside_app_without_mutating_bundle() {
        let root = unique_temp_dir("macos_bundle");
        let exe_dir = root.join("MD Editor.app").join("Contents").join("MacOS");
        std::fs::create_dir_all(&exe_dir).expect("bundle executable directory should exist");
        std::fs::write(root.join("portable.flag"), []).expect("portable marker should be writable");
        let exe = exe_dir.join("md-editor");

        assert_eq!(
            portable_config_dirs(&exe),
            [exe_dir, root.clone()].map(std::path::PathBuf::from)
        );
        assert_eq!(
            config_dir_for(Some(&exe), Some(root.join("platform-config"))),
            root
        );
    }
}
