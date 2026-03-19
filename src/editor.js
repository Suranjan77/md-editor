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
    const selected = view.state.sliceDoc(from, to);

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

function linkCommand(view) {
    const { from, to } = view.state.selection.main;
    const selected = view.state.sliceDoc(from, to);
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
        backgroundColor: '#0f1115',
        color: '#e4e6eb',
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': { overflow: 'auto', fontFamily: 'inherit' },
    '.cm-content': {
        padding: '48px 80px',
        maxWidth: '860px',
        margin: '0 auto',
        caretColor: '#6c8cff',
    },
    '.cm-line': { lineHeight: '1.8', padding: '0' },
    '.cm-cursor': { borderLeftColor: '#6c8cff', borderLeftWidth: '2px' },
    '.cm-selectionBackground, ::selection': {
        backgroundColor: 'rgba(108, 140, 255, 0.2) !important',
    },
    '.cm-activeLine': { backgroundColor: 'rgba(108, 140, 255, 0.04)' },
    '.cm-gutters': { display: 'none' },

    // ── Syntax markers ──────────────────────────────────────
    '.md-mark': {
        opacity: '0.25',
        fontSize: '0.72em',
        fontFamily: "'JetBrains Mono', monospace",
        fontWeight: '400',
        letterSpacing: '-0.5px',
        transition: 'opacity 0.15s ease',
    },
    '.cm-activeLine .md-mark': { opacity: '0.45' },

    // ── Headings ────────────────────────────────────────────
    '.md-h1-line': {
        fontSize: '2em', fontWeight: '700', lineHeight: '1.3',
        paddingBottom: '4px',
        borderBottom: '2px solid rgba(108, 140, 255, 0.3)',
    },
    '.md-h2-line': { fontSize: '1.55em', fontWeight: '600', color: '#6c8cff', lineHeight: '1.35' },
    '.md-h3-line': { fontSize: '1.3em', fontWeight: '600', lineHeight: '1.4' },
    '.md-h4-line': { fontSize: '1.15em', fontWeight: '600' },
    '.md-h5-line': { fontSize: '1.05em', fontWeight: '600', color: '#9ca0ab' },
    '.md-h6-line': { fontSize: '1em', fontWeight: '600', color: '#6b7080' },

    // ── Inline ──────────────────────────────────────────────
    '.cm-strong': { fontWeight: '700', color: '#e4e6eb' },
    '.cm-em': { fontStyle: 'italic', color: '#ffc46b' },
    '.cm-inline-code': {
        backgroundColor: '#1c1f28', color: '#6c8cff',
        padding: '2px 6px', borderRadius: '4px',
        fontFamily: "'JetBrains Mono', monospace", fontSize: '0.88em',
    },
    '.cm-link': { color: '#6c8cff', textDecoration: 'none' },
    '.cm-wikilink': { color: '#6bffb8', fontWeight: '500', cursor: 'pointer' },

    // ── Blockquotes ─────────────────────────────────────────
    '.md-blockquote-line': {
        borderLeft: '3px solid #6c8cff', paddingLeft: '14px', color: '#9ca0ab',
    },

    // ── Code blocks (container) ────────────────────────────
    '.md-code-line, .md-fence-line': {
        backgroundColor: '#161920',
        padding: '0 20px',
        borderLeft: '3px solid rgba(108, 140, 255, 0.15)',
    },
    '.md-code-line': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.9em', color: '#c8ccd4',
    },
    '.md-fence-line': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.82em', color: '#4a4e5a',
    },
    '.md-fence-open': {
        borderRadius: '8px 8px 0 0',
        marginTop: '8px',
        paddingTop: '8px',
    },
    '.md-fence-close': {
        borderRadius: '0 0 8px 8px',
        marginBottom: '8px',
        paddingBottom: '8px',
    },

    // ── Horizontal rule ─────────────────────────────────────
    '.md-hr-line': { textAlign: 'center', color: '#4a4e5a' },

    // ── Lists ───────────────────────────────────────────────
    '.md-list-line': { paddingLeft: '4px' },

    // ── Tables (container) ──────────────────────────────────
    '.md-table-line': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '0.9em',
        backgroundColor: '#161920',
        padding: '2px 16px',
        borderLeft: '3px solid rgba(108, 140, 255, 0.15)',
    },
    '.md-table-first': {
        borderRadius: '8px 8px 0 0',
        marginTop: '8px',
        paddingTop: '10px',
    },
    '.md-table-last': {
        borderRadius: '0 0 8px 8px',
        marginBottom: '8px',
        paddingBottom: '10px',
    },
    '.md-table-first.md-table-last': {
        borderRadius: '8px',
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
