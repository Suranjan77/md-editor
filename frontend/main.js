import { openFile, listVault, setVaultRoot, saveFile, createFile, getBacklinks } from './ipc.js';
import { setupInputHandling, forceSync } from './input.js';
import { setupUndoShortcuts } from './undo.js';
import { renderMarkdownToHTML } from './renderer.js';
import { saveCursorPosition, restoreCursorPosition, byteOffsetToLineCol } from './cursor.js';

const { open } = window.__TAURI__.dialog;

// ── App State ───────────────────────────────────────────────────────
const state = {
    currentPath: null,
    vaultRoot: null,
    fullText: '',
};

const editor = document.getElementById('editor');
const fileList = document.getElementById('file-list');
const backlinksList = document.getElementById('backlinks-list');
const welcomeScreen = document.getElementById('welcome-screen');
const newFileModal = document.getElementById('new-file-modal');
const inputNewFilename = document.getElementById('new-filename-input');

function init() {
    setupInputHandling(editor, () => state.fullText, handleSyncResponse);
    setupUndoShortcuts(handleFullResponse);

    document.getElementById('btn-open-folder').addEventListener('click', handleOpenFolder);
    document.getElementById('btn-open-welcome').addEventListener('click', handleOpenFolder);
    document.getElementById('btn-new-file').addEventListener('click', () => {
        newFileModal.classList.remove('hidden');
        setTimeout(() => inputNewFilename.focus(), 50);
    });
    document.getElementById('btn-cancel-new').addEventListener('click', () => {
        newFileModal.classList.add('hidden');
        inputNewFilename.value = '';
    });
    document.getElementById('btn-confirm-new').addEventListener('click', handleCreateFile);
    inputNewFilename.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') handleCreateFile();
        if (e.key === 'Escape') { newFileModal.classList.add('hidden'); inputNewFilename.value = ''; }
    });

    window.addEventListener('keydown', async (e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
            e.preventDefault();
            if (state.currentPath) {
                await forceSync(editor, () => state.fullText, handleSyncResponse);
                try { await saveFile(); flashEditor(); } catch (err) { console.error('Save failed:', err); }
            }
        }
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o') { e.preventDefault(); handleOpenFolder(); }
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
            e.preventDefault();
            if (state.vaultRoot) {
                newFileModal.classList.remove('hidden');
                setTimeout(() => inputNewFilename.focus(), 50);
            }
        }
    });

    editor.addEventListener('click', (e) => {
        if (e.target.classList.contains('wikilink')) {
            e.preventDefault();
            const target = e.target.getAttribute('data-target');
            if (target) handleOpenFile(target);
        }
    });
}

// ── Re-render with cursor preservation ──────────────────────────────
function rerender(cursorPos) {
    const saved = cursorPos || saveCursorPosition(editor);
    editor.innerHTML = renderMarkdownToHTML(state.fullText);
    requestAnimationFrame(() => {
        restoreCursorPosition(editor, saved);
    });
}

/**
 * After debounced sync: update stored text and re-render.
 */
function handleSyncResponse(response) {
    state.fullText = response.full_text;
    const saved = saveCursorPosition(editor);
    editor.innerHTML = renderMarkdownToHTML(state.fullText);
    requestAnimationFrame(() => {
        restoreCursorPosition(editor, saved);
    });
}

/**
 * After undo/redo or file open: full re-render with cursor from backend.
 */
function handleFullResponse(response) {
    state.fullText = response.full_text;
    const pos = byteOffsetToLineCol(state.fullText, response.cursor_byte_offset);
    editor.innerHTML = renderMarkdownToHTML(state.fullText);
    requestAnimationFrame(() => {
        restoreCursorPosition(editor, pos);
    });
}

// ── File Operations ─────────────────────────────────────────────────
async function handleOpenFolder() {
    try {
        const selected = await open({ directory: true, multiple: false });
        if (selected) {
            state.vaultRoot = selected;
            const entries = await setVaultRoot(selected);
            renderFileList(entries);
            welcomeScreen.classList.add('hidden');
        }
    } catch (err) { console.error('Failed to open folder:', err); }
}

async function handleOpenFile(relativePath) {
    if (!relativePath.endsWith('.md')) relativePath += '.md';
    try {
        const response = await openFile(relativePath);
        state.currentPath = relativePath;
        state.fullText = response.full_text;
        welcomeScreen.classList.add('hidden');
        rerender({ line: 0, column: 0 });
        editor.focus();
        updateSidebarSelection();
        await updateBacklinks(relativePath);
    } catch (err) { console.error('Failed to open file:', err); }
}

async function handleCreateFile() {
    let filename = inputNewFilename.value.trim();
    if (!filename) return;
    if (!filename.endsWith('.md')) filename += '.md';
    try {
        await createFile(filename);
        newFileModal.classList.add('hidden');
        inputNewFilename.value = '';
        const entries = await listVault();
        renderFileList(entries);
        await handleOpenFile(filename);
    } catch (err) { console.error('Failed to create file:', err); }
}

async function updateBacklinks(activePath) {
    try {
        const links = await getBacklinks(activePath);
        backlinksList.innerHTML = '';
        if (links.length === 0) { backlinksList.innerHTML = '<div class="empty-state">No backlinks</div>'; return; }
        links.forEach(absPath => {
            const relPath = state.vaultRoot ? absPath.replace(state.vaultRoot, '').replace(/^[\\/]/, '') : absPath;
            const el = document.createElement('div');
            el.className = 'backlink-item';
            el.innerHTML = `<span class="icon">🔗</span> ${relPath}`;
            el.addEventListener('click', () => handleOpenFile(relPath));
            backlinksList.appendChild(el);
        });
    } catch (err) { console.error('Failed to get backlinks:', err); }
}

function renderFileList(entries) {
    fileList.innerHTML = '';
    if (entries.length === 0) { fileList.innerHTML = '<div class="empty-state">No markdown files found</div>'; return; }
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

function flashEditor() {
    editor.style.opacity = '0.7';
    setTimeout(() => editor.style.opacity = '1', 150);
}

document.addEventListener('DOMContentLoaded', init);
