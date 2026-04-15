/**
 * IPC wrappers for Tauri commands.
 * With CM6, these are thin: just file I/O and backlinks.
 */

const { invoke } = window.__TAURI__.core;

export async function openFile(path) {
  return await invoke("open_file", { path });
}

export async function saveFile(path, content) {
  return await invoke("save_file", { path, content });
}

export async function createFile(path) {
  return await invoke("create_file", { path });
}

export async function createDir(path) {
  return await invoke("create_dir", { path });
}

export async function renameFile(oldPath, newPath) {
  return await invoke("rename_file", { oldPath, newPath });
}

export async function deleteFile(path) {
  return await invoke("delete_file", { path });
}

export async function listVault() {
  return await invoke("list_vault");
}

export async function searchVault(query) {
  return await invoke("search_vault", { query });
}

export async function setVaultRoot(path) {
  return await invoke("set_vault_root", { path });
}

export async function getBacklinks(path) {
  return await invoke("get_backlinks", { path });
}

export async function getSysConfig(key) {
  return await invoke("get_sys_config", { key });
}

export async function setSysConfig(key, value) {
  return await invoke("set_sys_config", { key, value });
}
