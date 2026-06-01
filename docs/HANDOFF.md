# Live Handoff

Last updated: 2026-06-01

## Purpose

This document is the live pickup point for humans and agents working on the
editor and PDF synergy roadmap. Update it at the end of every meaningful work
session, especially when a task is incomplete, blocked, or changes the expected
next step.

## Current Objective

Implement the multi-month markdown editor and PDF reader synergy plan in
`docs/EDITOR_PDF_SYNERGY_ROADMAP.md`, using TDD and the repository standards.

## Current State

- Roadmap exists in `docs/EDITOR_PDF_SYNERGY_ROADMAP.md`.
- Milestone 1 is complete:
  - PDF quote insertion uses `EditorCommand::InsertPdfQuoteLink`.
  - PDF annotation insertion uses `EditorCommand::InsertPdfAnnotationLink`.
  - Created/appended linked notes uses `build_linked_pdf_note_content`.
  - Context menu and command palette integrate citation commands properly.
  - PDF/Markdown panes split view correctly placed.
  - Delimiters `?` and `#` both supported in `pdf_links.rs`.
  - PDF stuck in loading bug fixed.
- Milestone 2 is complete/partially complete:
  - Reference link parsing support in `highlight.rs`.
  - Inline links, emphasis in headings, footnote references (`[^1]`), nested emphasis added.
  - Table of Contents outline generation moved to `highlight.rs` with `extract_outline`, decoupling it from the UI thread and preventing parsing errors.
  - Swapped split view width calculations in `pdf_available_width` and `estimated_editor_viewport_width` fixed.
  - Reference link click does not reset markdown scroll if it resolves to same file.
  - Automatic layout zoom calculations fixed for split view.
  - Programmatic scroll target cleared properly on manual scroll, and now robustly handles asynchronous loading and layout-induced offsets shifts.
  - Programmatic scroll bypasses the Ctrl-key modifier scroll cancellation intercept, fixing split view navigation from Ctrl+Clicked markdown reference links.

## Completed Files

- `docs/EDITOR_PDF_SYNERGY_ROADMAP.md`
- `native/src/editor/buffer.rs`
- `native/src/app.rs`
- `native/src/views/interactive_pdf.rs`
- `native/src/messages.rs`
- `native/src/views/pdf_viewer.rs`
- `native/src/views/modals.rs`
- `native/src/views/pdf_annotations.rs`
- `native/Cargo.toml`
- `Cargo.lock`
- `native/src/pdf_notes.rs`
- `native/src/pdf_links.rs`
- `native/src/views/toc.rs`
- `native/src/editor/highlight.rs`

## Tests And Checks

Last full verification passed:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Focused tests added:

- `editor::highlight::tests::test_extract_outline`
- `app::tests::test_manual_scroll_clears_programmatic_scroll_target`
- `app::tests::test_split_view_width_calculations`
- `app::tests::test_reference_link_resolves_and_preserves_scroll`
- `app::tests::test_ctrl_click_programmatic_scroll_bypasses_cancellation`

## Known Worktree State

- `core/pdfium/libpdfium.so` was already modified before this work. Do not
  revert it unless user explicitly asks.
- Roadmap and handoff docs are new files.

## Next Best Task

Continue Milestone 2: Markdown Intelligence or begin Milestone 3: PDF Engine Upgrade.

Recommended next slice:
1. Extend parser coverage in `highlight.rs` for backlinks index and metadata extraction, or backlinks sidebar view integration.
2. Follow up on citation rendering quality or command palette shortcuts.
3. Update this handoff with result, tests, and next task.

## Standards Reminder

- Markdown mutations go through `EditorCommand`.
- Parsing stays in `native/src/editor/highlight.rs`.
- Renderer stays viewport-bounded.
- `page_index` is internal 0-based PDF page; `page_number` is user-visible
  1-based label.
- `pdf://` links go through `native/src/pdf_links.rs`.
- Avoid `unwrap`/`expect` outside tests unless invariant is documented nearby.
