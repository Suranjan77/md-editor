# Refactor Baseline

Captured June 7, 2026 before codebase restructuring.

## Source Shape

| File | Lines |
| --- | ---: |
| `native/src/app.rs` | 10,001 |
| `native/src/editor/renderer.rs` | 4,801 |
| `native/src/editor/highlight.rs` | 2,322 |
| `native/src/editor/buffer.rs` | 2,180 |
| `core/src/pdf.rs` | 1,749 |
| `core/src/vault.rs` | 1,554 |
| `core/src/state.rs` | 796 |
| `native/src/messages.rs` | 328 |
| `native/src/main.rs` | 434 |

Additional structural measures:

- `MdEditor` leaf fields: 108.
- Flat `Message` variants: approximately 214.
- Native production modules using `rusqlite`: `app.rs`, `integrity.rs`.
- Workspace tests: 299 total, 35 core and 264 native.

## Existing Characterization Coverage

Current tests cover critical migration behavior:

- Overlay and modal close precedence.
- App shell persistence round trip.
- Editor command undo/redo and Unicode boundaries.
- Large-document highlight debounce and stale generation rejection.
- PDF generation, pending work, cache, viewport scheduling, and navigation.
- Cross-pane citation and navigation workflows.
- Vault save, rename, reference repair, indexing, and unified search.
- PDF annotation storage, linked notes, and cached text invalidation.
- Renderer cursor, selection, wrapping, height tree, and bounded block scans.

## Required Quality Gate

Every phase must pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Performance Invariants

Until automated benchmark fixtures exist, preserve these tested structural
properties:

- Editor draw and block scans remain viewport bounded.
- Height lookup remains logarithmic through `HeightTree`.
- PDF rendering schedules visible pages plus bounded preload only.
- PDF page, text, and link caches retain explicit limits and invalidation.
- Stale async results clear pending state before rejection.
- Markdown parsing remains outside renderer.

Performance-sensitive extraction requires before/after release-build smoke
measurement with representative 1,000-line and 10,000-line markdown files and
100-page PDF.

Run repeatable release-mode structural timings with:

```bash
just benchmark
```

Runner reports wall time, CPU time, and peak RSS for release build, 10,000-line
markdown parsing, logarithmic editor/PDF layout lookups, viewport-bounded PDF
scheduling, and unified vault search. Record host and commit when comparing
results.

## Change Control

- Phase commits contain only restructuring work and supporting tests/docs.
- Existing unrelated working-tree changes remain uncommitted.
- Mechanical moves stay separate from behavior changes where practical.
- Persisted schema changes require migration tests and independent commit.
