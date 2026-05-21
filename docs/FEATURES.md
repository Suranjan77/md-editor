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

### Search and Navigate

The app supports two search modes:

- Per-file search, opened with `Ctrl+F`, highlights matches in the active markdown file and navigates between them.
- Global search, opened from the toolbar, searches the indexed vault and PDF text.

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
- `editor`: markdown buffer, syntax highlighting, and custom renderer.
- `search`: reusable in-file search matching used by search navigation and editor highlights.
- `views`: sidebar, toolbar, search panel, PDF viewer, tracker, backlinks, modals, icons, and related UI.
