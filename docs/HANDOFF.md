# Live Handoff

Last updated: 2026-06-05

## Purpose

This document is the live pickup point for humans and agents working on the
editor and PDF synergy roadmap. Update it at the end of every meaningful work
session, especially when a task is incomplete, blocked, or changes the expected
next step.

## Current Objective

Continue with **Milestone 5** (PDF Annotation UX) from
`docs/UI_UX_IMPROVEMENT_ROADMAP.md`. Phase B, Phase A, and Milestone 4 have
been successfully completed.

## Why Phase B And Phase A Exist

A full code audit on 2026-06-05 (`docs/UI_UX_DUPLICATE_ANALYSIS.md`) found:

1. **Hardcoded "• Saved" bug** — `toolbar.rs` always shows "• Saved" regardless
   of `buffer.dirty`. The `_sync_status` param is wired to `None` at the call
   site and was never connected. The status bar correctly shows "● Unsaved" but
   the toolbar contradicts it with stale text.

2. **PDF page + zoom shown twice** — `pdf_viewer::toolbar` shows `"3 / 47"` and
   `"150%"`. `status_bar` independently derives and shows `"3 / 47 · 150%"`.
   Three total occurrences of overlapping data visible simultaneously.

3. **TOC toggle in two toolbars** — main top toolbar has `Icon::ListTree` →
   `ToggleTOC`. PDF toolbar has `☰` text button → `ToggleTOC`. Same action,
   two places, different visual appearance.

4. **Annotations toggle in wrong toolbar** — all panel toggles (backlinks,
   tracker, TOC) are in the main toolbar; annotations toggle is only in the
   PDF toolbar. Inconsistent ownership.

5. **PDF toolbar at bottom of pane** — standard convention is top. Bottom
   placement causes page number to visually stack directly above the status
   bar, doubling density at the bottom of the window.

6. **Design tokens exist but views don't use them** — `button::text` used
   everywhere with no hover states, no tooltips. This is Phase A's scope.

The prior HANDOFF and milestones marked "complete" states that don't match the
actual app behavior. This handoff corrects the record.

## Milestone Completion Status (Corrected)

| Milestone | Status | Notes |
|-----------|--------|-------|
| 0 Baseline | DONE | Audit, fixtures, release checklist |
| 1 App Shell | DONE | Shell model, persistence, status bar surface |
| 2 Visual Design System | DONE | Phase A applied hover/header/container conventions across main panels |
| 3 Keyboard / Command | DONE | Registry, palette, shortcuts, conflict detection |
| 4 Editor UX Polish | DONE | Renderer colors done; active line alpha 0.06; selection/search/link hover added; internal link resolution fixed |
| 5 PDF Annotation UX | IN PROGRESS | PDF toolbar reading-state groups, selection Cite, and core annotation tests done |
| 6 Split Research UX | NOT STARTED | |
| 7 Search / Outline | PARTIAL | Backend done; result row styling not done |
| 8 Onboarding / Recovery | PARTIAL | Citation palette done; welcome/recovery not done |
| 9 Accessibility | NOT STARTED | Tooltips required by Phase A A1 |
| 10 Performance | PARTIAL | Empty states done; progress/spinner not done |
| 11 Documentation | NOT STARTED | |
| 12 Release Hardening | NOT STARTED | |

---

## Phase B Task List — Do These First

Each task is independent but do B2 after B1 (B1 removes save text from toolbar,
B2 removes page/zoom from status bar — both clean up `AppShellStatus` and its
callers).

