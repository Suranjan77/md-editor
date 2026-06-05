# UI And UX Improvement Roadmap

This roadmap turns the completed research workspace into a polished,
keyboard-friendly, visually coherent application.

## Operating Rules

- Build UI work with test coverage first where practical: failing `iced_test`
  flow, implementation, regression coverage, then cleanup.
- Preserve existing architecture boundaries: `native/views` composes UI and
  wires messages, `native/app.rs` coordinates state, renderer code stays
  viewport-bounded.
- Keep markdown document mutations behind `EditorCommand`; UI workflows must
  dispatch commands instead of editing buffers directly.
- Keep markdown parsing in `native/src/editor/highlight.rs`; visual polish must
  not add parser rules to renderers.
- Keep all major interactions keyboard-accessible before declaring UX complete.
- Add loading, empty, disabled, conflict, and error states for every user-facing
  workflow changed by a milestone.
- Add accessibility acceptance checks for focus order, keyboard traversal,
  visible labels or tooltips, contrast, scalable text, and reduced-motion
  behavior. Do not claim screen-reader support beyond what current Iced
  accessibility APIs can actually expose.
- Keep performance budgets explicit: no full-document hot-path scans, no
  widget-per-PDF-rect overlays, no unbounded search/status updates.
- Prefer stable, dense research-tool UI over marketing-style surfaces:
  no decorative hero layouts, no nested cards, no one-hue palette, no oversized
  type inside tool panels.
- Avoid generic AI-generated visual patterns: no glassmorphism, frosted panels,
  blur-heavy overlays, gradient blobs, floating orb backgrounds, excessive glow,
  random pastel gradients, oversized rounded cards, decorative mockup frames,
  fake 3D depth, or stock SaaS dashboard composition. Visual decisions must
  come from the app's research/editor workflow, not trend defaults.
- Record design decisions in this roadmap or related docs when they create
  reusable UI conventions.
- Update `docs/HANDOFF.md` after meaningful implementation work.
- Run `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` before handoff for code changes.

## Non-Goals

- Do not change markdown syntax, parser semantics, or PDF file storage as part
  of visual polish.
- Do not replace Iced, PDFium, SQLite, or the editor renderer unless a separate
  architecture roadmap approves it.
- Do not add cloud sync, collaboration, AI features, or account systems.
- Do not make a marketing landing screen; first screen stays a usable vault or
  vault-opening workflow.

---

## Current Layout Stack

```
┌──────────────────────────────────────────────────────────────┐
│  toolbar.rs (48px top bar, always visible)                   │
│  [☰ sidebar] [full-path • Saved] [🔍][⌘][TOC][⊞][⏱]        │
├────────┬─────────────────────────────────────────┬───────────┤
│sidebar │  MAIN CONTENT                           │ right     │
│(file   │  (pdf_view | editor_view | split)       │ panels    │
│ tree)  │                                         │ (backlinks│
│        │  pdf_view column:                       │  toc      │
│        │   [pdf search bar]                      │  annots   │
│        │   [pdf scrollable content]              │  tracker) │
│        │   [pdf toolbar at BOTTOM ← WRONG]       │           │
│        │    ☰ [annots] [zoom] [page]             │           │
│        │                                         │           │
│        │  editor_view column:                    │           │
│        │   [editor search bar]                   │           │
│        │   [editor scrollable]                   │           │
├────────┴─────────────────────────────────────────┴───────────┤
│  status_bar.rs (24px bottom bar, always visible)             │
│  [EDITOR •] [✓ Saved] ── [toast] ── [search] [page/zoom]    │
└──────────────────────────────────────────────────────────────┘
```

---

## PHASE B: Eliminate Duplicates And Fix Wrong State (IMMEDIATE — Do First)

> Gap identified 2026-06-05 via full code audit of `app.rs::view`,
> `pdf_viewer.rs::toolbar`, `toolbar.rs`, `status_bar.rs`, and `app_shell.rs`.
> See `docs/UI_UX_DUPLICATE_ANALYSIS.md` for full evidence.

Phase B must complete before Phase A because some Phase A work (status bar
height, save status styling) is wasted effort if the wrong data is still shown.

