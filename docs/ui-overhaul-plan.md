# UI Overhaul Plan

**Goal:** rebuild MD Editor's interface on one design system so every surface looks,
behaves and moves the same way, works in dark and light, and is fully operable from the
keyboard, without regressing the editor's performance or the app's zero-idle-CPU rule.

**Design system:** [`docs/design-system/md-editor/MASTER.md`](design-system/md-editor/MASTER.md)
with page overrides in [`pages/`](design-system/md-editor/pages/). This plan turns it into
code. Produced with the UI/UX Pro Max skill (provenance in MASTER).

**Date:** 2026-11-23 · **Status:** Phases 0–6 implemented (2026-11-24), with the deviations and open items listed in §7.

**Companion plan:** [Bug Discovery — Headless End-to-End Testing](e2e-bug-discovery-plan.md).
Its harness (`native/src/e2e/`) already runs the real app headlessly and takes screenshots;
§5 there sets the order of the two plans (behavioural bugs first, then Phase 0).

---

## 1. Audit — where the UI is today

Measured on `main` at `e9fbd45`, plus the screenshots in `images/` (2026-09-08; re-check
items marked † against a current build).

### 1.1 Foundations

| # | Finding | Evidence | Skill rule |
| :--- | :--- | :--- | :--- |
| F1 | UI text is too small: body 11px, secondary 10px, badges 8px | `theme.rs` `TEXT_BASE/SM/XS` | `readable-font-size`, "text < 12px" anti-pattern |
| F2 | No bundled fonts; UI and documents use the OS default sans, so metrics and look differ per platform | 15× `Font::DEFAULT`, no `.font()` on the daemon | `font-pairing`, `consistency` |
| F3 | Text hierarchy collapses: `TEXT_MUTED` vs `TEXT_SECONDARY` contrast is **1.17:1**, so they read as one grey | measured | `visual-hierarchy` |
| F4 | Borders are nearly invisible: 1.18:1 (`BORDER_SUBTLE`), 1.90:1 (`BORDER`); inputs have no ≥3:1 boundary | measured | `color-accessible-pairs` (non-text 3:1) |
| F5 | Colour roles are misnamed and misused: `WARNING` is pale teal, `SUCCESS` near-white; the editor paints math with `WARNING` (10 uses) | `theme.rs`, `editor/` | `color-semantic` |
| F6 | Tokens are bypassed ~350 times (literal sizes, paddings, colours) — worst in graph inspector (34), PDF viewer (33), ink (29), graph toolbar (27) | grep of `views/`, `app/` | `color-semantic`, "raw hex in components" |
| F7 | Spacing scale has off-grid steps (2, 6px) | `theme.rs` | `spacing-scale` (4/8) |
| F8 | Icons are hand-drawn canvas paths plus Unicode glyphs as controls (`-` `+` `☰` `⧉` `⟳` `✎` `✕` `●`) | `views/icons.rs`, `pdf_viewer.rs`, `pptx_viewer.rs`, `sidebar.rs`, `tracker.rs`, `graph/*` | `no-emoji-icons`, `icon-style-consistent` |
| F9 | Only 3 tooltips in the app; most toolbar controls are icon-only without a name or shortcut | `tooltip(` × 3 | `aria-labels`, `nav-label-icon` |
| F10 | No component layer: each view styles its own buttons; only 9 hover/pressed state handlers; the graph has a private widget kit | grep | `state-clarity`, `consistency` |
| F11 | Dark only; no light theme, no system-theme follow, no settings screen at all | — | `dark-mode-pairing` |
| F12 | No visible keyboard focus on chrome controls; Tab reaches only text inputs | 4 focus operations in the codebase | `focus-states`, `keyboard-nav` |
| F13 | No reduced-motion option; caret glide, smooth scroll and graph physics always animate | `motion.rs` | `reduced-motion` |
| F14 | No accessibility tree (Iced 0.14 limitation) — screen readers cannot read the app | Cargo.lock: no AccessKit | `voiceover-sr` (blocked) |

