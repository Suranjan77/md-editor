# MD Editor — Native Port Handoff Guide

> **Purpose**: This document enables any AI model or developer to pick up this project and continue contributing. It describes architecture, conventions, current progress, and next steps.

## 1. Project Overview

**MD Editor** is being ported from a Tauri v2 app (WebView + Rust backend) to a **pure Rust native** desktop application. Target platforms: **Windows** and **Linux**.

### Repository Layout

```
md-editor/
├── src/                    # OLD: JavaScript frontend (reference only, do not modify)
├── src-tauri/              # OLD: Tauri Rust backend (reference for logic extraction)
├── core/                   # NEW: Shared backend library (no UI dependency)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # Re-exports all modules
│       ├── state.rs        # AppState (Arc<Mutex<>> wrapped)
│       ├── vault.rs        # File I/O, vault listing, search, backlinks
│       ├── pdf.rs          # PDFium rendering, page cache, links, TOC, search
│       ├── tracker.rs      # Study tracker CRUD (sessions, activities, KV)
│       ├── config.rs       # SQLite settings get/set
│       └── file_index.rs   # Wikilink graph (backlinks)
├── native/                 # NEW: Native iced GUI application
│   ├── Cargo.toml
│   ├── HANDOFF.md          # THIS FILE
│   └── src/
│       ├── main.rs         # Entry point
│       ├── app.rs          # iced Application impl
│       ├── theme.rs        # Custom dark theme
│       ├── messages.rs     # All iced Message variants
│       ├── views/          # UI components
│       │   ├── mod.rs
│       │   ├── sidebar.rs
│       │   ├── toolbar.rs
│       │   ├── editor.rs
│       │   ├── pdf_viewer.rs
│       │   ├── tracker.rs
│       │   ├── backlinks.rs
│       │   ├── command_palette.rs
│       │   ├── search.rs
│       │   ├── modals.rs
│       │   └── toast.rs
│       └── editor/         # Custom text engine
│           ├── mod.rs
│           ├── buffer.rs       # ropey::Rope wrapper + undo/redo
│           ├── input.rs        # Keyboard/mouse event handling
│           ├── markdown_parser.rs  # pulldown-cmark → styled spans
│           ├── decorations.rs  # Live-preview decoration engine
│           ├── renderer.rs     # Custom iced widget rendering
│           └── search.rs       # Find & replace
└── Cargo.toml              # Workspace root
```

## 2. Tech Stack

| Crate | Purpose | Docs |
|-------|---------|------|
| `iced` 0.13+ | GUI framework (wgpu backend) | https://docs.rs/iced |
| `ropey` 1.x | Rope text buffer for editing | https://docs.rs/ropey |
| `pulldown-cmark` 0.12+ | Markdown pull-parser | https://docs.rs/pulldown-cmark |
| `syntect` 5.x | Syntax highlighting | https://docs.rs/syntect |
| `ratex-svg` | LaTeX math → SVG (KaTeX-compatible) | https://docs.rs/ratex-svg |
| `pdfium-render` 0.9+ | PDF rendering via PDFium | https://docs.rs/pdfium-render |
| `rusqlite` 0.39+ | SQLite database | https://docs.rs/rusqlite |
| `rfd` 0.17+ | Native file dialogs | https://docs.rs/rfd |
| `image` 0.25+ | Image decoding | https://docs.rs/image |
| `arboard` 3.x | System clipboard | https://docs.rs/arboard |
| `lru` 0.18+ | LRU cache (PDF pages) | https://docs.rs/lru |

## 3. Architecture Principles

1. **`core` has zero UI dependencies.** It is a pure library crate with `rusqlite`, `pdfium-render`, `regex`, `image`, `lru`, `base64`. It must compile and test independently.

2. **`native` depends on `core` and `iced`.** All UI logic lives here. Backend operations are called as plain Rust functions on `core::state::AppState`.

