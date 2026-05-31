# Architecture Rules

## Crate Boundaries

- `core`: vault management, indexing, SQLite storage, PDF rendering/search, and
  domain data types. It must not depend on native UI.
- `native/app.rs`: application orchestration, routing, task batching, and state
  coordination.
- `native/editor`: markdown buffer, parser/highlighter, layout cache, layout
  tree, and custom renderer.
- `native/views`: UI composition and message wiring only.

## Module Ownership

Editor pipeline:

1. `buffer.rs`: source text, cursor, selection, undo/redo, editor commands.
2. `highlight.rs`: markdown source to `StyledLine`/`StyledSpan`.
3. `layout_tree.rs` and `layout_cache.rs`: measured height state and cache keys.
4. `renderer.rs`: iced widget layout, draw, hit testing, and visual movement.

PDF pipeline:

1. `core/src/pdf.rs`: PDFium calls, text extraction, search, annotations.
2. `native/pdf_layout.rs`: page slot geometry and visible range math.
3. `native/pdf_page_cache.rs`: image cache and eviction policy.
4. `native/views/interactive_pdf.rs`: page drawing and pointer interaction.
5. `native/app.rs`: task scheduling and pane coordination.

## High-Risk Boundaries

- Do not mutate markdown text directly from `app.rs`; use `EditorCommand`.
- Do not parse markdown inside renderer.
- Do not perform full-document measurement or drawing in renderer hot paths.
- Do not mix absolute filesystem paths with vault-relative paths.
- Do not mix 0-based page indexes with 1-based page labels.
- Do not store user documents in SQLite; vault files remain normal files.

## Refactor Targets

When touching these areas repeatedly, prefer extracting modules:

- `native/src/pdf_navigation.rs`: PDF/back-forward/page target behavior.
- `native/src/pdf_links.rs`: `pdf://` parsing and quote/link construction.
- `native/src/editor_actions.rs`: app-level editor command helpers.
- `native/src/media_cache.rs`: image/math cache loading and diagnostics.

Extraction rule: move behavior only when tests already cover it or the move adds
tests in the same change.
