# Live Handoff

Last updated: 2026-06-02

## Purpose

This document is the live pickup point for humans and agents working on the
editor and PDF synergy roadmap. Update it at the end of every meaningful work
session, especially when a task is incomplete, blocked, or changes the expected
next step.

## Current Objective

Implement the multi-month UI and UX improvement plan in
`docs/UI_UX_IMPROVEMENT_ROADMAP.md`, using TDD and the repository standards.

## Current State

- Roadmap exists in `docs/EDITOR_PDF_SYNERGY_ROADMAP.md`.
- UI/UX roadmap exists in `docs/UI_UX_IMPROVEMENT_ROADMAP.md`.
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
- Milestone 6 is complete:
  - Added `UnifiedSearchQuery`, `UnifiedSearchSource`, and `UnifiedSearchRanking` in `core/src/types.rs`.
  - `search_vault_unified` now delegates to `search_vault_unified_query`, preserving existing call sites while enabling typed source selection.
  - Query sources cover markdown content, PDF content, filenames, headings, annotations, and quick notes.
  - Quick notes now have a distinct `SearchResultGroup::QuickNote` and render/navigate like PDF annotation results.
  - Ranking boosts for current document, exact phrase, and linked notes are part of the query model.
  - App-level global search now builds and passes `UnifiedSearchQuery` to the core search path.
  - Global search tracks DB and active-PDF streaming completion separately, so successful DB results clear the spinner and active PDF streaming keeps it alive until finished.
  - Stale active-PDF search results are suppressed from global results when query/source state changes or global search closes.
  - Global search overlay now has explicit source toggles for files, headings, markdown, PDF text, annotations, and quick notes.
  - App state persists selected global search sources and feeds them into `UnifiedSearchQuery`; disabling PDF text cancels/suppresses active PDF streaming results.
  - Search result previews now use centered snippets around the matched text for markdown, annotations, quick notes, and active-PDF streamed results.
  - Active-PDF streamed global results include the number of PDF text rect areas in the preview context, making multi-rect PDF hits clearer.
  - Active-PDF streamed global results now carry the underlying PDF search match index, and clicking them activates the exact search hit so PDF highlight rects and scroll target are preserved.
  - Global PDF text search now also searches registered vault PDFs beyond the active PDF via a background task, merging results by search generation.
  - Vault-wide registered-PDF results are ignored when stale or when the global overlay is closed; active-PDF streaming remains separate so exact-hit activation still works for the open PDF.
  - Vault-wide registered-PDF search is bounded to 32 documents and 200 results per query, skips the active PDF, and stops collecting once the cap is reached.
  - Global search overlay now shows bounded PDF text search status such as searched document counts and whether document/result caps were reached.
  - Added durable `pdf_text_search` FTS cache table for PDF page text.
  - Loaded visible PDF page text is written into the durable cache, and vault-wide global PDF search (discovering all PDFs from disk) consults cached page text before falling back to extraction.
  - Opening a vault now kicks off a bounded background PDF text index pass for all PDFs discovered on disk, indexing up to 16 documents and 3 pages per document into the durable cache.
  - Optimized background PDF indexing to skip documents whose caches are already fresh and populated.
- Milestone 7 is complete:
  - Added new annotation types: Highlights, Underlines (in blue), Strikeouts (in red), notes, tags, and unresolved/resolved status.
  - Implemented multi-layered annotation filters (color, page, tag, linked note state, and unresolved/resolved status) rendering dynamic compact controls in the annotations panel.
  - Annotation sidebar action layout keeps `Cite` visible/clickable with the expanded filter controls, including enabled and disabled `iced_test` coverage.
  - Refactored batch markdown annotation exporter with reading-flow stable ordering `(page_capacity, text_capacity, timestamp)` including all metadata fields.
  - Built a reference repair engine in `rename_entry` that updates FTS searches, PDF metadata, annotation link bindings, and rewrites markdown link URLs in-place across the vault.
  - SQLite schema migration setup automatically updates tables to preserve tags and status fields.
