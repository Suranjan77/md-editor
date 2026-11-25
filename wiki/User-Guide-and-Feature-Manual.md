# User Guide & Feature Manual

This is the complete manual: vaults, markdown editing, PDF reading, search, split view, the
knowledge graph, handwriting, and the study tracker. Key bindings have their own page —
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
| Images | `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.svg`, `.webp` |
| Handwritten pages | `.ink` (made with the handwriting canvas) |

Images open in a dedicated preview pane. For slides, export the deck to PDF: it then gets
search, highlights, quick notes and `pdf://` links like any other paper.

### The sidebar

- **Open** — click any file.
- **Expand / collapse** — click a folder.
- **New note / new folder** — the buttons in the sidebar header.
- **Rename / delete** — select a file or folder; its row shows a rename and a delete
  button (or press `F2`). Delete asks for confirmation first. Renaming or moving a note,
  PDF, image, sketch or folder rewrites the wikilinks and Markdown links and images that
  point at it (never inside code).
- **Right-click** a file or folder for New note here, New folder here, Rename, Reveal in
  file manager, Copy relative path and Delete; right-click empty space for New note / New
  folder.
- **Filter** — the field at the top narrows the tree to matching paths.
- **Keyboard** — `F6` until the sidebar is focused, then `↑`/`↓` to move, `←`/`→` to fold,
  `Enter` to open.
- **Drop files** onto the tree to copy them into the vault (into the selected folder). A
  file with the same name is never overwritten; you get a warning instead.
- **Width** — drag the edge; double-click it to reset.

### Tabs

Every note, PDF and image you open gets a tab; opening one that is already
open switches to it. Each tab remembers its own place: a note keeps its caret, scroll and
undo history, a PDF its page and zoom, an image its zoom.

- **Switch** — click a tab, `Ctrl+Tab` for the most recently used, `Ctrl+PageDown`/`PageUp`
  in strip order, `Ctrl+1` … `Ctrl+9` by position.
- **Close** — the ×, a middle-click, or `Ctrl+W`. Closing a note writes your edits first;
  if they cannot be written the tab stays open. `Ctrl+Shift+T` reopens what you closed.
- **Reorder** — drag a tab, or `Ctrl+Shift+PageDown`/`PageUp`.
- **Open without switching** — middle-click a file in the sidebar.
- **Right-click** a tab for Close, Close others, Reveal in sidebar and Copy relative path;
  the ▾ at the end lists every tab.
- Tabs follow renames, show a "missing" mark when the file is deleted outside the app, and
  come back per vault the next time you open it.

### Settings — `Ctrl+,`

- **Appearance** — theme (System, Dark, Light), interface scale (90–130%), reduced motion
  (System, Standard, Reduced).
- **Editor** — reading font (Inter or Source Serif), font size (14–22px) and line width
  (Narrow 640, Default 720, Wide 880). Changes apply to the open note at once.
- **Files** — where pasted and dropped images go (vault root, beside the note, or a
  folder you choose), and folders to leave out of the vault (comma-separated names, applied
  with Enter or Apply; `node_modules`, `target`, `build`, `dist` and dot-folders are always
  left out).
- **Keyboard** — every shortcut, from the same list the tooltips and palette use.
- **About** — version, licence, and where your data and log live.

### External changes

A filesystem watcher follows the vault. If a file changes underneath you — a `git pull`, an
edit from another tool — the file tree and the backlink graph refresh without a restart, and
the open note reloads if you have no unsaved edits in it.

