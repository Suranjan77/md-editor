import {
  createEditor,
  setContent,
  getContent,
  hasFocus,
  setCurrentFilePath,
  scrollToId,
} from "./editor.js";
import Split from "split.js";
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
import {
  initCommandPalette,
  setCommandPaletteCommands,
  showCommandPalette,
  hideCommandPalette,
  toggleCommandPalette,
  isCommandPaletteOpen,
} from "./command-palette.js";

const { open, ask } = window.__TAURI__.dialog;

const state = {
  currentPath: null,
  activeMdPath: null,
  activePdfPath: null,
  vaultRoot: null,
  selectedSidebarPath: null, // Track which sidebar item is selected
  selectedSidebarIsDir: false, // Track if selection is a folder
  modalType: null, // 'file', 'folder', 'rename'
  modalTarget: null,
  isTrackerOpen: false,
  isSplitView: false,
  splitInstance: null,
  focusModePinned: false,
  panelSnapshot: null,
};

const cmHost = document.getElementById("cm-host");
const trackerHost = document.getElementById("tracker-host");
const pdfViewerHost = document.getElementById("pdf-viewer-host");
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
const vaultLabel = document.getElementById("vault-label");
const vaultPathEl = document.getElementById("vault-path");
const toolbarStatus = document.getElementById("toolbar-status");
const backlinksCount = document.getElementById("backlinks-count");
const btnToggleSidebar = document.getElementById("btn-toggle-sidebar");
const btnToggleBacklinks = document.getElementById("btn-toggle-backlinks");
const btnOpenTracker = document.getElementById("btn-open-tracker");
const btnVaultSearch = document.getElementById("btn-vault-search");
const btnShortcuts = document.getElementById("btn-shortcuts");
const btnFocusMode = document.getElementById("btn-focus-mode");
const appShell = document.getElementById("app");

const editor = createEditor(cmHost, handleSave);

let trackerModule = null;
let pdfViewerModule = null;

async function getTrackerModule() {
  if (!trackerModule) {
    trackerModule = await import("./tracker.js");
    trackerModule.initTracker(trackerHost);
  }
  return trackerModule;
}

async function getPdfViewerModule() {
  if (!pdfViewerModule) {
    pdfViewerModule = await import("./pdf-viewer.js");
    pdfViewerModule.initPdfViewer(pdfViewerHost);
  }
  return pdfViewerModule;
}

async function closeActivePdfViewer() {
  if (!pdfViewerModule || !pdfViewerModule.isPdfViewerActive()) return;
  await pdfViewerModule.closePdfViewer();
}

const IMAGE_EXTENSIONS = ["jpeg", "jpg", "png", "svg", "webp", "avif"];
function isImageFile(filename) {
  const ext = filename.split(".").pop().toLowerCase();
  return IMAGE_EXTENSIONS.includes(ext);
}

function isPdfFile(filename) {
  return filename.split(".").pop().toLowerCase() === "pdf";
}

