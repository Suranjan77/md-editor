# MD-Editor Overhaul — UX/UI Addendum

**Status:** This addendum is part of the master plan (`MD-EDITOR-OVERHAUL-PLAN.md`). It inserts a UX track into the existing phase order. All agent rules from the master plan §0 apply. Task IDs use `UX` prefixes; the ledger in `PLAN-NOTES.md` tracks them alongside the rest.

**Why this exists:** the base plan over-indexed on architecture. The product's core promise is a *calm, polished live-Markdown workspace*, and the current editing feel is clanky. Root causes identified in code:

1. **Line-based restyle on every keystroke.** `editor/highlight.rs` restyles whole lines; combined with marker concealment (`StyledSpan` hides `**`, `#`, `$`), any cursor move across a styled region makes text reflow and shift horizontally — the single biggest "clank."
2. **Binary conceal/reveal.** Syntax markers appear/disappear instantly with no transition and with width changes, so the line under the caret visibly jumps.
3. **No motion system.** Zero animation/easing/`Instant`-driven interpolation in renderer or theme: caret teleports, scroll snaps, panels pop.
4. **No incremental parse.** Full-document or large-region reparses on edit cause latency spikes on big files (also a Phase 4 perf item — these tasks coordinate).
5. **No design system.** `theme.rs` is a color list; spacing, type scale, radii, elevation, and state styles (hover/focus/pressed/disabled) are ad-hoc per view, so the UI feels assembled rather than designed.

---

## Where the UX track slots in

| Master phase | UX insert | Theme |
|---|---|---|
| Phase 1 | **UX-A** | Design system foundations + UX audit |
| Phase 3 (after T11) | **UX-B** | Shell & navigation polish |
| Phase 4 (interleaved) | **UX-C** | Live-editing feel — the centerpiece |
| Phase 5 | **UX-D** | PDF reading experience |
| Phase 7 | **UX-E** | Onboarding, empty states, settings UX |
| Phase 9 | **UX-F** | Motion, micro-interaction & final polish pass |

Rule: UX-C tasks may begin once Phase 4's safety nets (P4.T1 buffer proptests, P4.T2 parser conformance, P4.T4 golden renders) are green, because they intentionally change rendering behavior and will require deliberate snapshot updates.

---

# UX-A — Design System Foundations (during Phase 1)

### UXA.T1 — Heuristic UX audit (do this first)
- Run the app against a 30-item checklist (Nielsen heuristics adapted): visibility of state (saving? indexing?), latency feedback, undo affordances, consistency of spacing/typography across the 20 views, error message quality, focus visibility, discoverability of shortcuts. Capture screenshots into `docs/ux/audit-2026-06/` with one Markdown finding per issue: severity (S1 blocker → S4 nit), location, repro, proposed fix.
- **DoD:** `docs/ux/AUDIT.md` index with ≥25 concrete findings, each tagged to a UX task below or filed to Appendix A backlog. This audit is the evidence base; later DoDs reference finding IDs.

### UXA.T2 — Design tokens
- Refactor `native/src/theme.rs` into `native/src/design/`:
  - `tokens.rs`: spacing scale (4/8/12/16/24/32), radius scale (2/4/8), type scale (12/13/14/16/20/24/32 with line-heights), elevation (3 shadow levels), duration tokens (fast 90ms, base 160ms, slow 240ms) and easing constants (standard cubic-bezier equivalents as iced easing fns), z-layers.
  - `palette.rs`: semantic colors only — `surface`, `surface_raised`, `text`, `text_muted`, `accent`, `danger`, `success`, `selection`, `caret`, `syntax_marker`, `code_bg`, etc., each defined for light + dark. No view may reference a raw hex after this task (architecture-check rule: forbid `Color::from_rgb` outside `design/`).
  - `styles.rs`: shared widget style fns (button primary/secondary/ghost, input, list row with hover/selected, panel, divider) consuming tokens.
