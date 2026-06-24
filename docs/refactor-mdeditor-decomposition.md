# Plan: Decompose the `MdEditor` god-struct

> Status: **field grouping complete; pure-state methods moved.** All six
> sub-states have been extracted; `MdEditor` went from ~80 fields to 12. Each
> step was its own commit, build clean with all tests passing, no behavior
> change.
>
> Extraction order as executed: TrackerState → SearchState → UiState →
> PdfPane → VaultState → EditorPane.
>
> **Method moves (follow-up, done where clean):** the *logic that operates on a
> single pane's own state* now lives on that pane:
> - **PdfPane** owns the page-geometry cluster (display sizes, page
>   height/offset, total height, page-at-scroll, search-match scroll target,
>   link/annotation hit-testing) and the two `*_from` helpers.
> - **EditorPane** owns the highlight pipeline (`refresh_highlighting`,
>   `highlight_task`, `load_images`, `load_math`, `render_latex_task`, the
>   `plain_highlight_placeholders` helper, and the doc-size thresholds).
>   `refresh_highlighting` returns `(Task, load_resources)` so the only
>   shell-side glue is a thin `load_editor_resources()` that supplies the vault
>   root + active path (needed to resolve relative image paths).
>
> **Message routing (final pass, done):** every sub-state now has an
> `update(&mut self, msg) -> Task<Message>` (mirroring the original
> `TrackerState` pattern), and the shell forwards the message arms that mutate
> *only that pane's own state* via grouped `m @ (..) => self.<pane>.update(m)`
> arms. 24 arms moved: ui 14 (modals, link-note picker, command palette, toast,
> split-drag start), pdf 5 (page-size load, render-skip, page-text cache,
> link-preview close, selection clear), editor 2 (math cache, debounced
> highlight — `HIGHLIGHT_DEBOUNCE` moved to `editor_state` too), vault 2
> (sidebar toggle, folder expand), search 1 (replace-field text).
>
> **Intentionally staying on the shell (coordinator role):** orchestration that
> needs the shared `Arc<AppState>`, resolves vault paths, or coordinates
> multiple panes — PDF open/render/page-text/navigation, the layout-dependent
> `pdf_available_width` (sidebar/TOC/split/window state), and search navigation
> (moves the editor cursor / scrolls). Threading `&AppState` through these to
> force them onto a pane would fight the design, not improve it. This is the
> **terminal state**, not a gap: the bulk of `update_inner`'s remaining lines
> are inherently cross-pane dispatchers (`KeyboardShortcut`, `SidebarFileClicked`,
> `ToggleTOC`, the PDF-annotation cluster that persists via `AppState` and spawns
> vault notes), so `update_inner` stays a coordinator, not a thin forwarder. A
> handful of arms that *look* single-pane (`PdfLinkPreviewResult`,
> `PdfSearchResult`) keep both their `Ok` and `Err` halves on the shell because
> the two halves target different panes and can't route as one variant.

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

- ✅ `MdEditor` holds only sub-states + the shared `Arc<AppState>` + truly global
  UI flags (12 fields, down from ~80).
- ✅ Each sub-state has `update` and `view` (or view helpers) in its own module.
- ✅ `update_inner` routes every *single-pane* message arm to the owning
  sub-state's `update`. The remaining inline arms are genuinely cross-cutting
  coordination (need `Arc<AppState>`, resolve vault paths, or touch multiple
  panes) — keeping them on the shell is the intended coordinator design, not
  leftover business logic. See the top-of-file note for the boundary.
- ✅ All existing tests pass; no behavior change intended (pure refactor).

**Status: complete.** The god-struct is decomposed into six cohesive
sub-states, pure-state methods live on their pane, and message routing forwards
all cleanly-single-pane arms. The shell remains a deliberate coordinator for the
cross-pane orchestration that defines this app's interactions.
