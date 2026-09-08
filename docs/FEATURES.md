# MD Editor 1.0 Feature Document
---

## Core Workflows

### Open a Vault

The user opens a folder as a vault. The app indexes supported files, remembers the last vault, and restores it on the next launch. The sidebar exposes the vault as a file/folder tree.

### Edit Markdown

Markdown files open in the main editor. The editor supports:

- Inline markdown styling for headings, emphasis, links, code, checkboxes, blockquotes, and tables.
- Syntax-highlighted fenced code blocks.
- Rendered math blocks.
- Image references and image previews.
- Horizontal scrolling for wide code, math, and table blocks.
- In-file search with highlighted matches and previous/next navigation.
- Auto-pairing of brackets and quotes: typing `(`, `[`, or `{` inserts the
  matching closer with the cursor between them (or wraps the current selection);
  typing a closer that already sits at the cursor skips over it; `"`, `'`, and
  `` ` `` pair the same way, except a quote following a word character is left
  single so apostrophes in contractions are unaffected.
- Selection editing and common keyboard shortcuts.
- Undo and redo grouped into human-sized steps: a run of typing (or of
  backspaces) collapses into one undo, breaking at a pause, a newline, or a
  cursor jump, so `Ctrl+Z` takes back a word rather than a character.
- Large-document rendering optimizations using cached line heights, memoized
  text measurement, viewport culling, and debounced highlighting.

#### Saving And Durability

Work is never held only in memory:

- **Autosave.** The document is written 400ms after typing stops. `Ctrl+S`
  still forces an immediate save and confirms it.
- **Atomic writes.** Content is written to a temporary file in the same
  directory, flushed to disk, and then renamed over the destination. A crash or
  power loss leaves either the complete old file or the complete new one, never
  a truncated mix. Symlinked notes are written through to their target, and the
  destination's existing permissions are preserved, so a note kept at `0600`
  stays private.
- **Flush before switching.** Opening another file saves the current one first.
  If that write fails the app stays on the current file and says so, rather than
  navigating away from work it could not persist.
- **Flush on close.** Closing the window triggers one last save. This is
  best-effort and does not block the close: an app that refuses to close
  because a write is failing is a worse outcome than the few hundred
  milliseconds autosave has not yet committed.
- **Undo history survives navigation.** Switching away parks the document's
  buffer, so returning to a file resumes its undo stack and cursor rather than
  starting from a blank history. Up to 32 documents are kept this way per
  session, and a buffer is discarded if the file changed on disk meanwhile.

#### Session Continuity

Window size, and the scroll offset and cursor position of the open document,
are recorded on exit and restored on the next launch. A stored window size that
is malformed or no longer fits any display falls back to the default rather
than opening the app unusably small or large.

### Search and Navigate

The app supports three search modes:

- Per-file search, opened with `Ctrl+F`, highlights matches in the active markdown file and navigates between them.
- Global search, opened from the toolbar, searches the indexed vault and PDF text.
- PDF search, opened with `Ctrl+F` when the PDF pane is active, highlights
  matches directly on the rendered PDF pages and scrolls next/previous matches
  into view. A loose-whitespace toggle lets a phrase match even when it wraps
  across PDF line breaks (any whitespace run between words is allowed).

Additional navigation tools include:

- Table of contents generated from headings.
- Backlinks panel for wiki-style note discovery.
- Bare `[[Name]]` wikilinks resolve across subfolders by filename (shortest path
  wins), so a link, its navigation, and its backlink all reach the same file
  even when the target lives in a different folder.
- Command palette for common actions.

### Work With Reference Material

PDF files open in an integrated viewer with:

- Continuous page rendering.
- Fit-to-width zoom.
- Keyboard and scroll-wheel navigation.
- PDF table of contents. When a PDF has no embedded bookmarks, an outline is
  recovered from the document itself — first from a printed contents page (its
  link annotations, then its dot-leader text), and failing that from a
  typographic heading heuristic — so even bookmark-less PDFs get a usable TOC.
- PDF text search.
- Internal PDF link handling.
- Recognition of internal cross-references in PDFs that have no embedded links:
  numbered equations (e.g. `(3.14)`), figures and tables (e.g. `Figure 1.1`,
  `Table 6.1`), and sections (e.g. `Section 3.2`). References are detected by
  reading the text layer; the original PDF is never modified. Recognized
  references are marked with a subtle underline, and right-clicking one previews
  its target (the equation, figure, table, or section) in place without losing
  your reading position. Resolution runs once per document and is cached, so
  scanned/image-only PDFs (no text layer) simply yield no references.
- Text selection and clipboard copy for PDFs with embedded text.
- Sidecar PDF highlights, quick notes, and linked markdown notes without
  modifying the original PDF.
- Highlight colors cycle automatically through the palette (yellow, green, blue,
  pink, orange) on each quick highlight, so successive highlights stay visually
  distinct.
- An orphan/drift report flags highlights whose stored text no longer matches the
  text currently under their saved position, helping find annotations that have
  drifted (it reports how many of the loaded annotations were checkable).
- Linked-note creation through a searchable in-app vault picker. Users can
  select an existing markdown note to append a highlight section, or select a
  folder and create a new note path.
- Clean linked-note markdown with one section per highlight, quoted selected
  text, a navigable `pdf://` link back to the exact page/highlight, and a notes
  area for follow-up writing.
