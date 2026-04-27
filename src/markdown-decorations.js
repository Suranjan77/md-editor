/**
 * Markdown Decorations for CodeMirror 6
 */
import { Decoration, WidgetType, EditorView } from "@codemirror/view";
import { RangeSetBuilder, StateField, Facet } from "@codemirror/state";
import { openFile } from "./ipc.js";
import katex from "katex";
import MarkdownIt from "markdown-it";
import hljs from "highlight.js";
import "highlight.js/styles/atom-one-dark.css";

const md = new MarkdownIt();

/** Facet that holds the current file's vault-relative path (e.g. "A/notes.md"). */
export const currentFilePath = Facet.define({ combine: (v) => v[v.length - 1] || "" });

// Line Rules
const headingLines = [
  Decoration.line({ class: "md-h1-line" }),
  Decoration.line({ class: "md-h2-line" }),
  Decoration.line({ class: "md-h3-line" }),
  Decoration.line({ class: "md-h4-line" }),
  Decoration.line({ class: "md-h5-line" }),
  Decoration.line({ class: "md-h6-line" }),
];
const bqLine = Decoration.line({ class: "md-blockquote-line" });
const codeLineD = Decoration.line({ class: "md-code-line" });
const fenceOpen = Decoration.line({ class: "md-fence-line md-fence-open" });
const fenceClose = Decoration.line({ class: "md-fence-line md-fence-close" });
const mathLineD = Decoration.line({ class: "md-math-line" });
const mathFenceD = Decoration.line({ class: "md-math-line md-math-fence" });
const mathFenceOpenD = Decoration.line({
  class: "md-math-line md-math-fence md-math-fence-open",
});
const mathFenceCloseD = Decoration.line({
  class: "md-math-line md-math-fence md-math-fence-close",
});
const hrLine = Decoration.line({ class: "md-hr-line" });
const listLineDeco = Decoration.line({ class: "md-list-line" });
const tableFirst = Decoration.line({ class: "md-table-line md-table-first" });
const tableMid = Decoration.line({ class: "md-table-line" });
const tableLast = Decoration.line({ class: "md-table-line md-table-last" });
const tableOnly = Decoration.line({
  class: "md-table-line md-table-first md-table-last",
});
const tableDelim = Decoration.line({ class: "md-table-line md-table-delim" });

// Mark Rules
const markDim = Decoration.mark({ class: "md-mark" });
const markHide = Decoration.replace({});
const strongDeco = Decoration.mark({ class: "cm-strong" });
const emDeco = Decoration.mark({ class: "cm-em" });
const codeDeco = Decoration.mark({ class: "cm-inline-code" });
const linkDeco = Decoration.mark({ class: "cm-link" });
const wikiDeco = Decoration.mark({ class: "cm-wikilink" });
const strikeDeco = Decoration.mark({ class: "cm-strikethrough" });
const mathInlineDeco = Decoration.mark({ class: "md-math-inline" });

const imageWidgetImageCache = new Map();

/** Clear cached blob URLs to free memory (call on vault switch). */
export function clearImageCache() {
  for (const promise of imageWidgetImageCache.values()) {
    promise.then((url) => { if (url) URL.revokeObjectURL(url); });
  }
  imageWidgetImageCache.clear();
}

// ── Live Preview Widgets ───────────────────────────────────────────
class TaskCheckboxWidget extends WidgetType {
  constructor(checked) {
    super();
    this.checked = checked;
  }
  toDOM(view) {
    const span = document.createElement("span");
    span.className = "md-task-checkbox" + (this.checked ? " checked" : "");
    span.textContent = this.checked ? "✓" : "";
    span.onclick = (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (!view) return;
      const pos = view.posAtDOM(span);
      if (pos == null) return;
      // Find the [ ] or [x] bracket from the widget position
      const line = view.state.doc.lineAt(pos);
      const taskMatch = line.text.match(/^(\s*)([-*+])\s\[([ xX])\]/);
      if (!taskMatch) return;
      const checkCharPos = line.from + taskMatch[1].length + taskMatch[2].length + 2; // position of the space/x inside []
      const newChar = this.checked ? " " : "x";
      view.dispatch({
        changes: { from: checkCharPos, to: checkCharPos + 1, insert: newChar },
      });
    };
    return span;
  }
  eq(other) {
    return this.checked === other.checked;
  }
  ignoreEvent() {
    return false; // Allow click events to reach our handler
  }
}

