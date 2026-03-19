/**
 * Input handling for the WYSIWYG editor.
 * Browser handles text natively. We debounce-sync changes to the backend.
 * After sync, re-render to apply markdown styling.
 */
import { applyEdit } from './ipc.js';

const encoder = new TextEncoder();

/**
 * @param {HTMLElement} editor
 * @param {function} getStoredText - () => string
 * @param {function} onSyncComplete - (response) => void
 */
export function setupInputHandling(editor, getStoredText, onSyncComplete) {
    let syncTimeout = null;

    editor.addEventListener('input', () => {
        if (syncTimeout) clearTimeout(syncTimeout);
        // 300ms debounce — enough time for the user to finish a thought
        syncTimeout = setTimeout(() => syncEditor(editor, getStoredText, onSyncComplete), 300);
    });
}

export async function forceSync(editor, getStoredText, onSyncComplete) {
    await syncEditor(editor, getStoredText, onSyncComplete);
}

async function syncEditor(editor, getStoredText, onSyncComplete) {
    const oldText = getStoredText();
    const newText = extractText(editor);

    if (newText === oldText) return;

    const { byteOffset, deleteLen, insertText } = computeMinimalEdit(oldText, newText);
    if (deleteLen === 0 && insertText.length === 0) return;

    try {
        const response = await applyEdit({
            byte_offset: byteOffset,
            delete_length: deleteLen,
            insert_text: insertText,
            cursor_byte_offset: byteOffset + encoder.encode(insertText).length,
        });
        onSyncComplete(response);
    } catch (err) {
        console.error('Sync failed:', err);
    }
}

/**
 * Extract text from the editor.
 * Each direct child element (div) = one line, joined by \n.
 */
function extractText(editor) {
    const children = editor.children;
    if (children.length === 0) return editor.textContent || '';

    const lines = [];
    for (const child of children) {
        const text = child.textContent || '';
        // A div with only <br> and no text = empty line
        if (child.querySelector('br') && text.trim() === '' && !child.textContent.trim()) {
            lines.push('');
        } else {
            lines.push(text);
        }
    }
    return lines.join('\n');
}

function computeMinimalEdit(oldText, newText) {
    let prefixLen = 0;
    const minLen = Math.min(oldText.length, newText.length);
    while (prefixLen < minLen && oldText[prefixLen] === newText[prefixLen]) {
        prefixLen++;
    }

    let oldSuffix = 0;
    let newSuffix = 0;
    while (
        oldSuffix < (oldText.length - prefixLen) &&
        newSuffix < (newText.length - prefixLen) &&
        oldText[oldText.length - 1 - oldSuffix] === newText[newText.length - 1 - newSuffix]
    ) {
        oldSuffix++;
        newSuffix++;
    }

    const deletedChars = oldText.substring(prefixLen, oldText.length - oldSuffix);
    const insertedChars = newText.substring(prefixLen, newText.length - newSuffix);

    return {
        byteOffset: encoder.encode(oldText.substring(0, prefixLen)).length,
        deleteLen: encoder.encode(deletedChars).length,
        insertText: insertedChars,
    };
}