- [x] **B1** `toolbar.rs` + `app.rs` — Removed hardcoded `" • Saved"` and dead `_sync_status`/`_backlinks_visible` params. Now shows only basename.
- [x] **B2** `app_shell.rs` + `status_bar.rs` + `app.rs` — Removed `pdf_status` from `AppShellStatus`. Renamed `pdf_text_status`→`global_search_status`. Status bar no longer shows page/zoom.
- [x] **B3** `pdf_viewer.rs` — Removed TOC toggle `☰` and `toc_visible` param from PDF toolbar.
- [x] **B4** `pdf_viewer.rs` + `toolbar.rs` + `app.rs` — Moved annotations toggle to main toolbar (pdf_open-gated). Removed from PDF toolbar.
- [x] **B5** `app.rs` — PDF toolbar moved to TOP of PDF pane. Column order: `[pdf_toolbar, pdf_search_bar, scrollable]`.
- [x] **B6** — Decision documented: window title `●` is deliberate; toolbar `• Saved` was the bug (now fixed).
- [x] **B7** `app_shell.rs` + `app.rs` — Added `global_search_visible` field. Search status suppressed in status bar when overlay is open.
- [x] **B8** `app.rs` + `toolbar.rs` — Renamed `_shell_status` → `shell_status`. Removed `_backlinks_visible` and `_sync_status`.

