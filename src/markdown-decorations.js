/**
 * Typora-like Markdown Decorations for CodeMirror 6 with Live Preview.
 */
import { Decoration, WidgetType, EditorView } from '@codemirror/view';
import { RangeSetBuilder, StateField } from '@codemirror/state';
import katex from 'katex';
import MarkdownIt from 'markdown-it';

const md = new MarkdownIt();

// Line Rules
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
const mathLineD = Decoration.line({ class: 'md-math-line' });
const mathFenceD = Decoration.line({ class: 'md-math-line md-math-fence' });
const hrLine = Decoration.line({ class: 'md-hr-line' });
const listLineDeco = Decoration.line({ class: 'md-list-line' });
const tableFirst = Decoration.line({ class: 'md-table-line md-table-first' });
const tableMid = Decoration.line({ class: 'md-table-line' });
const tableLast = Decoration.line({ class: 'md-table-line md-table-last' });
const tableOnly = Decoration.line({ class: 'md-table-line md-table-first md-table-last' });
const tableDelim = Decoration.line({ class: 'md-table-line md-table-delim' });

// Mark Rules
const markDim = Decoration.mark({ class: 'md-mark' });
const markHide = Decoration.replace({});
const strongDeco = Decoration.mark({ class: 'cm-strong' });
const emDeco = Decoration.mark({ class: 'cm-em' });
const codeDeco = Decoration.mark({ class: 'cm-inline-code' });
const linkDeco = Decoration.mark({ class: 'cm-link' });
const wikiDeco = Decoration.mark({ class: 'cm-wikilink' });
const strikeDeco = Decoration.mark({ class: 'cm-strikethrough' });
const mathInlineDeco = Decoration.mark({ class: 'md-math-inline' });

// ── Live Preview Widgets ───────────────────────────────────────────
class TaskCheckboxWidget extends WidgetType {
    constructor(checked) { super(); this.checked = checked; }
    toDOM() {
        const span = document.createElement('span');
        span.className = 'md-task-checkbox' + (this.checked ? ' checked' : '');
        span.textContent = this.checked ? '✓' : '';
        return span;
    }
    eq(other) { return this.checked === other.checked; }
}

class ImageWidget extends WidgetType {
    constructor(src, alt, pos) { super(); this.src = src; this.alt = alt; this.pos = pos; }
    toDOM(view) {
        const img = document.createElement('img');
        img.className = 'md-image-widget';
        img.src = this.src;
        img.alt = this.alt || '';
        img.loading = 'lazy';
        img.onerror = () => { img.style.display = 'none'; };
        
        img.onmousedown = (e) => {
            if (view && this.pos !== undefined) {
                e.preventDefault();
                view.dispatch({ selection: { anchor: this.pos } });
                view.focus();
            }
        };
        return img;
    }
    eq(other) { return this.src === other.src && this.pos === other.pos; }
}

class HrWidget extends WidgetType {
    constructor(pos) { super(); this.pos = pos; }
    eq(other) { return this.pos === other.pos; }
    toDOM(view) {
        const div = document.createElement('div');
        div.className = 'md-hr-widget';
        div.onmousedown = (e) => {
            if (view && this.pos !== undefined) {
                e.preventDefault();
                view.dispatch({ selection: { anchor: this.pos } });
                view.focus();
            }
        };
        return div;
    }
}

class MathBlockWidget extends WidgetType {
    constructor(mathText, pos) { super(); this.mathText = mathText; this.pos = pos; }
    eq(other) { return this.mathText === other.mathText && this.pos === other.pos; }
    toDOM(view) {
        const div = document.createElement('div');
        div.className = 'md-math-render-block';
        try { katex.render(this.mathText, div, { displayMode: true, throwOnError: false }); }
        catch (e) { div.textContent = this.mathText; }
        
        div.onmousedown = (e) => {
            if (view && this.pos !== undefined) {
                e.preventDefault();
                view.dispatch({ selection: { anchor: this.pos } });
                view.focus();
            }
        };
        return div;
    }
}

class MathInlineWidget extends WidgetType {
    constructor(mathText, pos) { super(); this.mathText = mathText; this.pos = pos; }
    eq(other) { return this.mathText === other.mathText && this.pos === other.pos; }
    toDOM(view) {
        const span = document.createElement('span');
        span.className = 'md-math-render-inline';
        try { katex.render(this.mathText, span, { displayMode: false, throwOnError: false }); }
        catch (e) { span.textContent = this.mathText; }
        
        span.onmousedown = (e) => {
            if (view && this.pos !== undefined) {
                e.preventDefault();
                view.dispatch({ selection: { anchor: this.pos } });
                view.focus();
            }
        };
        return span;
    }
}