- Milestone 8 is complete:
  - Added Citation Palette sourced from selection, annotations, and SQLite FTS search matches.
  - Implemented collect-excerpts queue mode to collect citations and batch insert them into markdown files.
  - Added templating support for generated linked-note sections with Default, Detailed, and Minimal layout options.
  - Added page-range reading session notes with customizable date, page-range, and session notes formatting.
  - Guaranteed idempotency for all templates and reading session note appends.
- Milestone 9 has an initial editor hot-path slice complete:
  - Large code/table/math block draw metadata in `native/src/editor/renderer.rs` now scans a bounded visible/hovered-line window instead of scanning entire blocks every frame.
  - Code/table captions now use cached `block_ranges` starts instead of searching the whole document during draw.
  - Horizontal scrollbar width checks use the hovered line as a bounded scan hint.
  - Regression tests cover the bounded scan cap and large-block width hint behavior.
  - PDF render scheduling now uses named preload/cap constants, derives viewport ranges from `PdfLayout::visible_range`, and caps accidental large render ranges before queueing page render/text work.
  - CI-stable performance smoke tests now assert logarithmic operation counters for large `HeightTree` and `PdfLayout` lookups instead of relying on wall-clock timing.
- Milestone 10 has an initial command-palette slice complete:
  - Command palette now exposes unified cross-pane navigation back/forward commands.
  - `Shortcut::NavBack` and `Shortcut::NavForward` dispatch through the existing unified `PdfNavBack`/`PdfNavForward` handlers.
  - `iced_test` coverage verifies command-palette clicks emit the new navigation shortcuts.
- Milestone 10 has an initial UI state slice complete:
  - TOC panel now renders its empty state whenever the panel is visible instead of being suppressed when both markdown outline and PDF TOC are empty.
  - `iced_test` coverage verifies TOC, backlinks, and global-search empty/error states, including PDF search status text.
- Milestone 10 has an initial keyboard-first citation slice complete:
  - Citation palette input is focused when opened from shortcut or command dispatch.
  - Pressing Enter in the citation palette submits the first result.
  - App-level coverage verifies first-result submission queues citations in excerpt mode.
- UI/UX roadmap Milestone 0 has an initial baseline slice complete:
  - Created `docs/UI_UX_AUDIT.md` with current primary surfaces, states, message paths, keyboard paths, style sources, accessibility gaps, and missing coverage.
  - Added reusable app-state fixture helpers in `native/src/app.rs` tests for no-vault, markdown, PDF, split research, global search, command palette, active modal, and annotation-heavy PDF states.
  - Added app-level `iced_test` smoke coverage proving representative baseline states render stable user-visible labels and status text.
  - Extended Milestone 0 fixtures to cover large markdown, active file search, and narrow split research layout.
  - Added keyboard accessibility smoke coverage for command palette, citation palette, file search, TOC, focus mode, and Escape overlay priority.
  - Added deterministic app-shell layout smoke coverage asserting primary text labels do not overlap in markdown, PDF, and narrow split layouts.
  - Extracted app focus targets into testable `FocusTarget` mappings.
  - Added a deterministic command-palette input ID and default-focus task when opening the command palette.
  - Extended focus-target coverage to verify rendered file search, global search, PDF search, command palette, and citation palette input IDs.
  - Added large-annotation filter baseline counter coverage; current annotation filtering is explicitly measured as a linear pass with stable sorted output.
  - Added `docs/UI_UX_RELEASE_CHECKLIST.md` and linked it from launch/features docs for layout overlap, keyboard traps, labels, contrast, stale loading, reduced motion, and platform checks.