### 1.2 Surfaces

| # | Surface | Finding |
| :--- | :--- | :--- |
| S1 | File tree | Long names wrap mid-word into two lines; rename/delete only on the selected row with no keyboard path (F2/Delete) or context menu; no filter field |
| S2 | Toolbar | Icon-only, unlabelled; saved state is text only (" • Saved" / " • Unsaved changes"); no save-failure state; path shown flat, not as breadcrumbs |
| S3 | Editor | Content column up to 880px ≈ 95–100 characters per line; font-size literals in the highlighter; math coloured with `WARNING`†; reveal styling switches the line to italic source† |
| S4 | PDF pane | Toolbar at the bottom with text glyphs; page counter not editable; load errors as plain warning text; highlight actions discoverable only by right-click |
| S5 | Presentations | Same glyph toolbar as PDF, separate implementation |
| S6 | Search | Find bar toggles are text checkboxes; truncated-results notice is a 1px-text spacer hack (`text("").size(1)`) |
| S7 | Command palette | Good ranking and keyboard support; rows lack icons per kind beyond a letter badge; shortcut hints not styled as keys |
| S8 | Dialogs | Each modal hand-built; no focus trap; name validation errors only as toasts |
| S9 | Toasts | One style for info and errors; errors vanish after 3s with no action |
| S10 | Side panels | TOC and backlinks have different headers; no empty states; no current-heading indicator in TOC |
| S11 | Tracker | Full-screen overlay used as navigation; `-0.0h`; empty chart shows a flat axis; ALL-CAPS 8px labels; "X" close glyph |
| S12 | Knowledge graph | Private widget kit (~150 literals); node kinds by colour only; drag-only repositioning |
| S13 | Handwriting | Separate literal styling; colour swatches without names |
| S14 | Welcome | Single action; no recent vaults |
| S15 | Global | No status bar; no context menus anywhere; no empty/loading/error state catalogue |

---

## 2. Target architecture

```
native/src/
├── design/                 tokens and theming (no widgets)
│   ├── mod.rs              Tokens struct, current() accessor, ThemeMode
│   ├── palette.rs          primitive ramps + DARK/LIGHT semantic role tables
│   ├── type_scale.rs       TextRole → size, line height, weight, family
│   ├── metrics.rs          space, control sizes, radius, elevation, icon sizes, layers
│   ├── motion.rs           duration tokens, reduced-motion switch (absorbs motion.rs)
│   └── fonts.rs            bundled Inter / JetBrains Mono / Source Serif 4 bytes
├── ui/                     the component library (MASTER §6), built only from design/
│   ├── button.rs  icon_button.rs  text_field.rs  segmented.rs  toggle.rs  tabs.rs
│   ├── list_row.rs  tree_row.rs  panel.rs  card.rs  dialog.rs  popover.rs  menu.rs
│   ├── tooltip.rs  toast.rs  kbd.rs  badge.rs  progress.rs  empty_state.rs
│   ├── toolbar.rs  status_bar.rs  split_handle.rs
│   ├── icon.rs             Phosphor SVG set (iced `svg` feature), Icon enum
│   └── gallery.rs          #[cfg(test)] renders every component in every state
└── views/                  screens composed only from ui/ (no styling of their own)
```

### 2.1 Runtime theming

Iced style closures receive only `&Theme`, and the editor renderer and canvases also need
colours. Design: two `static` `Tokens` tables (`DARK`, `LIGHT`) and an `AtomicU8` selecting
the active one; `design::tokens()` returns `&'static Tokens`. Switching theme flips the atomic,
bumps a theme revision (so the editor's `LayoutKey`/paint caches and the graph's GPU buffers
refresh), and rebuilds the iced `Theme` (`Theme::custom_with_fn` from the role table so
built-in widgets — scrollbars, text inputs — match). The ink window keeps its own paper
colours through `themer`.

