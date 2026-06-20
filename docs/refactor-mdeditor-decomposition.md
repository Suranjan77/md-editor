# Plan: Decompose the `MdEditor` god-struct

> Status: **proposed / not started.** Deferred until after the core-hardening
> fixes (DB/XDG, path-traversal guard, async vault indexing, search-match
> cache, repo hygiene) land. This document is the migration plan for that work.

## Problem

`native/src/app.rs` defines a single `MdEditor` struct with ~80 flat fields and
a ~2,000-line `update()`. Every feature — vault, editor, PDF viewer, PDF
annotations/study, tracker, search, modals, command palette, TOC, split view,
window geometry — shares one struct and one dispatch. Consequences:

- No locality: changing one feature risks all others.
- The `update` match is too large to reason about as a unit.
- Sub-features that clearly form a unit (PDF has ~25 `pdf_*` fields) are smeared
  across the struct instead of owning their state.

## Goal

Group related fields and their message handling into cohesive sub-states, each
owning a slice of the struct and an `update(&mut self, msg) -> Task<Message>`
method. `MdEditor` becomes a thin shell that routes messages and composes views.

The `Message` enum is already grouped by comment (`// ── PDF ──`, etc.); mirror
that structure in the state.

## Target shape

```
MdEditor
├── vault:    VaultState     (root, entries, expanded folders, sidebar flags, backlinks)
├── editor:   EditorPane     (buffer, highlighted_lines, highlight generation/debounce,
│                             toc_entries, editor scroll/viewport, buffer_revision)
├── pdf:      PdfPane        (all pdf_* fields: pages, dimensions, zoom, scroll,
│                             annotations, selection, page text, study state)
├── tracker:  TrackerState   (sessions, kv, tabs, config content, manual-entry fields)
├── search:   SearchState    (query/replace/flags, results, match index/cache,
│                             pdf search results + page index)
└── ui:       UiState        (modals, command palette, toast, split view, window size)
```

Shared services (`Arc<AppState>`, `vault_root`) stay accessible to each sub-state
either by passing `&AppState` into their `update`/`view`, or by keeping the `Arc`
on the shell and threading it through.

## Migration strategy (incremental, compile-green at every step)

Do **one** sub-state at a time. Never move two domains in the same commit.

1. **Pick the most isolated domain first: `TrackerState`.** It has the fewest
   cross-cutting interactions (its own tab, its own DB-backed sessions/kv). This
   validates the pattern with low blast radius.
2. Create the sub-struct in a new module (e.g. `native/src/tracker_state.rs` or
   reuse `views/tracker.rs`). Move the fields off `MdEditor` into it; add
   `MdEditor { tracker: TrackerState, .. }`.
3. Update field accesses: `self.tracker_running` → `self.tracker.running`, etc.
   Mechanical; the compiler lists every site.
4. Move the `Tracker*` message arms into `TrackerState::update`. The shell arm
   becomes `Message::Tracker(msg) => self.tracker.update(msg, &self.state)`.
   (Optionally introduce a nested `Message::Tracker(TrackerMsg)` enum to match;
   or keep the flat `Message` and just forward — either works, nest later.)
5. Move the tracker view code to `TrackerState::view`.
6. `cargo build && cargo test`; commit.
7. **Repeat for `PdfPane`** (largest win, do it second so the pattern is proven).
   Then `SearchState`, `EditorPane`, `VaultState`, `UiState`.

### Why not all at once

A big-bang rewrite of an 80-field struct + 2,000-line match is unreviewable and
will sit broken for days. Per-domain moves keep the tree building and tests
passing after each step, so regressions are caught immediately and each commit
is reviewable.

## Risks & watch-points

- **Borrow conflicts:** methods that today read several `self.*` fields across
  domains (e.g. `view` composing editor + pdf + search highlight) will need
  explicit borrows of each sub-state. Prefer passing `&SubState` into helpers
  rather than `&self`.
- **Cross-domain reads:** search highlighting reads the editor buffer; PDF search
  reads pdf state. Keep these as explicit parameters, not hidden `self` access.
- **`buffer_revision` / `doc_match_cache`** (added in the search-cache fix) couple
  editor and search — decide whether the cache lives in `SearchState` (reading a
  revision number from `EditorPane`) or stays on the shell. Recommend: cache in
  `SearchState`, revision owned by `EditorPane`, passed in.
- **Message routing:** introducing nested message enums is optional and can be a
  follow-up; forwarding flat arms first keeps the diff smaller.

## Definition of done

- `MdEditor` holds only sub-states + the shared `Arc<AppState>` + truly global UI
  flags.
- Each sub-state has `update` and `view` (or view helpers) in its own module.
- `update_inner` in the shell is a routing layer, not business logic.
- All existing tests pass; no behavior change intended (pure refactor).
