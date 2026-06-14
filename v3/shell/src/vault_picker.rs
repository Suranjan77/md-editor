use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const RECENT_LIMIT: usize = 8;

#[derive(Debug, Default, Serialize, Deserialize)]
struct VaultHistory {
    #[serde(default)]
    recent: Vec<PathBuf>,
}

fn history_path() -> PathBuf {
    crate::paths::config_file("recent-vaults.json")
}

pub fn recent_vaults() -> Vec<PathBuf> {
    let path = history_path();
    let Ok(json) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<VaultHistory>(&json)
        .map(|history| {
            history
                .recent
                .into_iter()
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

pub fn record_recent(vault_path: &Path) {
    let path = history_path();
    let canonical = vault_path
        .canonicalize()
        .unwrap_or_else(|_| vault_path.to_path_buf());
    let mut recent = recent_vaults();
    recent.retain(|path| path != &canonical);
    recent.insert(0, canonical);
    recent.truncate(RECENT_LIMIT);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(json) = serde_json::to_string_pretty(&VaultHistory { recent }) else {
        return;
    };
    let _ = std::fs::write(path, json);
}

pub async fn pick_vault_async() -> Option<PathBuf> {
    pick_vault_async_with_title("Open Vault Folder").await
}

pub async fn pick_vault_async_with_title(title: &'static str) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(last) = recent_vaults().into_iter().next() {
        dialog = dialog.set_directory(last);
    }
    dialog
        .pick_folder()
        .await
        .map(|folder| folder.path().to_path_buf())
}

pub fn launch_vault(vault_path: &Path) -> std::io::Result<()> {
    record_recent(vault_path);
    std::process::Command::new(std::env::current_exe()?)
        .arg(vault_path)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_limit_is_bounded() {
        assert_eq!(RECENT_LIMIT, 8);
    }
}