- **System theme:** enable iced's `linux-theme-detection`; on Windows/macOS read the OS
  preference at startup and on window focus. Settings: *System* (default) · *Dark* · *Light*.
- **Interface size:** one scale factor (90–130%) multiplies type, control and icon tokens.

### 2.2 Guardrails (so it stays overhauled)

1. **Token lint test** (`native/src/design/tests.rs`): scans `views/`, `ui/`, `app/`, `ink/`
   and fails on `Color::from_rgb*`, numeric `.size(`, `.padding(`, `.spacing(`, `Fixed(` and
   `Duration::from_millis` outside `design/`. Allow-list with a written reason per entry.
   Starts as a ratchet: fails only if the count rises; reaches zero by Phase 4.
2. **Contrast test:** every documented fg/bg pair in both themes is computed and asserted
   ≥ 4.5:1 (text) or ≥ 3:1 (non-text), so palette edits cannot regress accessibility.
3. **Component gallery and screen snapshots:** render `ui::gallery` and the e2e scenarios'
   screens (`native/src/e2e/`, `Session::screenshot`) in both themes at 100% and 130% and
   diff against committed PNGs. Reviewers see visual changes in PRs.
4. **Glyph ban:** the lint also rejects single-character Unicode symbols in `text(…)` inside
   buttons.

---

## 3. Phased roadmap

Each phase ends shippable: tests green, no visual regressions outside its scope, wiki updated.
Sizes are relative (S ≈ a day or two, M ≈ a week, L ≈ two weeks or more of focused work).

### Phase 0 — Foundations (M)

| Task | Files | Done when |
| :--- | :--- | :--- |
| 0.1 Bundle Inter, JetBrains Mono, Source Serif 4 (OFL); register with `daemon.font()`; set the default font to Inter | `design/fonts.rs`, `main.rs`, `assets/fonts/`, `LICENSE-fonts` | Text renders identically on Linux/Windows/macOS; editor metrics tests pass (re-baseline `render_snapshot` transcript) |
| 0.2 `design/` token module: palette (dark + light), type roles, space/size/radius/elevation/layers, motion durations | `design/*` | MASTER §2–§5 expressed in code; old `theme.rs` constants become deprecated aliases mapping to roles |
| 0.3 Runtime theme switch + system-theme detection + theme revision invalidating editor/graph caches | `design/mod.rs`, `app/mod.rs`, `editor_state.rs`, `graph_state.rs` | Toggling theme repaints everything with no restart and no stale colours |
| 0.4 Reduced motion: setting + OS default (GTK `gtk-enable-animations`, Windows `SPI_GETCLIENTAREAANIMATION`, macOS `reduceMotion`); `motion::duration()` returns 0 when on | `design/motion.rs`, `platform.rs` | Caret glide, smooth scroll, panel slides, toast fade and graph physics are instant with it on |
| 0.5 Guardrails 1–4 (§2.2) in ratchet mode | `design/tests.rs`, `ui/gallery.rs` | CI-local `cargo test` fails on new literals or contrast regressions |
| 0.6 Phosphor icon set via `svg` feature; `ui::icon(Icon, size, role)` | `ui/icon.rs`, `assets/icons/` | Every existing `Icon` variant has a Phosphor equivalent; canvas icons deleted |

### Phase 1 — Component library (L)

Build every component in MASTER §6 with all listed states, in both themes, at both interface
sizes, visible in the gallery. Order by dependency: `icon` → `Button`/`IconButton`/`Tooltip`
→ `TextField`/`Toggle`/`Segmented`/`Tabs` → `ListRow`/`TreeRow` → `Panel`/`Card`/`Badge`/`Kbd`
→ `Popover`/`Menu`/`ContextMenu` → `Dialog`/`Toast` → `Toolbar`/`StatusBar`/`SplitHandle` →
`EmptyState`/`ProgressBar`.

Cross-cutting requirements:

