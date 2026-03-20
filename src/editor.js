/**
 * CodeMirror 6 editor setup with Typora-like keybindings.
 */
import { EditorView, keymap, highlightActiveLine, drawSelection } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { markdownDecorations } from './markdown-decorations.js';

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
        fontSize: '16px',
        fontFamily: "'Inter', -apple-system, BlinkMacSystemFont, sans-serif",
        backgroundColor: '#1a1d2e',
        color: '#e8eaf0',
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': { overflow: 'auto', fontFamily: 'inherit' },
    '.cm-content': {
        padding: '48px 80px',
        maxWidth: '860px',
        margin: '0 auto',
        caretColor: '#7c8cf8',
    },
    '.cm-line': {
        lineHeight: '1.8',
        padding: '0',
        transition: 'background-color 0.15s ease',
    },
    '.cm-cursor': { borderLeftColor: '#7c8cf8', borderLeftWidth: '2.5px' },
    '.cm-selectionBackground, ::selection': {
        backgroundColor: 'rgba(124, 140, 248, 0.18) !important',
    },
    '.cm-activeLine': { backgroundColor: 'rgba(124, 140, 248, 0.05)' },
    '.cm-gutters': { display: 'none' },

    // ── Syntax markers ──────────────────────────────────────
    '.md-mark': {
        opacity: '0.2',
        fontSize: '0.72em',
        fontFamily: "'JetBrains Mono', monospace",
        fontWeight: '400',
        letterSpacing: '-0.5px',
        transition: 'opacity 0.2s ease',
    },
    '.cm-activeLine .md-mark': { opacity: '0.4' },

    // ── Headings ────────────────────────────────────────────
    '.md-h1-line': {
        fontSize: '2.1em', fontWeight: '800', lineHeight: '1.3',
        borderBottom: '2px solid rgba(124, 140, 248, 0.25)',
        color: '#e8eaf0',
    },
    '.md-h2-line': {
        fontSize: '1.6em', fontWeight: '700', lineHeight: '1.35',
        color: '#7c8cf8',
    },
    '.md-h3-line': {
        fontSize: '1.35em', fontWeight: '650', lineHeight: '1.4',
        color: '#9b8cff',
    },
    '.md-h4-line': { fontSize: '1.18em', fontWeight: '600', color: '#a4a9c0' },
    '.md-h5-line': { fontSize: '1.08em', fontWeight: '600', color: '#8b90a8' },
    '.md-h6-line': { fontSize: '1em', fontWeight: '600', color: '#6e7496' },

    // ── Inline ──────────────────────────────────────────────
    '.cm-strong': { fontWeight: '700', color: '#e8eaf0' },
    '.cm-em': { fontStyle: 'italic', color: '#ffc46b' },
    '.cm-strikethrough': {
        textDecoration: 'line-through',
        textDecorationColor: 'rgba(248, 113, 113, 0.6)',
        color: '#6e7496',
    },
    '.cm-inline-code': {
        backgroundColor: 'rgba(124, 140, 248, 0.08)', color: '#9b8cff',
        padding: '2px 7px', borderRadius: '5px',
        fontFamily: "'JetBrains Mono', monospace", fontSize: '0.88em',
        border: '1px solid rgba(124, 140, 248, 0.12)',
    },
    '.cm-link': { color: '#7c8cf8', textDecoration: 'none' },
    '.cm-wikilink': { color: '#6bdfb8', fontWeight: '500', cursor: 'pointer' },

    // ── Math ────────────────────────────────────────────────
    '.md-math-inline': {
        color: '#c471ed', fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.92em', fontStyle: 'italic',
    },
    '.md-math-line': {
        backgroundColor: 'rgba(196, 113, 237, 0.06)',
        padding: '0 20px',
        borderLeft: '3px solid rgba(196, 113, 237, 0.2)',
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.9em', color: '#c471ed',
    },
    '.md-math-fence': {
        fontSize: '0.82em', color: '#6e7496',
        fontFamily: "'JetBrains Mono', monospace",
    },

    // ── Task checkboxes ─────────────────────────────────────
    '.md-task-checkbox': {
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        width: '17px', height: '17px',
        border: '2px solid #7c8cf8', borderRadius: '4px',
        marginRight: '8px', verticalAlign: 'middle',
        cursor: 'pointer', fontSize: '12px',
        transition: 'all 0.15s ease',
    },
    '.md-task-checkbox.checked': {
        backgroundColor: '#7c8cf8', color: '#fff',
    },

    // ── Image preview ───────────────────────────────────────
    '.md-image-widget': {
        display: 'inline-block',
        verticalAlign: 'bottom',
        maxWidth: '100%', maxHeight: '280px',
        borderRadius: '10px',
        margin: '0 6px',
        border: '1px solid rgba(124, 140, 248, 0.15)',
        boxShadow: '0 4px 16px rgba(0, 0, 0, 0.2)',
        transition: 'transform 0.2s ease, box-shadow 0.2s ease',
    },
    '.md-image-widget:hover': {
        transform: 'scale(1.02)',
        boxShadow: '0 8px 24px rgba(0, 0, 0, 0.3)',
    },

    // ── Blockquotes ─────────────────────────────────────────
    '.md-blockquote-line': {
        borderLeft: '3px solid #7c8cf8', paddingLeft: '16px',
        color: '#a4a9c0', fontStyle: 'italic',
    },

    // ── Code blocks (container) ────────────────────────────
    '.md-code-line, .md-fence-line': {
        backgroundColor: '#1f2336',
        padding: '0 20px',
        borderLeft: '3px solid rgba(124, 140, 248, 0.12)',
    },
    '.md-code-line': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.9em', color: '#c8ccd8',
    },
    '.md-fence-line': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.82em', color: '#6e7496',
    },
    '.md-fence-open': {
        borderRadius: '10px 10px 0 0',
    },
    '.md-fence-close': {
        borderRadius: '0 0 10px 10px',
    },

    // ── Horizontal rule ─────────────────────────────────────
    '.md-hr-line': { textAlign: 'center', color: '#4a5080' },

    // ── Lists ───────────────────────────────────────────────
    '.md-list-line': { paddingLeft: '4px' },

    // ── Tables (container) ──────────────────────────────────
    '.md-table-line': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.9em',
        backgroundColor: '#1f2336',
        padding: '0 16px',
        borderLeft: '3px solid rgba(124, 140, 248, 0.12)',
    },
    '.md-table-first': {
        borderRadius: '10px 10px 0 0',
    },
    '.md-table-last': {
        borderRadius: '0 0 10px 10px',
    },
    '.md-table-first.md-table-last': {
        borderRadius: '10px',
    },
    '.md-table-delim': {
        opacity: '0.4',
        fontSize: '0.8em',
    },
}, { dark: true });

// ── Create editor ───────────────────────────────────────────────────
export function createEditor(parent, onSave) {
    const view = new EditorView({
        state: EditorState.create({
            doc: '',
            extensions: [
                markdown(),
                markdownDecorations(),
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