class ImageWidget extends WidgetType {
  constructor(src, alt, seq, is_local) {
    super();
    this.src = src;
    this.alt = alt;
    this.seq = seq;
    this.is_local = is_local;
  }
  toDOM(view) {
    const container = document.createElement("div");
    container.className = "md-image-container";
    container.id = "figure-" + this.seq;

    const img = document.createElement("img");
    img.className = "md-image-widget";
    container.appendChild(img);

    if (this.is_local) {
      if (!imageWidgetImageCache.has(this.src)) {
        const imageFetch = openFile(this.src)
          .then((bytes) => {
            const img_ext = this.src.substring(this.src.lastIndexOf(".") + 1);
            const uint8Array = new Uint8Array(bytes);
            const blob = new Blob([uint8Array], { type: `image/${img_ext}` });
            return URL.createObjectURL(blob);
          })
          .catch((err) => {
            console.error("Failed to fetch local image:", err);
            return "";
          });

        imageWidgetImageCache.set(this.src, imageFetch);
      }

      imageWidgetImageCache.get(this.src).then((imageUrl) => {
        if (imageUrl) img.src = imageUrl;
      });
    } else {
      img.src = this.src;
    }

    img.alt = this.alt || "";
    img.loading = "lazy";
    img.onerror = () => {
      img.style.display = "none";
    };

    const caption = document.createElement("div");
    caption.className = "md-image-caption";
    caption.textContent = `Figure ${this.seq}: ${this.alt}`;
    container.appendChild(caption);

    container.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(container);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return container;
  }
  eq(other) {
    return (
      this.src === other.src && this.alt === other.alt && this.seq === other.seq
    );
  }
}

class HrWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM(view) {
    const div = document.createElement("div");
    div.className = "md-hr-widget";

    div.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(div);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return div;
  }
}

class MathBlockWidget extends WidgetType {
  constructor(mathText, seq) {
    super();
    this.mathText = mathText;
    this.seq = seq;
  }
  eq(other) {
    return this.mathText === other.mathText && this.seq === other.seq;
  }
  toDOM(view) {
    const div = document.createElement("div");
    div.className = "md-math-render-block";
    div.id = "equation-" + this.seq;

    const eqWrapper = document.createElement("div");
    eqWrapper.className =
      "md-math-eq-wrapper flex items-center justify-center relative w-full";

    const mathContent = document.createElement("div");
    try {
      katex.render(this.mathText, mathContent, {
        displayMode: true,
        throwOnError: false,
        output: "html",
      });    } catch (e) {
      mathContent.textContent = this.mathText;
    }

    const number = document.createElement("div");
    number.className =
      "md-math-number absolute right-0 text-[10px] opacity-40 font-mono tracking-wider";
    number.textContent = `(${this.seq})`;

    eqWrapper.appendChild(mathContent);
    eqWrapper.appendChild(number);
    div.appendChild(eqWrapper);

    div.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(div);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return div;
  }
}

class MathInlineWidget extends WidgetType {
  constructor(mathText) {
    super();
    this.mathText = mathText;
  }
  eq(other) {
    return this.mathText === other.mathText;
  }
  toDOM(view) {
    const span = document.createElement("span");
    span.className = "md-math-render-inline";
    try {
      katex.render(this.mathText, span, {
        displayMode: false,
        throwOnError: false,
        output: "html",
      });
    } catch (e) {
      span.textContent = this.mathText;
    }

    span.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(span);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return span;
  }
}

