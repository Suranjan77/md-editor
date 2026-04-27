import { createEditor, setContent, getContent, hasFocus, setCurrentFilePath } from "./editor.js";
import { clearImageCache } from "./markdown-decorations.js";
import {
  openFile,
  saveFile,
  createFile,
  createDir,
  renameFile,
  deleteFile,
  listVault,
  searchVault,
  setVaultRoot,
  getBacklinks,
  getSysConfig,
  setSysConfig,
} from "./ipc.js";
import "./style.css";

const { open, ask } = window.__TAURI__.dialog;

const state = { 
  currentPath: null, 
  vaultRoot: null,
  selectedSidebarPath: null, // Track which sidebar item is selected
  selectedSidebarIsDir: false, // Track if selection is a folder
  modalType: null, // 'file', 'folder', 'rename'
  modalTarget: null 
};

const cmHost = document.getElementById("cm-host");
const imagePreview = document.getElementById("image-preview");
const fileList = document.getElementById("file-list");
const backlinksList = document.getElementById("backlinks-list");
const welcomeScreen = document.getElementById("welcome-screen");
const nameInputModal = document.getElementById("name-input-modal");
const nameInputTitle = document.getElementById("name-input-title");
const nameInputField = document.getElementById("name-input-field");
const btnConfirmName = document.getElementById("btn-confirm-name");
const btnCancelName = document.getElementById("btn-cancel-name");
const sidebar = document.getElementById("sidebar");
const backlinksPane = document.getElementById("backlinks-pane");
const searchOverlay = document.getElementById("search-overlay");
const searchInput = document.getElementById("search-input");
const searchResults = document.getElementById("search-results");
const shortcutsOverlay = document.getElementById("shortcuts-overlay");
const toastContainer = document.getElementById("toast-container");

const editor = createEditor(cmHost, handleSave);

const IMAGE_EXTENSIONS = ["jpeg", "jpg", "png", "svg", "webp", "avif"];
function isImageFile(filename) {
  const ext = filename.split(".").pop().toLowerCase();
  return IMAGE_EXTENSIONS.includes(ext);
}