### B1: Fix Hardcoded "• Saved" In Toolbar — CRITICAL BUG

**Files**: `native/src/views/toolbar.rs`, `native/src/app.rs`

**Problem**: `toolbar.rs` line 39 shows `text(" • Saved")` hardcoded,
regardless of whether the document is dirty. The `_sync_status: Option<&str>`
parameter exists but is unused (leading `_`) and is always passed as `None`
from `app.rs:4181`. This means the toolbar shows "• Saved" even when the buffer
is dirty and the status bar correctly shows "● Unsaved". The user sees
contradictory save state information.

**Fix**:
1. Remove the `" • Saved"` text from `toolbar::view` entirely. The toolbar
   should show only the file basename (see B5). Save state belongs in the
   status bar.
2. Remove the `_sync_status` parameter from `toolbar::view` since it was never
   connected and is now unused.
3. Update the call site in `app.rs` to remove the `None` argument.
4. Add `iced_test` coverage: render toolbar with a path, assert "Saved" text
   is NOT present in the toolbar widget tree.

### B2: Remove PDF Page Number And Zoom From Status Bar

**Files**: `native/src/app_shell.rs`, `native/src/views/status_bar.rs`,
`native/src/app.rs`

**Problem**: When a PDF is open, page number and zoom appear in BOTH the PDF
toolbar (`"3 / 47"` and `"150%"` at `pdf_viewer.rs:471,499`) AND the status bar
(`"3 / 47 · 150%"` derived in `AppShellStatus::derive`). The PDF toolbar owns
these controls (zoom in/out buttons, page jump) so it is the correct owner of
this display. The status bar repeats it redundantly.

**Fix**:
1. In `app_shell.rs::AppShellStatus::derive`, remove the `pdf_status` field
   computation entirely (lines 255-264).
2. Remove `pdf_status: Option<String>` from the `AppShellStatus` struct.
3. Remove `pdf_text_status: Option<String>` from `AppShellStatusInputs` — this
   was a PDF search status, not a page number. Separate into distinct fields:
   - `global_search_status: Option<String>` for search-running text only.
4. In `status_bar::view`, remove the `pdf_status` display block (lines 114-124).
5. In `app.rs::app_shell_status`, remove the `pdf_text_status` field assignment
   and replace with `global_search_status` if needed.
6. Update all tests that assert `pdf_status` content.
7. Add `iced_test` coverage: with PDF open, assert status bar does NOT contain
   page number text.

### B3: Remove TOC Toggle From PDF Toolbar

**Files**: `native/src/views/pdf_viewer.rs`

**Problem**: The PDF toolbar has a `☰` button at line 443 that dispatches
`Message::ToggleTOC`, the same message as the ListTree icon in the main
toolbar. In any mode where both toolbars are visible, there are two TOC toggle
buttons. They look different (☰ text vs ListTree icon) but do the same thing.

**Fix**:
1. Remove the `toc_visible: bool` parameter from `pdf_viewer::toolbar`.
2. Remove the `button(text("☰"))` TOC toggle button from the PDF toolbar
   widget tree.
3. Update the call site in `app.rs` to remove the `toc_visible` argument.
4. Update any tests that reference the PDF toolbar's TOC button.

### B4: Move Annotations Toggle From PDF Toolbar To Main Toolbar

**Files**: `native/src/views/pdf_viewer.rs`, `native/src/views/toolbar.rs`,
`native/src/app.rs`

**Problem**: The PDF toolbar has an annotations toggle (lines 451-462) while
all other panel toggles (backlinks, tracker, TOC) live in the main top toolbar.
This inconsistency means the user must look in different places to manage
different panels. The PDF toolbar already has enough controls (zoom, page, mode).

**Fix**:
1. Remove `annotations_sidebar_visible: bool` from `pdf_viewer::toolbar` params.
2. Remove the `Icon::FileText` annotations toggle button from the PDF toolbar.
3. Add an annotations toggle button to `toolbar::view`:
   - Add `annotations_visible: bool` and `pdf_open: bool` parameters.
   - Render `button(icons::view(Icon::FileText, ..., 18.0))` only when
     `pdf_open` is true (annotations only apply to PDFs).
   - Dispatch `Message::PdfToggleAnnotationsSidebar`.
