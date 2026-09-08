# User Guide & Feature Manual

This is the complete manual: vaults, markdown editing, PDF reading, search, split view, and
the study tracker. Key bindings have their own page —
[Keyboard Shortcuts](Keyboard-Shortcuts.md).

---

## 1. Vaults

A **vault** is an ordinary folder. There is no import step, no proprietary container, and
nothing is moved or rewritten when you open one.

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

### Opening a vault

1. Launch MD Editor. The welcome screen offers **Open Existing Vault**.
2. Or press `Ctrl+O` at any time.
3. Pick a folder. MD Editor indexes its Markdown, PDF, and image files and shows them in the
   sidebar tree.

Indexing walks up to 32 directory levels and skips `node_modules`, `target`, `build`, `dist`,
`__pycache__`, `.trash`, and every dotfolder — so `.git` and `.obsidian` stay out of your way.

The last vault and last open file are restored the next time you launch.

### Supported files

| Type | Extensions |
| :--- | :--- |
| Markdown | `.md`, `.markdown` |
| PDF | `.pdf` |
| Images | `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.webp` |

Images open in a dedicated preview pane.

### The sidebar

- **Open** — click any file.
- **Expand / collapse** — click a folder.
- **New note / new folder** — the buttons in the sidebar header.
- **Delete** — the trash button on a file's row, which asks for confirmation first.

### External changes

A filesystem watcher follows the vault. If a file changes underneath you — a `git pull`, an
edit from another tool — buffers, the file tree, and the backlink graph refresh without a
restart.

---

## 2. Writing in Markdown

### Hybrid live preview

MD Editor shows formatted text and lets you edit the exact source, without a mode switch:

