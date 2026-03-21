import { createEditor, setContent, getContent } from './editor.js';
import { openFile, saveFile, createFile, listVault, setVaultRoot, getBacklinks } from './ipc.js';
import './style.css';

const { open } = window.__TAURI__.dialog;

const state = { currentPath: null, vaultRoot: null };

const cmHost = document.getElementById('cm-host');
const fileList = document.getElementById('file-list');
const backlinksList = document.getElementById('backlinks-list');
const welcomeScreen = document.getElementById('welcome-screen');
const newFileModal = document.getElementById('new-file-modal');
const inputNewFilename = document.getElementById('new-filename-input');
const sidebar = document.getElementById('sidebar');
const backlinksPane = document.getElementById('backlinks-pane');
const shortcutsOverlay = document.getElementById('shortcuts-overlay');
const toastContainer = document.getElementById('toast-container');

const editor = createEditor(cmHost, handleSave);

function init() {
    // ── Buttons ──────────────────────────────────────────────
    document.getElementById('btn-open-folder')?.addEventListener('click', handleOpenFolder);
    document.getElementById('btn-open-welcome')?.addEventListener('click', handleOpenFolder);
    document.getElementById('btn-new-file')?.addEventListener('click', showNewFileModal);
    document.getElementById('btn-cancel-new')?.addEventListener('click', hideNewFileModal);
    document.getElementById('btn-confirm-new')?.addEventListener('click', handleCreateFile);

    // ── Panel toggles (toolbar buttons) ─────────────────────
    document.getElementById('btn-toggle-sidebar')?.addEventListener('click', toggleSidebar);
    document.getElementById('btn-toggle-backlinks')?.addEventListener('click', toggleBacklinks);

    // ── Collapse chevron buttons ─────────────────────────────
    const btnCollapseSidebar = document.getElementById('btn-collapse-sidebar');
    if (btnCollapseSidebar) btnCollapseSidebar.addEventListener('click', toggleSidebar);

    const btnCollapseBacklinks = document.getElementById('btn-collapse-backlinks');
    if (btnCollapseBacklinks) btnCollapseBacklinks.addEventListener('click', toggleBacklinks);

    // ── Shortcuts overlay close on click outside ─────────────
    shortcutsOverlay.addEventListener('click', (e) => {
        if (e.target === shortcutsOverlay) hideShortcutsOverlay();
    });

    inputNewFilename.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') handleCreateFile();
        if (e.key === 'Escape') hideNewFileModal();
    });

    // ── Global shortcuts ────────────────────────────────────
    window.addEventListener('keydown', (e) => {
        const mod = e.ctrlKey || e.metaKey;

        // Ctrl+O — Open folder
        if (mod && e.key.toLowerCase() === 'o') { e.preventDefault(); handleOpenFolder(); return; }

        // Ctrl+N — New file
        if (mod && e.key.toLowerCase() === 'n') { e.preventDefault(); showNewFileModal(); return; }

        // Ctrl+\ — Toggle sidebar
        if (mod && e.key === '\\') { e.preventDefault(); toggleSidebar(); return; }

        // Ctrl+Shift+B — Toggle backlinks
        if (mod && e.shiftKey && e.key.toLowerCase() === 'b') { e.preventDefault(); toggleBacklinks(); return; }

        // Ctrl+/ — Toggle shortcuts overlay
        if (mod && e.key === '/') { e.preventDefault(); toggleShortcutsOverlay(); return; }

        // Escape — close overlays
        if (e.key === 'Escape') {
            if (!shortcutsOverlay.classList.contains('hidden')) { hideShortcutsOverlay(); return; }
            if (!newFileModal.classList.contains('hidden')) { hideNewFileModal(); return; }
        }
    });

    // ── Wikilink clicks ─────────────────────────────────────
    cmHost.addEventListener('click', (e) => {
        const wl = e.target.closest('.cm-wikilink');
        if (wl) {
            const text = wl.textContent;
            if (text) handleOpenFile(text.trim());
        }
    });
}

// ── Panel toggles ───────────────────────────────────────────────────
function toggleSidebar() {
    sidebar.classList.toggle('collapsed');
}

function toggleBacklinks() {
    backlinksPane.classList.toggle('collapsed');
}

// ── Shortcuts overlay ───────────────────────────────────────────────
function toggleShortcutsOverlay() {
    shortcutsOverlay.classList.toggle('hidden');
}
function hideShortcutsOverlay() {
    shortcutsOverlay.classList.add('hidden');
}

// ── File ops ────────────────────────────────────────────────────────
async function handleOpenFolder() {
    try {
        const selected = await open({ directory: true, multiple: false });
        if (selected) {
            state.vaultRoot = selected;
            const entries = await setVaultRoot(selected);
            renderFileList(entries);
            welcomeScreen.classList.add('hidden');
        }
    } catch (err) { console.error('Open folder:', err); }
}

async function handleOpenFile(relativePath) {
    if (!relativePath.endsWith('.md')) relativePath += '.md';
    try {
        const content = await openFile(relativePath);
        state.currentPath = relativePath;
        welcomeScreen.classList.add('hidden');
        setContent(editor, content);
        editor.focus();
        updateSidebarSelection();
        updateToolbarFilename();
        await updateBacklinks(relativePath);
    } catch (err) { console.error('Open file:', err); }
}

async function handleSave() {
    if (!state.currentPath) return;
    try {
        await saveFile(state.currentPath, getContent(editor));
        showToast('Saved', 'success');
    } catch (err) { console.error('Save:', err); }
}

async function handleCreateFile() {
    let filename = inputNewFilename.value.trim();
    if (!filename) return;
    if (!filename.endsWith('.md')) filename += '.md';
    try {
        await createFile(filename);
        hideNewFileModal();
        const entries = await listVault();
        renderFileList(entries);
        await handleOpenFile(filename);
    } catch (err) { console.error('Create:', err); }
}