async function init() {
  try {
    const lastRoot = await getSysConfig("last_vault_root");
    if (lastRoot) {
      state.vaultRoot = lastRoot;
      const entries = await setVaultRoot(lastRoot);
      renderFileList(entries);
      welcomeScreen.classList.add("hidden");

      const lastFile = await getSysConfig("last_file");
      if (lastFile) {
        await handleOpenMdFile(lastFile);
      }
    }
  } catch (err) {
    console.warn("Workspace cache miss:", err);
  }
  // ── Buttons ──────────────────────────────────────────────
  document
    .getElementById("btn-open-folder")
    ?.addEventListener("click", handleOpenFolder);
  document
    .getElementById("btn-open-welcome")
    ?.addEventListener("click", handleOpenFolder);
  document
    .getElementById("btn-new-file")
    ?.addEventListener("click", () => showNameModal("file"));
  document
    .getElementById("btn-new-folder")
    ?.addEventListener("click", () => showNameModal("folder"));
  btnCancelName?.addEventListener("click", hideNameModal);
  btnConfirmName?.addEventListener("click", handleNameModalConfirm);

  // ── Panel toggles (toolbar buttons) ─────────────────────
  document
    .getElementById("btn-toggle-sidebar")
    ?.addEventListener("click", toggleSidebar);
  document
    .getElementById("btn-toggle-backlinks")
    ?.addEventListener("click", toggleBacklinks);

  // ── Collapse chevron buttons ─────────────────────────────
  const btnCollapseSidebar = document.getElementById("btn-collapse-sidebar");
  if (btnCollapseSidebar)
    btnCollapseSidebar.addEventListener("click", toggleSidebar);

  const btnCollapseBacklinks = document.getElementById(
    "btn-collapse-backlinks",
  );
  if (btnCollapseBacklinks)
    btnCollapseBacklinks.addEventListener("click", toggleBacklinks);

  // ── Shortcuts overlay close on click outside ─────────────
  shortcutsOverlay.addEventListener("click", (e) => {
    if (e.target === shortcutsOverlay) hideShortcutsOverlay();
  });

  // ── Search overlay ───────────────────────────────────────
  searchOverlay.addEventListener("click", (e) => {
    if (e.target === searchOverlay) hideSearchOverlay();
  });

  searchInput.addEventListener("input", debounce(handleSearch, 300));

  nameInputField.addEventListener("keydown", (e) => {
    if (e.key === "Enter") handleNameModalConfirm();
    if (e.key === "Escape") hideNameModal();
  });

  // ── Global shortcuts ────────────────────────────────────
  window.addEventListener("keydown", (e) => {
    const mod = e.ctrlKey || e.metaKey;

    // Ctrl+O — Open folder
    if (mod && e.key.toLowerCase() === "o") {
      e.preventDefault();
      e.stopPropagation();
      handleOpenFolder();
      return;
    }

    // Ctrl+N — New file
    if (mod && e.key.toLowerCase() === "n") {
      e.preventDefault();
      e.stopPropagation();
      showNameModal("file");
      return;
    }

    // Ctrl+Shift+F — Vault search
    if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      e.stopPropagation();
      toggleSearchOverlay();
      return;
    }

    // Ctrl+\ — Toggle sidebar
    if (mod && e.key === "\\") {
      e.preventDefault();
      e.stopPropagation();
      toggleSidebar();
      return;
    }

    // Ctrl+Shift+B — Toggle backlinks
    if (mod && e.shiftKey && e.key.toLowerCase() === "b") {
      e.preventDefault();
      e.stopPropagation();
      toggleBacklinks();
      return;
    }

    // Ctrl+/ — Toggle shortcuts overlay
    if (mod && e.key === "/") {
      e.preventDefault();
      e.stopPropagation();
      toggleShortcutsOverlay();
      return;
    }

    // Delete — Delete selected file/folder
    if (e.key === "Delete" && state.selectedSidebarPath) {
      const isInputFocused = ["INPUT", "TEXTAREA"].includes(document.activeElement.tagName);
      
      if (!isInputFocused && !hasFocus(editor)) {
        e.preventDefault();
        e.stopPropagation();
        handleDelete(state.selectedSidebarPath);
        return;
      }
    }

    // F2 — Rename selected file/folder
    if (e.key === "F2" && state.selectedSidebarPath) {
      const isInputFocused = ["INPUT", "TEXTAREA"].includes(document.activeElement.tagName);

      if (!isInputFocused) {
        e.preventDefault();
        e.stopPropagation();
        showNameModal("rename", state.selectedSidebarPath);
        return;
      }
    }

    // Escape — close overlays
    if (e.key === "Escape") {
      if (!shortcutsOverlay.classList.contains("hidden")) {
        e.preventDefault();
        e.stopPropagation();
        hideShortcutsOverlay();
        return;
      }
      if (!nameInputModal.classList.contains("hidden")) {
        e.preventDefault();
        e.stopPropagation();
        hideNameModal();
        return;
      }
      if (!searchOverlay.classList.contains("hidden")) {
        e.preventDefault();
        e.stopPropagation();
        hideSearchOverlay();
        return;
      }
    }
  }, true); // Use capture phase

  // ── Wikilink clicks ─────────────────────────────────────
  cmHost.addEventListener("click", (e) => {
    const wl = e.target.closest(".cm-wikilink");
    if (wl) {
      let text = wl.textContent;
      if (text) {
        text = text.split("|")[0].trim(); // Strip alias from [[page|alias]]
        handleOpenMdFile(text);
      }
    }
  });
}

// ── Panel toggles ───────────────────────────────────────────────────
function toggleSidebar() {
  sidebar.classList.toggle("collapsed");
}

function toggleBacklinks() {
  backlinksPane.classList.toggle("collapsed");
}

// ── Shortcuts overlay ───────────────────────────────────────────────
function toggleShortcutsOverlay() {
  shortcutsOverlay.classList.toggle("hidden");
}
function hideShortcutsOverlay() {
  shortcutsOverlay.classList.add("hidden");
}

