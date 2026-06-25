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
- Save, undo/redo, selection editing, and common keyboard shortcuts.
- Large-document rendering optimizations using cached line heights, viewport culling, and debounced highlighting.

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
