# UI Duplicate And Structural Analysis

Completed: 2026-06-05

## Layout Stack (Vertical, Top-To-Bottom)

```
┌─────────────────────────────────────────────────────────────┐
│  toolbar (views/toolbar.rs)                        48px     │
│  [sidebar-toggle] [full-path] [• Saved]  [search][cmd]...  │
├────────┬──────────────────────────────────────────┬─────────┤
│sidebar │  MAIN CONTENT                            │right    │
│        │  (pdf or editor or split)                │panels   │
│        │  ┌──────────────────────────────────┐   │(backlinks│
│        │  │ PDF pane (split-left or full)     │   │toc,     │
│        │  │  [pdf search bar if active]       │   │annots,  │
│        │  │  [pdf scrollable content]         │   │tracker) │
│        │  │  [pdf toolbar at BOTTOM]          │   │         │
│        │  │   ☰ [annots] ... [zoom] [page]   │   │         │
│        │  └──────────────────────────────────┘   │         │
│        │  ┌──────────────────────────────────┐   │         │
│        │  │ Editor pane (split-right or full) │   │         │
│        │  │  [editor search bar if active]    │   │         │
│        │  │  [editor scrollable content]      │   │         │
│        │  └──────────────────────────────────┘   │         │
├────────┴──────────────────────────────────────────┴─────────┤
│  status_bar (views/status_bar.rs)                  24px     │
│  [EDITOR/PDF icon] [✓ Saved]  [toast]  [search] [page/zoom] │
└─────────────────────────────────────────────────────────────┘
```

---

## Confirmed Duplicates

### D1: PDF Page Number And Zoom — Three Occurrences

| Location | Code | What it shows |
|----------|------|---------------|
| `pdf_viewer::toolbar` line 471 | `text(format!("{:.0}%", zoom * 100.0))` | zoom percent |
| `pdf_viewer::toolbar` line 499 | `text(page_label)` where `page_label = "{} / {}"` | page N of M |
| `status_bar::view` via `AppShellStatus.pdf_status` | `"{} / {} · {:.0}%"` | page N of M + zoom |

**Result**: When a PDF is open, the user sees page N / M in the PDF toolbar AND
page N / M · zoom% in the status bar simultaneously. Zoom percentage appears in
the PDF toolbar AND in the status bar. Three separate renderings of overlapping
information.

**Fix**: The status bar `pdf_status` field should be removed or only shown when
the PDF toolbar is not visible (e.g. no PDF open). The PDF toolbar owns page
and zoom — the status bar should not duplicate it. Alternatively keep only
the status bar version and remove page/zoom from the pdf toolbar, but that
would break the toolbar's zoom controls context.

**Decision**: The PDF toolbar owns page + zoom controls (interactive). Status
bar should NOT duplicate page/zoom. Remove `pdf_status` from `AppShellStatus`
and from `status_bar::view`. The PDF toolbar already shows this information
contextually.

---

### D2: Save Status — Two Occurrences, One Wrong

| Location | Code | What it shows |
|----------|------|---------------|
| `toolbar::view` line 39 | `text(" • Saved").size(11)` | hardcoded "• Saved" always |
| `status_bar::view` | `SaveStatus::Saved / Unsaved` with proper state | real save state |

**Result**: The top toolbar ALWAYS shows "• Saved" regardless of whether the
document is actually saved or dirty. This is a hardcoded string — it never
changes to show unsaved state. Meanwhile the status bar correctly tracks
`buffer.dirty` via `AppShellStatus`.

The user sees contradictory information: toolbar says "• Saved", status bar
may say "● Unsaved" for the same document.

**Fix**: Remove the hardcoded `" • Saved"` from `toolbar::view`. The status bar
owns save state. The toolbar should show only the filename, not save status.

---

### D3: TOC Toggle — Two Separate Buttons

| Location | Code | What it does |
|----------|------|--------------|
| `toolbar::view` line 66-81 | `button(Icon::ListTree)` → `Message::ToggleTOC` | top toolbar TOC toggle |
| `pdf_viewer::toolbar` line 443 | `button(text("☰"))` → `Message::ToggleTOC` | PDF toolbar TOC toggle |

**Result**: Both the main toolbar (top) and the PDF toolbar (bottom of PDF pane)
have a TOC toggle button that dispatches the same `Message::ToggleTOC`. In
split view, users see two TOC toggle buttons simultaneously. The buttons
look different (icon vs ☰ character) but do the same thing.

**Fix**: Keep TOC toggle only in the main top toolbar. Remove it from the PDF
toolbar. The PDF toolbar's `☰` is confusing ("hamburger menu" affordance) — it
should be an icon consistent with the top toolbar.

---

### D4: Annotations Sidebar Toggle — Position Mismatch

| Location | Code | What it does |
|----------|------|--------------|
| `pdf_viewer::toolbar` line 451 | `Icon::FileText` → `Message::PdfToggleAnnotationsSidebar` | PDF toolbar |

The annotations toggle appears only in the PDF toolbar (bottom of pane), while
all other panel toggles (backlinks, tracker) are only accessible via the main
toolbar or keyboard shortcuts. This inconsistency means annotations are toggled
from a different location than all other workflow panels.