class CodeInlineWidget extends WidgetType {
  constructor(codeText) {
    super();
    this.codeText = codeText;
  }
  eq(other) {
    return this.codeText === other.codeText;
  }
  toDOM(view) {
    const span = document.createElement("span");
    span.className = "cm-inline-code";
    span.textContent = this.codeText;

    span.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(span);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return span;
  }
}

class CodeBlockWidget extends WidgetType {
  constructor(codeText, lang, seq) {
    super();
    this.codeText = codeText;
    this.lang = lang;
    this.seq = seq;
  }
  eq(other) {
    return (
      this.codeText === other.codeText &&
      this.lang === other.lang &&
      this.seq === other.seq
    );
  }
  toDOM(view) {
    const pre = document.createElement("pre");
    pre.className = "md-code-render-block hljs relative";
    pre.id = "listing-" + this.seq;

    const header = document.createElement("div");
    header.className =
      "md-code-header absolute top-[2px] right-[6px] text-[9px] text-[#b1ccc6]/40 font-mono tracking-wider uppercase";
    header.textContent = `Listing ${this.seq}`;

    const code = document.createElement("code");

    if (this.lang && hljs.getLanguage(this.lang)) {
      try {
        code.innerHTML = hljs.highlight(this.codeText, {
          language: this.lang,
        }).value;
      } catch (e) {
        code.textContent = this.codeText;
      }
    } else {
      code.textContent = this.codeText;
    }
    pre.appendChild(header);
    pre.appendChild(code);

    pre.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(pre);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return pre;
  }
}

class TableBlockWidget extends WidgetType {
  constructor(mdText) {
    super();
    this.mdText = mdText;
  }
  eq(other) {
    return this.mdText === other.mdText;
  }
  toDOM(view) {
    const div = document.createElement("div");
    div.className = "md-table-render-block";
    
    let html = md.render(this.mdText);
    // Add support for wikilinks and task checkboxes inside tables
    html = html.replace(/\[\[(.*?)\]\]/g, '<span class="cm-wikilink">[[$1]]</span>');
    html = html.replace(/\[([xX])\]/g, '<span class="md-task-checkbox checked">✓</span>');
    html = html.replace(/\[ \]/g, '<span class="md-task-checkbox"></span>');
    
    div.innerHTML = html;

    div.onmousedown = (e) => {
      if (view) {
        e.preventDefault();
        e.stopPropagation();
        const pos = view.posAtDOM(div);
        if (pos !== null) {
          view.dispatch({ selection: { anchor: pos } });
          view.focus();
        }
      }
    };
    return div;
  }
}

function isTableLine(doc, lineNum) {
  if (lineNum < 1 || lineNum > doc.lines) return false;
  return doc.line(lineNum).text.includes("|");
}