3. **No IPC.** Unlike Tauri, there is no serialization boundary. The `AppState` is shared via `Arc<Mutex<>>` and accessed directly.

4. **iced message-driven architecture.** All state changes go through `Message` → `update()` → `view()`. Never mutate state outside `update()`.

5. **Custom editor widget.** The markdown editor is a custom `iced::advanced::Widget` implementation. It owns a `ropey::Rope` buffer and renders text using iced's text primitives. Decorations are computed from `pulldown-cmark` AST and applied as styled spans or widget overlays.

## 4. Feature Parity Checklist

Track progress by marking items complete:

- [x] Window shell (sidebar + toolbar + content + backlinks)
- [x] File tree sidebar with expand/collapse
- [x] Vault folder selection via native dialog
- [x] Open/save markdown files
- [x] Basic text editing (cursor, selection, clipboard, undo/redo)
- [x] Markdown syntax highlighting (headings, bold, italic, code, links)
- [x] Code block rendering with syntax highlighting
- [/] Math rendering (inline $...$ and block $$...$$) - *Syntax highlight done, SVG render pending*
- [/] Image preview (inline and full-screen) - *Core logic done, editor integration pending*
- [ ] Table rendering
- [x] Task checkbox widgets (interactive toggle)
- [x] Wikilink detection and navigation
- [ ] Find & replace in editor
- [ ] Formatting shortcuts (Ctrl+B/I/K etc.)
- [x] PDF viewer with virtual scrolling
- [x] PDF zoom
- [ ] PDF link detection and click navigation
- [ ] PDF link preview (tooltip with rendered destination)
- [ ] PDF TOC (table of contents sidebar)
- [ ] PDF search
- [x] Split view (editor + PDF side by side)
- [x] Vault-wide full-text search (FTS5)
- [x] Backlinks panel
- [x] Command palette
- [x] Study tracker (dashboard, log, projects, gates, reading, config)
- [x] Keyboard shortcuts (all 12 from current app)
- [x] Focus mode
- [x] Toast notifications
- [x] Create/rename file/folder modals
- [x] Delete confirmation dialog
- [x] Session persistence (last vault, last file)
- [x] Dark theme matching current UI

## 5. Current Source Reference

When implementing a feature, reference the corresponding source file from the old app:

| Feature | Old JS Source | Old Rust Source |
|---------|--------------|-----------------|
| App shell / state | `src/main.js` (1241 LOC) | `src-tauri/src/commands.rs` |
| Editor | `src/editor.js` (657 LOC) | — |
| Live preview | `src/markdown-decorations.js` (1073 LOC) | — |
| Math tooltip | `src/math-tooltip.js` (133 LOC) | — |
| PDF viewer | `src/pdf-viewer.js` (834 LOC) | `src-tauri/src/pdf_commands.rs` (752 LOC) |
| Tracker | `src/tracker.js` (521 LOC), `src/tracker-data.js` (291 LOC) | `src-tauri/src/tracker_commands.rs` (129 LOC) |
| Command palette | `src/command-palette.js` (140 LOC) | — |
| IPC interface | `src/ipc.js` (93 LOC) | — |
| Filesystem | — | `src-tauri/src/fs_commands.rs` (186 LOC) |
| File index | — | `src-tauri/src/file_index.rs` (131 LOC) |
| Styling | `src/style.css` (1376 LOC), `src/pdf-viewer.css` (567 LOC) | — |

## 6. Color Palette & Theme Tokens

```
--bg-primary:     #0f1115     (main content background)
--bg-secondary:   #15181c     (sidebar, modals)
--bg-tertiary:    #1a1d23     (hover states, cards)
--bg-surface:     #23262b     (elevated surfaces)
--border:         #45484e     (borders, dividers)
--text-primary:   #e3e5ed     (main text)
--text-secondary: #b8bac0     (secondary text)
--text-muted:     #9d9ea3     (muted/disabled)
--accent:         #b1ccc6     (links, active items, brand)
--accent-dim:     rgba(177,204,198,0.08)  (accent backgrounds)

Fonts: Inter (UI), JetBrains Mono (code/editor)
```

