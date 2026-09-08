# User Guide & Feature Manual

Welcome to the **MD Editor User Guide**. This manual covers all features, workflows, split-view reading tools, search modes, and keyboard shortcuts.

---

## 1. Getting Started with Vaults

MD Editor operates on standard local folders called **Vaults**.

```
MyResearchVault/
├── Lectures/
│   ├── Week1.md
│   └── Week2.md
├── Papers/
│   └── AttentionIsAllYouNeed.pdf
├── Images/
│   └── architecture.png
└── Summary.md
```

### Opening a Vault
1. Launch MD Editor.
2. Click **Open Folder as Vault** on the welcome screen (or press `Ctrl+O`).
3. Select any folder on your computer.
4. MD Editor indexes all Markdown files, PDFs, and images, displaying them in the sidebar tree.

### File Operations in the Sidebar
- **Open File**: Click any file in the sidebar tree to open it.
- **New Note / Folder**: Click the `+` icon in the sidebar header or right-click any folder.
- **Delete File**: Right-click a file and choose **Delete** (prompts for confirmation).
- **Reveal in OS**: Right-click and choose **Show in System File Manager**.

---

## 2. Writing in Markdown

The Markdown editor combines the clarity of live preview with the precision of plain text.

### Typora-Style Hybrid Live Preview
- **When reading**: Syntax markers (such as `**`, `*`, `~~`, `$$`, ```) are concealed, displaying clean typographic formatting.
- **When editing**: Moving your cursor into a formatted line or word instantly unveils the raw markdown markers so you can edit delimiters directly.

### Supported Markdown Elements

| Element | Syntax Example | Notes |
| :--- | :--- | :--- |
| **Headings** | `# Heading 1` to `###### Heading 6` | Listed in Table of Contents panel |
| **Emphasis** | `**bold**`, `*italic*`, `~~strikethrough~~` | Concealed when cursor is away |
| **Inline Code** | `` `code` `` | Styled with monospace font |
| **Code Blocks** | ```` ```rust ... ``` ```` | Fenced syntax highlighting via `syntect` |
| **LaTeX Math** | `$E = mc^2$` or `$$\int_0^\infty f(x)dx$$` | Rendered inline via `ratex-render` |
| **Blockquotes** | `> Quoted reference` | Displayed with left accent border |
| **Checkboxes** | `- [ ] Pending` / `- [x] Completed` | Interactive task items |
| **Tables** | `\| A \| B \|` | Independent horizontal scrolling for wide tables |
| **Wikilinks** | `[[Note Name]]` or `[[Note Name\|Label]]` | Resolves across subfolders by filename |

### Smart Editing Behaviors
- **Auto-Pairing**: Typing `(`, `[`, `{`, `"`, or `` ` `` automatically inserts the matching closing character.
- **Selection Wrap**: Selecting text and typing a delimiter wraps the selection.
- **Contraction Protection**: Typing an apostrophe inside contractions (e.g. `don't`, `it's`) will not insert a stray closing quote.
- **List Auto-Continuation**: Pressing `Enter` on a bulleted, numbered, or checkbox list item continues the list on the next line. Pressing `Enter` on an empty list item clears the marker.

---

## 3. PDF Reading, Sidecars & Linked Notes

Open any `.pdf` from your sidebar to enter the integrated PDF reader.

```
┌─────────────────────────────────┬─────────────────────────────────┐
│ Markdown Notes                  │ Reference PDF                   │
│                                 │                                 │
│ ## Chapter 4 Review             │ Figure 4.1: Attention Matrix    │
│ The attention weights derive... │ ┌─────────────────────────────┐ │
│                                 │ │ [ Highlighted Passage ]     │ │
│ [Jump to PDF](pdf://...?page=12)│ └─────────────────────────────┘ │
│                                 │                                 │
└─────────────────────────────────┴─────────────────────────────────┘
```

### PDF Viewer Features
- **Continuous Scrolling**: Seamless page-to-page reading.
- **Fit-to-Width & Zoom**: Automatically adjusts page scaling to fit the pane width.
- **TOC Navigation**: Displays embedded bookmarks or synthesizes an outline using typography heuristics if no bookmarks exist.
- **In-Text Reference Preview**: Right-clicking numbered citations (e.g., `(3.14)`, `Figure 2`, `Table 1`) displays an instant hover popup of the target definition without scrolling.

### Non-Destructive Sidecar Highlights
- Highlight text by dragging your mouse and clicking the **Highlight** popup button.
- Colors cycle automatically across five calm pastels: Yellow, Green, Blue, Pink, and Orange.
- The original PDF file is never modified; all highlights are saved in your portable SQLite settings database.

### Creating Linked Notes (`pdf://`)
1. Select text in a PDF and choose **Link to Note**.
2. Select an existing Markdown file or enter a new note name.
3. MD Editor appends a block containing the quoted passage, your notes, and a clickable `pdf://` deep link that jumps back to the exact page and highlight.

---

## 4. Search Navigation

MD Editor provides three distinct search modes:

1. **In-Note Search (`Ctrl+F` in Markdown)**:
   - Highlights all occurrences in the active document.
   - Use `Enter` / `Shift+Enter` (or next/previous arrows) to jump between matches.
2. **Global Vault Search (Toolbar Search Icon)**:
   - Powered by SQLite FTS5.
   - Searches across all Markdown notes and extracted PDF text in the vault.
3. **PDF Text Search (`Ctrl+F` in PDF Pane)**:
   - Highlights matches on rendered PDF pages.
   - **Loose Whitespace Mode**: Matches phrases even when broken across PDF line breaks, margins, or hyphenated words.

---

## 5. Study Tracker

Open the Tracker panel from the toolbar or via `Ctrl+P` -> `Open Tracker`.

- **Interval Timer**: Start a deep-work timer while reading or writing.
- **Daily Targets**: Track your daily study hours against a customized target.
- **Milestone Gates**: Track project requirements and course milestones.
- **Study Logs**: Review past study sessions and notes.

---

## 6. Comprehensive Keyboard Shortcuts

| Shortcut | Context | Action |
| :--- | :--- | :--- |
| `Ctrl+P` | Global | Open Command Palette (fuzzy file & action launcher) |
| `Ctrl+O` | Global | Open a new Vault folder |
| `Ctrl+S` | Global | Force immediate save of active note |
| `Ctrl+F` | Editor Pane | Find in active Markdown note |
| `Ctrl+F` | PDF Pane | Search text in active PDF |
| `Ctrl+\` | Global | Toggle Sidebar (File Tree) |
| `Ctrl+T` | Global | Toggle Table of Contents panel |
| `Ctrl+B` | Global | Toggle Backlinks panel |
| `Ctrl+Z` | Editor Pane | Undo (word-sized run) |
| `Ctrl+Y` / `Ctrl+Shift+Z` | Editor Pane | Redo |
| `Ctrl+A` | Editor Pane | Select entire document |
| `Escape` | Global | Close Command Palette / dismiss active modal |
| `Enter` | Command Palette | Execute highlighted command or open note |
| `Up` / `Down` | Command Palette | Move selection in palette |
| `F3` / `Shift+F3` | Search | Jump to Next / Previous search match |
