import { performUndo, performRedo } from './ipc.js';
import { forceSync } from './input.js';

/**
 * Set up undo/redo shortcuts.
 * Syncs the current block before undo/redo, then does a full re-render.
 * @param {function} onResponse - callback(response) for full re-render
 */
export function setupUndoShortcuts(onResponse) {
    window.addEventListener('keydown', async (e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
            e.preventDefault();
            try {
                const response = e.shiftKey
                    ? await performRedo()
                    : await performUndo();
                onResponse(response);
            } catch (err) {
                if (!err.toString().includes('Nothing to')) {
                    console.error('Undo/Redo failed:', err);
                }
            }
        }
    });
}