- Migration is mechanical and may be split per-feature into UXA.T2a–T2g.
- **DoD:** zero raw color/spacing literals outside `design/` (CI grep check); both themes pass the contrast script (master plan P9.T2 pulls forward its color-pair checker to here); visual smoke screenshots before/after committed to `docs/ux/`.

### UXA.T3 — Typography & rhythm pass
- One reading font + one mono font, loaded with proper fallbacks; baseline-grid spacing in the editor (line-height = exact multiple of 4px at default size); consistent heading scale in *rendered* markdown matching the type scale; tabular numerals in tracker/status bar.
- **DoD:** golden render snapshots updated deliberately; audit findings about type inconsistency (reference IDs) closed.

---

# UX-B — Shell & Navigation Polish (after P3.T11)

### UXB.T1 — Layout grid & density
- Normalize all chrome to tokens: sidebar (file tree) row height, indent guides, hover/selected states, truncation with tooltips; status bar segment layout; toolbar icon sizing/spacing; consistent 1px hairline dividers with theme-aware color. Fix any audit S1/S2 layout findings.
- **DoD:** referenced audit findings closed with screenshots; widget style fns reused (no bespoke styling in feature views).

### UXB.T2 — Focus model & keyboard navigation
- One visible, consistent focus ring (2px accent, offset 1px) everywhere; `Tab` order audited per view; pane focus indicated by a subtle top border on the active pane; `Ctrl+1/2/3` sidebar/editor/secondary-pane focus; Esc consistently closes the topmost overlay only (define an overlay stack in `features/overlays`).
- **DoD:** keyboard-only walkthrough script in `docs/ux/KEYBOARD-WALKTHROUGH.md` passes; overlay-stack unit tests (push/pop/Esc).

### UXB.T3 — Command palette & search UX upgrade
- Fuzzy matching with match-character highlighting; recent-commands section; category grouping; result rows show shortcut hints right-aligned; ≤ 80ms render for 500 commands (bench).
- **DoD:** palette snapshot tests; bench committed; audit findings closed.

### UXB.T4 — Status & feedback system
- Unify toasts/status: every async action (save, index, export, PDF load) reports through one `StatusService`: inline spinners where local (e.g., PDF page placeholder), status-bar progress for vault-wide ops (indexing N/M with cancel), toasts only for completed/failed user-initiated actions, never for routine autosave. Add a subtle "saved ✓ → fades" indicator in the status bar instead of toast spam.
- **DoD:** message-flow doc table (action → feedback surface); toast count during a 5-minute normal-use script ≤ 3.

---

# UX-C — Live-Editing Feel (the centerpiece; interleaved with Phase 4)

**Target experience (write this into `docs/ux/EDITOR-FEEL.md` first, as UXC.T0):** typing is instant (<8ms keypress→frame on a 10k-line doc); the line you're editing shows syntax markers in a muted color (never hidden mid-edit); leaving the line conceals markers with a brief fade and **no horizontal text shift**; caret moves smoothly; checkboxes, links, and images are directly interactive; common Markdown chores (lists, pairs, tables, paste) are automated. Obsidian-class live preview is the bar.

### UXC.T1 — Stable-width concealment (kills the #1 clank)
- Replace binary hide/show of markers with **reserved-width rendering**: when a span's markers are concealed, lay out the line as if markers occupy zero width *consistently in both states* by switching strategy — markers on the *active line* render in `syntax_marker` muted color; on inactive lines they are removed **before layout** (current behavior). The shift happens only when the caret enters/leaves a line, so make the transition cheap and predictable: re-layout only that line, and cross-fade marker glyphs over `duration.fast`.
- Precisely: active-line state is an input to `highlight_line` (pass `cursor_line: Option<usize>` through the parser/render boundary — parsing rules stay in the parser per AGENTS.md; only the conceal decision is parameterized).
- **DoD:** golden render snapshots for (caret on / caret off) a bold+link+math line; manual capture gif in `docs/ux/`; no layout of *other* lines changes when caret moves (assert draw-command diff confined to the two affected lines in a test).

