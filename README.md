# MD Editor

MD Editor is a native desktop markdown workspace for local notes, PDFs, images, search, backlinks, and study tracking.

![Home screen](images/home_screen.png)

## Simple User Brief

Open a folder as your vault, write markdown notes, search across your work, read PDFs beside your notes, and keep everything stored as normal local files. The app is designed for research and study workflows where notes, papers, images, and progress tracking live in one desktop workspace.

## Highlights

- Local vault-based markdown editing.
- Sidebar file/folder tree with create and delete actions.
- Per-file search with highlighted matches and previous/next navigation.
- Global vault and PDF search.
- Backlinks and table of contents panels.
- Syntax-highlighted code blocks, markdown tables, task checkboxes, images, and math rendering.
- Built-in PDF viewer with continuous pages, fit-to-width, PDF links, and PDF text search.
- Split view for markdown plus PDF/reference material.
- Study tracker for sessions, reading, project stages, and tracker configuration.
- Cross-platform native UI built with Rust and Iced.

## Supported Platforms

Version 1.0 targets:

- Windows x64 and Windows ARM64
- Linux x64 and Linux ARM64
- macOS Intel and Apple Silicon

PDF support depends on PDFium. The build script downloads the matching PDFium binary for the target OS/architecture and copies the shared library next to the executable.

## Supported Files

- Markdown: `.md`, `.markdown`
- PDF: `.pdf`
- Images: `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.webp`

## Build From Source

Requirements:

- Rust stable with Cargo
- A desktop environment capable of creating native windows
- Internet access on the first build if PDFium is not already cached

Run in development:

```bash
cargo run
```

Build a release binary:

```bash
cargo build --release
```

The executable is created at:

- Windows: `target\release\md-editor.exe`
- Linux/macOS: `target/release/md-editor`

The PDFium library is copied into the same Cargo profile output directory during the build.

## Portability

Md-editor is 100% portable. It stores all its configuration and the SQLite database in the same directory as the executable.

- **Settings and State:** `md_editor_settings.sqlite` (located next to the executable).
- **PDF Support:** The PDFium shared library (`pdfium.dll`, `libpdfium.so`, or `libpdfium.dylib`) should be placed in a `resources` folder next to the executable or in the same directory as the executable.

The app does not use system-wide configuration directories like `%APPDATA%` or `~/Library/Application Support`.

## Technical Overview

This repository is a Rust workspace:

- `core`: vault management, SQLite state, full-text search, PDF rendering, and tracker storage.
- `native`: Iced desktop application, editor UI, custom markdown renderer, panels, and commands.

Useful commands:

```bash
cargo check
cargo test -p md-editor-native
cargo test
```

## Feature Document

See [docs/FEATURES.md](docs/FEATURES.md) for the version 1 feature document, platform support notes, architecture summary, and release readiness details.