- **Focus ring:** a `Focusable` wrapper widget that participates in iced's focus
  operations (`focus_next`/`focus_previous`) and draws the 2px `focus.ring`; Tab and
  Shift+Tab cycle in reading order within the focused region.
- **Pointer feedback:** press state within 100ms; hover via `motion.fast`.
- **Tooltips:** `IconButton` cannot be constructed without a label; tooltip shows label +
  `Kbd` shortcut.
- **Truncation:** `TreeRow`/`ListRow` ellipsize single-line text and expose the full text in
  a tooltip (S1).

Done when: gallery covers every component × state × theme × size; snapshot tests committed.

### Phase 2 — Application shell (L)

| Task | Replaces | Notes |
| :--- | :--- | :--- |
| 2.1 Layout frame: toolbar / sidebar / main / side panel / status bar with `SplitHandle`s (persisted widths, double-click resets) | `app/view.rs` stack | Panel widths become tokens; keep the clipped width animation |
| 2.2 Toolbar: grouped `IconButton`s with tooltips; breadcrumb path (click a segment → reveal in tree); save-state `Badge` (Saved · Saving · Unsaved · **Save failed** with retry) | `views/toolbar.rs` | S2; uses `status.*` with icon + text |
| 2.3 Status bar (new): save state, cursor Ln/Col, word count, selection count, indexing progress, vault name | — | `caption` text; hidden in focus mode |
| 2.4 File tree: `TreeRow` with indent guides, Phosphor file-type icons, ellipsis + tooltip, hover-revealed actions, **context menu** (New note here, New folder, Rename, Delete, Reveal in file manager, Copy path), keyboard (↑↓←→, Enter, F2, Delete), filter field, empty state | `views/sidebar.rs` | S1, S15 |
| 2.5 Side panels: one `Panel` for TOC, Backlinks, Graph inspector; TOC current-heading indicator; empty states | `views/toc.rs`, `backlinks.rs` | S10 |
| 2.6 Command palette on `Dialog` elevation 2: section headers (Commands · Files · Recent), Phosphor icon per kind, `Kbd` shortcut hints, "No matches — create note *query*" action | `views/command_palette.rs` | S7 |
| 2.7 Dialogs: every modal on `Dialog` (create, rename, delete, quick note, link note, link-note picker); focus trap, Escape, Enter = primary; **inline validation** under the field (empty, invalid characters, already exists) | `views/modals.rs`, `link_note_picker.rs` | S8 |
| 2.8 Toasts: info/success/error variants with icon; errors persist with an action (Retry / Show details); one toast at a time with a queue; bottom-centre above the status bar | `views/toast.rs`, `ui_state.rs` | S9 |
| 2.9 Welcome: wordmark, *Open folder…* (primary), *Create vault…*, recent vaults list with paths, shortcut hints | `views/welcome.rs` | S14 |
| 2.10 Settings view (new): Appearance (theme, interface size, reduced motion), Editor (reading font, size, line width, spellcheck placeholder), Files (excluded folders), Keyboard (read-only shortcut list), About | new `views/settings.rs`, `config` keys | F11; persisted via `settings` table |

### Phase 3 — Content surfaces (L)