async function init() {
  try {
    const lastRoot = await getSysConfig("last_vault_root");
    if (lastRoot) {
      state.vaultRoot = lastRoot;
      updateVaultChrome(lastRoot);
      const entries = await setVaultRoot(lastRoot);
      renderFileList(entries);
      welcomeScreen.classList.add("hidden");

      const lastFile = await getSysConfig("last_file");
      if (lastFile) {
        if (isPdfFile(lastFile)) {
          await handleOpenPdfFile(lastFile);
        } else if (isImageFile(lastFile)) {
          await handleOpenImageFile(lastFile);
        } else {
          await handleOpenMdFile(lastFile);
        }
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
    .getElementById("btn-open-tracker")
    ?.addEventListener("click", handleOpenTracker);
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
  document
    .getElementById("btn-split-view")
    ?.addEventListener("click", toggleSplitView);
  btnVaultSearch?.addEventListener("click", showSearchOverlay);
  btnShortcuts?.addEventListener("click", toggleShortcutsOverlay);
  btnFocusMode?.addEventListener("click", toggleFocusModePinned);

  initCommandPalette({ onExecute: runCommand });
  registerCommands();

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
  window.addEventListener(
    "keydown",
    (e) => {
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

      // Ctrl+P — Command palette
      if (mod && e.key.toLowerCase() === "p") {
        e.preventDefault();
        e.stopPropagation();
        toggleCommandPalette();
        return;
      }

      // Ctrl+Shift+E — Focus mode
      if (mod && e.shiftKey && e.key.toLowerCase() === "e") {
        e.preventDefault();
        e.stopPropagation();
        toggleFocusModePinned();
        return;
      }

      // Delete — Delete selected file/folder
      if (e.key === "Delete" && state.selectedSidebarPath) {
        const isInputFocused = ["INPUT", "TEXTAREA"].includes(
          document.activeElement.tagName,
        );

        if (!isInputFocused && !hasFocus(editor)) {
          e.preventDefault();
          e.stopPropagation();
          handleDelete(state.selectedSidebarPath);
          return;
        }
      }

      // F2 — Rename selected file/folder
      if (e.key === "F2" && state.selectedSidebarPath) {
        const isInputFocused = ["INPUT", "TEXTAREA"].includes(
          document.activeElement.tagName,
        );

        if (!isInputFocused) {
          e.preventDefault();
          e.stopPropagation();
          showNameModal("rename", state.selectedSidebarPath);
          return;
        }
      }

      // Escape — close overlays
      if (e.key === "Escape") {
        if (isCommandPaletteOpen()) {
          e.preventDefault();
          e.stopPropagation();
          hideCommandPalette();
          return;
        }
        if (state.focusModePinned) {
          e.preventDefault();
          e.stopPropagation();
          exitFocusMode(true);
          return;
        }
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
    },
    true,
  ); // Use capture phase

  // ── Wikilink clicks ─────────────────────────────────────
  cmHost.addEventListener("click", (e) => {
    const wl = e.target.closest(".cm-wikilink");
    if (wl) {
      let text = wl.textContent;
      if (text) {
        text = text.split("|")[0].trim(); // Strip alias from [[page|alias]]
        
        // Handle internal links
        if (text.startsWith("#")) {
          const id = text.slice(1);
          const found = scrollToId(editor, id);
          if (!found) {
            showToast(`Target "${text}" not found in document`, "error");
          }
          return;
        }

        handleOpenMdFile(text);
      }
    }
  });
}

// ── Panel toggles ───────────────────────────────────────────────────
function syncPanelToggleButtons() {
  const sidebarOpen = !sidebar.classList.contains("collapsed");
  const backlinksOpen = !backlinksPane.classList.contains("collapsed");
  btnToggleSidebar?.classList.toggle("is-active", sidebarOpen);
  btnToggleSidebar?.setAttribute("aria-pressed", String(sidebarOpen));
  btnToggleBacklinks?.classList.toggle("is-active", backlinksOpen);
  btnToggleBacklinks?.setAttribute("aria-pressed", String(backlinksOpen));
}

function toggleSidebar() {
  if (state.focusModePinned) exitFocusMode(true);
  sidebar.classList.toggle("collapsed");
  syncPanelToggleButtons();
}

function toggleBacklinks() {
  if (state.focusModePinned) exitFocusMode(true);
  backlinksPane.classList.toggle("collapsed");
  syncPanelToggleButtons();
}

function snapshotPanels() {
  return {
    sidebarCollapsed: sidebar.classList.contains("collapsed"),
    backlinksCollapsed: backlinksPane.classList.contains("collapsed"),
  };
}

function applyFocusModePanels() {
  sidebar.classList.add("collapsed");
  backlinksPane.classList.add("collapsed");
  appShell?.classList.add("focus-mode");
  btnFocusMode?.classList.add("is-active");
  btnFocusMode?.setAttribute("aria-pressed", "true");
  syncPanelToggleButtons();
}

function restoreFocusModePanels(snapshot) {
  if (!snapshot) return;
  sidebar.classList.toggle("collapsed", snapshot.sidebarCollapsed);
  backlinksPane.classList.toggle("collapsed", snapshot.backlinksCollapsed);
  appShell?.classList.remove("focus-mode");
  btnFocusMode?.classList.remove("is-active");
  btnFocusMode?.setAttribute("aria-pressed", "false");
  syncPanelToggleButtons();
}

function exitFocusMode(clearPinned = false) {
  if (clearPinned) state.focusModePinned = false;
  if (state.panelSnapshot) {
    restoreFocusModePanels(state.panelSnapshot);
    state.panelSnapshot = null;
  } else {
    appShell?.classList.remove("focus-mode");
    btnFocusMode?.classList.remove("is-active");
    btnFocusMode?.setAttribute("aria-pressed", "false");
    syncPanelToggleButtons();
  }
}

function toggleFocusModePinned() {
  if (state.focusModePinned) {
    exitFocusMode(true);
    return;
  }
  state.focusModePinned = true;
  if (!state.panelSnapshot) state.panelSnapshot = snapshotPanels();
  applyFocusModePanels();
  editor.focus();
}

function registerCommands() {
  setCommandPaletteCommands([
    {
      id: "open-vault",
      label: "Open vault",
      icon: "folder_open",
      shortcut: "Ctrl+O",
      keywords: "folder workspace",
    },
    {
      id: "new-file",
      label: "New file",
      icon: "note_add",
      shortcut: "Ctrl+N",
      keywords: "create note markdown",
      when: () => !!state.vaultRoot,
    },
    {
      id: "new-folder",
      label: "New folder",
      icon: "create_new_folder",
      keywords: "create directory",
      when: () => !!state.vaultRoot,
    },
    {
      id: "search",
      label: "Search vault",
      icon: "search",
      shortcut: "Ctrl+Shift+F",
      keywords: "find",
      when: () => !!state.vaultRoot,
    },
    {
      id: "tracker",
      label: "Study tracker",
      icon: "school",
      keywords: "curriculum progress",
    },
    {
      id: "toggle-sidebar",
      label: "Toggle sidebar",
      icon: "side_navigation",
      shortcut: "Ctrl+\\",
    },
    {
      id: "toggle-backlinks",
      label: "Toggle backlinks",
      icon: "link",
      shortcut: "Ctrl+Shift+B",
      when: () => !!state.currentPath && state.currentPath.endsWith(".md"),
    },
    {
      id: "focus-mode",
      label: "Focus mode",
      icon: "center_focus_strong",
      shortcut: "Ctrl+Shift+E",
    },
    {
      id: "shortcuts",
      label: "Keyboard shortcuts",
      icon: "keyboard",
      shortcut: "Ctrl+/",
    },
  ]);
}

function runCommand(id) {
  switch (id) {
    case "open-vault":
      handleOpenFolder();
      break;
    case "new-file":
      showNameModal("file");
      break;
    case "new-folder":
      showNameModal("folder");
      break;
    case "search":
      showSearchOverlay();
      break;
    case "tracker":
      handleOpenTracker();
      break;
    case "toggle-sidebar":
      toggleSidebar();
      break;
    case "toggle-backlinks":
      toggleBacklinks();
      break;
    case "focus-mode":
      toggleFocusModePinned();
      break;
    case "shortcuts":
      toggleShortcutsOverlay();
      break;
    default:
      break;
  }
}

function setTrackerNavActive(active) {
  btnOpenTracker?.classList.toggle("is-active", active);
}

function updateVaultChrome(rootPath) {
  if (!vaultLabel || !vaultPathEl) return;
  if (!rootPath) {
    vaultLabel.textContent = "MD Editor";
    vaultPathEl.textContent = "No vault open";
    return;
  }
  const normalized = rootPath.replace(/\\/g, "/");
  const name = normalized.split("/").filter(Boolean).pop() || normalized;
  vaultLabel.textContent = name;
  vaultPathEl.textContent = normalized;
  vaultPathEl.title = normalized;
}

function toggleSplitView() {
  state.isSplitView = !state.isSplitView;
  const btn = document.getElementById("btn-split-view");
  if (state.isSplitView) {
    btn?.classList.add("is-active");
    
    // Show both
    cmHost.classList.remove("hidden");
    pdfViewerHost.classList.remove("hidden");
    
    // Hide others
    imagePreview.classList.add("hidden");
    trackerHost.classList.add("hidden");
    welcomeScreen.classList.add("hidden");
    
    // Init split
    state.splitInstance = Split(['#cm-host', '#pdf-viewer-host'], {
      sizes: [50, 50],
      minSize: 200,
      gutterSize: 4,
      cursor: 'col-resize'
    });
  } else {
    btn?.classList.remove("is-active");
    
    if (state.splitInstance) {
      state.splitInstance.destroy();
      state.splitInstance = null;
    }
    
    // Clean up inline widths
    cmHost.style.width = '';
    pdfViewerHost.style.width = '';
    
    if (state.currentPath && isPdfFile(state.currentPath)) {
      cmHost.classList.add("hidden");
    } else {
      pdfViewerHost.classList.add("hidden");
    }
  }
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
  const query = searchInput.value.trim();
  if (query.length < 2) {
    searchResults.innerHTML =
      '<div class="empty-state">Type at least 2 characters…</div>';
    return;
  }
  if (results.length === 0) {
    searchResults.innerHTML =
      '<div class="empty-state">No results found.</div>';
    return;
  }

  results.forEach((res) => {
    const item = document.createElement("div");
    item.className = "search-result-item";
    item.innerHTML = `
      <div class="flex justify-between items-center gap-2">
        <span class="search-result-path">${res.path}</span>
        <span class="search-result-meta">Line ${res.line}</span>
      </div>
      <div class="search-result-context">${res.context}</div>
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
      updateVaultChrome(selected);
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
    state.activeMdPath = relativePath;
    state.selectedSidebarPath = relativePath; // Sync selection with open file
    state.selectedSidebarIsDir = false;
    state.isTrackerOpen = false;
    setTrackerNavActive(false);
    await setSysConfig("last_file", relativePath);
    // Close PDF if it was open, UNLESS split view is active
    if (!state.isSplitView) {
      await closeActivePdfViewer();
      pdfViewerHost.classList.add("hidden");
    }
    // Hide all other views, show editor
    imagePreview.classList.add("hidden");
    cmHost.classList.remove("hidden");
    welcomeScreen.classList.add("hidden");
    trackerHost.classList.add("hidden");
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

async function handleOpenTracker() {
  exitFocusMode(true);
  state.isTrackerOpen = true;
  state.selectedSidebarPath = null;
  state.currentPath = "Study Tracker";
  // Close PDF if it was open
  await closeActivePdfViewer();
  welcomeScreen.classList.add("hidden");
  cmHost.classList.add("hidden");
  pdfViewerHost.classList.add("hidden");
  imagePreview.classList.add("hidden");
  trackerHost.classList.remove("hidden");
  setTrackerNavActive(true);
  updateSidebarSelection();
  updateToolbarFilename();

  // Render the tracker view whenever opened
  const tracker = await getTrackerModule();
  await tracker.renderTracker();
}

async function handleOpenPdfFile(relativePath) {
  try {
    exitFocusMode(true);
    state.currentPath = relativePath;
    state.activePdfPath = relativePath;
    state.selectedSidebarPath = relativePath;
    state.selectedSidebarIsDir = false;
    state.isTrackerOpen = false;
    setTrackerNavActive(false);
    await setSysConfig("last_file", relativePath);

    // Hide all other views, show PDF viewer
    if (!state.isSplitView) {
      cmHost.classList.add("hidden");
    }
    imagePreview.classList.add("hidden");
    trackerHost.classList.add("hidden");
    welcomeScreen.classList.add("hidden");
    pdfViewerHost.classList.remove("hidden");

    updateSidebarSelection();
    updateToolbarFilename();

    // Open the PDF in the viewer
    const pdfViewer = await getPdfViewerModule();
    await pdfViewer.openPdfFile(relativePath);
    pdfViewerHost.focus();
  } catch (err) {
    console.error("Open PDF:", err);
    showToast(`Failed to open PDF: ${err}`, "error");
  }
}

async function handleOpenImageFile(relativePath) {
  try {
    exitFocusMode(true);
    const url = `md-img://localhost/${relativePath}`;

    state.currentPath = null; // Not an editable file
    state.selectedSidebarPath = relativePath;
    state.selectedSidebarIsDir = false;
    state.isTrackerOpen = false;
    setTrackerNavActive(false);
    await setSysConfig("last_file", relativePath);

    // Close PDF if it was open
    await closeActivePdfViewer();

    // Hide all other views, show image preview
    cmHost.classList.add("hidden");
    welcomeScreen.classList.add("hidden");
    pdfViewerHost.classList.add("hidden");
    trackerHost.classList.add("hidden");
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
  if (!state.activeMdPath) return;
  try {
    await saveFile(state.activeMdPath, getContent(editor));
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
    }
    if (state.activeMdPath === oldPath) {
      state.activeMdPath = newPath;
    }
    if (state.activePdfPath === oldPath) {
      state.activePdfPath = newPath;
    }
    updateToolbarFilename();
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
    let changed = false;
    if (
      state.currentPath &&
      (state.currentPath === path || state.currentPath.startsWith(path + "/"))
    ) {
      state.currentPath = null;
      changed = true;
    }
    if (
      state.activeMdPath &&
      (state.activeMdPath === path || state.activeMdPath.startsWith(path + "/"))
    ) {
      state.activeMdPath = null;
      setContent(editor, "");
      changed = true;
    }
    if (
      state.activePdfPath &&
      (state.activePdfPath === path || state.activePdfPath.startsWith(path + "/"))
    ) {
      state.activePdfPath = null;
      changed = true;
    }

    if (changed) {
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

function toVaultRelative(absPath) {
  if (!state.vaultRoot) return absPath;
  const root = state.vaultRoot.replace(/\\/g, "/").replace(/\/$/, "");
  const abs = absPath.replace(/\\/g, "/");
  if (abs.startsWith(`${root}/`)) return abs.slice(root.length + 1);
  return absPath;
}

async function updateBacklinks(path) {
  try {
    const links = await getBacklinks(path);
    backlinksList.innerHTML = "";
    if (backlinksCount) {
      if (links.length) {
        backlinksCount.textContent = String(links.length);
        backlinksCount.classList.remove("hidden");
      } else {
        backlinksCount.classList.add("hidden");
      }
    }
    if (!links.length) {
      backlinksList.innerHTML =
        '<div class="empty-state backlinks-empty">Links to this note appear here.</div>';
      return;
    }
    links.forEach((absPath) => {
      const rel = toVaultRelative(absPath);
      const el = document.createElement("button");
      el.type = "button";
      el.className = "backlink-item";
      el.textContent = rel;
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
      btn.className =
        "sidebar-delete-btn material-symbols-outlined flex-shrink-0";
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
      const iconName = isPdfFile(child.name) ? "picture_as_pdf" : isImageFile(child.name) ? "image" : "draft";
      label.innerHTML = `<span class="material-symbols-outlined !text-[14px]">${iconName}</span> <span class="truncate">${child.name}</span>`;
      el.appendChild(label);
      el.appendChild(createDeleteBtn(child.path));

      el.addEventListener("click", (e) => {
        if (isPdfFile(child.path)) {
          handleOpenPdfFile(child.path);
        } else if (isImageFile(child.path)) {
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
    fileList.innerHTML =
      '<div class="empty-state sidebar-empty">No files yet — use New file above.</div>';
    return;
  }
  const tree = buildTree(entries);
  renderTreeNodes(tree, fileList, 0);
  updateSidebarSelection();
}

function updateSidebarSelection() {
  document.querySelectorAll(".file-item").forEach((el) => {
    const path = el.getAttribute("data-path");
    const isActive = path === state.activeMdPath || path === state.activePdfPath;
    const isSelected = path === state.selectedSidebarPath;

    // Reset classes first
    el.classList.remove(
      "text-[#b1ccc6]",
      "text-[#e3e5ed]",
      "text-[#9d9ea3]",
      "bg-[#23262b]",
      "bg-[#23262b]/30",
      "font-medium",
      "font-bold",
      "border",
      "border-[#45484e]/80",
      "border-[#45484e]/30",
      "border-[#b1ccc6]/30",
      "shadow-sm",
    );

    if (isSelected) {
      // Primary selection (Target for F2/Del) - Strong highlight
      el.classList.add(
        "text-[#e3e5ed]",
        "bg-[#23262b]",
        "font-bold",
        "border",
        "border-[#45484e]/80",
        "shadow-sm",
      );
    } else if (isActive) {
      // Active file in editor - Subtle highlight
      el.classList.add(
        "text-[#b1ccc6]",
        "font-medium",
        "border",
        "border-[#b1ccc6]/30",
      );
    } else {
      // Default state
      el.classList.add("text-[#9d9ea3]");
    }
  });
}

function updateToolbarFilename(overridePath) {
  const el = document.getElementById("toolbar-filename");
  const path = overridePath || state.currentPath;

  if (!el) return;

  if (!path) {
    el.textContent = "No file open";
    el.title = "No file open";
    if (toolbarStatus) toolbarStatus.textContent = "";
    return;
  }

  const normalizedPath = path.replace(/\\/g, "/");
  const segments = normalizedPath.split("/").filter(Boolean);
  const filename = segments[segments.length - 1] || normalizedPath;
  const parent = segments.length > 1 ? segments.slice(0, -1).join("/") : "";

  el.textContent = filename;
  el.title = normalizedPath;
  if (toolbarStatus) {
    toolbarStatus.textContent =
      state.isTrackerOpen
        ? "Study tracker"
        : parent || (state.vaultRoot ? "Vault root" : "");
  }
}

// ── Toast notification ──────────────────────────────────────────────
function showToast(message, type = "info") {
  const toast = document.createElement("div");
  toast.className = `toast${type === "success" ? " toast--success" : ""}`;
  const icons = { success: "check_circle", info: "info", error: "error" };
  toast.innerHTML = `<span class="material-symbols-outlined" style="font-size:16px">${icons[type] || icons.info}</span> ${message}`;
  toastContainer.appendChild(toast);

  setTimeout(() => {
    toast.classList.add("toast-out");
    setTimeout(() => toast.remove(), 300);
  }, 2000);
}

updateToolbarFilename();
syncPanelToggleButtons();

init();
