import { showTooltip } from "@codemirror/view";
import { StateField } from "@codemirror/state";
import katex from "katex";

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
        if (text[i] === '$' && text[i + 1] !== '$') {
            let end = text.indexOf('$', i + 1);
            if (end !== -1 && text[end + 1] === '$') end = -1;
            
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

    // 2. Fast check for block math around cursor
    let blockStartLine = -1;
    let blockEndLine = -1;

    // Scan backwards to find the nearest $$
    for (let j = line.number; j >= Math.max(1, line.number - 50); j--) {
        if (/^\$\$\s*$/.test(state.doc.line(j).text)) {
            // Check if this is an opening or closing by scanning further back,
            // but for a simple tooltip scanner, finding nearest bounds is usually ok.
            blockStartLine = j;
            break;
        }
    }

    // Scan forwards to find the terminating $$
    for (let j = line.number; j <= Math.min(state.doc.lines, line.number + 50); j++) {
        if (/^\$\$\s*$/.test(state.doc.line(j).text)) {
            // Cannot be the same line if we are looking for a pair
            if (j !== blockStartLine) {
                blockEndLine = j;
                break;
            }
        }
    }

    if (blockStartLine !== -1 && blockEndLine === -1) {
        blockEndLine = Math.min(state.doc.lines, blockStartLine + 50);
    }

    if (blockStartLine !== -1 && blockEndLine !== -1 && blockStartLine <= blockEndLine) {
        if (line.number >= blockStartLine && line.number <= blockEndLine) {
            let mathLines = [];
            for (let j = blockStartLine + 1; j < blockEndLine; j++) {
                mathLines.push(state.doc.line(j).text);
            }
            const mathText = mathLines.join('\n');
            // Show tooltip above the start of the block
            return createTooltip(state.doc.line(blockStartLine).from, mathText, true);
        }
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