| Task | Scope |
| :--- | :--- |
| 3.1 Editor typography per `pages/editor.md`: measure 720px default + setting; heading ×1.25 scale; highlighter font-size literals → `metrics.rs` roles; bundled mono | `editor/highlight/*`, `editor/renderer/metrics.rs` |
| 3.2 Editor colour roles: `doc.*` roles in both themes; math off `WARNING`; find vs selection distinguished by outline; syntect themes generated from tokens | `editor/renderer/draw/*`, `highlight/code.rs` |
| 3.3 Reveal styling: markers in `doc.syntax`, no reflow on reveal, no italic source switch | `renderer/spans.rs`, `draw/inline.rs` (with renderer property tests at 3000 cases) |
| 3.4 Editor context menu: Cut/Copy/Paste, Bold/Italic/Code/Link, Insert table/math/checkbox | `renderer/events.rs` → message |
| 3.5 Shared viewer toolbar at the top for PDF, slides, images (`pages/viewers.md`): editable page field, zoom field, fit width/page, outline, find, overflow | `views/pdf_viewer.rs`, `pptx_viewer.rs`, image view |
| 3.6 PDF canvas: theme-aware ground, real-size page placeholders with shimmer (off under reduced motion), `EmptyState` errors with actions, annotation colours from the content palette, focus outline from `focus.ring` | `views/pdf_viewer.rs`, `interactive_pdf.rs` |
| 3.7 PDF selection popover: Highlight (swatches) · Copy · Add note · Link note | new `ui::Popover` usage, `app/pdf.rs` |
| 3.8 Image view: fit/100%/zoom controls, checkerboard behind transparency | `app/view.rs` image branch |

### Phase 4 — Feature surfaces (L)

| Task | Scope |
| :--- | :--- |
| 4.1 Find bar: compact bar with `IconButton` toggles (match case, regex, PDF loose) + tooltips, match counter "3 of 17", replace row expander; remove the `size(1)` spacer | `views/search.rs` |
| 4.2 Vault search: results grouped by file with highlighted snippets, keyboard navigation, truncation notice as a proper row | `views/search.rs` |
| 4.3 Tracker as a view (`pages/tracker.md`): tabs, single primary timer action with tabular time, stat cards, bar chart with values and empty state, `1h 05m` formatting (fix `-0.0h`), inline JSON errors | `views/tracker.rs`, `tracker_state.rs`, routing |
| 4.4 Knowledge graph on shared components (`pages/graph.md`): shape-coded kinds, legend card, inspector as standard panel, keyboard navigation and pin without drag, reduced-motion settle; delete `views/graph/widgets.rs` | `views/graph*`, `gpu_graph.rs`, `shaders/graph.wgsl` |
| 4.5 Handwriting window on shared components (`pages/ink.md`): toolbar, named swatches, unsaved chip, paper colours | `views/ink.rs`, `ink/canvas.rs`, `ink/radial.rs` |

### Phase 5 — Keyboard and accessibility completion (M)

| Task | Done when |
| :--- | :--- |
| 5.1 Region focus model: F6 cycles toolbar → sidebar → editor → side panel → status bar; focus visible everywhere | Every control reachable without a mouse |
| 5.2 Shortcut audit: resolve the `Ctrl+B` conflict (bold vs sidebar — `issues.md`), add `Ctrl+,` (Settings), `Ctrl+Shift+Z` redo, `F2` rename; shortcuts shown in tooltips, menus and Settings from one registry | No shortcut does two things in one context |
| 5.3 Text scaling check at 130%: no clipped labels, truncation everywhere has a tooltip | Gallery + screenshot pass at 130% |
| 5.4 Contrast and colour-only audit in both themes | Guardrail 2 green; status, graph and annotations all have a non-colour cue |
| 5.5 Screen-reader readiness: keep every control's label in one place (`IconButton` label) so it can feed AccessKit when Iced supports it | Tracked issue linked to Iced upstream |

### Phase 6 — Finish (S)

- Token lint switched from ratchet to zero tolerance; delete deprecated `theme.rs` aliases.
- Wiki: rewrite *Design Tokens & Motion* as *Design System* (links MASTER), update *Native
  Desktop GUI* views section, *Keyboard Shortcuts*, user guide screenshots.
- README screenshots retaken in dark and light; handbook marked for rebuild.
- Run the pre-delivery checklist (§5) on every surface and record results in the PR.

---

## 4. Risks and how each is handled

