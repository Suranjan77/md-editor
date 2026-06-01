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
  - `highlight.rs` now exposes `extract_document_metadata`, including outline, markdown links, and navigable anchors.
  - Parser metadata now includes shallow top-of-file frontmatter aliases/tags through `extract_frontmatter_metadata`.
  - Markdown/PDF link anchor lookup in `app.rs` now uses parser metadata for headings and generated widget IDs before falling back to raw explicit-anchor syntax.
  - Editor saves now feed parser-derived local markdown link targets into `core::FileIndex` through `save_file_with_markdown_link_targets`, avoiding a native-to-core dependency inversion.
  - Native markdown saves are centralized through `save_markdown_file_with_parser_targets`; editor saves and PDF linked-note creation both use parser-derived link targets.
  - Opening a vault now runs native parser-backed backlink reindex after core FTS/list setup, replacing regex-only backlinks with parser-derived local links.
  - Opening a markdown file now reindexes that file through parser metadata, so external edits get corrected before backlinks are shown.
  - Backlink indexing now accepts parser-derived wiki links, local inline markdown links, and `pdf://` links resolved to vault PDF paths, while filtering external schemes and local anchors.
  - Reference-style link definitions `[ref]: target` are now parsed, highlighted as clickable links in the editor, and resolved `[text][ref]` / `[ref]` references index as backlinks using their resolved targets.
  - Regression tests for large-document highlight debounce and stale highlight generation handling are now added and verified.
- Milestone 3 has an initial reliability slice complete:
  - PDF render and query/search workers are protected by `with_pdfium_access`, a process-wide PDFium mutex wrapper.
  - This fixes `malloc(): smallbin double linked list corrupted` crashes seen when PDF search interleaved with rendering.
  - Regression coverage now runs streamed PDF search while repeatedly rendering the same document.
  - Subagent docs scan found no additional stale PDFium worker/search docs after the architecture updates.
  - Subagent engine review recommends keeping PDFium: current app needs rendering, text geometry, search rects, links, TOC, preview crops, and portable packaging; MuPDF/Poppler remain migration candidates only if instability persists after serialization.
  - Page text cache invalidation by document identity (clearing cached page text on opening a new PDF document) has been verified and tested.
- Milestone 4 is complete:
  - Added `native/src/integrity.rs` for vault-wide reference checking (detecting missing PDFs, deleted annotations, missing notes, and moved vault paths).
  - Fixed database schemas (`document_id` primary key and non-null required columns) in the integrity checking code and mock database inserts.
  - Vault integrity checks are run automatically when opening a vault.
  - Linked-note syncing added: when annotation notes or text are edited, the changes are propagated to the linked markdown note on disk. If the synced note is currently open, the editor buffer is updated and re-highlighted in real-time.
  - Escaping and targets for pages, annotations, and paths containing spaces/special characters are fully handled.
- Milestone 5 is complete:
  - Extracted unified cross-pane back/forward history tracking into `native/src/pdf_navigation.rs` supporting both Markdown and PDF pane locations (`NavigationTarget`).
  - Unified navigation history is integrated into the core message dispatcher (`Message::PdfNavBack` and `Message::PdfNavForward`), supporting switching active panels and file reloading on backward/forward jumps.
  - Sidebar clicks and search result selections correctly record to the unified history stack prior to navigation.
  - Keyboard shortcuts Alt+G and Alt+U mapped to FollowCitation and ShowUsages respectively.
  - Follow Citation locates the link under the editor cursor and triggers navigation.
  - Show Usages queries all referencing markdown files in the vault and opens/focuses the backlinks sidebar.
  - Combined Outline and TOC navigator sidebar implemented to display both Markdown headings and PDF bookmarks simultaneously.

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
- `native/src/pdf_navigation.rs`
- `native/src/views/toc.rs`
- `native/src/editor/highlight.rs`
- `native/src/integrity.rs`
- `core/src/file_index.rs`
- `core/src/vault.rs`
- `core/src/pdf.rs`
- `docs/PDF_VIEWER_ARCH.md`

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
- `pdf::tests::pdf_search_and_render_share_pdfium_safely`
- `editor::highlight::tests::extract_markdown_links_reports_backlink_metadata`
- `editor::highlight::tests::extract_document_metadata_reports_outline_links_and_anchors`
- `editor::highlight::tests::extract_frontmatter_metadata_reports_aliases_and_tags`
- `file_index::tests::update_file_targets_accepts_parser_metadata_targets`
- `vault::tests::save_file_with_markdown_link_targets_uses_parser_supplied_links`
- `app::tests::indexable_markdown_link_target_filters_external_links`
- `app::tests::save_markdown_file_with_parser_targets_indexes_local_links`
- `app::tests::reindex_vault_with_parser_targets_replaces_regex_backlinks`
- `app::tests::reindex_markdown_file_with_parser_targets_updates_opened_file`
- `editor::highlight::tests::reference_style_link_resolution_and_indexing`
- `app::tests::save_markdown_file_with_reference_links_indexes_resolved_targets`
- `app::tests::test_large_doc_highlight_debounce_and_reset`
- `app::tests::test_stale_highlight_generation_handling`
- `app::tests::test_pdf_open_clears_page_text_cache`
- `integrity::tests::test_vault_integrity_checks`
- `integrity::tests::test_vault_integrity_moved_paths`
- `pdf_notes::tests::test_sync_annotation_note_in_markdown`
- `app::tests::test_sync_quick_note_to_linked_note_file`
- `app::tests::test_cross_pane_navigation_history`
- `app::tests::test_follow_citation`
- `app::tests::test_show_usages`
- `app::tests::test_combined_outline_toc_navigator`

## Known Worktree State

- `core/pdfium/libpdfium.so` was already modified before this work. Do not
  revert it unless user explicitly asks.
- Roadmap and handoff docs are new files.

## Next Best Task

Begin Milestone 6: Unified Search.

Recommended next slice:
1. Define the query model for global search to encompass markdown, PDF text, filenames, headings, annotations, and quick notes.

## Standards Reminder

- Markdown mutations go through `EditorCommand`.
- Parsing stays in `native/src/editor/highlight.rs`.
- Renderer stays viewport-bounded.
- `page_index` is internal 0-based PDF page; `page_number` is user-visible
  1-based label.
- `pdf://` links go through `native/src/pdf_links.rs`.
- Avoid `unwrap`/`expect` outside tests unless invariant is documented nearby.