4. Update call sites and add `iced_test` coverage.

### B5: Move PDF Toolbar To Top Of PDF Pane

**Files**: `native/src/app.rs`

**Problem**: `app.rs:4361` renders `column![left_panel, pdf_toolbar]` which
places the PDF toolbar at the BOTTOM of the PDF pane. This means:
- Page number "3 / 47" appears at the bottom of the PDF pane AND in the status
  bar directly below (after B2 is done, status bar removes its page number, but
  the layout is still wrong).
- Zoom controls are below the content rather than above it.
- When reading PDFs, users must move their eyes to the very bottom to access
  navigation controls.

Standard convention: document toolbars sit above document content.

**Fix**:
1. In `app.rs` PDF view construction, change:
   `column![left_panel, pdf_toolbar]` → `column![pdf_toolbar, pdf_search_bar_or_space, pdf_content]`
   where `left_panel` currently bundles search bar and scrollable content.
2. Restructure the pdf_view to:
   ```
   column![
     pdf_toolbar,
     if pdf_search_active { search_bar } else { zero-height space },
     scrollable(view_continuous(...)),
   ]
   ```
3. Verify that the existing tests for PDF toolbar rendering still pass.
4. Add `iced_test` smoke: with PDF open, toolbar elements render before
   scroll content in the widget tree.

### B6: Fix Window Title Dirty Indicator Duplication

**Files**: `native/src/app.rs` (title method)

**Problem**: The window title shows `"● "` prefix when `buffer.dirty`. With
the status bar also showing `"● Unsaved"`, save state is shown in two places.

**Decision**: Keep the window title indicator (it is conventional and OS
window switchers use it). Remove from the top toolbar (already done in B1).
The status bar is the primary in-app save indicator.

**Fix**: No code change needed once B1 is complete. Just document this
deliberate decision here — window title `●` is intentional; toolbar `• Saved`
is the bug.

### B7: Suppress Search Status In Status Bar When Global Search Overlay Is Visible

**Files**: `native/src/app_shell.rs`, `native/src/app.rs`

**Problem**: `AppShellStatusInputs` always fills `search_status` regardless
of whether the global search overlay is open. When the overlay is visible, the
status bar behind the overlay shows the same "Searching..." or "Searched N PDFs"
text that the overlay already shows in its own status area.

**Fix**:
1. Add `global_search_visible: bool` to `AppShellStatusInputs`.
2. In `AppShellStatus::derive`, set `search_status = None` when
   `global_search_visible` is true.
3. Pass `self.search_visible` for this new field in `app_shell_status`.
4. Update relevant tests.

### B8: Rename `_shell_status` And Clean Up Dead Code

**Files**: `native/src/app.rs`

**Problem**: Line 4170 declares `let _shell_status = ...`. The `_` prefix
convention means "intentionally unused" in Rust. The variable IS used at 4487,
making the prefix wrong and misleading. Additionally `_sync_status` (removed
in B1) and `_backlinks_visible` in `toolbar.rs` should be reviewed.

**Fix**:
1. Rename `_shell_status` → `shell_status` at line 4170 and update 4487.
2. After B1 removes `_sync_status`, remove `_backlinks_visible: bool` from
   `toolbar::view` signature if it is also unused (check `toolbar.rs:13`).
3. Run clippy to catch any remaining `_`-prefixed used variables.

### B Phase Acceptance Gate

After B1-B8:
- [ ] PDF open: page number visible exactly once (in PDF toolbar, at top).
- [ ] PDF open: zoom visible exactly once (in PDF toolbar).
- [ ] Dirty document: "● Unsaved" visible in status bar; toolbar does NOT
      say "• Saved"; window title shows "● " prefix.
- [ ] Clean document: "✓ Saved" visible in status bar; toolbar shows only
      filename baseline; window title has no prefix.
- [ ] TOC toggle: exactly one button visible (main toolbar only).
- [ ] Annotations toggle: exactly one button visible (main toolbar, PDF-only).
- [ ] Global search open: no duplicate search status in status bar behind overlay.

---