- UI/UX roadmap Milestone 1 has an initial app-shell model slice complete:
  - Added `docs/APP_SHELL_SPEC.md` defining work zones, layout modes, panel persistence rules, active-pane rules, command groups, status-surface direction, and open integration work.
  - Added `native/src/app_shell.rs` with a tested pure app-shell state model covering layout-mode derivation, split prerequisites, search/palette override, panel width clamping, narrow-window collapse, and command groups.
  - `MdEditor::view` now derives an `AppShellState` snapshot and command-group set without changing rendered behavior, keeping the shell model live for incremental integration.
  - Added app-shell document visibility predicates and routed `MdEditor::view` no-vault, split research, PDF, and image branch selection through the shell state.
  - Added app-level regression coverage proving UI audit fixtures derive the expected shell modes, active panes, command groups, and narrow-window persistence collapse behavior.
  - Added compact `app_shell_persistence` config storage for sidebar collapsed state, workflow tab, split ratio, reference width, and last focused pane.
  - User-driven shell changes now save persistence on sidebar/workflow toggles, split toggles, split-resizer completion, Show Usages, and active-pane changes.
  - Added an initial app-shell status model covering save state, search/PDF status text, active pane, toast, and background errors.
  - `MdEditor::view` now derives the shell status snapshot without changing rendered behavior, and tests cover dirty markdown, PDF page/zoom labels, search progress, active pane, and toast/error priority.

## Completed Files

