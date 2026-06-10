# ADR-0002: Render PDFs with pdfium via core

Status: Accepted · Date: 2026-06-10

## Context

PDF viewing/annotation is a core product feature. Options considered at
adoption time: `pdfium-render` (Chromium's PDF engine, C library, fetched per
platform by `core/build.rs`), pure-Rust crates (`pdf`, `lopdf` — no rendering
or immature), `mupdf` (AGPL), poppler (GPL, heavy system dependency).

## Decision

Use pdfium through the `pdfium-render` crate with the `thread_safe` feature.
All pdfium calls live in `core` (`infrastructure/pdfium/`); `native` consumes
rendered RGBA images and extracted text/geometry through core types only
(enforced: `native` may not import `pdfium_render`).

## Consequences

- Battle-tested rendering fidelity and text extraction, including CJK,
  rotated pages, and malformed-file tolerance (exercised by
  `tests-fixtures/pdf/`).
- A native binary dependency must be fetched/bundled per platform; the
  build.rs download needs pinned checksums and license verification at
  release time (Phase 10).
- Rendering is fallible and slow enough to need a worker/queue model; that
  complexity stays inside core's pdfium module, never in views.
