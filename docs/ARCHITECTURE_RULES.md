# Architecture Rules

Rules here are enforceable contracts. Migration goals belong in
`CODEBASE_RESTRUCTURING_PLAN.md`; decisions and rationale belong in `docs/adr/`.

Run boundary checks with:

```bash
just architecture
```

## Dependency Direction

Dependencies point inward:

```text
presentation -> feature coordination -> application services -> domain
                                                        ^
                                                        |
                                              infrastructure adapters
```

- `core` must never depend on `native`.
- `core` must not import Iced or native presentation modules.
- `native/src/app` may compose UI and map messages. It must not import
  `rusqlite` or `pdfium-render`.
- Filesystem, SQLite, PDFium, and OS APIs stay behind core or platform
  boundaries as those boundaries are extracted.
- Direct `rusqlite` use in native is forbidden. Native uses core repository and
  service APIs.

See [ADR-0001](adr/0001-dependency-direction.md) and
[ADR-0003](adr/0003-repository-boundaries.md).

## Application Coordination

- Top-level application update routes messages and coordinates cross-feature
  outcomes.
- Feature state owns feature-specific derived state and invalidation.
- Feature reducers may construct typed tasks or effects. They must not execute
  filesystem, database, or PDFium work directly.
- Shared source of truth remains singular; feature states must not maintain
  unsynchronized copies.
- Cross-feature actions use explicit messages or typed effects, not a global
  event bus.

See [ADR-0002](adr/0002-feature-reducers-and-messages.md).

## Editor Pipeline

Ownership order:

1. `native/src/editor/buffer`: source, cursor, selection, undo/redo, commands.
2. `native/src/editor/parser`: markdown parsing and styled projection.
3. `native/src/editor/layout_tree.rs` and `layout_cache.rs`: measured layout.
4. `native/src/editor/renderer`: widget layout, drawing, hit testing, movement.

Rules:

- Markdown document mutations go through `EditorCommand`.
- Direct `DocBuffer::set_text` is allowed only for file load, an explicit
  whole-buffer transaction, or tests.
- Markdown grammar and parsing stay in `native/src/editor/parser` or its
  future submodules under `native/src/editor`.
- Renderer consumes parser output. It must not call markdown parsers or import
  parser implementation crates.
- Renderer layout and drawing must remain proportional to visible content.
- Buffer domain code must not depend on Iced.

## PDF Pipeline

Ownership order:

1. `core/src/infrastructure/pdfium`: PDFium adapter, extraction, search, annotations.
2. `native/src/features/pdf/navigation.rs`: page geometry and visible ranges.
3. `native/src/features/pdf/tasks.rs`: image cache and eviction.
4. `native/src/app/interactive_pdf.rs`: drawing and pointer interaction.
5. Application coordination: task scheduling and pane state.

Rules:

- `page_index` means 0-based internal index.
- `page_number` means 1-based UI or link label.
- PDF annotations remain sidecar data; source PDFs are never mutated.
- PDF rendering and scheduling remain viewport-bounded.

## Paths And Persistence

- `vault_path` means vault-relative path.
- `abs_path` means absolute filesystem path.
- Resolve vault paths through one checked boundary; reject traversal outside
  vault root.
- User markdown and PDF files remain normal files, never SQLite blobs.
- Schema changes require migrations and fresh-database plus upgrade tests.

See [ADR-0004](adr/0004-path-and-page-types.md).

## Enforcement

`scripts/architecture-check.sh` hard-fails:

- reverse `core -> native` dependency;
- UI dependencies imported by `core`;
- SQLite or PDFium imported by views;
- markdown parser implementation used by renderer production code;
- direct buffer `set_text` in production code.

Oversized source files remain warning-only migration metrics. Native direct SQL
and public `AppState` infrastructure fields are hard failures.
