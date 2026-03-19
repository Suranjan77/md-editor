/**
 * Typora-like Markdown Decorations for CodeMirror 6
 *
 * - On the ACTIVE LINE: syntax markers are dimmed (visible but subtle)
 * - On INACTIVE LINES: syntax markers are completely hidden via Decoration.replace()
 * - Headings, bold, italic, code, links, wikilinks, blockquotes, lists, tables all styled
 * - Code blocks and tables (multiline) handled with state tracking across lines
 */

import { ViewPlugin, Decoration, WidgetType } from '@codemirror/view';
import { RangeSetBuilder } from '@codemirror/state';

const markDim = Decoration.mark({ class: 'md-mark' });
const markHide = Decoration.replace({});
const strongDeco = Decoration.mark({ class: 'cm-strong' });
const emDeco = Decoration.mark({ class: 'cm-em' });
const codeDeco = Decoration.mark({ class: 'cm-inline-code' });
const linkDeco = Decoration.mark({ class: 'cm-link' });
const wikiDeco = Decoration.mark({ class: 'cm-wikilink' });

const headingLines = [
    Decoration.line({ class: 'md-h1-line' }),
    Decoration.line({ class: 'md-h2-line' }),
    Decoration.line({ class: 'md-h3-line' }),
    Decoration.line({ class: 'md-h4-line' }),
    Decoration.line({ class: 'md-h5-line' }),
    Decoration.line({ class: 'md-h6-line' }),
];
const bqLine = Decoration.line({ class: 'md-blockquote-line' });
const codeLineD = Decoration.line({ class: 'md-code-line' });
const fenceOpen = Decoration.line({ class: 'md-fence-line md-fence-open' });
const fenceClose = Decoration.line({ class: 'md-fence-line md-fence-close' });
const hrLine = Decoration.line({ class: 'md-hr-line' });
const listLineDeco = Decoration.line({ class: 'md-list-line' });
const tableFirst = Decoration.line({ class: 'md-table-line md-table-first' });
const tableMid = Decoration.line({ class: 'md-table-line' });
const tableLast = Decoration.line({ class: 'md-table-line md-table-last' });
const tableOnly = Decoration.line({ class: 'md-table-line md-table-first md-table-last' });
const tableDelim = Decoration.line({ class: 'md-table-line md-table-delim' });

function isTableLine(doc, lineNum) {
    if (lineNum < 1 || lineNum > doc.lines) return false;
    return doc.line(lineNum).text.includes('|');
}