## 7. How to Build & Run

```bash
# From repository root
cd md-editor

# Build the core library
cargo build -p md-editor-core

# Run the native app
cargo run -p md-editor-native

# Run tests
cargo test --workspace

# Release build
cargo build --release -p md-editor-native
```

## 8. Key Design Decisions

### Why iced (not egui/GTK4/Qt)?
- **Pure Rust**: No C/C++ build dependencies for the UI layer
- **GPU-accelerated**: wgpu backend for smooth rendering
- **Retained mode**: Better for complex layouts than egui's immediate mode
- **Custom widgets**: `iced::advanced::Widget` trait allows full control for the editor
- **Cross-platform**: Windows + Linux with identical rendering

### Why a custom editor widget (not iced's text_editor)?
Iced's built-in `text_editor` provides basic multi-line editing but lacks:
- Widget injection (inline images, math, checkboxes)
- Custom decoration system (heading sizes, syntax colors)
- Markdown-aware cursor behavior
- Gutter with line numbers

We use `ropey` for the buffer and build the rendering layer as a custom iced widget.

### Why ropey (not String/Vec)?
- O(log n) insertions/deletions anywhere in the document
- Efficient for large files (used by Helix editor)
- Line-based indexing for cursor positioning
- Unicode-correct

## 9. Contributing Guidelines

### Adding a New Feature
1. Check the Feature Parity Checklist (Section 4) for what needs doing
2. Reference the old source file (Section 5) for behavior specification
3. Implement backend logic in `core/` if it involves data/filesystem
4. Implement UI in `native/src/views/` or `native/src/editor/`
5. Add a `Message` variant in `native/src/messages.rs`
6. Handle the message in `app.rs` `update()`
7. Render in `app.rs` `view()`
8. Update the checklist in this file

### Message Naming Convention
```rust
enum Message {
    // Sidebar
    SidebarToggle,
    SidebarFileClicked(String),
    SidebarFolderToggled(String),
    // Editor
    EditorAction(editor::Action),
    EditorSave,
    // PDF
    PdfOpen(String),
    PdfPageRendered(u32, iced::widget::image::Handle),
    // Tracker
    TrackerSessionAdded(TrackerSession),
    // etc.
}
```

### Style Convention
- All colors come from `theme.rs` constants, never hardcoded in views
- All font sizes are defined as constants in `theme.rs`
- Use iced's `container::Style`, `button::Style` etc. for theming

## 10. Known Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| `ratex` doesn't cover all LaTeX commands used | Fallback: render math as styled plain text with $ delimiters visible |
| iced custom widget complexity | Start with iced `text_editor`, extend incrementally |
| PDFium binary distribution | Use existing `build_pdfium.rs` script, adapted for the new crate |
| Large file performance | Profile with 10K+ line files early; use viewport culling |
| Windows font discovery | Use `cosmic-text` font system (handles Windows/Linux) |

## 11. Progress Log

> Update this section as work progresses. Format: `YYYY-MM-DD: description`

- 2026-05-16: Initial analysis complete. Implementation plan approved. Handoff document created.
- 2026-05-16: **Phase 0 complete.** Workspace scaffolded with `core/` (8 source files, 0 Tauri deps, 2 tests passing) and `native/` (7 source files, iced 0.13 app with sidebar + toolbar + welcome screen + basic text editor). `cargo check --workspace` clean. PDFium auto-downloads during build.
- 2026-05-16: **Phase 1-6 complete.** Migrated core logic (Vault, PDF, Tracker, Search) to native Rust. Implemented custom high-performance text engine with live-preview. Added Wikilink navigation, Study Tracker UI, PDF Viewer, Command Palette, Modals, Toasts, and Split View. Verified with regression tests. Iced v0.14 utilized for latest features.