- `docs/EDITOR_PDF_SYNERGY_ROADMAP.md`
- `docs/UI_UX_IMPROVEMENT_ROADMAP.md`
- `native/src/editor/buffer.rs`
- `native/src/app.rs`
- `native/src/views/interactive_pdf.rs`
- `native/src/messages.rs`
- `native/src/views/pdf_viewer.rs`
- `native/src/views/modals.rs`
- `native/src/views/pdf_annotations.rs`
- `native/src/views/command_palette.rs`
- `native/src/views/search.rs`
- `native/src/views/backlinks.rs`
- `native/Cargo.toml`
- `Cargo.lock`
- `native/src/pdf_notes.rs`
- `native/src/pdf_links.rs`
- `native/src/views/citation_palette.rs`
- `native/src/pdf_navigation.rs`
- `native/src/editor/renderer.rs`
- `native/src/editor/layout_tree.rs`
- `native/src/pdf_layout.rs`
- `native/src/views/toc.rs`
- `native/src/editor/highlight.rs`
- `native/src/integrity.rs`
- `core/src/file_index.rs`
- `core/src/vault.rs`
- `core/src/pdf.rs`
- `core/src/types.rs`
- `docs/PDF_VIEWER_ARCH.md`
- `docs/UI_UX_AUDIT.md`
- `docs/UI_UX_RELEASE_CHECKLIST.md`
- `docs/APP_SHELL_SPEC.md`
- `native/src/app_shell.rs`

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
- `vault::tests::unified_search_query_filters_sources_and_splits_quick_notes`
- `vault::tests::search_result_preview_centers_match_and_preserves_label`
- `vault::tests::unified_search_markdown_results_use_context_preview`
- `vault::tests::cached_pdf_text_search_returns_page_results`
- `app::tests::stale_pdf_matches_do_not_enter_global_results`
- `app::tests::global_search_query_uses_source_toggles`
- `app::tests::pdf_content_global_result_activates_matching_search_hit`
- `app::tests::vault_pdf_text_results_merge_only_for_visible_current_search`
- `app::tests::registered_pdf_search_targets_skip_active_and_cap_work`
- `app::tests::pdf_search_status_reports_result_cap_first`
- `app::tests::registered_pdf_index_targets_cap_documents`
- `vault::tests::test_pdf_cache_freshness_and_invalidation`
- `vault::tests::test_list_all_pdf_files_discovers_unregistered_pdfs`
- `app::tests::test_search_registered_pdf_text_results_does_not_deadlock`
- `app::tests::test_search_unopened_pdf_discovered_from_disk`
- `app::tests::test_pdf_toc_navigation_completes_if_already_scrolled`
- `views::pdf_annotations::tests::test_annotation_filtering`
- `vault::tests::test_rename_pdf_and_markdown_repairs_references`
- `views::citation_palette::tests::test_citation_palette_selection_renders_and_clicks`
- `views::citation_palette::tests::test_citation_palette_annotation_renders_and_clicks`
- `views::citation_palette::tests::test_citation_palette_search_hit_renders_and_clicks`
- `app::tests::test_excerpt_mode_queue_and_batch_insert`
- `pdf_notes::tests::test_templates_linked_note_and_idempotency`
- `pdf_notes::tests::test_reading_session_and_idempotency`
- `editor::renderer::tests::bounded_block_scan_range_caps_large_blocks`
- `editor::renderer::tests::block_content_width_uses_bounded_scan_hint`
- `app::tests::pdf_render_page_range_caps_accidental_large_spans`
- `app::tests::pdf_viewport_render_range_uses_visible_pages_plus_small_preload`
- `editor::layout_tree::tests::large_height_tree_queries_stay_logarithmic`
- `pdf_layout::tests::large_pdf_layout_page_lookup_stays_logarithmic`
- `views::command_palette::tests::command_palette_navigation_clicks_emit_cross_pane_shortcuts`
- `views::toc::tests::empty_toc_renders_empty_state`
- `views::backlinks::tests::empty_visible_backlinks_panel_renders_empty_state`
- `views::search::tests::visible_global_search_renders_empty_state_when_query_has_no_results`
- `views::search::tests::visible_global_search_renders_error_and_pdf_status`
- `views::citation_palette::tests::citation_palette_input_submits_first_item_message`
- `app::tests::citation_palette_submit_first_queues_first_item_in_excerpt_mode`
- `app::tests::ui_audit_fixture_no_vault_renders_welcome`
- `app::tests::ui_audit_fixture_markdown_file_renders_shell_and_editor`
- `app::tests::ui_audit_fixture_pdf_file_renders_pdf_toolbar`
- `app::tests::ui_audit_fixture_split_research_renders_both_active_paths`
- `app::tests::ui_audit_fixture_overlays_and_sidebars_render_stable_states`
- `app::tests::ui_audit_fixture_large_and_narrow_states_render_stable_shell`
- `app::tests::ui_audit_keyboard_shortcuts_expose_baseline_accessibility_paths`
- `app::tests::ui_audit_focus_targets_map_to_rendered_input_ids`
- `app::tests::ui_audit_escape_closes_modal_before_background_overlays`
- `app::tests::ui_audit_shell_labels_do_not_overlap_in_baseline_layouts`
- `views::command_palette::tests::command_palette_input_has_focusable_id`
- `views::pdf_annotations::tests::large_annotation_filter_reports_linear_baseline_counter`
- `app_shell::tests::derives_primary_layout_modes`
- `app_shell::tests::split_research_requires_markdown_and_pdf`
- `app_shell::tests::search_and_palette_modes_override_document_layout`
- `app_shell::tests::persistence_clamps_widths_and_narrow_window_collapses_sidebars`
- `app_shell::tests::command_groups_match_layout_context`

## Known Worktree State

- `core/pdfium/libpdfium.so` was already modified before this work. Do not
  revert it unless user explicitly asks.
- UI/UX roadmap and handoff docs were updated in this session.

## Next Best Task

Continue Milestone 1 in `docs/UI_UX_IMPROVEMENT_ROADMAP.md`.

Recommended next slice:
1. Render active-pane/status indicators through a unified status surface using `AppShellStatus`.
2. Replace remaining fixed sidebar width assumptions with the persisted shell width model.
3. Add active-pane indicator tests once UI renders the indicator.

## Standards Reminder

- Markdown mutations go through `EditorCommand`.
- Parsing stays in `native/src/editor/highlight.rs`.
- Renderer stays viewport-bounded.
- `page_index` is internal 0-based PDF page; `page_number` is user-visible
  1-based label.
- `pdf://` links go through `native/src/pdf_links.rs`.
- Avoid `unwrap`/`expect` outside tests unless invariant is documented nearby.
