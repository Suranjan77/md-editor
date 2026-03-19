# Build instructions: WYSIWYG markdown editor (Tauri + CodeMirror 6)

## Project overview

Build a desktop WYSIWYG markdown editor using the Tauri framework with CodeMirror 6 as the editor engine. The editor should feel like Typora — headings render large as you type, bold text appears bold, syntax markers (`#`, `**`, `` ` ``) are subtly dimmed, and the cursor always maps 1:1 to the raw markdown text.

CodeMirror 6 handles the hard problems (cursor management, input/IME, selections, undo/redo, viewport rendering). We write CM6 plugins for live-preview markdown decorations.

---

## Why CodeMirror 6

Building a Typora-like editor on raw `contenteditable` is extremely difficult — cursor management, input composition (IME), and DOM mutation tracking are unsolvable without a framework. Every production editor (Typora, Obsidian, Notion) uses an editor engine internally.

CodeMirror 6 solves this with its **Decoration API**: the document model is always the raw markdown text, but visual overlays make headings large, bold text bold, etc. This means:
- Cursor position always maps 1:1 to raw text (no offset mismatch)
- No `innerHTML` re-renders or cursor restoration hacks
- Built-in undo/redo, keybindings, IME handling, accessibility
- Incremental rendering (only the viewport is rendered)

---

## Tech stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri 2.x |
| Backend language | Rust (stable) |
| Editor engine | CodeMirror 6 (`@codemirror/view`, `@codemirror/state`, `@codemirror/lang-markdown`) |
| Markdown styling | Custom CM6 ViewPlugin with Decoration API |
| Build tool | Vite (for bundling CM6 + plugins) |
| IPC serialisation | `serde` + `serde_json` on Rust side |

Do not use React, Vue, or any UI framework. The frontend is CodeMirror 6 + vanilla JS for the sidebar/chrome. Do not use Electron.

---

## Repository structure

```
/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs               # Tauri app entry point
│   │   ├── commands.rs           # Tauri commands (file ops, backlinks)
│   │   ├── file_index.rs         # Wikilink graph and backlinks
│   │   └── fs_commands.rs        # File system utilities
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── main.js                   # App entry, sidebar, file management
│   ├── editor.js                 # CodeMirror 6 setup + extensions
│   ├── markdown-decorations.js   # CM6 ViewPlugin for live-preview decorations
│   ├── wikilink-plugin.js        # CM6 plugin for [[wikilink]] highlighting + click
│   ├── ipc.js                    # Tauri IPC wrappers
│   ├── sidebar.js                # File list rendering
│   └── style.css                 # App chrome + CodeMirror theme
├── index.html
├── package.json
└── vite.config.js
```

---

## Frontend (CodeMirror 6)

### 1. Editor setup (`editor.js`)

Create a CodeMirror 6 `EditorView` with these extensions:

```js
import { EditorView, keymap, lineNumbers } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { markdownDecorations } from './markdown-decorations.js';
import { wikilinkPlugin } from './wikilink-plugin.js';
import { editorTheme } from './theme.js';

const view = new EditorView({
  state: EditorState.create({
    doc: '',
    extensions: [
      markdown(),
      markdownDecorations(),
      wikilinkPlugin(),
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      editorTheme,
      // Save listener
      EditorView.updateListener.of(update => {
        if (update.docChanged) onDocChanged(update);
      }),
    ],
  }),
  parent: document.getElementById('editor-container'),
});
```

### 2. Live-preview decorations (`markdown-decorations.js`)

This is the core of the Typora-like experience. Create a `ViewPlugin` that:

1. **Scans visible lines** using `view.visibleRanges` (never processes lines outside the viewport)
2. **Matches markdown patterns** with regex on each line
3. **Creates Decorations** that style the text in-place:

```
Pattern          | Decoration
# Heading        | Heading text → large/bold, # marker → Decoration.mark({ class: 'md-mark' })
**bold**         | Bold text → Decoration.mark({ class: 'cm-strong' }), ** → Decoration.mark({ class: 'md-mark' })
*italic*         | Italic text → Decoration.mark({ class: 'cm-em' }), * → Decoration.mark({ class: 'md-mark' })
`code`           | Code text → Decoration.mark({ class: 'cm-inline-code' }), ` → Decoration.mark({ class: 'md-mark' })
[text](url)      | Link text → Decoration.mark({ class: 'cm-link' }), [](url) → Decoration.mark({ class: 'md-mark' })
```

**Key rules:**
- Use `Decoration.mark()` for inline styling (adds CSS classes to ranges)
- Use `Decoration.line()` for line-level styling (heading size, blockquote border)
- Use `Decoration.widget()` for horizontal rules (replace `---` with an `<hr>` widget)
- **Never use `Decoration.replace()`** on syntax markers — they must remain in the text for the cursor to work. Instead, use CSS to make them nearly invisible.
- Only process lines in `view.visibleRanges` for performance

### 3. Wikilink plugin (`wikilink-plugin.js`)

- Match `[[target]]` and `[[target|alias]]` patterns
- Add `Decoration.mark()` with a class for green styling
- Dim `[[` and `]]` markers
- On click, navigate to the target file via IPC

### 4. Theme (`style.css` + `editorTheme`)

Use `EditorView.theme()` and `EditorView.baseTheme()` to style:

```js
const editorTheme = EditorView.theme({
  '&': { fontSize: '16px', fontFamily: 'Inter, sans-serif' },
  '.cm-content': { padding: '48px 80px', maxWidth: '860px', margin: '0 auto' },
  '.cm-line': { lineHeight: '1.8' },

  // Syntax markers: nearly invisible
  '.md-mark': { opacity: '0.25', fontSize: '0.75em', fontFamily: 'JetBrains Mono, monospace' },
  // Show marks slightly when cursor is on the same line
  '.cm-activeLine .md-mark': { opacity: '0.5' },

  // Headings
  '.md-h1': { fontSize: '2em', fontWeight: '700' },
  '.md-h2': { fontSize: '1.5em', fontWeight: '600', color: '#6c8cff' },
  '.md-h3': { fontSize: '1.25em', fontWeight: '600' },

  // Inline
  '.cm-strong': { fontWeight: '600' },
  '.cm-em': { fontStyle: 'italic', color: '#ffc46b' },
  '.cm-inline-code': { background: '#22262f', color: '#6c8cff', padding: '1px 5px', borderRadius: '4px' },

  // Links
  '.cm-link': { color: '#6c8cff', textDecoration: 'none' },
  '.cm-wikilink': { color: '#6bffb8', fontWeight: '500' },

  // Blockquotes
  '.md-blockquote': { borderLeft: '3px solid #6c8cff', paddingLeft: '14px' },

  // Code blocks
  '.md-code-line': { fontFamily: 'JetBrains Mono, monospace', fontSize: '0.9em' },
});
```

---

## Rust backend

The Rust backend is significantly simplified with CodeMirror 6 handling document state. Rust is responsible for:

### 1. File operations (`commands.rs`)

```rust
#[tauri::command]
fn open_file(path: String) -> Result<String, String>        // Returns file content as string
#[tauri::command]
fn save_file(path: String, content: String) -> Result<(), String>
#[tauri::command]
fn create_file(path: String) -> Result<(), String>
#[tauri::command]
fn delete_file(path: String) -> Result<(), String>
#[tauri::command]
fn list_vault(root: String) -> Result<Vec<FileEntry>, String>
#[tauri::command]
fn set_vault_root(path: String) -> Result<Vec<FileEntry>, String>
```

**Note:** No piece table, no tree-sitter, no AST diff. CodeMirror 6 owns the document state and parsing. Rust is a thin file I/O + indexing layer.

### 2. Wikilink index (`file_index.rs`)

```rust
struct FileIndex {
    outgoing: HashMap<PathBuf, HashSet<PathBuf>>,  // file → files it links to
    incoming: HashMap<PathBuf, HashSet<PathBuf>>,  // file → files that link to it
}
```

- On save, extract `[[target]]` patterns from the document text (sent from JS)
- Expose `get_backlinks(path) -> Vec<String>`

### 3. File entry type

```rust
#[derive(Serialize)]
struct FileEntry {
    path: String,
    name: String,
    is_dir: bool,
}
```

---

## Data flow

```
User types → CodeMirror handles it → CM6 decoration plugin re-decorates visible lines → User sees styled markdown

User saves → JS reads CM6 doc.toString() → IPC save_file(path, content) → Rust writes to disk

User opens file → IPC open_file(path) → Rust reads file → Returns string → JS calls view.dispatch({changes: ...})
```

**No round-trip IPC on keystrokes.** All editing is local in JavaScript. IPC is only used for file I/O and backlinks.

---

## Build and run

```bash
# Install dependencies
npm install

# Install Tauri CLI (if needed)
cargo install tauri-cli

# Dev server (Vite + Tauri)
cargo tauri dev

# Production build
cargo tauri build
```

### npm dependencies

```json
{
  "dependencies": {
    "@codemirror/view": "^6",
    "@codemirror/state": "^6",
    "@codemirror/lang-markdown": "^6",
    "@codemirror/commands": "^6",
    "@codemirror/language": "^6",
    "@lezer/markdown": "^1"
  },
  "devDependencies": {
    "vite": "^5",
    "@tauri-apps/api": "^2"
  }
}
```

---

## Implementation order

1. **Scaffold** — `npm create vite`, install CM6 packages, wire Tauri
2. **Basic editor** — CM6 EditorView with markdown language, dark theme
3. **Heading decorations** — `ViewPlugin` that styles `# ` lines as large headings, dims `#` marker
4. **Inline decorations** — bold, italic, code, links with dimmed syntax markers
5. **Wikilink plugin** — `[[target]]` detection, styling, click-to-navigate
6. **Code block decorations** — fence lines dimmed, code lines monospace
7. **Sidebar + file ops** — file list, open/save/create via Rust IPC
8. **Backlinks** — wikilink index in Rust, backlinks pane
9. **Polish** — blockquotes, tables, images, horizontal rules, transitions

---

## Key invariants

- CodeMirror 6 owns the document state. Rust never stores document text.
- All editing is local — no IPC round-trip on keystrokes.
- Decoration plugins only process visible lines (`view.visibleRanges`).
- Syntax markers are never removed from the text — they are styled with CSS (opacity, small font size) to appear dim.
- File index updates are async and never block the UI.
- The Rust backend is a thin file I/O layer, nothing more.
