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

const editor = createEditor(cmHost, handleSave);

function init() {
    // ── Buttons ──────────────────────────────────────────────
    document.getElementById('btn-open-folder').addEventListener('click', handleOpenFolder);
    document.getElementById('btn-open-welcome').addEventListener('click', handleOpenFolder);
    document.getElementById('btn-new-file').addEventListener('click', showNewFileModal);
    document.getElementById('btn-cancel-new').addEventListener('click', hideNewFileModal);
    document.getElementById('btn-confirm-new').addEventListener('click', handleCreateFile);

    // ── Panel toggles ───────────────────────────────────────
    document.getElementById('btn-toggle-sidebar').addEventListener('click', () => {
        sidebar.classList.toggle('collapsed');
    });
    document.getElementById('btn-toggle-backlinks').addEventListener('click', () => {
        backlinksPane.classList.toggle('collapsed');
    });

    inputNewFilename.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') handleCreateFile();
        if (e.key === 'Escape') hideNewFileModal();
    });

    // ── Global shortcuts ────────────────────────────────────
    window.addEventListener('keydown', (e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o') { e.preventDefault(); handleOpenFolder(); }
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') { e.preventDefault(); showNewFileModal(); }
        if ((e.ctrlKey || e.metaKey) && e.key === '\\') { e.preventDefault(); sidebar.classList.toggle('collapsed'); }
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
        await updateBacklinks(relativePath);
    } catch (err) { console.error('Open file:', err); }
}

async function handleSave() {
    if (!state.currentPath) return;
    try {
        await saveFile(state.currentPath, getContent(editor));
        flashSave();
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
        if (!links.length) { backlinksList.innerHTML = '<div class="empty-state">No backlinks</div>'; return; }
        links.forEach(absPath => {
            const rel = state.vaultRoot ? absPath.replace(state.vaultRoot, '').replace(/^[\\/]/, '') : absPath;
            const el = document.createElement('div');
            el.className = 'backlink-item';
            el.innerHTML = `<span class="icon">🔗</span> ${rel}`;
            el.addEventListener('click', () => handleOpenFile(rel));
            backlinksList.appendChild(el);
        });
    } catch (err) { console.error('Backlinks:', err); }
}

// ── UI helpers ──────────────────────────────────────────────────────
function renderFileList(entries) {
    fileList.innerHTML = '';
    if (!entries.length) { fileList.innerHTML = '<div class="empty-state">No markdown files</div>'; return; }
    entries.forEach(entry => {
        const el = document.createElement('div');
        el.className = `file-item ${entry.path === state.currentPath ? 'active' : ''}`;
        el.setAttribute('data-path', entry.path);
        el.innerHTML = `<span class="icon">${entry.is_dir ? '📁' : '📝'}</span> ${entry.name}`;
        if (!entry.is_dir) el.addEventListener('click', () => handleOpenFile(entry.path));
        fileList.appendChild(el);
    });
}

function updateSidebarSelection() {
    document.querySelectorAll('.file-item').forEach(el => {
        el.classList.toggle('active', el.getAttribute('data-path') === state.currentPath);
    });
}

function flashSave() {
    const el = cmHost.querySelector('.cm-content');
    if (el) { el.style.opacity = '0.7'; setTimeout(() => el.style.opacity = '1', 200); }
}

document.addEventListener('DOMContentLoaded', init);