class TableBlockWidget extends WidgetType {
    constructor(mdText, pos) { super(); this.mdText = mdText; this.pos = pos; }
    eq(other) { return this.mdText === other.mdText && this.pos === other.pos; }
    toDOM(view) {
        const div = document.createElement('div');
        div.className = 'md-table-render-block';
        div.innerHTML = md.render(this.mdText);
        
        div.onmousedown = (e) => {
            if (view && this.pos !== undefined) {
                // Prevent standard DOM selection and force CodeMirror state integration
                e.preventDefault();
                view.dispatch({ selection: { anchor: this.pos } });
                view.focus();
            }
        };
        return div;
    }
}

function isTableLine(doc, lineNum) {
    if (lineNum < 1 || lineNum > doc.lines) return false;
    return doc.line(lineNum).text.includes('|');
}

// Scans document to construct bounding coordinates for multi-line block elements
function findBlocks(doc) {
    const blocks = [];
    let inCode = false;
    let i = 1;
    while (i <= doc.lines) {
        const line = doc.line(i);
        const text = line.text;

        if (!inCode) {
            const fenceMatch = text.match(/^(\s*)(`{3,}|~{3,})/);
            if (fenceMatch) {
                const fenceChar = fenceMatch[2].charAt(0);
                const infoString = text.slice(fenceMatch[0].length);
                if (!infoString.includes(fenceChar)) {
                    inCode = fenceChar;
                }
                i++; continue;
            }
        } else {
            const closeRegex = new RegExp(`^(\\s*)(${inCode}{3,})\\s*$`);
            if (closeRegex.test(text)) inCode = false;
            i++; continue;
        }

        // Math Block
        if (/^\$\$\s*$/.test(text)) {
            const startFrom = line.from;
            const startLine = i;
            let mathLines = [];
            i++;
            let closed = false;
            let endTo = line.to;
            while (i <= doc.lines) {
                const nextLine = doc.line(i);
                endTo = nextLine.to;
                if (/^\$\$\s*$/.test(nextLine.text)) {
                    closed = true;
                    i++;
                    break;
                }
                mathLines.push(nextLine.text);
                i++;
            }
            if (closed && mathLines.length > 0) {
                blocks.push({
                    type: 'math',
                    from: startFrom,
                    to: endTo,
                    startLine,
                    endLine: i - 1,
                    content: mathLines.join('\n')
                });
            }
            continue;
        }

        // Table Block
        if (text.includes('|') && isTableLine(doc, i)) {
            const startFrom = line.from;
            const startLine = i;
            let tableLines = [text];
            let endTo = line.to;
            i++;
            while (i <= doc.lines && isTableLine(doc, i)) {
                const nextLine = doc.line(i);
                tableLines.push(nextLine.text);
                endTo = nextLine.to;
                i++;
            }
            if (tableLines.length >= 2) {
                blocks.push({
                    type: 'table',
                    from: startFrom,
                    to: endTo,
                    startLine,
                    endLine: i - 1,
                    content: tableLines.join('\n')
                });
            }
            continue;
        }
        i++;
    }
    return blocks;
}

function buildDecorations(state) {
    const builder = new RangeSetBuilder();
    const doc = state.doc;
    const selection = state.selection.main;
    const cursor = selection.head;
    const activeLine = doc.lineAt(cursor).number;
    let inCodeBlock = false;

    // Phase 1: Identify all multiline Live Preview structural blocks
    const blocks = findBlocks(doc);

    // Phase 2: Process document lines sequentially, collapsing inactive blocks to HTML Native widgets
    for (let i = 1; i <= doc.lines; i++) {
        const line = doc.line(i);
        const text = line.text;
        const from = line.from;
        const isActive = (i === activeLine);
        const structMark = markDim; 
        const mark = isActive ? markDim : markHide;

        const block = blocks.find(b => i >= b.startLine && i <= b.endLine);
        if (block) {
            const isBlockActive = cursor >= block.from && cursor <= block.to;
            
            if (!isBlockActive) {
                // Return a fully-rendered block replacement widget if cursor is outside
                if (i === block.startLine) {
                    let widgetDeco;
                    if (block.type === 'math') {
                        widgetDeco = Decoration.replace({ widget: new MathBlockWidget(block.content, block.from), block: true });
                    } else if (block.type === 'table') {
                        widgetDeco = Decoration.replace({ widget: new TableBlockWidget(block.content, block.from), block: true });
                    }
                    builder.add(block.from, block.to, widgetDeco);
                    i = block.endLine;
                }
                continue;
            }
            
            // Render active blocks natively in plain-text mode with raw structural markers
            if (block.type === 'math') {
                if (/^\$\$\s*$/.test(text)) {
                    builder.add(from, from, mathFenceD);
                    safeMark(builder, from, from + text.length, markDim);
                } else {
                    builder.add(from, from, mathLineD);
                }
                continue;
            }
            if (block.type === 'table') {
                const isDelim = /^\s*\|?\s*[-:]+[-|:\s]+/.test(text);
                if (isDelim) {
                    builder.add(from, from, tableDelim);
                    safeMark(builder, from, from + text.length, structMark);
                } else if (i === block.startLine) {
                    builder.add(from, from, tableFirst);
                    addTableRow(builder, text, from);
                } else if (i === block.endLine) {
                    builder.add(from, from, tableLast);
                    addTableRow(builder, text, from);
                } else {
                    builder.add(from, from, tableMid);
                    addTableRow(builder, text, from);
                }
                continue;
            }
        }

        // ── Standard Line Formatting ─────────────────────────────────────
        if (!inCodeBlock) {
            const fenceMatch = text.match(/^(\s*)(`{3,}|~{3,})/);
            if (fenceMatch) {
                const fenceChar = fenceMatch[2].charAt(0);
                const infoString = text.slice(fenceMatch[0].length);
                if (!infoString.includes(fenceChar)) {
                    inCodeBlock = fenceChar; 
                    builder.add(from, from, fenceOpen);
                    safeMark(builder, from, from + text.length, structMark);
                    continue;
                }
            }
        } else {
            const closeRegex = new RegExp(`^(\\s*)(${inCodeBlock}{3,})\\s*$`);
            if (closeRegex.test(text)) {
                inCodeBlock = false;
                builder.add(from, from, fenceClose);
                safeMark(builder, from, from + text.length, structMark);
                continue;
            } else {
                builder.add(from, from, codeLineD);
                continue;
            }
        }

        const hm = text.match(/^(#{1,6})\s/);
        if (hm) {
            const level = hm[1].length;
            builder.add(from, from, headingLines[level - 1]);
            safeMark(builder, from, from + hm[0].length, mark);
            addInline(builder, text.slice(hm[0].length), from + hm[0].length, isActive, selection);
            continue;
        }

        if (/^\s*([-*_])\s*\1\s*\1[\s\-*_]*$/.test(text)) {
            if (!isActive) {
                const widgetDeco = Decoration.replace({ widget: new HrWidget(from), block: true });
                safeMark(builder, from, from + text.length, widgetDeco);
            } else {
                builder.add(from, from, hrLine);
                safeMark(builder, from, from + text.length, structMark);
            }
            continue;
        }

        const bqm = text.match(/^(>\s?)/);
        if (bqm) {
            builder.add(from, from, bqLine);
            safeMark(builder, from, from + bqm[1].length, structMark);
            addInline(builder, text.slice(bqm[1].length), from + bqm[1].length, isActive, selection);
            continue;
        }

        const taskm = text.match(/^(\s*)([-*+])\s\[([ xX])\]\s/);
        if (taskm) {
            builder.add(from, from, listLineDeco);
            const isChecked = taskm[3] !== ' ';
            const markerStart = from + taskm[1].length;
            const markerEnd = markerStart + taskm[2].length + 1;
            const brackStart = markerEnd;
            const brackEnd = brackStart + 3;
            const spaceEnd = brackEnd + 1;

            safeMark(builder, markerStart, markerEnd, structMark);
            if (!isActive) {
                const widgetDeco = Decoration.replace({ widget: new TaskCheckboxWidget(isChecked) });
                safeMark(builder, brackStart, spaceEnd, widgetDeco);
            } else {
                safeMark(builder, brackStart, spaceEnd, markDim);
            }
            addInline(builder, text.slice(taskm[0].length), from + taskm[0].length, isActive, selection);
            continue;
        }

        const lm = text.match(/^(\s*)([-*+]|\d+[.)]) /);
        if (lm) {
            builder.add(from, from, listLineDeco);
            const ms = from + lm[1].length;
            const me = ms + lm[2].length + 1;
            safeMark(builder, ms, me, structMark);
            addInline(builder, text.slice(lm[0].length), from + lm[0].length, isActive, selection);
            continue;
        }

        if (text.length > 0) {
            addInline(builder, text, from, isActive, selection);
        }
    }

    return builder.finish();
}

function addInline(builder, text, offset, isActive, selection) {
    const mark = isActive ? markDim : markHide;
    let i = 0;

    while (i < text.length) {
        // Image: ![alt](url)
        if (text[i] === '!' && text[i + 1] === '[') {
            const altEnd = text.indexOf(']', i + 2);
            if (altEnd !== -1 && text[altEnd + 1] === '(') {
                const urlEnd = text.indexOf(')', altEnd + 2);
                if (urlEnd !== -1) {
                    const alt = text.slice(i + 2, altEnd);
                    let rawUrl = text.slice(altEnd + 2, urlEnd).trim();
                    const spaceIndex = rawUrl.indexOf(' ');
                    const url = spaceIndex !== -1 ? rawUrl.substring(0, spaceIndex) : rawUrl;

                    if (!isActive) {
                        safeMark(builder, offset + i, offset + i + 2, mark);
                        safeMark(builder, offset + i + 2, offset + altEnd, linkDeco);
                        
                        let replaceDeco = mark;
                        if (url) {
                            let srcUrl = url;
                            if (!url.startsWith('http') && window.__TAURI__ && window.__TAURI__.core) {
                                try { srcUrl = window.__TAURI__.core.convertFileSrc(url); } catch (e) {}
                            }
                            replaceDeco = Decoration.replace({ widget: new ImageWidget(srcUrl, alt, offset + i) });
                        }
                        safeMark(builder, offset + altEnd, offset + urlEnd + 1, replaceDeco);
                    } else {
                        safeMark(builder, offset + i, offset + urlEnd + 1, markDim);
                    }
                    i = urlEnd + 1;
                    continue;
                }
            }
        }

        // Inline math: $...$
        if (text[i] === '$' && text[i + 1] !== '$') {
            const end = text.indexOf('$', i + 1);
            if (end !== -1 && end > i + 1 && text[end + 1] !== '$') {
                const mathText = text.slice(i + 1, end).trim();
                const isMathActive = selection.head >= offset + i && selection.head <= offset + end + 1;

                if (isMathActive) {
                    safeMark(builder, offset + i, offset + i + 1, mark);
                    safeMark(builder, offset + i + 1, offset + end, mathInlineDeco);
                    safeMark(builder, offset + end, offset + end + 1, mark);
                } else {
                    const widgetDeco = Decoration.replace({ widget: new MathInlineWidget(mathText, offset + i) });
                    safeMark(builder, offset + i, offset + end + 1, widgetDeco);
                }
                i = end + 1;
                continue;
            }
        }

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

        if (text[i] === '~' && text[i + 1] === '~') {
            const end = text.indexOf('~~', i + 2);
            if (end !== -1) {
                safeMark(builder, offset + i, offset + i + 2, mark);
                safeMark(builder, offset + i + 2, offset + end, strikeDeco);
                safeMark(builder, offset + end, offset + end + 2, mark);
                i = end + 2;
                continue;
            }
        }

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

function addTableRow(builder, text, from) {
    for (let j = 0; j < text.length; j++) {
        if (text[j] === '|') safeMark(builder, from + j, from + j + 1, markDim);
    }
}

function findClose(text, start, char) {
    for (let j = start; j < text.length; j++) {
        if (text[j] === char && text[j + 1] !== char && (j === 0 || text[j - 1] !== char)) return j;
    }
    return -1;
}

function safeMark(builder, from, to, deco) {
    if (from < to) builder.add(from, to, deco);
}

const blockDecoField = StateField.define({
    create(state) {
        return buildDecorations(state);
    },
    update(value, tr) {
        if (tr.docChanged || tr.selection) {
            return buildDecorations(tr.state);
        }
        return value;
    },
    provide: f => EditorView.decorations.from(f)
});

export function markdownDecorations() { return blockDecoField; }