- **Cursor elsewhere** — syntax markers (`**`, `*`, `~~`, `$`, `$$`, `` ``` ``) are hidden and
  the line renders as formatted typography.
- **Cursor on the line** — that line's raw markers reappear in place, so you edit the real
  characters. For code, math, and table blocks the whole block reveals its source.

### Supported elements

| Element | Syntax | Notes |
| :--- | :--- | :--- |
| Headings | `# H1` … `###### H6` | Listed in the table of contents |
| Emphasis | `**bold**`, `*italic*`, `~~strike~~` | `Ctrl+B` / `Ctrl+I` toggle the first two |
| Inline code | `` `code` `` | `Ctrl+E` toggles it |
| Code blocks | ```` ```rust … ``` ```` | Syntax highlighted by `syntect` |
| LaTeX math | `$E = mc^2$`, `$$\int_0^\infty f(x)\,dx$$` | Rendered by `ratex-render`; display math is centred |
| Blockquotes | `> quoted` | Left accent border |
| Task lists | `- [ ]` / `- [x]` | Click the box to toggle it |
| Tables | `\| A \| B \|` | Render as grids and scroll horizontally on their own |
| Images | `![alt](path.png)` | Rendered inline with the alt text as a caption |
| Links | `[text](target)` | `Ctrl` + click to follow |
| Wikilinks | `[[Note]]`, `[[Note\|Label]]` | Resolve across subfolders by filename |
| Horizontal rule | `---` | |

### Smart editing

- **Auto-pairing** — `(`, `[`, `{` insert their closer with the cursor between. `"`, `'`, and
  `` ` `` do the same.
- **Selection wrapping** — with text selected, typing one of those characters wraps the
  selection instead of replacing it.
- **Skip-over** — typing a closer that is already sitting at the cursor steps over it rather
  than doubling it.
- **Contraction protection** — an apostrophe typed straight after a letter or digit stays
  single, so `don't`, `it's`, and `users'` come out right.
- **List continuation** — `Enter` on a list item continues the list; ordered lists increment
  (`1.` → `2.`); `Enter` on an *empty* list item clears the marker and leaves list mode.
- **Word-sized undo** — one `Ctrl+Z` takes back a burst of typing, not one character. A run
  breaks at a pause, a newline, or a cursor jump.

### Saving

You do not have to think about it:

- The document is written **400ms after typing stops**.
- `Ctrl+S` forces an immediate save and confirms it with a toast.
- Every write is **atomic** — a crash or power loss leaves either the complete old file or
  the complete new one, never a truncated mix. Symlinked notes are written through to their
  target, and existing permissions (a note kept at `0600`) are preserved.
- Opening another file saves the current one first. If that write fails, the app **stays put**
  and tells you, rather than navigating away from work it could not persist.
- Closing the window triggers one last save.
- Switching away and back keeps the note's **undo history, cursor, and scroll position**, for
  up to 32 documents per session.

### Wikilinks and backlinks

- `[[Note Name]]` links to a note; `[[Note Name|Label]]` shows different text.
- A bare `[[Name]]` resolves across subfolders by filename — if several match, the shortest
  path wins, deterministically, so the link, the click-through, and the backlink always agree.
- Linking to a note that does not exist yet is fine; creating it later makes the backlink
  appear.
- The **Backlinks panel** (`Ctrl+P` → *Toggle Backlinks*) lists what points at the open
  document: notes that wikilink to it, and PDF highlights linked to it. Activating a highlight
  opens that PDF at the exact page.

### Table of contents

`Ctrl+T` opens an outline of the document's headings. For a PDF it shows the outline instead
(see below). In split view each pane keeps its own.

---

## 3. Reading PDFs

Open any `.pdf` from the sidebar to enter the integrated reader.

```
┌─────────────────────────────────┬─────────────────────────────────┐
│ Markdown notes                  │ Reference PDF                   │
│                                 │                                 │
│ ## Chapter 4 review             │ Figure 4.1: Attention matrix    │
│ The attention weights derive…   │ ┌─────────────────────────────┐ │
│                                 │ │ [ highlighted passage ]     │ │
│ [Open highlight in PDF](pdf://…)│ └─────────────────────────────┘ │
│                                 │                                 │
└─────────────────────────────────┴─────────────────────────────────┘
```

### Viewing

- **Continuous scrolling** through the document.
- **Fit-to-width** (on by default) and manual zoom. Pages are rendered above their displayed
  size, scaled by your display's factor, so they stay sharp in a narrow split pane and on
  HiDPI screens.
- **Keyboard scrolling** — arrows and page keys.
- **Table of contents** — embedded bookmarks when the PDF has them. When it does not, MD
  Editor recovers one: first from a printed contents page's link annotations, then from its
  dot-leader text (`Chapter 3 ......... 47`), and failing both from a typographic heuristic
  over font sizes and numbering. A recovered outline is marked as such. A scanned or
  structureless PDF correctly yields an empty outline rather than a dump of page content.
- **Internal links** — embedded PDF links work.

### Recognized cross-references

Many papers say "see equation (3.14)" or "Figure 1.1" without embedding a link. MD Editor
reads the text layer and recognizes numbered **equations**, **figures**, **tables**, and
**sections** — then draws a subtle underline under each mention.

**Right-click one to preview its target in place**, without losing your reading position.
For a figure or table the preview is aimed at the artwork rather than the caption.

Recognition is conservative on purpose: a mention only becomes a link when its number matches
exactly one target in the document, so stray numbers — intervals, quantities, citation
years — do not turn into false links. Resolution runs once per document and is cached, and a
scanned PDF with no text layer simply yields none.

### Text selection

Drag to select, `Ctrl+C` to copy. This needs an embedded text layer; scanned PDFs need OCR
outside the app.

### Highlights and notes

- Drag to select, then choose **Highlight**.
- Colours cycle automatically — yellow → green → blue → pink → orange — so consecutive
  highlights stay distinct. You can also pick one explicitly.
- Add a **quick note** to a highlight.
- **The original PDF is never modified.** Highlights, notes, and links live in the portable
  settings database.
- **Orphan report** — if a PDF is replaced or recompiled, stored positions can drift. The
  report compares each highlight's saved text against what is now under its rectangle and
  tells you how many drifted, and how many were checkable (page text is loaded lazily).

### Linked notes

1. Select text in a PDF and choose **Link to Note**.
2. A searchable picker lets you choose an existing note, or a folder plus a new note name.
3. MD Editor writes a `## Page N` section with the quoted passage, an
   `[Open highlight in PDF](pdf://…)` link, and a `### Notes` area for your own writing.

Linking another highlight to the same note **appends** a section rather than overwriting the
file, and re-linking the same highlight changes nothing. `Ctrl` + click the `pdf://` link from
any note to jump to that exact page and highlight — which opens split view without resetting
the PDF's position.

### Split view

`Ctrl+P` → *Split View*, or the toolbar button, with both a note and a PDF open. Drag the
divider to resize. If fit-to-width is on, the PDF re-fits to the narrower pane while keeping
your page and scroll position.

---

## 4. Search

### In-note search — `Ctrl+F` with a note active

Highlights every match in the document and navigates between them. Options:

- **Regex** — treat the query as a regular expression.
- **Match case**.
- **Replace / Replace all** — replace-all is an ordinary edit: it is undoable and arms
  autosave like any other change.

### Global vault search — the toolbar search icon

Powered by SQLite FTS5 over the whole vault, covering **both Markdown notes and extracted PDF
text**. Results show the path and a highlighted snippet, ranked by relevance, capped at 100.
The query runs 200ms after you stop typing, so a ten-character query is one scan, not ten.

### PDF search — `Ctrl+F` with the PDF pane active

Highlights matches on the rendered pages and scrolls the active match into view, with next
and previous navigation.

**Loose whitespace** is the toggle that matters for papers: it lets any run of whitespace
match between your words, so a phrase still matches when it wraps across a PDF line break or
between columns. (It does not rejoin a word hyphenated across a line break — search for
`algo` or `rithm` separately in that case.)

In split view, `Ctrl+F` follows whichever pane you last touched, so markdown and PDF searches
stay independent.

### Command palette — `Ctrl+P`

Searches commands *and* vault files at once, interleaved by relevance. Characters must appear
in order but need not be adjacent, so initials work: `sv` finds *Split View*, `tocfg` finds
*Tracker Configuration*. A match in a file's own name beats a match in a folder along its
path. `↑`/`↓` to move, `Enter` to open or run, `Escape` to close.

---

## 5. Study Tracker

`Ctrl+P` → *Study Tracker*, or the toolbar button.

- **Timer** — one button. *Start Timer* begins; *Stop Timer* saves the elapsed interval as a
  session immediately. There is no pomodoro cycle and no post-stop dialog.
- **Log** — the session history, newest first, with per-row delete and a manual-entry form
  (date, hours, notes) for sessions you did not time.
- **Dashboard** — total hours, session count, average per session, and how many curriculum
  phases are configured.
- **Projects / Gates / Reading** — checklists driven by your configuration. Gates are a
  self-assessment checklist; nothing in the app blocks progress on an unticked gate.
- **Config** — the whole curriculum as JSON. It ships seeded with a four-year efficient-ML
  research roadmap, which you are meant to replace: the tracker only knows about phases,
  projects, gates, and reading sections, so a language course or a thesis fits just as well.
  An invalid document is rejected rather than saved.

Full detail: [Study Tracker](Study-Tracker.md).

---

## 6. Where Your Data Lives

- **Your content** stays exactly where you put it: plain Markdown, PDF, and image files in
  your own folder.
- **Everything else** — settings, window size, per-document scroll and cursor positions, PDF
  highlights and notes, cached PDF text, and study history — lives in a single SQLite file
  named `md_editor_settings.sqlite`, **beside the executable** by default, so the whole app
  travels as one portable folder.
- Only when the executable's directory is read-only does it fall back to the per-user
  platform data directory and then the current directory.
- Nothing is sent anywhere. There are no accounts and no telemetry.
