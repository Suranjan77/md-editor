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
- First implementation slice is complete: PDF quote insertion now uses
  `EditorCommand::InsertPdfQuoteLink` instead of raw markdown insertion from the
  PDF message handler.
- Second implementation slice is complete: focused PDF highlights can be
  inserted into the active markdown note with
  `EditorCommand::InsertPdfAnnotationLink`, preserving the exact annotation
  target in the `pdf://` link.
- Quote-link markdown formatting lives beside editor command logic in
  `native/src/editor/buffer.rs`.
- Annotation-link markdown formatting also lives in
  `native/src/editor/buffer.rs`.
- PDF insert action in `native/src/app.rs` now builds an editor command from
  current PDF selection, page number, and `pdf://` link.
- PDF annotation insert action in `native/src/app.rs` now builds an editor
  command from the focused annotation, page number, and `pdf://` link with
  `annotation=...`.
- Copy-with-source-link still writes markdown to clipboard, but now reuses the
  same quote-link formatter so clipboard and insertion stay consistent.
- `native/src/views/interactive_pdf.rs` has a small clippy-only cleanup in the
  keyboard copy handler.
- `native/src/views/pdf_viewer.rs` exposes a focused-highlight `Cite` control
  when a markdown file is open.
- `native/src/views/pdf_annotations.rs` exposes the same `Cite` action in the
  annotation sidebar when a markdown file is open.
- `native/src/views/modals.rs` exposes a right-click annotation menu item for
  inserting the focused highlight into the active markdown note.
- Annotation context-menu item construction now lives behind
  `pdf_annotation_context_menu_items`, with `iced_test` coverage for the insert
  item and unavailable markdown-note state.
- `iced_test` is now a native dev-dependency for headless UI tests.
- UI-focused tests now cover focused-highlight toolbar `Cite` and annotation
  sidebar `Cite`, including inert behavior when no markdown note is open.
- Linked PDF note creation and append now flow through
  `build_linked_pdf_note_content`, which reports whether content was created,
  appended, or left unchanged.
- Command palette now adds contextual `Insert PDF Quote` and
  `Insert PDF Highlight` actions only when the active state can build the
  corresponding editor command.
- User-reported issue pass:
  - Esc now clears both PDF link preview and hidden context-menu modal so the
    next PDF reference can be clicked.
  - Split view now renders PDF on the left and markdown on the right.
  - Inserted PDF citations are more compact: no blank quoted spacer line and
    source label is `PDF p. N`.
- Started Milestone 2 (Markdown Intelligence): Added reference-style link recognition
  in `native/src/editor/highlight.rs` (both full and shortcut reference links),
  ensuring they expose metadata while remaining visually inactive or correct.
- Extended parser coverage in `highlight.rs` to support nested emphasis, inline
  links/emphasis inside headings, and footnote references (`[^1]`).
- Fixed Windows path separation mismatches and CRLF testing issues across several
  app and parser tests.

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

## Tests And Checks

Last full verification passed:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Focused tests added or updated:

- `editor::buffer::tests::pdf_quote_link_insert_is_single_undoable_transaction`
- `app::tests::pdf_selection_quote_link_command_targets_page`
- `editor::buffer::tests::pdf_annotation_link_insert_is_single_undoable_transaction`
- `editor::buffer::tests::pdf_annotation_link_insert_requires_text_and_link`
- `app::tests::pdf_insert_annotation_link_uses_annotation_target`
- `views::pdf_viewer::tests::focused_annotation_toolbar_cite_click_emits_insert_message`
- `views::pdf_viewer::tests::focused_annotation_toolbar_cite_is_inert_without_markdown_file`
- `views::pdf_annotations::tests::annotation_sidebar_cite_click_emits_insert_message`
- `views::pdf_annotations::tests::annotation_sidebar_cite_is_inert_without_markdown_file`
- `pdf_notes::tests::linked_pdf_note_builder_reports_create_append_and_unchanged`
- `pdf_notes::tests::linked_pdf_note_builder_handles_empty_selected_text_deliberately`
- `views::modals::tests::annotation_context_menu_includes_insert_only_when_markdown_open`
- `views::modals::tests::annotation_context_menu_prefers_open_linked_note_when_present`
- `views::modals::tests::annotation_context_menu_insert_click_emits_action_message`
- `views::command_palette::tests::command_palette_pdf_quote_click_emits_shortcut`
- `views::command_palette::tests::command_palette_pdf_highlight_click_emits_shortcut`
- `app::tests::command_palette_adds_pdf_insert_actions_only_when_available`
- `app::tests::pdf_quote_insert_requires_markdown_file`
- `app::tests::escape_closing_pdf_link_preview_clears_hidden_context_menu`
- `app::tests::split_view_places_pdf_before_markdown`
- `editor::highlight::tests::reference_link_span_exposes_metadata_but_is_inactive`
- `editor::highlight::tests::reference_link_span_reconstructs_source_lines`
- `editor::highlight::tests::malformed_reference_syntax_remains_plain_text`
- `editor::highlight::tests::headings_parse_inline_links_and_emphasis`
- `editor::highlight::tests::nested_emphasis_combines_bold_and_italic`
- `editor::highlight::tests::footnotes_parsed_as_links`

## Known Worktree State

- `core/pdfium/libpdfium.so` was already modified before this work. Do not
  revert it unless user explicitly asks.
- Roadmap and handoff docs are new files.

## Next Best Task

Continue Milestone 2: Markdown Intelligence.

Recommended next slice:

1. Follow up on citation rendering quality in the actual editor renderer (from Milestone 1):
   inspect how blockquote links render after the compact citation format and add
   renderer/UI regression coverage if spacing or link hit testing is still off.
2. Extend parser coverage in `highlight.rs` for robust table metadata, or expose
   structural metadata for outline and backlinks.
3. Write parser tests first for the chosen feature, following TDD.
4. Do not add markdown rules in `renderer.rs`.
5. Run required checks (`cargo fmt`, `clippy`, `cargo test`).
6. Update this handoff with result, tests, and next task.

UI testing rule for next slices:

- Any new visible button, context-menu item, modal, toolbar action, or sidebar
  action needs an `iced_test::simulator` test that proves the control renders and
  emits the expected `Message`.
- Any disabled-looking or unavailable UI state needs an `iced_test` test proving
  it is inert and does not emit a mutating message.
- Prefer focused view-level tests first; add app-level tests when state
  coordination matters.

## Roadmap Update Protocol

When completing work:

- Mark completed slices in `docs/EDITOR_PDF_SYNERGY_ROADMAP.md` only when the
  implementation and tests landed.
- Keep this file more operational than the roadmap: current state, exact files,
  checks run, blockers, and next best task.
- If a task stops mid-change, write:
  - changed files
  - last passing command
  - failing command and exact failure summary
  - safest resume step

## Standards Reminder

- Markdown mutations go through `EditorCommand`.
- Parsing stays in `native/src/editor/highlight.rs`.
- Renderer stays viewport-bounded.
- `page_index` is internal 0-based PDF page; `page_number` is user-visible
  1-based label.
- `pdf://` links go through `native/src/pdf_links.rs`.
- Avoid `unwrap`/`expect` outside tests unless invariant is documented nearby.
