use crate::domain::path::{AbsPath, VaultPath};
use crate::domain::{BacklinkItem, FileEntry};
use crate::state::AppState;

pub struct VaultService<'a> {
    state: &'a AppState,
}

impl<'a> VaultService<'a> {
    pub const fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub fn open(&self, vault_path: &VaultPath) -> Result<Vec<u8>, String> {
        crate::vault::open_file(self.state, &vault_path.to_string())
    }

    pub fn save(&self, vault_path: &VaultPath, content: &str) -> Result<(), String> {
        crate::vault::save_file(self.state, &vault_path.to_string(), content)
    }

    pub fn create_file(&self, vault_path: &VaultPath) -> Result<(), String> {
        crate::vault::create_file(self.state, &vault_path.to_string())
    }

    pub fn create_dir(&self, vault_path: &VaultPath) -> Result<(), String> {
        crate::vault::create_dir(self.state, &vault_path.to_string())
    }

    pub fn rename(&self, old_path: &VaultPath, new_path: &VaultPath) -> Result<(), String> {
        crate::vault::rename_entry(self.state, &old_path.to_string(), &new_path.to_string())
    }

    pub fn delete(&self, vault_path: &VaultPath) -> Result<(), String> {
        crate::vault::delete_entry(self.state, &vault_path.to_string())
    }

    pub fn backlinks(&self, vault_path: &VaultPath) -> Result<Vec<BacklinkItem>, String> {
        crate::vault::get_mixed_backlinks(self.state, &vault_path.to_string())
    }

    pub fn set_root(&self, abs_path: &AbsPath) -> Result<Vec<FileEntry>, String> {
        crate::vault::set_vault_root(self.state, &abs_path.to_string())
    }
}