function buildDecorations(view) {
    const builder = new RangeSetBuilder();
    const doc = view.state.doc;
    const cursor = view.state.selection.main.head;
    const activeLine = doc.lineAt(cursor).number;
    let inCodeBlock = false;

    for (let i = 1; i <= doc.lines; i++) {
        const line = doc.line(i);
        const text = line.text;
        const from = line.from;
        const isActive = (i === activeLine);
        const mark = isActive ? markDim : markHide;

        // ── Code fences ─────────────────────────────────────
        if (/^`{3,}/.test(text)) {
            builder.add(from, from, inCodeBlock ? fenceClose : fenceOpen);
            if (isActive) {
                safeMark(builder, from, from + text.length, markDim);
            } else {
                safeMark(builder, from, from + text.length, markHide);
            }
            inCodeBlock = !inCodeBlock;
            continue;
        }
        if (inCodeBlock) {
            builder.add(from, from, codeLineD);
            continue;
        }

        // ── Headings ────────────────────────────────────────
        const hm = text.match(/^(#{1,6})\s/);
        if (hm) {
            const level = hm[1].length;
            builder.add(from, from, headingLines[level - 1]);
            safeMark(builder, from, from + hm[0].length, mark);
            addInline(builder, text.slice(hm[0].length), from + hm[0].length, isActive);
            continue;
        }

        // ── Thematic break ──────────────────────────────────
        if (/^\s*([-*_])\s*\1\s*\1[\s\-*_]*$/.test(text)) {
            builder.add(from, from, hrLine);
            safeMark(builder, from, from + text.length, mark);
            continue;
        }

        // ── Blockquote ──────────────────────────────────────
        const bqm = text.match(/^(>\s?)/);
        if (bqm) {
            builder.add(from, from, bqLine);
            safeMark(builder, from, from + bqm[1].length, mark);
            addInline(builder, text.slice(bqm[1].length), from + bqm[1].length, isActive);
            continue;
        }

        // ── Lists ───────────────────────────────────────────
        const lm = text.match(/^(\s*)([-*+]|\d+[.)]) /);
        if (lm) {
            builder.add(from, from, listLineDeco);
            const ms = from + lm[1].length;
            const me = ms + lm[2].length + 1;
            safeMark(builder, ms, me, mark);
            addInline(builder, text.slice(lm[0].length), from + lm[0].length, isActive);
            continue;
        }

        // ── Table rows (with container boundaries) ──────────
        if (text.includes('|')) {
            const prevIsTable = isTableLine(doc, i - 1);
            const nextIsTable = isTableLine(doc, i + 1);
            const isDelim = /^\s*\|?\s*[-:]+[-|:\s]+/.test(text);

            if (isDelim) {
                builder.add(from, from, tableDelim);
                safeMark(builder, from, from + text.length, mark);
            } else if (!prevIsTable && !nextIsTable) {
                builder.add(from, from, tableOnly);
                addTableRow(builder, text, from, isActive);
            } else if (!prevIsTable) {
                builder.add(from, from, tableFirst);
                addTableRow(builder, text, from, isActive);
            } else if (!nextIsTable) {
                builder.add(from, from, tableLast);
                addTableRow(builder, text, from, isActive);
            } else {
                builder.add(from, from, tableMid);
                addTableRow(builder, text, from, isActive);
            }
            continue;
        }

        // ── Regular paragraph ───────────────────────────────
        if (text.length > 0) {
            addInline(builder, text, from, isActive);
        }
    }

    return builder.finish();
}

/**
 * Add inline decorations: bold, italic, code, links, wikilinks.
 * On active line: syntax markers are dimmed.
 * On inactive lines: syntax markers are hidden.
 */
function addInline(builder, text, offset, isActive) {
    const mark = isActive ? markDim : markHide;
    let i = 0;

    while (i < text.length) {
        // Inline code: `code`
        if (text[i] === '`') {
            const end = text.indexOf('`', i + 1);
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 1, mark);
                safeMark(builder, offset + i + 1, offset + end, codeDeco);
                safeMark(builder, offset + end, offset + end + 1, mark);
                i = end + 1;
                continue;
            }
        }

        // Bold: **text**
        if (text[i] === '*' && text[i + 1] === '*') {
            const end = text.indexOf('**', i + 2);
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 2, mark);
                safeMark(builder, offset + i + 2, offset + end, strongDeco);
                safeMark(builder, offset + end, offset + end + 2, mark);
                i = end + 2;
                continue;
            }
        }

        // Bold: __text__
        if (text[i] === '_' && text[i + 1] === '_') {
            const end = text.indexOf('__', i + 2);
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 2, mark);
                safeMark(builder, offset + i + 2, offset + end, strongDeco);
                safeMark(builder, offset + end, offset + end + 2, mark);
                i = end + 2;
                continue;
            }
        }

        // Wikilink: [[target]] or [[target|alias]]
        if (text[i] === '[' && text[i + 1] === '[') {
            const end = text.indexOf(']]', i + 2);
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 2, mark);
                safeMark(builder, offset + i + 2, offset + end, wikiDeco);
                safeMark(builder, offset + end, offset + end + 2, mark);
                i = end + 2;
                continue;
            }
        }

        // Image: ![alt](url)
        if (text[i] === '!' && text[i + 1] === '[') {
            const altEnd = text.indexOf(']', i + 2);
            if (altEnd !== -1 && text[altEnd + 1] === '(') {
                const urlEnd = text.indexOf(')', altEnd + 2);
                if (urlEnd !== -1) {
                    safeMark(builder, offset + i, offset + urlEnd + 1, mark);
                    i = urlEnd + 1;
                    continue;
                }
            }
        }

        // Link: [text](url)
        if (text[i] === '[') {
            const textEnd = text.indexOf(']', i + 1);
            if (textEnd !== -1 && text[textEnd + 1] === '(') {
                const urlEnd = text.indexOf(')', textEnd + 2);
                if (urlEnd !== -1) {
                    safeMark(builder, offset + i, offset + i + 1, mark);
                    safeMark(builder, offset + i + 1, offset + textEnd, linkDeco);
                    safeMark(builder, offset + textEnd, offset + urlEnd + 1, mark);
                    i = urlEnd + 1;
                    continue;
                }
            }
        }

        // Italic: *text*
        if (text[i] === '*' && text[i + 1] !== '*' && (i === 0 || text[i - 1] !== '*')) {
            const end = findClose(text, i + 1, '*');
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 1, mark);
                safeMark(builder, offset + i + 1, offset + end, emDeco);
                safeMark(builder, offset + end, offset + end + 1, mark);
                i = end + 1;
                continue;
            }
        }

        // Italic: _text_
        if (text[i] === '_' && text[i + 1] !== '_' && (i === 0 || text[i - 1] !== '_')) {
            const end = findClose(text, i + 1, '_');
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 1, mark);
                safeMark(builder, offset + i + 1, offset + end, emDeco);
                safeMark(builder, offset + end, offset + end + 1, mark);
                i = end + 1;
                continue;
            }
        }

        i++;
    }
}

function addTableRow(builder, text, from, isActive) {
    const mark = isActive ? markDim : markHide;
    for (let j = 0; j < text.length; j++) {
        if (text[j] === '|') {
            safeMark(builder, from + j, from + j + 1, mark);
        }
    }
}

function findClose(text, start, char) {
    for (let j = start; j < text.length; j++) {
        if (text[j] === char && text[j + 1] !== char && (j === 0 || text[j - 1] !== char)) {
            return j;
        }
    }
    return -1;
}

function safeMark(builder, from, to, deco) {
    if (from < to) builder.add(from, to, deco);
}

const plugin = ViewPlugin.fromClass(
    class {
        decorations;
        constructor(view) { this.decorations = buildDecorations(view); }
        update(update) {
            // Update on doc change, viewport change, OR cursor move
            if (update.docChanged || update.viewportChanged || update.selectionSet) {
                this.decorations = buildDecorations(update.view);
            }
        }
    },
    { decorations: v => v.decorations }
);

export function markdownDecorations() {
    return plugin;
}