## PHASE A: Immediate Visual Polish (After Phase B)

> Gap identified 2026-06-05: Design tokens exist in `theme.rs` / `app_shell.rs`
> but view files still render bare content using raw `button::text` with no
> hover states, no tooltips, and no visual hierarchy.

### A1: Toolbar — Visible Labels And Hover States

**File**: `native/src/views/toolbar.rs`

**Current problems**:
- Icon-only buttons with no tooltip, aria-label, or visible text.
- `button::text` used everywhere: no hover background, no pressed state.
- Full filesystem path shown instead of basename.
- No visual separator between path area and action buttons.

**Required changes**:
1. Add `tooltip` wrapper to every icon-only button: "Toggle Sidebar",
   "Global Search (Ctrl+F)", "Command Palette (Ctrl+P)", "Outline / TOC",
   "Split View", "Annotations" (new, from B4), "Study Tracker".
   Use `iced::widget::tooltip::Position::Bottom`.
2. Replace `button::text` with a local style closure:
   - Normal: transparent.
   - Hovered: `theme::bg_tertiary()`, `RADIUS_REGULAR`.
   - Pressed: `theme::bg_surface()`.
   - Active: always show `theme::accent_dim()` background.
3. Show only file basename (`std::path::Path::file_name`), not full path.
4. Add 1px vertical `border_subtle()` divider between path and action buttons.
5. `iced_test` coverage: tooltip present, active split button has accent bg.

### A2: Sidebar — Empty State, Row Hover, Section Header

**File**: `native/src/views/sidebar.rs`

**Current problems**:
- Blank panel when `entries.is_empty()`.
- Active indicator bar hardcoded `Height::Fixed(22.0)`.
- Delete button always visible (noisy).
- No divider between header and tree content.

**Required changes**:
1. Empty state: centered column with `Icon::FolderOpen` 32px muted, "No files
   yet" 13px muted, "Open Vault" button with `theme::accent()` style.
2. 1px bottom border on header via `border_subtle()`.
3. Active bar height: `Length::Fill` inside the row.
4. Delete button: pass `hovered_path: Option<&str>` from app state. Show delete
   icon at full `text_muted()` opacity when row path matches; otherwise
   `Color { a: 0.0, ..text_muted() }`.
5. `iced_test`: empty entries renders "No files yet".

### A3: Welcome Screen — Styled Buttons And Version Badge

**File**: `native/src/views/welcome.rs`

**Current problems**:
- `button::primary` uses Iced palette primary, not `theme::accent()`.
- No secondary action ("Create New Vault").
- Shortcut hint is plain text, not keyboard-key-like.
- No version information.

**Required changes**:
1. Local button style: `theme::accent()` bg, `theme::bg_primary()` text,
   `RADIUS_REGULAR`. Hovered: `theme::accent_secondary()`.
2. Add "Create New Vault" (`Message::CreateFolderDialog`) using `button::text`
   with `text_secondary()`.
3. Wrap "Ctrl+O" in keyboard badge: `bg_tertiary()` bg, `border()` border,
   `RADIUS_SMALL`, 11px monospace.
4. Version text: `env!("CARGO_PKG_VERSION")`, 10px, `text_muted()`.
5. `iced_test`: "Open Existing Vault" and "Md-editor" visible.

### A4: Status Bar — Contrast Fix And Readable Height

**File**: `native/src/views/status_bar.rs`