function showNewFileModal() {
    if (!state.vaultRoot) return;
    newFileModal.classList.remove('hidden');
    setTimeout(() => inputNewFilename.focus(), 50);
}

function hideNewFileModal() {
    newFileModal.classList.add('hidden');
    inputNewFilename.value = '';
}

async function updateBacklinks(path) {
    try {
        const links = await getBacklinks(path);
        backlinksList.innerHTML = '';
        if (!links.length) {
            backlinksList.innerHTML = '<div class="empty-state">No backlinks</div>';
            return;
        }
        links.forEach((absPath, idx) => {
            const rel = state.vaultRoot ? absPath.replace(state.vaultRoot, '').replace(/^[\\/]/, '') : absPath;
            const el = document.createElement('div');
            el.className = 'backlink-item flex flex-col gap-1 p-3 bg-[#23262b]/50 hover:bg-[#23262b] border border-[#45484e]/30 rounded-xl transition-all cursor-pointer group mb-3';
            el.innerHTML = `<span class="text-on-surface text-[11px] font-medium tracking-normal normal-case group-hover:text-primary transition-colors">${rel}</span>`;
            el.addEventListener('click', () => handleOpenFile(rel));
            backlinksList.appendChild(el);
        });
    } catch (err) { console.error('Backlinks:', err); }
}

// ── UI helpers ──────────────────────────────────────────────────────
function buildTree(entries) {
    const root = { children: {} };
    for (const entry of entries) {
        const parts = entry.path.split('/');
        let current = root;
        for (let i = 0; i < parts.length; i++) {
            const part = parts[i];
            if (!current.children[part]) {
                current.children[part] = {
                    name: part,
                    path: parts.slice(0, i + 1).join('/'),
                    is_dir: i < parts.length - 1 || entry.is_dir,
                    children: {}
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
        if (child.is_dir) {
            const details = document.createElement('details');
            details.className = 'tree-dir mb-1';
            if (level === 0) details.open = true;

            const summary = document.createElement('summary');
            summary.className = `file-item flex items-center gap-3 p-2 text-[#9d9ea3] hover:text-[#b1ccc6] hover:bg-[#23262b] rounded-lg transition-all cursor-pointer text-xs whitespace-nowrap outline-none select-none`;
            summary.setAttribute('data-path', child.path);
            summary.style.paddingLeft = `${8 + level * 12}px`;
            summary.innerHTML = `<span class="material-symbols-outlined !text-[14px]">folder</span> ${child.name}`;
            
            const childrenContainer = document.createElement('div');
            childrenContainer.className = 'tree-children';
            renderTreeNodes(child, childrenContainer, level + 1);
            
            details.appendChild(summary);
            details.appendChild(childrenContainer);
            container.appendChild(details);
        } else {
            const el = document.createElement('div');
            const isActive = child.path === state.currentPath;
            el.className = `file-item flex items-center gap-3 p-2 rounded-lg transition-all cursor-pointer text-xs whitespace-nowrap mb-1 outline-none select-none ${isActive ? 'text-[#b1ccc6] bg-[#23262b] font-medium border border-[#45484e]/50 shadow-sm' : 'text-[#9d9ea3] hover:text-[#b1ccc6] hover:bg-[#23262b]/50'}`;
            el.setAttribute('data-path', child.path);
            el.style.paddingLeft = `${8 + level * 12}px`;
            el.innerHTML = `<span class="material-symbols-outlined !text-[14px]">draft</span> ${child.name}`;
            el.addEventListener('click', () => handleOpenFile(child.path));
            container.appendChild(el);
        }
    });
}

function renderFileList(entries) {
    fileList.innerHTML = '';
    if (!entries.length) {
        fileList.innerHTML = '<div class="empty-state">No markdown files</div>';
        return;
    }
    const tree = buildTree(entries);
    renderTreeNodes(tree, fileList, 0);
}

function updateSidebarSelection() {
    document.querySelectorAll('.file-item').forEach(el => {
        const isActive = el.getAttribute('data-path') === state.currentPath;
        if (!el.matches('summary')) {
            el.className = `file-item flex items-center gap-3 p-2 rounded-lg transition-all cursor-pointer text-xs whitespace-nowrap mb-1 outline-none select-none ${isActive ? 'text-[#b1ccc6] bg-[#23262b] font-medium border border-[#45484e]/50 shadow-sm' : 'text-[#9d9ea3] hover:text-[#b1ccc6] hover:bg-[#23262b]/50'}`;
        }
    });
}

function updateToolbarFilename() {
    const el = document.getElementById('toolbar-filename');
    if (el && state.currentPath) {
        el.textContent = state.currentPath;
    }
}

// ── Toast notification ──────────────────────────────────────────────
function showToast(message, type = 'info') {
    const toast = document.createElement('div');
    const border = type === 'success' ? 'border-[#b1ccc6]' : 'border-[#45484e]';
    toast.className = `toast px-5 py-3 bg-[#181a1d] text-[#e3e5ed] text-xs font-medium rounded-xl shadow-2xl border ${border} flex items-center gap-3 w-max`;
    const icons = { success: 'check_circle', info: 'info', error: 'error' };
    toast.innerHTML = `<span class="material-symbols-outlined !text-[16px] ${type === 'success' ? 'text-[#b1ccc6]' : ''}">${icons[type] || icons.info}</span> ${message}`;
    toastContainer.appendChild(toast);

    setTimeout(() => {
        toast.classList.add('toast-out');
        setTimeout(() => toast.remove(), 300);
    }, 2000);
}

init();