If the open note changes on disk while you *do* have unsaved edits, nothing is thrown away:
your version is saved to the note, and the other version is kept next to it as
`Note (conflict 2026-11-24 15-04-05).md`. A toast tells you the name; compare the two and
delete the copy when you are done.

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
| Callouts | `> [!note] Title`, `> [!warning]`, `> [!tip]`, `> [!danger]`, … | The quote is framed in the type's colour, with its name before the title |
| Frontmatter | `---` / `key: value` / `---` at the very top | Shown as a YAML block, not as rules |
| Task lists | `- [ ]` / `- [x]` | Click the box to toggle it |
| Tables | `\| A \| B \|` | Render as grids and scroll horizontally on their own |
| Images | `![alt](path.png)` | Rendered inline with the alt text as a caption |
| Links | `[text](target)` | `Ctrl` + click to follow |
| Wikilinks | `[[Note]]`, `[[Note\|Label]]` | Resolve across subfolders by filename |
| Horizontal rule | `---` | |

### Pasting images

`Ctrl+V` in a note pastes images, not just text:

- **A screenshot or copied image** is saved as `attachments/Pasted image <date time>.png`
  and an image line is inserted at the caret.
- **Image files copied in a file manager** (or their paths/`file://` URIs copied as text)
  are inserted too: files outside the vault are copied into `attachments/`, files already
  in the vault are linked where they are. Copied notes and PDFs from the vault become
  `[[wikilinks]]`; other file types are not inserted.
- Pasting the same image twice reuses the stored file. `Ctrl+Z` removes the inserted line
  in one step; the image file stays in the vault.
- Paths with spaces are written as `![](<attachments/Pasted image ….png>)`, the CommonMark
  form; `%20`-encoded paths load too.
- The folder is set in Settings → Files: the vault root (default, `attachments/`), beside
  the note (`attachments/` next to it), or a vault-relative folder of your choice.
- **Insert image…** (command palette, or right-click in the note) picks image files with a
  file dialog and inserts them the same way.
- **Dropping** image files onto the note inserts them at the caret; dropping onto the
  sidebar copies them into the vault instead.

Dragging files onto the window is not supported on Wayland yet (the windowing library does
not deliver drops there); copy and paste, or use Insert image….

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
- **Completion** — typing `[[` lists vault notes and files that match what follows;
  `/` at the start of a line lists blocks to insert (headings, tasks, callouts, code, math,
  a table, a divider). `↑`/`↓` choose, `Enter` or `Tab` inserts, `Escape` closes, and
  clicking a row inserts it. A link to a note whose name is unique uses just the name.
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

`Ctrl+Shift+O` opens an outline of the document's headings. For a PDF it shows the outline instead
(see below). In split view each pane keeps its own.

### Knowledge graph

The Knowledge Graph is a research workspace for seeing how notes and source material connect.
Open it from the toolbar, from the command palette (*Open Knowledge Graph*), or with `Ctrl+G`.

- **Global** scope shows indexed Markdown notes, PDFs, and unresolved wikilink targets.
  **Local** scope needs an active note or PDF and follows connections one or two hops out.
  Images and ordinary Markdown links are not graph edges.
- Notes, PDFs, and missing link targets are visually distinct nodes. Edges are resolved
  wikilinks plus links from PDF annotations to their linked notes; repeated annotation links
  are aggregated.
- The canvas supports panning, zoom, dragging and pinning nodes, selection, opening a file
  directly, and fit-to-view. Dragged positions last for the session only.
- Search narrows the visible graph while keeping immediate neighbours for context. Filters
  toggle PDFs, unresolved targets, and unconnected nodes without touching any vault file.
- Selecting a node shows its incoming and outgoing connections and graph-derived related
  notes. Vault-health summaries point out unconnected notes, unresolved links, hubs, and the
  number of disconnected groups.

The graph is built from the same live link index as navigation and backlinks, so it refreshes
after saves and external vault changes. It is a derived view: layout and filtering never
rewrite note content.

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

- **The toolbar** above the page: outline, the page field (type a number, `Enter` jumps),
  zoom out / in, the zoom field (type a percentage), fit width, fit page, and find. With
  text selected it shows the highlight colours; with a highlight focused, its note, linked
  note, copy and delete actions.