**Current problems** (some pre-conditions removed by Phase B):
- 24px height is cramped.
- `theme::success()` (#d9f2d2 on dark) fails WCAG contrast on `bg_secondary`.
- `●` character for unsaved instead of icon.
- Empty `text("")` center placeholder causes layout jitter.

**Required changes**:
1. Height `Length::Fixed(28.0)`, padding `[4, 14]`.
2. Saved checkmark: use `theme::accent()` not `theme::success()`.
3. Unsaved icon: `icons::view(Icon::Circle, theme::danger(), 8.0)`.
4. Unsaved state: 2px `theme::danger()` left accent bar.
5. Center placeholder: `Space::new()` not `text("")`.
6. `iced_test`: saved → accent icon; unsaved → danger icon.

### A5: TOC Panel — Visual Hierarchy And Active Position

**File**: `native/src/views/toc.rs`

**Current problems**:
- 16px bold title is too large for a side panel.
- Section labels use `text_primary()` — no hierarchy.
- 2px padding on entries — too small.
- No active-position indicator.

**Required changes**:
1. Title: `text("OUTLINE & TOC").size(11).color(text_muted())`. 1px bottom border.
2. Section labels: `text_muted()`, 11px, not bold.
3. Entry padding `[4, 8]`. Hover bg `bg_tertiary()` at `RADIUS_SMALL`.
4. Level color: H1 `text_secondary()`, H2+ `text_muted()`.
5. Add `active_line: Option<usize>` param. Highlight ancestor heading with
   `accent_dim()` bg + `accent()` text.
6. `iced_test`: active-line param renders accent color on matching entry.

### A6: Backlinks Panel — Match Sidebar Style

**File**: `native/src/views/backlinks.rs`

**Current problems**:
- "BACKLINKS" header at 10px instead of 11px.
- Rows use `button::text` with no hover.
- No visual distinction between file and annotation backlinks.

**Required changes**:
1. Header: 11px, `text_muted()`. 1px bottom `border_subtle()`.
2. Row hover: `bg_tertiary()` at `RADIUS_SMALL`.
3. Annotation backlink: 2px `accent_secondary()` left border.
4. `iced_test`: annotation backlink renders distinct left border.

### A7: Command Palette — Group Separators And Hover States

**File**: `native/src/views/command_palette.rs`

**Status**: Done 2026-06-05.

**Current problems**:
- No group separator labels between groups.
- Icon column uses emoji/text chars, not `Icon` values.
- No hover background on entries.
- Width `Length::Fixed(520.0)` can overflow narrow windows.

**Required changes**:
1. Group separator rows between consecutive different-group commands:
   `text(group_name).size(10).color(text_muted())`, non-clickable.
2. `shortcut_to_icon(Shortcut) -> Icon` mapping. Replace text-char containers
   with `icons::view(icon, color, 14.0)`. Fall back to `Icon::Command`.
3. Hover style: `bg_tertiary()` at `RADIUS_SMALL`.
4. Accept `window_width: f32`. Clamp to `f32::min(520.0, window_width - 40.0)`.
5. `iced_test`: group separator text renders between entries.

### A8: Consistency Pass

**Status**: Done 2026-06-05.

After A1-A7 merged:
- All panel headers: 11px uppercase `text_muted()`, 1px bottom `border_subtle()`,
  `[12, 14]` padding.
- All list rows: hover → `bg_tertiary()` at `RADIUS_SMALL`.
- All panel containers: `bg_secondary()` bg, 1px `border()` on relevant edge.
- Smoke test asserting expected label text across toolbar, sidebar, status bar,
  TOC.

Completion notes:
- Command palette uses group headers, divider rows, icon widgets, hover styling,
  shell group ordering, and narrow-window width clamp.
- Global search rows use matching hover styling and muted section headers.
- Annotation panel renders a matching empty state when filters return no rows.

---

## Milestone 0: UX Baseline And Test Harness (Complete)

- Audit in `docs/UI_UX_AUDIT.md`, `iced_test` helpers, layout smoke tests,
  accessibility checklist, `docs/UI_UX_RELEASE_CHECKLIST.md`. DONE

## Milestone 1: Information Architecture And App Shell (Complete)

- `docs/APP_SHELL_SPEC.md`, `native/src/app_shell.rs`, panel persistence,
  shell status model, rendered status bar surface. DONE

## Milestone 2: Visual Design System (Tokens Done — Views Incomplete)

Tokens in `theme.rs` are complete. Theme switching works. Views do not apply
hover/pressed/focus state styles. Phase A completes this milestone.

Completed: tokens, resolvers, three themes, persistence, hover states in views, tooltips, and component-state matrix applied.
Milestone 2 = done (Phase A passes except Command Palette).

## Milestone 3: Keyboard And Command Model (Complete)

Command registry, palette integration, shortcuts, conflict detection. DONE

## Milestone 4: Editor UX Polish (Complete)

Completed:
1. Theme color resolvers in renderer.
2. Active line alpha: increased 0.03 → 0.06.
3. Selection background: `accent_dim()` at 0.25.
4. Search match highlight: `warning()` alpha 0.30 non-active, `accent()` 0.50 active.
5. Link hover: `accent()` underline on cursor-over-link.
6. Internal link resolution logic fixed (hash fragments no longer marked broken).
7. Find bar: match count connected to editor state and displayed correctly.
8. `iced_test` coverage added.

## Milestone 5: PDF Reading And Annotation UX

(After Phase B restructures PDF toolbar ownership)
- PDF toolbar redesign: stable command groups, visible reading state.
- Keyboard-first PDF annotation.
- Text selection feedback and quote insertion affordance.
- Tests for page nav, zoom, selection-to-citation, annotation creation.
- Note: Phase B (B3-B5) resolves the structural issues before milestone 5 work.

Progress 2026-06-05:
- PDF toolbar now has stable page and zoom command groups with visible `PAGE`
  and `ZOOM` labels, bounded button hover states, and left-aligned reading
  state.
- Focused annotation and selection controls remain context-sensitive in the PDF
  toolbar.
- Added `iced_test` smoke coverage for toolbar reading state labels.
- Selected PDF text now exposes a toolbar `Cite` action that dispatches quote
  insertion, with `Ctrl+H` shown beside highlight swatches.
- `Ctrl+H` without an active PDF selection now shows a toast instead of doing
  nothing.
- Added regression coverage for selection citation, missing-selection keyboard
  feedback, and annotation creation from selected PDF text.

Remaining:
- Keyboard-first annotation flow beyond default yellow highlight.
- Stronger text selection feedback in the page overlay.

## Milestone 6: Split Research Workflow UX

- Deliberate split mode: active pane indicator, follow-citation, return.
- Synced research mode, companion-note surfacing.
- Drag/resizer polish, narrow-window fallback.
- Cross-pane toasts.

## Milestone 7: Search, Outline, Navigation UX (Backend Complete)

Backend: unified search, streaming, bounded PDF, stale suppression. DONE
Not done: search result hover styles, group separators, match-context highlighting.

## Milestone 8: Onboarding, Empty States, Recovery (Partially Done)

Done: citation palette, excerpt queue, templates, reading session notes.
Not done: first-run vault flow with recent vaults, recoverable error UX,
indexing notifications, welcome screen improvements (Phase A A3).

## Milestone 9: Accessibility And Inclusive UX

- Toolbar tooltips (Phase A A1).
- Contrast validation for all theme pairs.
- Focus-ring visibility, reduced-motion behavior, keyboard traversal.

## Milestone 10: Performance And Perceived Speed (Initial Slice Done)

Done: command palette navigation, empty states, citation keyboard submit.
Not done: indexing progress placeholders, PDF spinner, annotation debounce,
debug diagnostics panel.

## Milestone 11: Documentation And Learnability

User guides, screenshot examples, shortcut reference, in-app help, terminology
consistency.

## Milestone 12: Release UX Hardening

Cross-platform smoke pass, portable settings, DPI scaling, release checklist,
visual-authenticity checklist, freeze and bug bash.

---

## Sequencing Notes

1. **Phase B first** — fixes wrong data (hardcoded "Saved", duplicate page
   numbers). These are bugs, not style. Do not apply Phase A styling to broken
   data.
2. **Phase A second** — visual polish once data is correct.
3. **Milestone 4 remaining** — can run in parallel with Phase A/B (different
   file: `editor/renderer.rs`).
4. **Milestones 5-12** after Phase A/B complete.

## Acceptance Criteria

- Phase B gate: page number appears exactly once when PDF open; save state
  correct (toolbar never says "Saved" when dirty); TOC toggle in one place.
- Phase A gate: all panels show hover backgrounds; toolbar tooltips present;
  28px status bar; sidebar shows empty-vault state.
- Common workflows complete without mouse.
- All primary screens have loading, empty, disabled, and error states.
- All shared controls use common style tokens.
- Large files and vault searches remain responsive.
- Accessibility checklist passes: focus, labels, contrast, keyboard, motion.
