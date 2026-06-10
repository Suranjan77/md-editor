# ADR-0003: SQLite sidecar for annotations & metadata

Status: Accepted · Date: 2026-06-10

## Context

PDF highlights/notes, tracker sessions, settings, and search metadata need
persistence. Alternatives: writing annotations into the PDF files themselves
(mutates user documents, slow, lossy for our quad/color/link model), JSON
sidecar files per document (no querying, no transactions, sync hazards), or a
single SQLite database (rusqlite, bundled).

## Decision

One SQLite database per app profile acts as a sidecar store: PDF annotations,
study tracker, settings/KV, and search support tables. User files (.md, .pdf)
are never mutated for metadata purposes. Access is only through repository
modules in `core/src/database/`, which stay private to core behind services.

## Consequences

- Transactions, querying, and FTS5 (Phase 6 persistent index) come for free.
- Annotations are decoupled from documents, so they survive PDF re-downloads
  only if keyed robustly — Phase 5 re-keys them by document SHA-256 and adds a
  numbered-migration runner.
- Export/import (Phase 8) is required for annotations to leave the app, since
  the PDFs themselves stay clean; optional burn-in export is a separate spike.
- The local-first ethos holds: no network, single file users can back up.
