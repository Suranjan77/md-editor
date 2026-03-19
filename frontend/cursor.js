/**
 * Cursor utilities for the Typora-like WYSIWYG editor.
 * Uses line + column position for robust save/restore across re-renders.
 */

/**
 * Save cursor as { line, column } — robust across innerHTML re-renders.
 * Line = index of the direct child element of editor containing the cursor.
 * Column = character offset within that line element.
 * @param {HTMLElement} editor
 * @returns {{ line: number, column: number } | null}
 */
export function saveCursorPosition(editor) {
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0 || !sel.focusNode) return null;
    if (!editor.contains(sel.focusNode)) return null;

    // Find the direct child of editor that contains the cursor
    let lineEl = sel.focusNode;
    if (lineEl === editor) {
        // Cursor is directly in the editor element (e.g., at the end)
        return { line: editor.children.length - 1, column: Infinity };
    }
    while (lineEl && lineEl.parentNode !== editor) {
        lineEl = lineEl.parentNode;
    }
    if (!lineEl || lineEl.parentNode !== editor) return null;

    // Line index
    let lineIndex = 0;
    let sibling = lineEl.previousElementSibling;
    while (sibling) {
        lineIndex++;
        sibling = sibling.previousElementSibling;
    }

    // Column: character offset within this line's text content
    const range = document.createRange();
    range.selectNodeContents(lineEl);
    range.setEnd(sel.focusNode, sel.focusOffset);
    const column = range.toString().length;

    return { line: lineIndex, column };
}

/**
 * Restore cursor from a { line, column } position.
 * @param {HTMLElement} editor
 * @param {{ line: number, column: number } | null} pos
 */
export function restoreCursorPosition(editor, pos) {
    if (!pos) return;

    const children = editor.children;
    if (children.length === 0) return;

    const lineIndex = Math.min(pos.line, children.length - 1);
    const lineEl = children[lineIndex];
    if (!lineEl) return;

    const column = pos.column;

    // Walk text nodes within the line to find the right position
    const walker = document.createTreeWalker(lineEl, NodeFilter.SHOW_TEXT);
    let node = walker.nextNode();
    let accumulated = 0;

    while (node) {
        const len = node.textContent.length;
        if (accumulated + len >= column) {
            setCaret(node, column - accumulated);
            return;
        }
        accumulated += len;
        node = walker.nextNode();
    }

    // Past end of line: put cursor at end
    const lastWalker = document.createTreeWalker(lineEl, NodeFilter.SHOW_TEXT);
    let last = null;
    let n = lastWalker.nextNode();
    while (n) { last = n; n = lastWalker.nextNode(); }
    if (last) {
        setCaret(last, last.textContent.length);
    } else {
        // Line has no text nodes (empty line with <br>)
        const sel = window.getSelection();
        sel.removeAllRanges();
        const range = document.createRange();
        range.selectNodeContents(lineEl);
        range.collapse(false);
        sel.addRange(range);
    }
}

function setCaret(node, offset) {
    const sel = window.getSelection();
    sel.removeAllRanges();
    const range = document.createRange();
    try {
        range.setStart(node, Math.min(offset, node.textContent.length));
        range.collapse(true);
        sel.addRange(range);
    } catch (e) {
        // Fallback
        range.selectNodeContents(node);
        range.collapse(false);
        sel.addRange(range);
    }
}

/**
 * Convert a byte offset (from Rust) to a { line, column } position.
 * @param {string} text
 * @param {number} byteOffset
 * @returns {{ line: number, column: number }}
 */
export function byteOffsetToLineCol(text, byteOffset) {
    const encoder = new TextEncoder();
    const bytes = encoder.encode(text);
    const charOffset = byteOffset >= bytes.length
        ? text.length
        : new TextDecoder().decode(bytes.slice(0, byteOffset)).length;

    const lines = text.split('\n');
    let accumulated = 0;
    for (let i = 0; i < lines.length; i++) {
        const lineLen = lines[i].length;
        if (accumulated + lineLen >= charOffset) {
            return { line: i, column: charOffset - accumulated };
        }
        accumulated += lineLen + 1; // +1 for the \n
    }
    return { line: lines.length - 1, column: lines[lines.length - 1].length };
}
