# Editor And PDF Synergy Roadmap

This roadmap turns the markdown editor and PDF reader into one research
workspace. It is intentionally sized for a multi-month effort by a team of
about ten developers.

## Operating Rules

- Build every feature with TDD: failing unit test, failing state/integration
  test where relevant, implementation, regression coverage, then cleanup.
- UI-visible workflows need `iced_test` coverage: use headless simulator tests
  for rendered controls, click dispatch, disabled/inert states, and basic layout
  regressions before declaring UI work done.
- Keep markdown document mutations behind `EditorCommand`.
- Keep markdown parsing in `native/src/editor/highlight.rs`.
- Keep editor layout and draw proportional to visible content.
- Keep PDF annotations in SQLite sidecar storage; never mutate source PDFs.
- Use `page_index` for 0-based PDF pages and `page_number` for 1-based labels.
- Build and parse `pdf://` targets through `native/src/pdf_links.rs`.
- Keep `docs/HANDOFF.md` current at the end of each meaningful work session so
  the next human or agent can resume from the last verified state.
- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace` before handoff for code changes.
- Update the HANDOFF.md

## Team Lanes

- Editor Core: `DocBuffer`, `EditorCommand`, undo/redo, selection, markdown
  transforms.
- Editor Rendering: layout cache, height tree, renderer, media sizing,
  viewport-bounded drawing.
- PDF Core: PDFium worker, text geometry, page rendering, search, cache
  scheduling.
- PDF UX: page interaction, overlays, annotation sidebar, selection, keyboard
  navigation.
- Synergy: citations, `pdf://` links, linked notes, backlinks, companion notes.
- Search: file search, PDF search, global ranking, result preview.
- Storage: SQLite schema, migrations, sidecar integrity, vault index.
- QA Automation: fixtures, integration tests, performance tests, release smoke.
- Platform: PDFium packaging, portable settings, crash recovery.
- Workflow Design: command palette, split panes, keyboard-first research flows.

## Milestone 0: Test Foundation

- Maintain live handoff state in `docs/HANDOFF.md`, including current objective,
  completed slices, tests run, known dirty files, blockers, and next best task.
- Add markdown fixtures for large files, tables, math, images, links, headings,
  and hidden syntax markers.
- Add PDF fixtures for embedded text, rotated pages, internal links, TOC,
  different page sizes, scanned pages, and corrupt files.
- Add golden tests proving `StyledSpan.text` reconstructs each physical source
  line unless explicitly synthetic.
- Add PDF coordinate tests for zoom, page bounds, text rects, search scroll
  targets, and annotation hit testing.
- Add command tests for every mutating `EditorCommand`.
- Add `iced_test` UI tests for every new user-facing control, especially
  markdown/PDF synergy actions that appear in toolbars, sidebars, context menus,
  and modals.
- Add performance smoke tests for editor visible draw, PDF visible scheduling,
  and global search.

## Milestone 1: Editor Command Expansion

- Add explicit commands for PDF quote insertion, citation block insertion,
  linked-note section insertion, table cell editing, batch checkbox toggles, and
  block moves.
- Make each command one undoable transaction unless user-visible behavior
  requires separate steps.
- Add cursor, selection, undo, redo, dirty-state, and source-preservation tests.
- Keep app orchestration in `native/src/app.rs`; do not mutate markdown text
  directly from app state.

Initial slices complete:

- PDF quote insertion now uses `EditorCommand::InsertPdfQuoteLink` instead of
  preformatted markdown insertion from the PDF message handler.
- PDF annotation insertion now uses `EditorCommand::InsertPdfAnnotationLink`,
  preserving exact `pdf://` annotation targets from focused highlights.
- UI-focused `iced_test` coverage now exercises the focused-highlight toolbar
  and annotation-sidebar `Cite` controls, including inert states when no
  markdown file is open.
- Linked PDF note creation and append now use a canonical
  `build_linked_pdf_note_content` helper that reports created/appended/unchanged
  outcomes and preserves annotation targets.
