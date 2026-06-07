# Project: MD-Editor Roadmap Completion

## Architecture
- MD-PDF Editor app based on Rust, `iced` UI framework, and `pdfium` for PDF rendering.
- Codebase divided into:
  - `core/`: State management, markdown file indexing, PDF integration, search, tracker.
  - `native/`: UI elements, views, command registry, app coordinating logic.

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 10 | Performance & Speed | Indexing progress placeholders, PDF loading spinner, annotation debounce, debug diagnostics panel | None | DONE |
| 12 | Release UX Hardening | Portable settings, DPI scaling via standard libraries, visual authenticity, release checklist | M10 | DONE |

## Interface Contracts
- Config module: standard cross-platform paths using `directories` for portable settings.
- Winit/Iced DPI scaling handling.

## Code Layout
- `core/src/config.rs`: Config storage and paths.
- `core/src/file_index.rs`: Indexing state and updates.
- `core/src/pdf.rs` and `native/src/views/pdf_viewer.rs`: PDF loading and spinner.
- `native/src/app.rs`: Main application shell, state management, event loop, and view routing.