// ── Search overlay ───────────────────────────────────────────────
function toggleSearchOverlay() {
  if (searchOverlay.classList.contains("hidden")) {
    showSearchOverlay();
  } else {
    hideSearchOverlay();
  }
}
function showSearchOverlay() {
  searchOverlay.classList.remove("hidden");
  searchInput.focus();
}
function hideSearchOverlay() {
  searchOverlay.classList.add("hidden");
  searchInput.value = "";
  renderSearchResults([]);
}

async function handleSearch() {
  const query = searchInput.value.trim();
  if (query.length < 2) {
    renderSearchResults([]);
    return;
  }
  try {
    const results = await searchVault(query);
    renderSearchResults(results);
  } catch (err) {
    console.error("Search error:", err);
  }
}

function renderSearchResults(results) {
  searchResults.innerHTML = "";
  if (results.length === 0) {
    searchResults.innerHTML =
      '<div class="empty-state">No results found.</div>';
    return;
  }

  results.forEach((res) => {
    const item = document.createElement("div");
    item.className =
      "p-3 rounded-lg hover:bg-[#23262b] cursor-pointer transition-colors border border-transparent hover:border-[#45484e]/30";
    item.innerHTML = `
      <div class="flex justify-between items-center mb-1">
        <span class="text-sm font-bold text-[#b1ccc6]">${res.path}</span>
        <span class="text-[10px] text-[#9d9ea3]">Line ${res.line}</span>
      </div>
      <div class="text-[12px] text-[#e3e5ed] opacity-70 truncate">${res.context}</div>
    `;
    item.addEventListener("click", () => {
      handleOpenMdFile(res.path);
      hideSearchOverlay();
    });
    searchResults.appendChild(item);
  });
}

function debounce(fn, delay) {
  let timeout;
  return function (...args) {
    clearTimeout(timeout);
    timeout = setTimeout(() => fn.apply(this, args), delay);
  };
}

// ── File ops ────────────────────────────────────────────────────────
async function handleOpenFolder() {
  try {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      state.vaultRoot = selected;
      clearImageCache(); // Free old blob URLs before switching vaults
      await setSysConfig("last_vault_root", selected);
      const entries = await setVaultRoot(selected);
      renderFileList(entries);
      welcomeScreen.classList.add("hidden");
    }
  } catch (err) {
    console.error("Open folder:", err);
  }
}

async function handleOpenMdFile(relativePath) {
  // This line is necessary for backlinks to work without writing extensions
  if (!relativePath.endsWith(".md")) relativePath += ".md";
  try {
    const content = await openFile(relativePath).then((c) => {
      return new TextDecoder().decode(new Uint8Array(c));
    });
    state.currentPath = relativePath;
    state.selectedSidebarPath = relativePath; // Sync selection with open file
    state.selectedSidebarIsDir = false;
    await setSysConfig("last_file", relativePath);
    // Hide image preview, show editor
    imagePreview.classList.add("hidden");
    cmHost.classList.remove("hidden");
    welcomeScreen.classList.add("hidden");
    // Update the current file path facet for relative image resolution
    setCurrentFilePath(editor, relativePath);
    setContent(editor, content);
    editor.focus();
    updateSidebarSelection();
    updateToolbarFilename();
    await updateBacklinks(relativePath);
  } catch (err) {
    console.error("Open file:", err);
  }
}

async function handleOpenImageFile(relativePath) {
  try {
    const bytes = await openFile(relativePath);
    const ext = relativePath.split(".").pop().toLowerCase();
    const uint8Array = new Uint8Array(bytes);
    const mimeType = ext === "svg" ? "image/svg+xml" : `image/${ext}`;
    const blob = new Blob([uint8Array], { type: mimeType });
    const url = URL.createObjectURL(blob);

    state.currentPath = null; // Not an editable file
    state.selectedSidebarPath = relativePath;
    state.selectedSidebarIsDir = false;

    // Hide editor, show image preview
    cmHost.classList.add("hidden");
    welcomeScreen.classList.add("hidden");
    imagePreview.classList.remove("hidden");
    imagePreview.innerHTML = `
      <div class="image-preview-inner">
        <img src="${url}" alt="${relativePath.split("/").pop()}" />
        <div class="image-preview-filename">${relativePath}</div>
      </div>
    `;

    updateSidebarSelection();
    updateToolbarFilename(relativePath);
  } catch (err) {
    console.error("Open image:", err);
  }
}