### UXC.T2 — Incremental restyle
- Restyle only dirty lines: maintain per-line style cache keyed by (line text hash, block-context state in/out). Multi-line constructs (fenced code, math blocks, tables) carry an entry/exit state so a single-line edit re-styles forward only until state convergence. This coordinates with master P4.T5 perf budgets.
- **DoD:** bench: keypress restyle cost O(1) lines amortized — typing in the middle of a 100k-line doc restyles <16 lines (instrumented counter assert); conformance suite still green.

### UXC.T3 — Motion primitives + smooth caret/scroll
- Add `design/motion.rs`: a tiny tween system (start value, target, duration token, easing; ticked via the existing iced subscription/frame redraws, auto-sleeping when idle so we keep the "calm/no busy redraw" ethos — assert zero redraw requests when fully idle).
- Apply to: caret position (60–90ms ease-out slide, instant on click/large jumps), scrolling (wheel smoothing + animated scroll-to for TOC/search-result jumps with a brief target-line background pulse), selection growth (no animation — must feel immediate), conceal cross-fade (from UXC.T1).
- Setting: "Reduce motion" (also default-on if OS reports it where detectable) disables all tweens.
- **DoD:** motion unit tests (tween math, sleep-when-idle); reduce-motion path verified; gif captures.

### UXC.T4 — Typing ergonomics bundle (split T4a–T4f, each its own commit)
- **a. Auto-pairs:** `*_`` ()[]{}"` pairing with type-over and wrap-selection; smart enough to not pair inside words or code-language-aware contexts.
- **b. Smart lists:** Enter continues `-`, `1.`, `- [ ]` (renumbering ordered lists), Enter on empty item removes it; Tab/Shift+Tab indent/outdent list items; checkbox state preserved.
- **c. Smart paste:** pasting a URL over selected text creates a link; pasting multi-line into a list continues markers; paste of image data writes file into vault `assets/` and inserts reference (file-name prompt inline).
- **d. Heading/format cycling:** `Ctrl+1..6` set heading level on current line, repeat to toggle off; `Ctrl+B/I/K` already? — audit and make wrap/unwrap idempotent on partial selections.
- **e. Table editing:** Tab/Shift+Tab cell navigation, Enter adds row, auto-align pipes on leave-line (uses existing `buffer/table.rs` — audit it first), column resize keeps alignment row consistent.
- **f. Markdown-aware soft-wrap indent:** wrapped continuation lines hang-indent to content start of list items/quotes.
- All mutations via `EditorCommand` so each is a single undo step (property tests: every feature's action then undo == original).
- **DoD per sub-task:** unit tests incl. CRLF and unicode; undo-roundtrip property; docs/SHORTCUTS.md updated.

### UXC.T5 — Interactive rendered elements
- Click checkbox toggles it (single undo step); Ctrl+Click link opens (note → tab, URL → browser, with hover hint in status bar); image references render inline (size-capped thumbnail with click-to-open in pane; async load with placeholder shimmer); footnote/reference hover shows peek popover.
- **DoD:** hit-test unit tests for each interactive region (extend `renderer/hit_test.rs` tests); golden snapshots; no parser logic added to renderer (architecture rule).

### UXC.T6 — Editing-latency instrumentation & budget
- Add a debug HUD (diagnostics view toggle) showing rolling keypress→present latency, restyle line count, layout time. CI bench gate: p95 keypress latency on 10k-line fixture < 8ms (loose CI multiplier ×3 documented).
- **DoD:** HUD works; bench in perf-smoke; numbers recorded in docs/PERF.md.

### UXC.T7 — Caret/selection visual quality `[parallel-ok]`
- 2px caret with proper blink (pause-on-type, restart-on-idle), correct height across mixed font-size spans (headings), bidi-safe placement deferred (note in backlog); selection rendering with rounded corners on run ends and correct rect merging across spans/lines; dim non-active panes' carets.
- **DoD:** golden snapshots for caret in heading vs body, selection across a heading+code line.

---

# UX-D — PDF Reading Experience (within Phase 5; extends P5.T3/T5)

### UXD.T1 — Reading polish
- Page shadows + correct page gaps from tokens; render-scale aware of zoom (re-render at threshold crossings, never blurry upscales > 1.4×); smooth zoom anchored at cursor; pinch/Ctrl+wheel; double-click word-select, triple-click line; selection rectangles with rounded merging matching editor style.
- **DoD:** smoke gif; zoom-anchor unit test on transform math.

### UXD.T2 — Annotation interaction feel
- Highlight color popover appears at selection end with keyboard support (1–4 pick color, N adds note); existing highlights hover-raise slightly; margin note indicators with count badges; side-panel rows animate scroll-to-quad with target pulse (reuses motion primitives).
- **DoD:** snapshot + hit-test tests; audit findings closed.

### UXD.T3 — Continuity cues
- Page number pill while scrolling (fades out); reading-position restore indicator on reopen ("resumed at p. 47" status flash); TOC current-section auto-highlight while scrolling.
- **DoD:** tests for section-from-scroll mapping; gif.

---

# UX-E — Onboarding, Empty States, Settings (within Phase 7)

### UXE.T1 — First-run & welcome redesign
- Rework `welcome` view: open-vault, recent vaults (with paths, remove option), "create sample vault" that generates a small tutorial vault demonstrating links/PDF/tracker; keyboard-first.
- **DoD:** sample-vault generator tested; snapshot.

### UXE.T2 — Empty & error states everywhere
- Design tokens-based empty states for: empty vault, no search results (with tips), no backlinks, no annotations, broken PDF (clear error + "open externally"), tracker with no sessions. Each: icon, one sentence, one action.
- **DoD:** every panel has an explicit empty state (checklist in audit doc); snapshots.

### UXE.T3 — Settings modal UX (extends P7.T5)
- Grouped, searchable settings; every control shows live preview where possible (theme, font size, wrap width apply immediately); "restore defaults" per group; changed-from-default dot indicator.
- **DoD:** round-trip tests from base plan plus search-filter unit test.

---

# UX-F — Final Polish Pass (within Phase 9)

### UXF.T1 — Micro-interaction sweep
- Hover/pressed states on every interactive element (style-fn coverage audit); panel open/close uses motion tokens; toast slide+fade; tooltip delay standard (600ms) with shortcut hints.
- **DoD:** style-fn coverage grep shows no bare `button(` without a style; reduce-motion respected globally.

### UXF.T2 — Visual QA matrix
- Screenshot matrix script (debug command dumps key views) at 1×/1.5×/2× scale factors, both themes → `docs/ux/qa-matrix/`; eyeball + fix scaling bugs (the M12 DPI work claims done — verify).
- **DoD:** matrix committed; DPI findings fixed or filed S-rated.

### UXF.T3 — External feel review
- Recruit 3 outside users (or simulate via the audit checklist re-run cold); 20-minute task script (open vault, write a note with table+math, read+highlight a PDF, link it, find it again). Log friction events; fix top 5.
- **DoD:** session notes in docs/ux/; 5 fixes landed referencing notes.

---

## Coordination & guardrail updates
- `budgets.toml` gains: `max_keypress_latency_p95_ms`, `max_restyle_lines_per_edit`, `toast_budget_smoke` — wired into perf-smoke.
- Architecture-check additions: no raw colors outside `design/`; renderer gains no parser rules (existing); `design/` imports nothing from `features/`.
- Master-plan DoD amendment: from UX-A onward, any task touching a view must use design tokens (reviewer check item).
- New ADR-0007: "Live-preview concealment strategy" documenting UXC.T1's active-line approach and rejected alternatives (always-visible markers; full WYSIWYG block widgets).