**After each task**: run `cargo fmt --all -- --check &&
cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

**Phase B acceptance gate**: PDF open → page visible once (top of PDF pane);
dirty document → "● Unsaved" in status bar and NOTHING about save state in
toolbar; TOC toggle in exactly one place.

---

## Phase A Task List — After Phase B

Each task targets one view file. Assign independently, merge independently.

- [x] **A1** `toolbar.rs` — Tooltips on icon buttons, hover states, basename path,
      divider between path and actions, annotations toggle (from B4).
- [x] **A2** `sidebar.rs` — Empty vault state, header border, active bar height
      fix, hover-only delete button opacity.
- [x] **A3** `welcome.rs` — Accent-styled button, secondary action, keyboard
      badge for Ctrl+O, version text.
- [x] **A4** `status_bar.rs` — 28px height, accent color for saved (not success),
      danger icon for unsaved, `Space::new()` not `text("")` placeholder.
- [x] **A5** `toc.rs` — 11px muted header, entry hover bg, level color hierarchy,
      active-line highlight param.
- [x] **A6** `backlinks.rs` — 11px header, hover bg on rows, annotation backlink
      left accent border.
- [x] **A7** `command_palette.rs` — Group separators, icon mapping, hover states,
      window-width clamping.
- [x] **A8** Consistency pass — all panels match header/row/container conventions.
      Global search rows now use matching hover styling and muted section headers;
      annotation panel now renders an empty state.

**Phase A acceptance gate**: running the app shows hover backgrounds on all
panel rows, toolbar tooltips, 28px status bar, sidebar empty-vault state.

### Latest Session Notes — 2026-06-05

- `native/src/views/command_palette.rs`: completed A7 with shell-ordered group
  headers/dividers, icon widgets, hover styling, and narrow-window clamp helper.
- `native/src/views/icons.rs`: added `Icon::ChevronLeft` for back navigation.
- `native/src/views/search.rs`: global search result rows now use panel hover
  styling; group headers use muted 11px panel hierarchy.
- `native/src/views/pdf_annotations.rs`: empty annotation/filter result state
  now renders `"No annotations found"`.
- `native/src/views/pdf_viewer.rs`: started Milestone 5 by redesigning the PDF
  toolbar into stable `PAGE` and `ZOOM` reading-state groups with hover styling.
- `native/src/views/pdf_viewer.rs` + `native/src/app.rs`: selection toolbar now
  exposes `Cite`, `Ctrl+H` missing-selection path shows a toast, and tests cover
  selection citation plus annotation creation from selected text.

## Milestone 5 Next Steps

- Add keyboard-first annotation variants beyond default yellow highlight.
- Strengthen visual selection feedback in the PDF page overlay.

---

## Milestone 4 Remaining Work (DONE)

All items from Milestone 4 have been successfully completed:
1. Active line alpha: increased to 0.06 in `theme.rs`.
2. Selection background: verified `accent_dim()` at 0.25 opacity.
3. Search match highlight: `warning()` alpha 0.30 non-active, `accent()` 0.50 active.
4. Link hover: `accent()` underline when cursor on link span implemented.
5. Internal link resolution: fixed logic so hash fragments (`[[#equation-8]]`) are not incorrectly marked broken and do not render with strikethrough.
6. Find bar: match count connected to editor state and displayed correctly in `search::file_bar`.
7. `iced_test` coverage: test `file_bar_renders_match_count` added, rendering coverage verified.

---

## Key Files And Their Responsibilities (After Phase B)

| File | Owns |
|------|------|
| `toolbar.rs` | Sidebar toggle, filename (basename only), global search, command palette, TOC toggle, split toggle, annotations toggle (PDF-only), tracker toggle |
| `pdf_viewer::toolbar` | Zoom controls, page navigation, fit-to-width/page, rotate, highlight/annotation context actions |
| `status_bar.rs` | Save state, active pane indicator, transient toasts, search status (when overlay hidden) |
| `sidebar.rs` | File tree, empty vault state |
| `toc.rs` | Outline + TOC navigator |
| `backlinks.rs` | Backlink list |
| `pdf_annotations.rs` | Annotation list with filters |

---

## Previously Completed Work (Reference)

### Editor / PDF Synergy Milestones (Complete)
- PDF quote/annotation insertion via `EditorCommand`.
- Linked notes via `build_linked_pdf_note_content`.
- PDF stuck-in-loading bug fixed.

### Parser And Backlink (Complete)
- Reference link parsing, inline links, emphasis, footnotes, table of contents.
- `extract_document_metadata`, `extract_frontmatter_metadata`.
- Parser-derived backlinks replacing regex.
- Regression tests for debounce and stale highlights.

### Core Search (Complete)
- `UnifiedSearchQuery`, source filters, streaming, stale suppression.
- Global PDF search across vault with bounded cap.
- Durable `pdf_text_search` FTS cache, background PDF indexing.

### Annotation And Research (Complete)
- Annotation types: highlight, underline, strikeout, notes, tags, status.
- Multi-layer filters, batch export, reference repair engine.
- Citation palette, excerpt queue, templates, reading session notes.

### Navigation And Integrity (Complete)
- Unified cross-pane back/forward history in `pdf_navigation.rs`.
- Follow Citation, Show Usages, combined Outline + TOC sidebar.
- Vault integrity checks in `integrity.rs`.
- Linked-note syncing, PDFium mutex wrapper.

### App Shell Infrastructure (Complete — Data Layer)
- `app_shell.rs` with layout-mode derivation, persistence, command groups.
- `AppShellStatus` model.
- Status bar surface rendered at bottom.
- Theme switching with persistence.

## Completed Files

- `docs/EDITOR_PDF_SYNERGY_ROADMAP.md`
- `docs/UI_UX_IMPROVEMENT_ROADMAP.md`
- `docs/UI_UX_DUPLICATE_ANALYSIS.md` (new, 2026-06-05)
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
- `native/src/views/status_bar.rs`
- `docs/DESIGN_TOKENS.md`
- `native/src/command_registry.rs`

## Tests And Checks

Last full verification:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Full list of focused tests from prior sessions: see prior HANDOFF version or
grep `#[test]` in `native/src/app.rs`, `native/src/app_shell.rs`,
and `native/src/views/*.rs`.

## Known Worktree State

- `core/pdfium/libpdfium.so` was already modified before this work. Do not
  revert it unless user explicitly asks.
- UI/UX roadmap, handoff, and duplicate analysis docs updated 2026-06-05.

## Standards Reminder

- Markdown mutations go through `EditorCommand`.
- Parsing stays in `native/src/editor/highlight.rs`.
- Renderer stays viewport-bounded.
- `page_index` = internal 0-based; `page_number` = user-visible 1-based.
- `pdf://` links through `native/src/pdf_links.rs`.
- Avoid `unwrap`/`expect` outside tests unless invariant documented nearby.
