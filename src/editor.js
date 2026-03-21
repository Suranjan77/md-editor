/**
 * CodeMirror 6 editor setup with Typora-like keybindings.
 */
import { EditorView, keymap, highlightActiveLine, drawSelection } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { markdownDecorations } from './markdown-decorations.js';
import { mathTooltip } from './math-tooltip.js';

// ── Markdown formatting commands ────────────────────────────────────
function wrapSelection(view, wrapper) {
    const { from, to } = view.state.selection.main;

    // Toggle: if already wrapped, unwrap
    const wLen = wrapper.length;
    if (from >= wLen && to + wLen <= view.state.doc.length) {
        const before = view.state.sliceDoc(from - wLen, from);
        const after = view.state.sliceDoc(to, to + wLen);
        if (before === wrapper && after === wrapper) {
            view.dispatch({
                changes: [
                    { from: from - wLen, to: from, insert: '' },
                    { from: to, to: to + wLen, insert: '' },
                ],
                selection: { anchor: from - wLen, head: to - wLen },
            });
            return true;
        }
    }

    if (from === to) {
        // No selection: insert wrapper pair and place cursor inside
        view.dispatch({
            changes: { from, insert: wrapper + wrapper },
            selection: { anchor: from + wLen },
        });
    } else {
        // Wrap selection
        view.dispatch({
            changes: [
                { from, insert: wrapper },
                { from: to, insert: wrapper },
            ],
            selection: { anchor: from + wLen, head: to + wLen },
        });
    }
    return true;
}

function boldCommand(view) { return wrapSelection(view, '**'); }
function italicCommand(view) { return wrapSelection(view, '*'); }
function codeCommand(view) { return wrapSelection(view, '`'); }
function strikethroughCommand(view) { return wrapSelection(view, '~~'); }

function wikilinkCommand(view) {
    const { from, to } = view.state.selection.main;
    const selected = view.state.sliceDoc(from, to);
    if (from === to) {
        view.dispatch({
            changes: { from, insert: '[[]]' },
            selection: { anchor: from + 2 },
        });
    } else {
        view.dispatch({
            changes: [
                { from, insert: '[[' },
                { from: to, insert: ']]' },
            ],
            selection: { anchor: from + 2, head: to + 2 },
        });
    }
    return true;
}

function linkCommand(view) {
    const { from, to } = view.state.selection.main;
    if (from === to) {
        view.dispatch({
            changes: { from, insert: '[](url)' },
            selection: { anchor: from + 1 },
        });
    } else {
        view.dispatch({
            changes: [
                { from, insert: '[' },
                { from: to, insert: '](url)' },
            ],
            selection: { anchor: to + 3, head: to + 6 }, // select "url"
        });
    }
    return true;
}

