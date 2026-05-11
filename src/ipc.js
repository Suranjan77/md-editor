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

// ── PDF Commands ────────────────────────────────────────────────────

export async function openPdf(path) {
  return await invoke("open_pdf", { path });
}

export async function closePdf() {
  return await invoke("close_pdf");
}



export async function getPageLinks(pageIndex) {
  return await invoke("get_page_links", { pageIndex });
}

export async function getLinkPreview(destPage, destY) {
  return await invoke("get_link_preview", { destPage, destY });
}

export async function searchPdf(query) {
  return await invoke("search_pdf", { query });
}