async function handleSave() {
  if (!state.currentPath) return;
  try {
    await saveFile(state.currentPath, getContent(editor));
    showToast("Saved", "success");
  } catch (err) {
    console.error("Save:", err);
  }
}

async function handleCreateFile() {
  let filename = nameInputField.value.trim();
  if (!filename) return;
  if (!filename.endsWith(".md")) filename += ".md";

  let parent = "";
  if (state.selectedSidebarPath) {
    if (state.selectedSidebarIsDir) {
      parent = state.selectedSidebarPath;
    } else {
      parent = state.selectedSidebarPath.split("/").slice(0, -1).join("/");
    }
  }
  const fullPath = parent ? `${parent}/${filename}` : filename;

  try {
    await createFile(fullPath);
    hideNameModal();
    const entries = await listVault();
    renderFileList(entries);
    await handleOpenMdFile(fullPath);
  } catch (err) {
    console.error("Create file:", err);
  }
}

async function handleCreateFolder() {
  let foldername = nameInputField.value.trim();
  if (!foldername) return;

  let parent = "";
  if (state.selectedSidebarPath) {
    if (state.selectedSidebarIsDir) {
      parent = state.selectedSidebarPath;
    } else {
      parent = state.selectedSidebarPath.split("/").slice(0, -1).join("/");
    }
  }
  const fullPath = parent ? `${parent}/${foldername}` : foldername;

  try {
    await createDir(fullPath);
    hideNameModal();
    const entries = await listVault();
    renderFileList(entries);
  } catch (err) {
    console.error("Create folder:", err);
  }
}

async function handleRename() {
  const newName = nameInputField.value.trim();
  if (!newName || !state.modalTarget) return;
  
  const oldPath = state.modalTarget;
  const parts = oldPath.split("/");
  parts[parts.length - 1] = newName;
  const newPath = parts.join("/");

  try {
    await renameFile(oldPath, newPath);
    hideNameModal();
    const entries = await listVault();
    renderFileList(entries);
    if (state.currentPath === oldPath) {
      state.currentPath = newPath;
      updateToolbarFilename();
    }
    state.selectedSidebarPath = newPath;
    updateSidebarSelection();
  } catch (err) {
    console.error("Rename:", err);
  }
}

async function handleDelete(path) {
  const confirmed = await ask(`Are you sure you want to delete ${path}?`, {
    title: "Confirm Delete",
    kind: "warning",
  });
  if (!confirmed) return;

  try {
    await deleteFile(path);
    // Clear editor if the deleted item is the current file or its parent folder
    if (
      state.currentPath &&
      (state.currentPath === path || state.currentPath.startsWith(path + "/"))
    ) {
      state.currentPath = null;
      setContent(editor, "");
      updateToolbarFilename();
    }
    if (state.selectedSidebarPath === path) {
      state.selectedSidebarPath = null;
    }
    const entries = await listVault();
    renderFileList(entries);
  } catch (err) {
    console.error("Delete:", err);
  }
}

function showNameModal(type, target = null) {
  if (!state.vaultRoot) return;
  state.modalType = type;
  state.modalTarget = target;
  
  if (type === "file") {
    nameInputTitle.textContent = "Create New File";
    nameInputField.placeholder = "filename.md";
    nameInputField.value = "";
  } else if (type === "folder") {
    nameInputTitle.textContent = "Create New Folder";
    nameInputField.placeholder = "folder name";
    nameInputField.value = "";
  } else if (type === "rename") {
    nameInputTitle.textContent = "Rename";
    const currentName = target.split("/").pop();
    nameInputField.value = currentName;
  }
  
  nameInputModal.classList.remove("hidden");
  setTimeout(() => nameInputField.focus(), 50);
}

