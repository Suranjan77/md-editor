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
- Save, undo/redo, selection editing, and common keyboard shortcuts.
- Large-document rendering optimizations using cached line heights, viewport culling, and debounced highlighting.

### Search and Navigate

The app supports three search modes:

- Per-file search, opened with `Ctrl+F`, highlights matches in the active markdown file and navigates between them.
- Global search, opened from the toolbar, searches the indexed vault and PDF text.
- PDF search, opened with `Ctrl+F` when the PDF pane is active, highlights
  matches directly on the rendered PDF pages and scrolls next/previous matches
  into view.

Additional navigation tools include:

- Table of contents generated from headings.
- Backlinks panel for wiki-style note discovery.
- Command palette for common actions.

### Work With Reference Material

PDF files open in an integrated viewer with:

- Continuous page rendering.
- Fit-to-width zoom.
- Keyboard and scroll-wheel navigation.
- PDF table of contents.
- PDF text search.
- Internal PDF link handling.
- Text selection and clipboard copy for PDFs with embedded text.
- Direct quote insertion from a PDF selection into the active markdown note,
  including a `pdf://` page link back to the source page.
- Sidecar PDF highlights, quick notes, and linked markdown notes without
  modifying the original PDF.
- Linked-note creation through a searchable in-app vault picker. Users can
  select an existing markdown note to append a highlight section, or select a
  folder and create a new note path.
- Clean linked-note markdown with one section per highlight, quoted selected
  text, a navigable `pdf://` link back to the exact page/highlight, and a notes
  area for follow-up writing.
- Mixed backlinks between markdown notes, PDFs, and PDF highlights.
- Companion-note memory for linked PDF notes, so future PDF/note workflows can
  reopen the same research pairing.
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

Md-editor is 100% portable. All application settings, session state, and the SQLite database are stored in a file named `md_editor_settings.sqlite` located in the same directory as the executable.

The app does not write to system-wide configuration directories like `%APPDATA%` or `~/Library/Application Support` automatically.

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

- `md-editor-core`: vault management, indexing, SQLite config, full-text search, PDF rendering, and tracker storage.
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
constraints. See [UI_UX_RELEASE_CHECKLIST.md](UI_UX_RELEASE_CHECKLIST.md) for
layout, keyboard, label, contrast, loading, and reduced-motion release checks.