- Mixed backlinks between markdown notes, PDFs, and PDF highlights.
- Higher-resolution page rendering so fit-to-width pages remain sharp in split
  view.

In split view, `Ctrl+F` follows the active pane. If the markdown pane is active,
it opens markdown search. If the PDF pane is active through scrolling, clicking,
or selecting text, it opens PDF search. This keeps markdown and PDF search
isolated while still supporting both panes.

Image files open in a dedicated image preview.

### Track Study Activity

The tracker panel provides a structured way to record study sessions, project progress, gates, reading, and tracker configuration.

## Platform Support

Version 1.0 targets:

- Windows x64 and Windows ARM64.
- Linux x64 and Linux ARM64.
- macOS Intel and Apple Silicon.

Md-editor is portable. All application settings, session state, and study
history live in a single SQLite database named `md_editor_settings.sqlite`,
stored **next to the executable by default**, so the entire app travels as one
self-contained folder and does not write to system-wide configuration
directories.

The only exception is a read-only install: when the executable's own directory
is not writable, the database falls back to the per-user platform data directory
(`%APPDATA%\md-editor\` on Windows, `~/Library/Application Support/md-editor/` on
macOS, `$XDG_DATA_HOME/md-editor/` or `~/.local/share/md-editor/` on Linux), and
finally the current directory. An interim version stored the database in that
per-user directory; on first run the app migrates such a database (including its
write-ahead-log sidecars) back beside the executable, leaving the original in
place. The database uses WAL journal mode.

On Linux, optional desktop launcher integration (desktop entry shortcuts and multi-size application icons) can be explicitly installed or uninstalled using command-line arguments:
- `--install` or `--install-desktop`: Installs the desktop entry and system icons.
- `--uninstall` or `--uninstall-desktop`: Removes the desktop entry and system icons.

PDF support uses a platform-specific PDFium dynamic library. The application looks for the library (e.g., `pdfium.dll`, `libpdfium.so`, or `libpdfium.dylib`) in a `resources` folder next to the executable or directly in the same directory as the executable.


## Supported File Types

- Markdown: `.md`, `.markdown`
- PDF: `.pdf`
- Images: `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.webp`

## Motion

Panels, overlays and toasts move between their states rather than snapping.
The vocabulary is one easing curve and three durations (120ms for small
transitions, 180ms for panels, 140ms for overlays), declared in
`native/src/motion.rs` alongside the animation state.

Side panels animate their width and clip their contents, so opening the file
tree or the table of contents reads as a reveal instead of a jump. The toast
fades in and out, and survives the underlying message being cleared so the
fade-out has something to draw.

The per-frame redraw subscription is armed only while something is actually
moving; a settled window uses no CPU at all.

## Command Palette

`Ctrl+P` opens a palette that searches commands and vault files together. The
query field takes focus immediately, so the palette is reachable without the
mouse.

- Fuzzy matching: characters must appear in order but need not be adjacent, so
  initials (`sv` for Split View) and partial names both work.
- Word starts outrank mid-word hits, runs of adjacent characters outrank
  scattered ones, and a match in a file's own name outranks one in a folder
  along its path.
- Commands and files are ranked against each other and interleaved by
  relevance rather than split into fixed sections.
- Up and Down move the highlighted row, Enter activates it, Escape closes.
  Files open in the right viewer for their type.

Commands are declared in one registry in `views/command_palette.rs`, so the
palette and the shortcut list cannot drift apart.

## Visual System

Application chrome draws from a single set of design tokens in
`native/src/theme.rs`: a six-step type scale, a seven-step spacing scale on a
2px grid, three corner radii, and the colour palette. View code selects a token
rather than a literal, so sizes and gaps stay consistent across panels instead
of drifting per call site. Document typography — heading sizes and code/math
scale inside a note — is governed separately by the markdown renderer.

## Architecture

The workspace is split into two crates:

- `md-editor-core`: vault management, indexing, SQLite config, full-text search, PDF rendering, internal-reference resolution, and tracker storage.
- `md-editor-native`: Iced desktop UI, editor rendering, views, commands, and interaction state.

Important native modules:

- `app`: application state, update loop, routing, and layout composition.
- `editor`: markdown buffer, syntax highlighting, height/layout caching, and custom renderer.
- `pdf_notes`: linked PDF note path normalization and markdown section formatting.
- `search`: reusable in-file search matching used by search navigation and editor highlights.
- `views`: sidebar, toolbar, search panel, PDF viewer, linked-note picker,
  tracker, backlinks, modals, icons, and related UI.

Editor performance notes:

- `editor/layout_tree.rs` stores visual line heights in a Fenwick tree for fast y-to-line and line-to-y lookup.
- `editor/layout_cache.rs` stores line measurement cache keys and invalidates cached heights when text, edit state, layout width, or media/math dimensions change.
- `editor/renderer.rs` draws only visible lines and visible block backgrounds.
- `app.rs` debounces syntax highlighting for large documents and ignores stale background highlight results with generation ids.
- PDF overlays are drawn as a small number of canvas layers rather than one UI
  widget per search match or annotation rectangle, keeping large annotated PDFs
  responsive.

## Release Readiness

See [LAUNCH.md](LAUNCH.md) for the release checklist, smoke-test flow, PDFium
packaging notes, Linux desktop integration commands, and current known
constraints.