- **Continuous scrolling** through the document.
- **Fit-to-width** (on by default) and manual zoom. Fit-to-width — automatic or via the fit
  button — stops at 200% so a wide pane doesn't blow pages up; the zoom buttons still go to
  400%. Pages are rendered above their displayed
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
- **The original PDF is never modified.** Highlights, notes, and links live in the
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

## 4. Handwritten Notes

`Ctrl+I` opens a handwriting canvas in its own window — put it on a tablet or a second
monitor beside your notes. A pen's pressure shapes each stroke (Windows, and Linux on
Wayland); with a mouse, strokes have a fixed weight.

- **Tools**: pen, highlighter (drawn behind writing), eraser (rub out part of a stroke, or
  whole strokes), and lasso (circle strokes, then drag them or press `Delete`).
- **Paper**: plain, ruled, grid, or dotted.
- **Pen buttons**: hold the lower side button (or flip to the eraser end) to erase. Press
  the upper side button for a ring of tools and colours at the nib.
- `Ctrl+Z` / `Ctrl+Y` undo and redo; `Ctrl+S` saves.

A page is saved beside the open note (`Week1.md` → `Week1.ink`), or at the vault root as
`handwritten-<date>.ink` when no note is open; a second page for the same note becomes
`Week1-2.ink`. Pages save automatically a second after you stop writing.
Saved pages appear in the sidebar and reopen in the canvas. Developer detail is in
[Handwritten Notes](Handwritten-Notes.md).

---

## 5. Search

### In-note search — `Ctrl+F` with a note active

Highlights every match in the document and navigates between them. Options:

- **Regex** — treat the query as a regular expression.
- **Match case**.
- **Replace** — the arrow at the left of the find bar opens the replace row. Replace (or
  `Enter` in the replace field) replaces the current match and moves to the next; Replace
  all replaces every match as one ordinary edit, undoable and autosaved like any other.

### Vault search — `Ctrl+Shift+F` or the toolbar search icon

Powered by SQLite FTS5 over the whole vault, covering **Markdown notes, extracted PDF text
and your PDF quick notes**. Matches in the open PDF come first, then notes with a snippet,
ranked by relevance (a match in a file's name ranks highest), capped at 100. Two letters are
enough (`ML`, `AI`; one character for Chinese, Japanese or Korean), and the last word
matches as you type it. `↑`/`↓` move through the results and `Enter` opens one: a note opens
with the find bar showing your query and the matching line selected; a PDF opens with its
matches highlighted. The query runs 200ms after you stop typing, so a ten-character query
is one scan, not ten.

### PDF search — `Ctrl+F` with the PDF pane active

Highlights matches on the rendered pages and scrolls the active match into view, with next
and previous navigation.

**Loose whitespace** is the toggle that matters for papers: it lets any run of whitespace
match between your words, so a phrase still matches when it wraps across a PDF line break or
between columns, and it rejoins a word the typesetter hyphenated across a line break
(`algo-` / `rithm` matches `algorithm`).

In split view, `Ctrl+F` follows whichever pane you last touched, so markdown and PDF searches
stay independent.

### Command palette — `Ctrl+P`

Searches commands *and* vault files at once, interleaved by relevance. Characters must appear
in order but need not be adjacent, so initials work: `sv` finds *Split View*, `tocfg` finds
*Tracker Configuration*. A match in a file's own name beats a match in a folder along its
path. `↑`/`↓` to move, `Enter` to open or run, `Escape` to close.

---

## 6. Study Tracker

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

## 7. Where Your Data Lives

- **Your content** stays exactly where you put it: plain Markdown, PDF, and image files in
  your own folder.
- **Everything else** — settings, window size, per-document scroll and cursor positions, PDF
  highlights and notes, cached PDF text, and study history — lives in a single SQLite file
  named `md_editor_settings.sqlite` in `~/.local/share/md-editor/`. The log is
  `~/.local/state/md-editor/md-editor.log`.
- Uninstalling the app leaves both in place; delete those folders to remove all app data.
- Nothing is sent anywhere. There are no accounts and no telemetry.