function hideNameModal() {
  nameInputModal.classList.add("hidden");
  nameInputField.value = "";
  state.modalType = null;
  state.modalTarget = null;
}

function handleNameModalConfirm() {
  if (state.modalType === "file") handleCreateFile();
  else if (state.modalType === "folder") handleCreateFolder();
  else if (state.modalType === "rename") handleRename();
}

async function updateBacklinks(path) {
  try {
    const links = await getBacklinks(path);
    backlinksList.innerHTML = "";
    if (!links.length) {
      backlinksList.innerHTML = '<div class="empty-state">No backlinks</div>';
      return;
    }
    links.forEach((absPath, idx) => {
      const rel = state.vaultRoot
        ? absPath.replace(state.vaultRoot, "").replace(/^[\\/]/, "")
        : absPath;
      const el = document.createElement("div");
      el.className =
        "backlink-item flex flex-col gap-1 p-3 bg-[#23262b]/50 hover:bg-[#23262b] border border-[#45484e]/30 rounded-xl transition-all cursor-pointer group mb-3";
      el.innerHTML = `<span class="text-on-surface text-[11px] font-medium tracking-normal normal-case group-hover:text-primary transition-colors">${rel}</span>`;
      el.addEventListener("click", () => handleOpenMdFile(rel));
      backlinksList.appendChild(el);
    });
  } catch (err) {
    console.error("Backlinks:", err);
  }
}

// ── UI helpers ──────────────────────────────────────────────────────
function buildTree(entries) {
  const root = { children: {} };
  for (const entry of entries) {
    const parts = entry.path.split("/");
    let current = root;
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      if (!current.children[part]) {
        current.children[part] = {
          name: part,
          path: parts.slice(0, i + 1).join("/"),
          is_dir: i < parts.length - 1 || entry.is_dir,
          children: {},
        };
      }
      current = current.children[part];
    }
  }
  return root;
}

function renderTreeNodes(node, container, level = 0) {
  const children = Object.values(node.children).sort((a, b) => {
    if (a.is_dir === b.is_dir) return a.name.localeCompare(b.name);
    return a.is_dir ? -1 : 1;
  });

  children.forEach((child, idx) => {
    const itemContainer = document.createElement("div");
    itemContainer.className = "group relative";

    // Reusable delete button for sidebar items
    function createDeleteBtn(path) {
      const btn = document.createElement("button");
      btn.className = "sidebar-delete-btn material-symbols-outlined !text-[14px] p-1 rounded-md opacity-0 group-hover:opacity-60 hover:!opacity-100 hover:text-[#ee7d77] hover:bg-[#ee7d77]/10 transition-all duration-150 flex-shrink-0";
      btn.textContent = "delete";
      btn.title = "Delete";
      btn.addEventListener("click", (e) => {
        e.preventDefault();
        e.stopPropagation();
        handleDelete(path);
      });
      return btn;
    }

    if (child.is_dir) {
      const details = document.createElement("details");
      details.className = "tree-dir mb-1";
      if (level === 0) details.open = true;

      const summary = document.createElement("summary");
      summary.className = `file-item flex items-center justify-between gap-3 p-2 text-[#9d9ea3] hover:text-[#b1ccc6] hover:bg-[#23262b] rounded-lg transition-all cursor-pointer text-xs whitespace-nowrap outline-none select-none focus:bg-[#23262b] focus:text-[#e3e5ed]`;
      summary.setAttribute("data-path", child.path);
      summary.setAttribute("tabindex", "0");
      summary.style.paddingLeft = `${8 + level * 12}px`;
      
      const label = document.createElement("div");
      label.className = "flex items-center gap-3 overflow-hidden";
      label.innerHTML = `<span class="material-symbols-outlined !text-[14px]">folder</span> <span class="truncate">${child.name}</span>`;
      summary.appendChild(label);
      summary.appendChild(createDeleteBtn(child.path));

      summary.addEventListener("click", (e) => {
        state.selectedSidebarPath = child.path;
        state.selectedSidebarIsDir = true;
        updateSidebarSelection();
        summary.focus();
      });

      const childrenContainer = document.createElement("div");
      childrenContainer.className = "tree-children";
      renderTreeNodes(child, childrenContainer, level + 1);

      details.appendChild(summary);
      details.appendChild(childrenContainer);
      itemContainer.appendChild(details);
    } else {
      const el = document.createElement("div");
      el.className = `file-item flex items-center justify-between gap-3 p-2 rounded-lg transition-all cursor-pointer text-xs whitespace-nowrap mb-1 outline-none select-none focus:bg-[#23262b] focus:text-[#e3e5ed]`;
      el.setAttribute("data-path", child.path);
      el.setAttribute("tabindex", "0");
      el.style.paddingLeft = `${8 + level * 12}px`;
      
      const label = document.createElement("div");
      label.className = "flex items-center gap-3 overflow-hidden";
      const iconName = isImageFile(child.name) ? "image" : "draft";
      label.innerHTML = `<span class="material-symbols-outlined !text-[14px]">${iconName}</span> <span class="truncate">${child.name}</span>`;
      el.appendChild(label);
      el.appendChild(createDeleteBtn(child.path));
      
      el.addEventListener("click", (e) => {
        if (isImageFile(child.path)) {
          handleOpenImageFile(child.path);
        } else {
          handleOpenMdFile(child.path);
        }
        state.selectedSidebarPath = child.path;
        state.selectedSidebarIsDir = false;
        updateSidebarSelection();
        el.focus();
      });
      itemContainer.appendChild(el);
    }
    container.appendChild(itemContainer);
  });
}