- Annotation context-menu construction now has focused tests for insert,
  linked-note, and unavailable markdown-note states.
- Command palette now exposes contextual PDF quote/highlight insertion actions
  when state can build the underlying editor command, with `iced_test` click
  coverage and unavailable-state checks.
- Live handoff process exists in `docs/HANDOFF.md`; update it after each
  meaningful work session.

## Milestone 2: Markdown Intelligence

- Extend parser coverage in `highlight.rs` for reference links, footnotes,
  nested emphasis, headings with inline links, and robust table metadata.
- Expose structural metadata for outline, backlinks, and source-preserving
  block edits.
- Keep renderer consuming metadata only; no markdown parser rules in
  `renderer.rs`.
- Add regression tests for large-doc debounce and stale highlight generation
  handling.

## Milestone 3: PDF Engine Upgrade

- Separate PDF render, query, and search queues while preserving one PDFium
  binding worker.
- Prioritize visible pages and newest navigation target.
- Add cancelable streaming search with generation validation.
- Add page text cache invalidation by document identity.
- Add bounded high-zoom rendering strategy and eviction tests.

## Milestone 4: Deep Linking And Citations

- Support stable targets for page, selection, annotation, named destination, and
  focused backlink.
- Add bidirectional relationships between markdown notes, PDF documents,
  annotations, and quote blocks.
- Add linked-note sync when annotation text or notes change.
- Detect missing PDFs, deleted annotations, missing notes, and moved vault paths.
- Test path escaping for spaces, `?`, `#`, `&`, parentheses, and markdown
  delimiter characters.

## Milestone 5: Unified Navigation

- Extract `native/src/pdf_navigation.rs` for page targets, history, and
  scroll/focus behavior.
- Add cross-pane history for editor cursor, markdown links, PDF links, search
  hits, annotations, and backlinks.
- Add follow-citation and show-usages commands.
- Add combined markdown outline plus PDF TOC navigator.

## Milestone 6: Unified Search

- Define one query model for markdown, PDF text, filenames, headings,
  annotations, and quick notes.
- Keep pane-local `Ctrl+F`; add global research search with typed result groups.
- Add deterministic ranking with current-document, exact-phrase, and linked-note
  boosts.
- Add streaming results, cancellation, and result previews from PDF rects or
  markdown context.

## Milestone 7: Annotation System 2.0

- Add highlight, underline, strike, area note, free note, tags, colors, and
  status.
- Add filters by page, color, tag, linked state, and unresolved state.
- Add batch export with stable ordering.
- Add sidecar migrations and integrity checks.
- Add rename/move repair for linked PDF and markdown paths.

## Milestone 8: Research Writing Workflow

- Add citation palette sourced from current PDF selection, search result, or
  annotation.
- Add collect-excerpts mode with queued highlights and batch markdown insertion.
- Add reading-session notes bound to page ranges.
- Add templates for generated linked-note sections.
- Add idempotent append tests for every generated section format.

## Milestone 9: Scale And Performance

- Target 50k markdown files, 5k PDFs, and 1M annotations.
- Keep editor draw and layout bounded by visible content.
- Keep PDF scheduling bounded by visible pages plus small preload window.
- Add performance thresholds to CI for hot paths.
- Add debug assertions or counters for accidental full-document hot-path scans.

## Milestone 10: UX Completion

- Build and maintain an `iced_test` regression suite for critical UI flows:
  command palette actions, PDF context menus, annotation controls, split-pane
  focus, search bars, modal submit/cancel paths, and disabled states.
- Add command-palette entries for cross-pane actions.
- Add keyboard-first annotation and citation flows.
- Add consistent loading, empty, and error states.
- Add layouts for editor-only, PDF-only, split, and synced research mode.
- Add accessibility pass for focus order, labels, contrast, and keyboard
  navigation.

## Milestone 11: Release Hardening

- Package PDFium for each target platform.
- Validate portable settings and SQLite location next to executable.
- Add SQLite migration backup and rollback.
- Add crash-safe markdown save path.
- Add release smoke tests from `docs/LAUNCH.md`.
