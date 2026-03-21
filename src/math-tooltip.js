import { showTooltip } from "@codemirror/view";
import { StateField } from "@codemirror/state";
import katex from "katex";
import { findBlocks } from './markdown-decorations.js';

export function mathTooltip() {
    return mathTooltipField;
}

const mathTooltipField = StateField.define({
    create: getMathTooltip,
    update(tooltips, tr) {
        if (!tr.docChanged && !tr.selection) return tooltips;
        return getMathTooltip(tr.state);
    },
    provide: f => showTooltip.computeN([f], state => {
        const tooltip = state.field(f);
        return tooltip ? [tooltip] : [];
    })
});

function getMathTooltip(state) {
    const pos = state.selection.main.head;
    const line = state.doc.lineAt(pos);
    const text = line.text;
    const offset = pos - line.from;

    // 1. Fast check if inline math
    let i = 0;
    while (i < text.length) {
        if (text[i] === '`') {
            const end = text.indexOf('`', i + 1);
            if (end !== -1) {
                i = end + 1;
                continue;
            }
        }

        if (text[i] === '$' && text[i + 1] !== '$' && text[i + 1] !== ' ' && text[i + 1] !== '\t') {
            let end = text.indexOf('$', i + 1);
            while (end !== -1) {
                if (text[end + 1] !== '$' && text[end - 1] !== ' ' && text[end - 1] !== '\t') break;
                end = text.indexOf('$', end + 1);
            }
            if (end !== -1 && text[end + 1] === '$') end = -1;
            
            // If completely unclosed, make sure it doesn't look like currency
            if (end === -1) {
                if (/\d/.test(text[i + 1])) {
                    i++;
                    continue; 
                }
            }

            const actualEnd = end !== -1 ? end : text.length;
            if (offset >= i && offset <= actualEnd + (end !== -1 ? 1 : 0)) {
                const mathText = text.slice(i + 1, actualEnd).trim();
                return createTooltip(line.from + i, mathText, false);
            }
            if (end !== -1) {
                i = end + 1;
                continue;
            } else {
                break;
            }
        }
        i++;
    }

    // 2. Block math check via AST parser
    const blocks = findBlocks(state.doc, true);
    const block = blocks.find(b => b.type === 'math' && line.number >= b.startLine && line.number <= b.endLine);
    
    if (block) {
        return createTooltip(state.doc.line(block.startLine).from, block.content, true);
    }

    return null;
}

function createTooltip(pos, mathText, isBlock) {
    return {
        pos: pos,
        above: true,
        strictSide: true,
        arrow: true,
        create: () => {
            const dom = document.createElement("div");
            dom.className = "cm-tooltip-math";
            try {
                katex.render(mathText, dom, { displayMode: isBlock, throwOnError: false });
            } catch (e) {
                dom.textContent = mathText;
            }
            return { dom };
        }
    };
}