function renderFileList(entries) {
  fileList.innerHTML = "";
  if (!entries.length) {
    fileList.innerHTML = '<div class="empty-state">No markdown files</div>';
    return;
  }
  const tree = buildTree(entries);
  renderTreeNodes(tree, fileList, 0);
  updateSidebarSelection();
}

function updateSidebarSelection() {
  document.querySelectorAll(".file-item").forEach((el) => {
    const path = el.getAttribute("data-path");
    const isActive = path === state.currentPath;
    const isSelected = path === state.selectedSidebarPath;
    
    // Reset classes first
    el.classList.remove(
      "text-[#b1ccc6]", "text-[#e3e5ed]", "text-[#9d9ea3]",
      "bg-[#23262b]", "bg-[#23262b]/30", "font-medium", "font-bold",
      "border", "border-[#45484e]/80", "border-[#45484e]/30", "border-[#b1ccc6]/30",
      "shadow-sm"
    );

    if (isSelected) {
      // Primary selection (Target for F2/Del) - Strong highlight
      el.classList.add("text-[#e3e5ed]", "bg-[#23262b]", "font-bold", "border", "border-[#45484e]/80", "shadow-sm");
    } else if (isActive) {
      // Active file in editor - Subtle highlight
      el.classList.add("text-[#b1ccc6]", "font-medium", "border", "border-[#b1ccc6]/30");
    } else {
      // Default state
      el.classList.add("text-[#9d9ea3]");
    }
  });
}

function updateToolbarFilename(overridePath) {
  const el = document.getElementById("toolbar-filename");
  const path = overridePath || state.currentPath;
  if (el && path) {
    el.textContent = path;
  }
}

// ── Toast notification ──────────────────────────────────────────────
function showToast(message, type = "info") {
  const toast = document.createElement("div");
  const border = type === "success" ? "border-[#b1ccc6]" : "border-[#45484e]";
  toast.className = `toast px-5 py-3 bg-[#181a1d] text-[#e3e5ed] text-xs font-medium rounded-xl shadow-2xl border ${border} flex items-center gap-3 w-max`;
  const icons = { success: "check_circle", info: "info", error: "error" };
  toast.innerHTML = `<span class="material-symbols-outlined !text-[16px] ${type === "success" ? "text-[#b1ccc6]" : ""}">${icons[type] || icons.info}</span> ${message}`;
  toastContainer.appendChild(toast);

  setTimeout(() => {
    toast.classList.add("toast-out");
    setTimeout(() => toast.remove(), 300);
  }, 2000);
}

init();