// ── Theme ───────────────────────────────────────────────────────────
const editorTheme = EditorView.theme({
    '&': {
        height: '100%',
        fontSize: '18px',
        fontFamily: "var(--font-sans)",
        backgroundColor: 'transparent',
        color: 'var(--text-primary)',
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': { overflowX: 'hidden', overflowY: 'auto', fontFamily: 'inherit' },
    '.cm-content': {
        caretColor: 'var(--accent)',
    },
    '.cm-line': {
        lineHeight: '1.6',
        padding: '0',
        transition: 'background-color 0.15s ease',
    },
    '.cm-cursor': { borderLeftColor: 'var(--accent)', borderLeftWidth: '2px' },
    '.cm-selectionBackground, ::selection': {
        backgroundColor: 'var(--accent-dim) !important',
    },
    '.cm-activeLine': { backgroundColor: 'transparent' },
    '.cm-gutters': { display: 'none' },

    // ── Syntax markers ──────────────────────────────────────
    '.md-mark': {
        opacity: '0.3',
        fontSize: '0.85em',
        fontFamily: "var(--font-mono)",
        fontWeight: '400',
        letterSpacing: '-0.5px',
        transition: 'opacity 0.2s ease',
        color: 'var(--text-muted)'
    },
    '.cm-activeLine .md-mark': { opacity: '0.6' },

    // ── Headings ────────────────────────────────────────────
    '.md-h1-line': {
        fontSize: '2.25rem', fontWeight: '800', lineHeight: '1.2',
        color: 'var(--text-primary)',
        paddingTop: '24px', paddingBottom: '8px',
    },
    '.md-h2-line': {
        fontSize: '1.875rem', fontWeight: '700', lineHeight: '1.3',
        color: 'var(--text-primary)',
        paddingTop: '20px', paddingBottom: '8px',
    },
    '.md-h3-line': { fontSize: '1.5rem', fontWeight: '600', lineHeight: '1.4', color: 'var(--text-primary)' },
    '.md-h4-line': { fontSize: '1.25rem', fontWeight: '600', color: 'var(--text-primary)' },
    '.md-h5-line': { fontSize: '1.125rem', fontWeight: '600', color: 'var(--text-secondary)' },
    '.md-h6-line': { fontSize: '1rem', fontWeight: '600', color: 'var(--text-secondary)', textTransform: 'uppercase' },

    // ── Inline ──────────────────────────────────────────────
    '.cm-strong': { fontWeight: '700', color: 'var(--text-primary)' },
    '.cm-em': { fontStyle: 'italic', color: 'var(--text-primary)' },
    '.cm-strikethrough': {
        textDecoration: 'line-through',
        textDecorationColor: 'var(--danger)',
        color: 'var(--text-muted)',
    },
    '.cm-inline-code': {
        backgroundColor: 'var(--bg-tertiary)', color: 'var(--accent)',
        padding: '2px 6px', borderRadius: '4px',
        fontFamily: "var(--font-mono)", fontSize: '0.85em',
        border: '1px solid var(--border)',
    },
    '.cm-link': { color: 'var(--accent)', textDecoration: 'underline', textUnderlineOffset: '4px' },
    '.cm-wikilink': { color: 'var(--accent-secondary)', fontWeight: '500', cursor: 'pointer', textDecoration: 'underline', textUnderlineOffset: '4px' },

    // ── Math ────────────────────────────────────────────────
    '.md-math-inline': {
        color: 'var(--accent-secondary)', fontFamily: "var(--font-mono)",
        fontSize: '0.92em', fontStyle: 'italic',
    },
    '.md-math-line': {
        backgroundColor: 'var(--bg-tertiary)',
        padding: '0 16px',
        borderLeft: '1px solid var(--accent-dim)', borderRight: '1px solid var(--accent-dim)',
        fontFamily: "var(--font-mono)",
        fontSize: '14px', lineHeight: '1.6', color: 'var(--accent-secondary)',
    },
    '.md-math-fence': { color: 'var(--text-muted)' },
    '.md-math-fence-open': {
        borderRadius: 'var(--radius-md) var(--radius-md) 0 0',
        paddingTop: '24px', borderTop: '1px solid var(--accent-dim)',
    },
    '.md-math-fence-close': {
        borderRadius: '0 0 var(--radius-md) var(--radius-md)',
        paddingBottom: '24px', borderBottom: '1px solid var(--accent-dim)',
    },

    // ── Task checkboxes ─────────────────────────────────────
    '.md-task-checkbox': {
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        width: '18px', height: '18px',
        border: '2px solid var(--accent)', borderRadius: '4px',
        marginRight: '8px', verticalAlign: 'middle',
        cursor: 'pointer', fontSize: '12px',
        transition: 'all 0.15s ease',
    },
    '.md-task-checkbox.checked': {
        backgroundColor: 'var(--accent)', color: 'var(--bg-primary)',
    },

    // ── Image preview ───────────────────────────────────────
    '.md-image-widget': {
        display: 'block',
        margin: '0 auto',
        maxWidth: '100%', maxHeight: '400px',
        borderRadius: 'var(--radius-md)',
        border: '1px solid var(--border)',
        boxShadow: 'var(--shadow-md)',
        transition: 'transform 0.2s ease, box-shadow 0.2s ease',
    },
    '.md-image-widget:hover': {
        transform: 'scale(1.01)',
        boxShadow: 'var(--shadow-lg)',
    },
    '.md-image-caption': {
        textAlign: 'center',
        paddingTop: '8px',
        fontSize: '13px',
        color: 'var(--text-muted)',
    },

    // ── Blockquotes ─────────────────────────────────────────
    '.md-blockquote-line': {
        borderLeft: '3px solid var(--accent-dim)', paddingLeft: '24px',
        color: 'var(--text-secondary)', fontStyle: 'italic',
    },

    // ── Code blocks (container) ────────────────────────────
    '.md-code-line, .md-fence-line': {
        backgroundColor: 'var(--bg-secondary)',
        padding: '0 20px',
        borderLeft: '1px solid var(--border)', borderRight: '1px solid var(--border)',
    },
    '.md-code-line': {
        fontFamily: "var(--font-mono)",
        fontSize: '14px', lineHeight: '1.6', color: 'var(--text-primary)',
    },
    '.md-fence-line': {
        fontFamily: "var(--font-mono)",
        fontSize: '14px', lineHeight: '1.6', color: 'var(--text-muted)',
    },
    '.md-fence-open': {
        borderRadius: 'var(--radius-md) var(--radius-md) 0 0',
        paddingTop: '10px', borderTop: '1px solid var(--border)'
    },
    '.md-fence-close': {
        borderRadius: '0 0 var(--radius-md) var(--radius-md)',
        paddingBottom: '10px', borderBottom: '1px solid var(--border)'
    },

    // ── Horizontal rule ─────────────────────────────────────
    '.md-hr-line': { textAlign: 'center', color: 'var(--border)' },

    // ── Lists ───────────────────────────────────────────────
    '.md-list-line': { paddingLeft: '4px' },

    // ── Tables (container) ──────────────────────────────────
    '.md-table-line': {
        fontFamily: "var(--font-mono)",
        fontSize: '0.9em',
        backgroundColor: 'var(--bg-secondary)',
        padding: '0 16px',
        borderLeft: '3px solid var(--border)',
    },
    '.md-table-first': { borderRadius: 'var(--radius-md) var(--radius-md) 0 0' },
    '.md-table-last': { borderRadius: '0 0 var(--radius-md) var(--radius-md)' },
    '.md-table-first.md-table-last': { borderRadius: 'var(--radius-md)' },
    '.md-table-delim': { opacity: '0.4', fontSize: '0.8em' },
}, { dark: true });

// ── Create editor ───────────────────────────────────────────────────
export function createEditor(parent, onSave) {
    const view = new EditorView({
        state: EditorState.create({
            doc: '',
            extensions: [
                markdown(),
                markdownDecorations(),
                mathTooltip(),
                history(),
                keymap.of([
                    { key: 'Mod-b', run: boldCommand },
                    { key: 'Mod-i', run: italicCommand },
                    { key: 'Mod-e', run: codeCommand },
                    { key: 'Mod-k', run: linkCommand },
                    { key: 'Mod-Shift-x', run: strikethroughCommand },
                    { key: 'Mod-Shift-k', run: wikilinkCommand },
                    { key: 'Mod-s', run: () => { onSave(); return true; } },
                    ...defaultKeymap,
                    ...historyKeymap,
                    indentWithTab,
                ]),
                highlightActiveLine(),
                drawSelection(),
                editorTheme,
                EditorView.lineWrapping,
            ],
        }),
        parent,
    });
    return view;
}

export function setContent(view, content) {
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: content } });
}

export function getContent(view) {
    return view.state.doc.toString();
}