| Risk | Mitigation |
| :--- | :--- |
| Font change shifts editor layout and breaks caret/hit-test agreement | Font swap lands alone in 0.1 with the render transcript and property suites run at 3000 cases; metrics come from the font file, so the layout engine adapts rather than hard-coding |
| Theme switching leaves stale colours in caches (editor layout cache, graph GPU buffers, slide canvases) | Theme revision folded into `LayoutKey`/`layout_revision` and graph instance rebuilds (0.3); gallery snapshot in both themes |
| Zero-idle-CPU regression from new hover/tooltip/shimmer animations | All new motion uses the existing `Motion` frame gate; shimmer only while a page is pending; an idle-CPU check stays in the release checklist |
| Iced lacks a focus model for buttons | `Focusable` wrapper built on iced's widget `operation` focus API in Phase 1; if insufficient, a small custom focus manager in `ui/` |
| Big-bang migration stalls | Ratcheting lint lets surfaces migrate one PR at a time; old `theme.rs` names alias new roles until Phase 6 |
| No screen-reader support | Stated plainly in docs; labels centralised for AccessKit later |
| Light theme exposes hard-coded assumptions (white PDF pages, ink paper, syntect theme) | Content palette and paper colours are explicitly separate from UI roles (MASTER §2.4, `pages/ink.md`) |

---

## 5. Definition of done (per surface)

Adapted from the skill's pre-delivery checklist for a desktop app (its mobile-only items —
safe areas, touch targets in pt/dp, orientation — are replaced by desktop equivalents).

- [ ] Uses only `ui/` components and `design/` tokens (lint clean for the file)
- [ ] Every icon is Phosphor, one weight per surface; no Unicode/emoji glyphs as controls
- [ ] Every icon-only control has a tooltip with its name and shortcut
- [ ] Rest, hover, pressed, focused, selected, disabled states all visible and distinct; no
      layout shift on press
- [ ] Empty, loading and error states designed and reachable
- [ ] Text ≥ 12px; text contrast ≥ 4.5:1 and control boundaries/focus ≥ 3:1 in **both** themes
- [ ] Colour is never the only signal
- [ ] Fully operable by keyboard; focus always visible and never hidden behind overlays
- [ ] Works at 90% and 130% interface size with no clipped or overlapping text
- [ ] Reduced motion on: nothing animates except instant state changes
- [ ] Animations interruptible, exits faster than entries, ≤ 2 animated elements per view
- [ ] Zero CPU when idle (no new always-on subscription)
- [ ] Gallery/screenshot snapshots updated and reviewed in both themes

---

## 6. Suggested first PRs

1. **0.1 Fonts** — smallest change with the largest visual effect; validates the
   renderer's font-agnostic metrics early.
2. **0.2 + 0.5 Tokens and guardrails (ratchet)** — makes every later PR measurable.
3. **0.6 + 1.x `icon`, `Button`, `IconButton`, `Tooltip`** — then migrate the toolbar (2.2)
   as the first visible end-to-end slice.

## 7. Progress

**2026-11-24 — Phases 0–2, first pass (uncommitted at hand-over, then made to build).**

Landed: bundled fonts (0.1), `design/` tokens with runtime Dark/Light and interface scale
(0.2, 0.3), reduced-motion preference with OS default (0.4), guardrail tests for contrast,
the 12 px floor and token-only `ui/` (0.5), Phosphor SVG icons (0.6, except the canvas set
still used by graph, ink, deck and search); the `ui/` component library with a gallery test
in both themes at 90/100/130 % (Phase 1); toolbar, tab strip, sidebar `TreeRow`s with
context menus, `Panel`-based outline and backlinks, status bar, toast queue with Retry,
`Dialog` modals with inline validation, welcome with recent vaults, and a Settings page
(2.1–2.10). The old `native/src/motion.rs` is gone (its springs and tests live in
`design/motion.rs`).

**2026-11-24 — Phases 3–6 and the gaps from the first pass.**