// Scans document to construct bounding coordinates for multi-line block elements
export function findBlocks(doc, includeUnclosed = false) {
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
        const infoString = text.slice(fenceMatch[0].length).trim();
        if (!infoString.includes(fenceChar)) {
          inCode = {
            char: fenceChar,
            lang: infoString,
            startLine: i,
            startFrom: line.from,
          };
        }
        i++;
        continue;
      }
    } else {
      const closeRegex = new RegExp(`^(\\s*)(${inCode.char}{3,})\\s*$`);
      if (closeRegex.test(text)) {
        let codeLines = [];
        for (let j = inCode.startLine + 1; j < i; j++) {
          codeLines.push(doc.line(j).text);
        }
        blocks.push({
          type: "code",
          from: inCode.startFrom,
          to: line.to,
          startLine: inCode.startLine,
          endLine: i,
          lang: inCode.lang,
          content: codeLines.join("\n"),
        });
        inCode = false;
      }
      i++;
      continue;
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
        if (includeUnclosed && mathLines.length >= 50) {
          break;
        }
      }
      if ((closed || includeUnclosed) && mathLines.length > 0) {
        blocks.push({
          type: "math",
          from: startFrom,
          to: endTo,
          startLine,
          endLine: i - 1,
          content: mathLines.join("\n"),
        });
      }
      continue;
    }

    // Table Block
    if (text.includes("|") && isTableLine(doc, i)) {
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
          type: "table",
          from: startFrom,
          to: endTo,
          startLine,
          endLine: i - 1,
          content: tableLines.join("\n"),
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

  // Resolve the current file's parent directory for relative image paths
  const filePath = state.facet(currentFilePath);
  const currentFileDir = filePath.includes("/") ? filePath.substring(0, filePath.lastIndexOf("/")) : "";

  // Phase 1: Identify all multiline Live Preview structural blocks
  const blocks = findBlocks(doc);
  const passState = { math: 0, code: 0, img: 0 };

  // Phase 2: Process document lines sequentially, collapsing inactive blocks to HTML Native widgets
  for (let i = 1; i <= doc.lines; i++) {
    const line = doc.line(i);
    const text = line.text;
    const from = line.from;
    const isActive = i === activeLine;
    const structMark = markDim;
    const mark = isActive ? markDim : markHide;

    const block = blocks.find((b) => i >= b.startLine && i <= b.endLine);
    if (block) {
      const isBlockActive = cursor >= block.from && cursor <= block.to;

      if (!isBlockActive) {
        // Return a fully-rendered block replacement widget if cursor is outside
        if (i === block.startLine) {
          let widgetDeco;
          if (block.type === "math") {
            passState.math++;
            widgetDeco = Decoration.replace({
              widget: new MathBlockWidget(block.content, passState.math),
              block: true,
            });
          } else if (block.type === "table") {
            widgetDeco = Decoration.replace({
              widget: new TableBlockWidget(block.content),
              block: true,
            });
          } else if (block.type === "code") {
            passState.code++;
            widgetDeco = Decoration.replace({
              widget: new CodeBlockWidget(
                block.content,
                block.lang,
                passState.code,
              ),
              block: true,
            });
          }
          builder.add(block.from, block.to, widgetDeco);
          i = block.endLine;
        }
        continue;
      }

      // Render active blocks natively in plain-text mode with raw structural markers
      if (block.type === "math") {
        if (i === block.startLine) {
          builder.add(from, from, mathFenceOpenD);
          safeMark(builder, from, from + text.length, markDim);
        } else if (i === block.endLine) {
          builder.add(from, from, mathFenceCloseD);
          safeMark(builder, from, from + text.length, markDim);
        } else {
          builder.add(from, from, mathLineD);
        }
        continue;
      }
      if (block.type === "table") {
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
      addInline(
        builder,
        text.slice(hm[0].length),
        from + hm[0].length,
        isActive,
        selection,
        passState,
        currentFileDir,
      );
      continue;
    }

    if (/^\s*([-*_])\s*\1\s*\1[\s\-*_]*$/.test(text)) {
      if (!isActive) {
        const widgetDeco = Decoration.replace({
          widget: new HrWidget(),
          block: true,
        });
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
      addInline(
        builder,
        text.slice(bqm[1].length),
        from + bqm[1].length,
        isActive,
        selection,
        passState,
        currentFileDir,
      );
      continue;
    }

    const taskm = text.match(/^(\s*)([-*+])\s\[([ xX])\]\s/);
    if (taskm) {
      builder.add(from, from, listLineDeco);
      const isChecked = taskm[3] !== " ";
      const markerStart = from + taskm[1].length;
      const markerEnd = markerStart + taskm[2].length + 1;
      const brackStart = markerEnd;
      const brackEnd = brackStart + 3;
      const spaceEnd = brackEnd + 1;

      safeMark(builder, markerStart, markerEnd, structMark);
      if (!isActive) {
        const widgetDeco = Decoration.replace({
          widget: new TaskCheckboxWidget(isChecked),
        });
        safeMark(builder, brackStart, spaceEnd, widgetDeco);
      } else {
        safeMark(builder, brackStart, spaceEnd, markDim);
      }
      addInline(
        builder,
        text.slice(taskm[0].length),
        from + taskm[0].length,
        isActive,
        selection,
        passState,
        currentFileDir,
      );
      continue;
    }

    const lm = text.match(/^(\s*)([-*+]|\d+[.)]) /);
    if (lm) {
      builder.add(from, from, listLineDeco);
      const ms = from + lm[1].length;
      const me = ms + lm[2].length + 1;
      safeMark(builder, ms, me, structMark);
      addInline(
        builder,
        text.slice(lm[0].length),
        from + lm[0].length,
        isActive,
        selection,
        passState,
        currentFileDir,
      );
      continue;
    }

    if (text.length > 0) {
      addInline(builder, text, from, isActive, selection, passState, currentFileDir);
    }
  }

  return builder.finish();
}

function addInline(builder, text, offset, isActive, selection, passState, currentFileDir = "") {
  const mark = isActive ? markDim : markHide;
  let i = 0;

  while (i < text.length) {
    // Image: ![alt](url)
    if (text[i] === "!" && text[i + 1] === "[") {
      const altEnd = text.indexOf("]", i + 2);
      if (altEnd !== -1 && text[altEnd + 1] === "(") {
        const urlEnd = text.indexOf(")", altEnd + 2);
        if (urlEnd !== -1) {
          const alt = text.slice(i + 2, altEnd);
          let rawUrl = text.slice(altEnd + 2, urlEnd).trim();
          const spaceIndex = rawUrl.indexOf(" ");
          const url =
            spaceIndex !== -1 ? rawUrl.substring(0, spaceIndex) : rawUrl;

          if (!isActive) {
            if (url) {
              let srcUrl = url;
              let is_local = false;
              if (
                !url.startsWith("http") &&
                window.__TAURI__ &&
                window.__TAURI__.core
              ) {
                if (url.startsWith("./")) {
                  srcUrl = url.substring(2);
                } else if (url.startsWith("/")) {
                  srcUrl = url.substring(1);
                }
                // Resolve relative to current file's directory
                if (currentFileDir && !srcUrl.startsWith("/")) {
                  srcUrl = currentFileDir + "/" + srcUrl;
                }
                is_local = true;
              }
              passState.img++;
              const replaceDeco = Decoration.replace({
                widget: new ImageWidget(srcUrl, alt, passState.img, is_local),
              });
              safeMark(builder, offset + i, offset + urlEnd + 1, replaceDeco);
            } else {
              safeMark(builder, offset + i, offset + i + 2, mark);
              safeMark(builder, offset + i + 2, offset + altEnd, linkDeco);
              safeMark(builder, offset + altEnd, offset + urlEnd + 1, mark);
            }
          } else {
            safeMark(builder, offset + i, offset + urlEnd + 1, markDim);
          }
          i = urlEnd + 1;
          continue;
        }
      }
    }

    // Inline code: `...`
    if (text[i] === "`") {
      const end = text.indexOf("`", i + 1);
      if (end !== -1) {
        const codeText = text.slice(i + 1, end);
        const isCodeActive =
          selection.head >= offset + i && selection.head <= offset + end + 1;

        if (isCodeActive) {
          safeMark(builder, offset + i, offset + end + 1, mark);
        } else {
          const widgetDeco = Decoration.replace({
            widget: new CodeInlineWidget(codeText),
          });
          safeMark(builder, offset + i, offset + end + 1, widgetDeco);
        }
        i = end + 1;
        continue;
      }
    }

    // Inline math: $...$
    if (
      text[i] === "$" &&
      text[i + 1] !== "$" &&
      text[i + 1] !== " " &&
      text[i + 1] !== "\t"
    ) {
      let end = text.indexOf("$", i + 1);
      while (end !== -1) {
        // Ensure it's not a $$ and no space before closing $
        if (
          text[end + 1] !== "$" &&
          text[end - 1] !== " " &&
          text[end - 1] !== "\t"
        )
          break;
        end = text.indexOf("$", end + 1);
      }

      if (end !== -1 && end > i + 1) {
        const mathText = text.slice(i + 1, end).trim();
        const isMathActive =
          selection.head >= offset + i && selection.head <= offset + end + 1;

        if (isMathActive) {
          safeMark(builder, offset + i, offset + i + 1, mark);
          safeMark(builder, offset + i + 1, offset + end, mathInlineDeco);
          safeMark(builder, offset + end, offset + end + 1, mark);
        } else {
          const widgetDeco = Decoration.replace({
            widget: new MathInlineWidget(mathText),
          });
          safeMark(builder, offset + i, offset + end + 1, widgetDeco);
        }
        i = end + 1;
        continue;
      }
    }

    if (text[i] === "[" && text[i + 1] === "[") {
      const end = text.indexOf("]]", i + 2);
      if (end !== -1) {
        safeMark(builder, offset + i, offset + i + 2, mark);
        safeMark(builder, offset + i + 2, offset + end, wikiDeco);
        safeMark(builder, offset + end, offset + end + 2, mark);
        i = end + 2;
        continue;
      }
    }

    if (text[i] === "[") {
      const textEnd = text.indexOf("]", i + 1);
      if (textEnd !== -1 && text[textEnd + 1] === "(") {
        const urlEnd = text.indexOf(")", textEnd + 2);
        if (urlEnd !== -1) {
          safeMark(builder, offset + i, offset + i + 1, mark);
          safeMark(builder, offset + i + 1, offset + textEnd, linkDeco);
          safeMark(builder, offset + textEnd, offset + urlEnd + 1, mark);
          i = urlEnd + 1;
          continue;
        }
      }
    }

    if (text[i] === "~" && text[i + 1] === "~") {
      const end = text.indexOf("~~", i + 2);
      if (end !== -1) {
        safeMark(builder, offset + i, offset + i + 2, mark);
        safeMark(builder, offset + i + 2, offset + end, strikeDeco);
        safeMark(builder, offset + end, offset + end + 2, mark);
        i = end + 2;
        continue;
      }
    }

    if (text[i] === "*" && text[i + 1] === "*") {
      const end = text.indexOf("**", i + 2);
      if (end !== -1) {
        safeMark(builder, offset + i, offset + i + 2, mark);
        safeMark(builder, offset + i + 2, offset + end, strongDeco);
        safeMark(builder, offset + end, offset + end + 2, mark);
        i = end + 2;
        continue;
      }
    }

    if (
      text[i] === "*" &&
      text[i + 1] !== "*" &&
      (i === 0 || text[i - 1] !== "*")
    ) {
      const end = findClose(text, i + 1, "*");
      if (end !== -1) {
        safeMark(builder, offset + i, offset + i + 1, mark);
        safeMark(builder, offset + i + 1, offset + end, emDeco);
        safeMark(builder, offset + end, offset + end + 1, mark);
        i = end + 1;
        continue;
      }
    }

    if (text[i] === "_" && text[i + 1] === "_") {
      const end = text.indexOf("__", i + 2);
      if (end !== -1) {
        safeMark(builder, offset + i, offset + i + 2, mark);
        safeMark(builder, offset + i + 2, offset + end, strongDeco);
        safeMark(builder, offset + end, offset + end + 2, mark);
        i = end + 2;
        continue;
      }
    }

    if (
      text[i] === "_" &&
      text[i + 1] !== "_" &&
      (i === 0 || text[i - 1] !== "_")
    ) {
      const end = findClose(text, i + 1, "_");
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
    if (text[j] === "|") safeMark(builder, from + j, from + j + 1, markDim);
  }
}

function findClose(text, start, char) {
  for (let j = start; j < text.length; j++) {
    if (
      text[j] === char &&
      text[j + 1] !== char &&
      (j === 0 || text[j - 1] !== char)
    )
      return j;
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
  provide: (f) => EditorView.decorations.from(f),
});

export function markdownDecorations() {
  return blockDecoField;
}