**Fix**: Move the annotations toggle to the main top toolbar alongside the other
panel toggle buttons. Remove it from the PDF toolbar.

---

### D5: PDF Toolbar Position Is Wrong

The PDF toolbar is rendered at the **bottom** of the PDF pane
(`column![left_panel, pdf_toolbar]` at line 4361). Standard UI convention
for document toolbars in research/reading applications is **top** of the
content pane. Bottom placement:

- Conflicts with the status bar immediately below it (two adjacent bars of
  controls at the bottom of the window)
- Is visually disconnected from the content navigation expectation
- Makes page number appear far from where the user is reading

**Fix**: Swap to `column![pdf_toolbar, pdf_search_bar, pdf_scrollable]`.
The toolbar goes on top of the PDF content. The search bar stays directly
below the toolbar (between toolbar and content), same as the editor layout.

---

### D6: `_shell_status` Prefixed With Underscore In View

At `app.rs` line 4170:
```rust
let _shell_status = self.app_shell_status(shell_state);
```

This was computed but the `_` prefix indicates it was not originally integrated.
It IS used (line 4487: `views::status_bar::view(_shell_status)`), so the prefix
is misleading and should be renamed to `shell_status`. Minor code smell but
indicates rushed integration.

**Fix**: Rename `_shell_status` → `shell_status` in `app::view`.

---

### D7: Two Separate Active Pane Concepts In Parallel

The codebase has both `self.active_panel: ActivePanel` (an older enum in
`app.rs`) and `AppShellPane` (the newer shell model). Both track which pane
is focused. They are kept in sync by `set_active_panel` and
`load_shell_persistence` but represent the same thing twice.

The `ActivePanel` enum exists in `app.rs` and is used for legacy branching
logic. `AppShellPane` in `app_shell.rs` is the new model. They should be one.

**Fix**: Audit all uses of `self.active_panel` vs `shell_state.active_pane`.
Where they agree, prefer `shell_state.active_pane`. Where `self.active_panel`
drives behavior, migrate to derive from `shell_state`. This is a larger
refactor — document it as a task rather than doing it in one pass.

---

## Structural Issues

### S1: PDF Toolbar Has No Consistent Visual Owner

The PDF toolbar renders controls that belong to different conceptual groups:
- TOC toggle (should be in main toolbar)
- Annotations toggle (should be in main toolbar)
- Highlight color picker (context-specific — OK here)
- Zoom controls (OK here — interactive)
- Page navigation (OK here — interactive)

This mixes "panel layout controls" (TOC, annotations) with "document view
controls" (zoom, page). Layout controls should be in the main toolbar with
the other panel toggles.

### S2: Search Status In Status Bar Duplicates Global Search Overlay

The `status_bar` shows `search_status` (search running / "Searched N PDFs")
even when the global search overlay is visible and already shows its own
status. The user sees identical status text in two places simultaneously.

**Fix**: Only show `search_status` in the status bar when the global search
overlay is NOT visible.

### S3: Toolbar Shows Full Filesystem Path

`toolbar::view` passes `active_path: Option<&str>` which is the full
vault-relative or filesystem path. Users see long paths like
`research/papers/2024/notes.md` instead of just `notes.md`. This makes the
toolbar feel cramped and the current file hard to identify at a glance.

**Fix**: Show only basename. Show folder context only on hover or in a tooltip.

---

## Priority Order For Fixes

| Priority | Fix | Effort | Impact |
|----------|-----|--------|--------|
| P0 | D2: Remove hardcoded `" • Saved"` from toolbar | Trivial | Fixes wrong info |
| P0 | D1: Remove `pdf_status` from status bar | Small | Removes duplicate |
| P0 | D5: Move PDF toolbar to top of pane | Small | Fixes layout |
| P1 | D3: Remove TOC toggle from PDF toolbar | Trivial | Removes duplicate |
| P1 | D4: Move annotations toggle to main toolbar | Small | Consolidates controls |
| P1 | S3: Show basename in toolbar, not full path | Trivial | Readability |
| P1 | S2: Suppress search status when overlay visible | Small | Reduces noise |
| P2 | D6: Rename `_shell_status` to `shell_status` | Trivial | Code clarity |
| P3 | D7: Unify `ActivePanel` and `AppShellPane` | Large | Architectural |

---

## What A Cleaned-Up Toolbar Layout Looks Like

### Main Top Toolbar (after fixes)
```
[☰ sidebar] [basename.md]  ─────  [🔍] [⌘] [TOC] [⊞ split] [📎 annots] [⏱ tracker]
```

### PDF Toolbar (after fixes, now at TOP of PDF pane)
```
[─ zoom] [100%] [+ zoom] [Fit W] [Fit P] [⟳]  ─────  [highlight controls]  ─────  [12 / 48]
```

### Status Bar (after fixes, 28px)
```
[EDITOR ●]  [● Unsaved]  ─────  [search running text]  ─────  [empty right]
```

When PDF only, status bar right side is empty (page/zoom is in PDF toolbar).
When search overlay hidden, search status still shown if actively searching.