- **Editor (3.1–3.2):** body size, reading font and line width from Settings apply live
  (`metrics::body_font_size`/`scaled`, `measure::body_font`, `max_content_width`; the line
  height cache is invalidated on `theme_revision`). Heading scale 33 · 27 · 21 · 17; rows
  are never shorter than iced's text box. All highlighter and renderer colours are `doc.*`
  roles; switching theme re-highlights (`Highlighted` remembers its theme) and re-renders
  math in the new ink (`MathRendered` carries its colour, stale renders are dropped).
  Code blocks use `base16-ocean.dark` / `InspiredGitHub` per theme. The active find match
  has a 1px outline. Bundled Inter / JetBrains Mono / Source Serif.
- **Viewers (3.5, 3.8):** `views/viewer_toolbar.rs` at the top of PDFs, decks and images:
  editable page/slide field, zoom field, fit width/page, find. PDF highlight swatches use
  `ContentHue`; placeholders are page-sized; errors are `EmptyState`s.
- **Find and search (4.1, 4.2):** find bar with replace expander, icon toggles, "N of M",
  Replace and Replace all; vault search grouped (open PDF, notes) on `ListRow`s with the
  keyboard selection shown, a truncation row and empty states.
- **Tracker (4.3):** stat cards, one primary Start/Stop action, underline tabs, a 14-day
  bar chart with a value tooltip per bar and an empty state, `1h 05m` durations, inline
  JSON errors with Save kept enabled.
- **Graph (4.4):** toolbar on `Segmented`/`TextField`/filter chips, inspector as a standard
  `Panel` with stat cards, legend card with icons per kind, kinds coloured
  `accent`/`status.info`/`fg.tertiary`; `views/graph/widgets.rs` deleted.
- **Ink (4.5):** toolbar on components (filled icon for the active tool, segmented widths
  and paper, Unsaved chip, primary Save), 28px swatches with a focus ring.
- **Keyboard (5.1–5.2):** the sidebar and side panels show the focus ring when focused;
  F6 into the main region, and closing the palette, find or a dialog, put the keyboard back
  in the note (the editor is `Focusable`). Every registry chord is checked against the key
  handler by a test; `Ctrl+Shift+Z` redoes on the ink canvas.
- **Finish (6):** `theme.rs` and the canvas icon set are deleted; the colour lint covers
  `views/` and `app/` at zero tolerance; wiki pages rewritten (Design System & Motion,
  Native Desktop GUI views, Keyboard Shortcuts, User Guide).
- **Gaps closed from the first pass:** excluded folders apply to the index, tree and
  watcher; context menus open at the pointer (`ui::context_anchor`); the PDF page/zoom
  fields, Fit page, the find bar's replace, and the deck's slide field are all reachable.
- **Fixes found on the way:** `Button`, chips, segmented items, menu and tree rows
  top-aligned their labels; the editor test harness clicked at a hard-coded 880px; app boot
  reset the body size to 16 (the spec and default are 17); theme, scale and typography were
  process-global and raced between parallel tests (`design::Setting`); a dropped file could
  overwrite a vault file of the same name.

Still open:

- **3.3 reveal styling** (no italic source switch on reveal) — untouched; it needs the
  renderer property suites at 3,000 cases around a behaviour change.
- **3.6** loading shimmer and **3.7** the PDF selection popover (selection tools live in the
  toolbar instead); **3.8** checkerboard behind transparent images.
- **4.4** shape-coded node kinds in `shaders/graph.wgsl` (circle for notes, rounded page rectangle for PDFs, diamond for sketches, hollow ring for missing targets), styled epistemic edges (`Supports`, `Contradicts`, `Derives`), keyboard navigation (`Tab`/`Shift+Tab`/`←`/`→` + `P` pin + `Enter` open), and named per-vault **Pinboards** — **completed** (2026-11-26).
- **Light paper for handwriting** (`pages/ink.md`): the paper is part of the ink document
  format, so it needs a format change.
- README screenshots in both themes.
- ~~In headless e2e screenshots the graph's label layer and GPU layer disagree on
  positions~~ — fixed: until the GPU layer commits a fit, the labels frame the graph with
  the same fresh fit (`GraphFrame::label_transform`).
