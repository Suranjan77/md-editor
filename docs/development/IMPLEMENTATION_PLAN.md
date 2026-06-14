# V3 Implementation Plan — remaining work, step by step

> Audience: an agent (or human) who has **not** read the rest of this repo.
> Follow phases in order. Phase 0 exists because completed features kept
> shipping with paint-level bugs the test suite could not see; do not skip it.
> Companion documents: `docs/HANDOFF.md` (execution ledger — update it
> after every phase), `docs/GROUND_UP_PLAN.md` (the master plan, cited as
> "plan §…"), `docs/SHORTCUTS.md` (generated — never edit by hand).
>
> **Current position (2026-06-13):** Phases 0–5, UX phases 0–6, course
> correction 6.0–6.6, and Typora-grade phases 7.0–7.6 are implemented.
> `gui/mod.rs` is 1,441 lines (from 4,838), `editor/src/buffer.rs` is 833
> (from 1,911), and every extracted module is within the 700-line hard
> limit. Automated gates are green. Remaining acceptance work is the
> real-vault manual smoke checklist, items 27–37.

---

## 0. How to work in this codebase (read before writing code)

### 0.1 Layout

```
v3/                    independent cargo workspace (root workspace excludes it)
  kernel/              commands, keymap, panes, focus      (no deps on the rest)
  editor/              markdown engine (rope, parser, layout)
  pdf/                 pdf engine (pure: tile/scroll/select/outline; impure: render.rs behind `pdfium` feature)
  vault/               fs, FTS index, annotations, sessions, migrations
  shell/               the iced GUI binary (the only crate that sees iced)
core/, native/         v2 — the shipping app. REFERENCE ONLY. Never modify.
tests-fixtures/pdf/    generated corpus (edit scripts/gen-fixtures.py, not the PDFs)
```

Engines are peers: `vault` must not depend on `pdf` (composition happens in
the shell, e.g. the `TextExtractor` seam). The kernel is serde-free. Only the
shell knows file formats and iced.

### 0.2 Hard rules

- `unwrap`/`expect` are **banned outside `#[cfg(test)]`** (clippy enforces).
  `Result<_, String>` is banned in v3 crates — use `thiserror` enums.
- Every keystroke enters through `Shell::on_key` → `Workspace::handle_key`.
  **Never** give a widget its own key binding (that is v2's Bug A).
- Every user action is a registered command (`kernel/src/defaults.rs`).
  New command = new `spec(...)` entry there + handler arm in
  `gui/mod.rs::run_command` + regenerate the shortcuts doc (see gate below).
- New SQLite tables go through `vault::migrations` with a **new** component
  name or a new numbered step. Never edit an existing migration (a
  fingerprint test will fail if you do).
- The decision log in `HANDOFF.md` is append-only, newest last.

The following rules were added 2026-06-12 after a course-correction review
(see Phase 6). Each one exists because it was violated; none is optional.

- **The gate is binary.** `cargo test --workspace` in *both* feature configs
  green is part of "done". A red test is never handed off, documented around,
  or deferred to "a later decision" — either fix the code or change the test
  contract *in the same unit of work*, and record which in the decision log.
  (Cautionary example: the 2026-06-12 tracker-wiring failure was shipped with
  a paragraph explaining it instead of a fix. Its resolution is Phase 6.0.)
- **Size budgets (ratchet).** New v3 files: soft limit 400 lines, hard limit
  700. Functions: soft limit 75. Files already over the hard limit are frozen
  at their current ratchet and may only shrink:
  `shell/src/gui/mod.rs` (2 587; initial 4 838),
  `editor/src/buffer.rs` (1 557; initial 1 911),
  `shell/src/gui/tracker_view.rs` (1 218), `shell/src/gui/editor_canvas.rs`
  (755), `kernel/src/pane.rs` (725), `pdf/src/render.rs` (704). The last two
  were discovered when Phase 6.1 enumerated every Rust file; omitting them
  from the original review list did not make them compliant. Adding a
  feature to a frozen file requires extracting at least as many lines as
  you add. Until the CI script exists (Phase 6.1), check with `wc -l` before
  handoff. A 4 800-line `mod.rs` is v2's god-file disease —
  the exact thing the ground-up plan (§1) was written to kill — regrowing
  inside v3; it does not get to keep growing.
- **Layout math belongs to the engine's measure phase.** Anything that
  affects a line's height or vertical offset (heading scale, images, math
  blocks, wrap width) flows Styler → Measurer → height tree → paint
  (plan §3.2). The canvas paints; it never invents geometry the engine has
  not measured. Hit-testing must read the *same* measured grid that paint
  uses — "click-to-caret mapping is approximate" describes a defect, not a
  polish item. The BUG-B gate ("caret motion damages ≤ 2 lines") applies to
  every renderer feature, including rendered assets.
- **Dependencies and global mutable state need an ADR *before* the code.**
  ADR-0102 was written after the `open` crate had already shipped — do not
  repeat that ordering. `static`/atomic globals for app state are banned;
  state belongs on `Shell` (persisted via the snapshot when it should
  survive a restart). The former light-theme atomic was removed in Phase
  6.6; do not reintroduce that pattern.
- **The handoff stays truthful.** A status-board ✅ means: gate green, every
  suite named explicitly (never "etc."), smoke items run. Known quality gaps
  make the row 🔶 with the gap stated in the row. The verification snapshot
  is updated only *after* committing, so it always describes a reachable
  state (retrospective rule, now binding).

### 0.2.1 Document canon

When two documents disagree, the higher row wins; fix the lower one in the
same change. Do not create new top-level docs without explicit user
instruction — sprawl is how plans stop being followed.

| Authority | Document |
|---|---|
| Product bar, architecture, quality gates | `GROUND_UP_PLAN.md` |
| Working rules + step-by-step next work | `IMPLEMENTATION_PLAN.md` (this file) |
| UX direction and chrome phases | `UX_OVERHAUL_PLAN.md` |
| Execution state (status board, decision log) | `HANDOFF.md` |
| Dated reports (e.g. `V3_STABILIZATION_*.md`) | historical record only — never work from them directly |
| `SHORTCUTS.md` | generated — never hand-edit |

### 0.3 The verification gate (run after every unit of work)

```bash
cd v3
cargo fmt --all                                            # then commit no diff
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features md-shell/pdfium -- -D warnings
cargo test  --workspace
cargo test  --workspace --features md-shell/pdfium
cargo run -p md-shell -- --demo                           # must end all-ok
cargo run -q -p md-shell -- --dump-shortcuts > ../docs/SHORTCUTS.md
git diff --exit-code ../docs/SHORTCUTS.md               # fresh ⇒ no diff (commit it if you added commands)
```

pdfium-gated tests must **skip, not fail** when `libpdfium` is absent
(pattern: `let Some(renderer) = renderer() else { eprintln!("skipping…"); return; }`).

A task is *done* when: the gate passes, new behavior has tests at the lowest
layer that can express it, the manual smoke checklist (`docs/SMOKE.md`,
created in Phase 0) passes for touched surfaces, and `HANDOFF.md` has a
status-board row + decision-log entries for any non-obvious choice.

### 0.4 Pitfalls register — bugs this codebase already shipped. Reread the relevant entry before touching that area.

| # | Area | Pitfall | Rule |
|---|---|---|---|
| P1 | iced canvas | Within one render layer, iced_wgpu draws **all images after all vector fills**, regardless of call order. A `fill_rectangle` after `draw_image` in the same frame paints *under* the image. | Anything that must paint over images goes on its own stacked canvas (`stack![]` children after the first get their own layer). The overlay canvas must return `None` from `update` and default `mouse_interaction` so events fall through. |
| P2 | PDF coordinates | Three spaces exist: **page points** (top-left origin, zoom-independent), **strip px** (points × zoom, the scroll space), **viewport px** (strip px − scroll, what canvases see). | Persist and store state in *page points* only (selections, annotations, link rects). Project to viewport px per frame via `DocLayout::placed_pages`/`page_at_point`/`point_in_page`. Never cache a projection. |
| P3 | pdfium glyphs | `loose_bounds` boxes span full line height (tightly-leaded lines overlap by 1–2 pt) **and** synthesized space chars can be degenerate (zero-height). Both broke selection. | Line banding is *vertical-center containment* (`select::joins_line`); caret resolution tie-breaks by horizontal distance. Test any geometry change against the synthetic grids in `select.rs` **and** real fixtures. |
| P4 | pdfium FFI | pdfium is single-threaded; `pdfium-render`'s `thread_safe` feature only makes handles Send/Sync, it does **not** serialize calls (concurrent calls SIGSEGV). | Every `PdfRenderer` method takes the process-wide `pdfium_lock()` first. Keep that pattern in any new method. |
| P5 | pdfium coords | pdfium rects are **bottom-left** origin; v3 stores **top-left**. | Flip with `y_top_left = page_height - y_bottom_left` at the FFI boundary (`render.rs`) and nowhere else. |
| P6 | pdfium text | `\r\n` come back as control chars and are dropped by `page_chars`, so multi-word search can't match across a line wrap. | Known limitation; don't "fix" it ad hoc — whitespace-elastic matching is a pure `select::find` change with tests. |
| P7 | status line | Closed in UX Phase 0.4: command messages and caret/page position are separate fields. | `sync_status()` writes only the position segment; handlers write only the message segment. Never merge them again. |
| P8 | borrows | `self.focused_pdf_mut()` borrows all of `self`; touching `self.status`/`self.vault_root` while it lives won't compile. | Clone `let root = self.vault_root.clone();` *before* taking the session; write `self.status` *after* the last session use. |
| P9 | keymap | A chord claimed by any reachable scope never falls through to raw input. Raw-input matches in `pdf_raw_input`/`editor_raw_input` ignore modifiers. | Check `docs/SHORTCUTS.md` before picking a chord. Bind in the narrowest scope that works. Overlay scope is a modal fence — while an overlay is open only Overlay+Global resolve. |
| P10 | tests vs paint | The windowless suites drive `Shell::update` and never call `draw()`. They **cannot see paint bugs** (P1 was invisible to 200 green tests). | Paint geometry must live in pure "paint plan" functions (Phase 0.1) with unit tests; toolkit-level effects are covered by the manual smoke checklist only. |
| P11 | fixtures | The generated fixtures are *friendly* (uniform boxes, generous leading) — they missed P3 entirely. | Hostile fixtures exist after Phase 0.2 (`tight-leading.pdf`, `two-column.pdf`). Run selection/search tests against them, not just `single-page.pdf`. |
| P12 | feature configs | Code/tests gated on `pdfium` rot silently in the other config. | Always run the gate's two clippy + two test invocations. Imports used only by gated tests need `#[cfg(feature = "pdfium")]`. |
| P13 | shell growth | Every UX phase landed its handlers and views directly in `gui/mod.rs`, growing it to 4 838 lines — twice the size of the v2 reducer this rebuild exists to escape. | New handler/view logic goes in a surface module (`gui/<surface>.rs`); `mod.rs` only routes. Frozen-file budget applies (§0.2). Decomposition program: Phase 6.2. |
| P14 | feedback channels | Some handlers report transient outcomes via the status message segment, others via toasts; a test asserting one channel met the other (the red tracker test). | Contract: **toasts carry transient command outcomes**; the status-left segment is for lightweight inline echo only (selection counts, mode hints); position stays right (P7). When moving a feedback path between channels, update its tests in the same change. |

---

## Phase 0 — Hardening the shipped surfaces (do first, in this order)

Rationale: three user-visible selection bugs shipped while the suite was
green. Each sub-phase converts one class of invisible bug into a visible one.

### 0.1 Extract pure paint plans from the PDF canvases

**Goal:** the geometry that `draw()` paints becomes a pure function with unit
tests, so "state says selected but nothing would draw" fails in CI.

Files: `shell/src/gui/pdf_view.rs`, new `shell/src/gui/paint.rs`,
new test `shell/tests/pdf_paint_plan.rs`.

1. In `gui/paint.rs` define toolkit-free types:
   ```rust
   pub struct RectPx { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }
   pub enum Tint { Annotation { color: String, picked: bool }, Selection }
   pub struct TintOp { pub rect: RectPx, pub tint: Tint }
   ```
2. Move the body of `TintCanvas::draw`'s per-page loop into
   `pub(crate) fn tint_plan(session: &PdfSession, viewport: (f32, f32)) -> Vec<TintOp>`
   — same math (`placed_pages`, `zoom`, the `project` closure), no iced types.
   `TintCanvas::draw` becomes: `for op in tint_plan(...) { frame.fill_rectangle(...) }`
   (translate `Tint` → `iced::Color` there; alphas: annotation 0.35, picked
   0.55, selection 0.30 — keep the existing `quad_color` helper).
3. Same treatment for the page strip: `pub(crate) fn page_plan(...) -> (Vec<RectPx> /*sheets*/, Vec<(TileKey, RectPx)> /*tiles*/)`.
4. Tests (no pdfium needed — build a `PdfSession` by hand, set
   `layout = Some(DocLayout::new(vec![(612.0, 792.0); 3], 1.0, 16.0))`):
   - `selection_on_a_visible_page_produces_tint_ops` — plant a
     `PdfSelection` on page 1, scroll so page 1 is visible, assert ≥1
     `Tint::Selection` op and that its rect lies inside the page-1 sheet rect
     from `page_plan`.
   - `selection_scrolled_offscreen_produces_no_ops`.
   - `same_line_selection_single_quad_is_painted` — one-quad selection ⇒
     exactly one selection op (this is the regression the user hit).
   - `picked_annotation_is_distinguishable` — `selected_annotation` set ⇒
     its ops carry `picked: true`.
5. Run the gate. Update handoff.

**Done when:** `draw()` bodies contain no geometry math beyond iterating a
plan, and the four tests above are green.

### 0.2 Hostile glyph fixtures + real-glyph selection tests

Files: `scripts/gen-fixtures.py`, `tests-fixtures/pdf/README.md`,
new test `pdf/tests/selection_real_glyphs.rs`.

1. In `gen-fixtures.py` add (copy the style of `fixture_single_page`):
   - `tight-leading.pdf` — one page, 6 lines, font size 12 with **leading 12**
     (`text_stream(..., size=12, leading=12)`), so loose boxes overlap.
   - `two-column.pdf` — one page, two `text_stream` calls at x=72 and x=320,
     same y values, 4 lines each.
   Regenerate (`python3 scripts/gen-fixtures.py`), update the README table,
   commit the new PDFs.
2. Tests (pdfium-gated, copy the `renderer()` helper from
   `pdf/tests/fts_bridge.rs`): for each of `single-page.pdf`,
   `tight-leading.pdf`, `two-column.pdf`:
   - `page_chars` non-empty; every box satisfies `x0<x1`, `y0<=y1` and is
     within page bounds ±1pt.
   - **Same-line drag:** take the glyphs of the *third* text line (group with
     `select::select` over a whole-line drag first to find its band), drag
     from the center of its first glyph to the center of its last glyph;
     assert the selected text equals that line's text and `quads.len() == 1`
     and the quad's vertical band contains those glyphs (not line 1's).
   - For `two-column.pdf`: drag inside column 2 only; assert no column-1
     text in the result.
3. Run the gate.

**Done when:** the user's "same-line drag" bug is reproduced by a fixture
that fails on the *old* `joins_line` (verify by temporarily reverting it —
then restore) and passes on the current one.

### 0.3 Rotated-pages audit

File: `pdf/tests/selection_real_glyphs.rs` (extend), possibly
`pdf/src/render.rs`.

`page_chars` flips y with `page.height()`. For `/Rotate 90/270` pdfium
reports post-rotation dimensions, but glyph boxes may not match the flip.
1. Write the test first: for each page of `rotated-pages.pdf`, run the
   bounds-check + whole-page-drag assertions from 0.2. 
2. If red: the fix belongs in `render.rs::page_chars` only (P5 — flip at the
   boundary). Likely shape: use `page.width()`/`page.height()` *after*
   rotation consistently for both the box flip and `page_size` (tiles already
   use the same source, so selection and paint stay aligned).
3. If green: record in the handoff decision log that rotation is covered.

### 0.4 Manual smoke checklist (`docs/SMOKE.md`)

Toolkit-level bugs (P1) are invisible to every automated layer we have.
Create `docs/SMOKE.md` with this exact list; run it (~5 min) whenever a
GUI surface changes, and extend it with every new feature:

```
Run: cd v3 && cargo run -p md-shell --features pdfium -- <a real vault with a real-world PDF>
 1. Quick-open (ctrl+p) a .md file; type; undo (ctrl+z); redo; save (ctrl+s) — dirty dot clears.
 2. Split (ctrl+\), open a PDF in the right pane. Both panes render.
 3. ctrl+z in the PDF pane opens zoom input (NOT editor undo); ctrl+z in the md pane undoes (Bug A check).
 4. PDF: mouse wheel scrolls; pgup/pgdn; ctrl+g jumps; zoom 150% re-renders crisply (no blur > a beat).
 5. Select text on ONE line mid-page: blue tint appears exactly under the cursor (the recurring bug).
 6. Select across three lines: three per-line tints.
 7. Hover text: I-beam. Hover a highlight: pointer.
 8. ctrl+h: highlight appears (yellow), persists after closing+reopening the tab.
 9. ctrl+n on a picked highlight: note saves; delete removes it.
10. ctrl+f: type a word visible on a later page; enter jumps there and tints it; ctrl+h highlights it.
11. ctrl+t: outline listed; enter jumps; status pill shows "· § <section>".
12. alt+left returns; alt+right re-jumps.
13. Quit and relaunch: layout, focus, PDF page and zoom restored ("resumed at p. N").
14. Status line never sticks on "⌘ command" — it settles to the caret/page pill.
```

Each phase below appends its own lines to this file.

### 0.5 `pdf.find` scale guard

File: `shell/src/gui/mod.rs::open_pdf_find`.

Loading glyphs for *every* page is synchronous; a 500-page document would
freeze the UI for seconds. Until the async worker (Phase 5.1) exists:
1. Cap the eager load at 200 pages: `for page in 0..session.page_count().min(200) as u32`.
2. If `page_count() > 200`, set
   `self.status = format!("find: searching first 200 of {} pages", n)` after
   opening the overlay (mind P7).
3. Test (always-on): construct the overlay path with a fake; assert no panic
   and that the cap math is right (pure helper if needed). Record the cap in
   the handoff as a deliberate stopgap pointing at Phase 5.1.

---

## Phase 1 — PDF internal-reference popup (user item 1)

**Behavior (v2 parity):** right-click an internal link in a PDF → a modal
popup shows the referenced destination (rendered page region); `esc` closes
it. Left-click on a link navigates to the destination (and `alt+left`
returns). URI links: left-click opens in the OS browser, right-click shows
the URI in the status line.

v2 reference implementation (read, do not copy verbatim):
`core/src/infrastructure/pdfium/worker.rs` (`GetLinks`, `RenderLinkPreview`,
destination y-extraction incl. `PdfDestinationViewSettings`),
`native/src/features/pdf/update.rs` (`LinkPreviewResult`, `CloseLinkPreview`).
Fixture: `tests-fixtures/pdf/internal-links.pdf` (page 1 has a /Link → page 2).

### 1.1 Engine: link extraction (`pdf`)

1. In `pdf/src/select.rs` *nothing changes.* Add to `pdf/src/render.rs`:
   ```rust
   /// One link annotation on a page; rect in page points, top-left origin.
   #[derive(Debug, Clone, PartialEq)]
   pub struct LinkBox {
       pub rect: crate::SelRect,
       /// Internal destination: 0-based page + optional y (page points, top-left).
       pub dest: Option<(u32, Option<f32>)>,
       pub uri: Option<String>,
   }

   pub fn page_links(&self, path: &Path, page: u32) -> Result<Vec<LinkBox>, PdfError>
   ```
   Implementation: `pdfium_lock()` first (P4); iterate `page.links().iter()`;
   rect from `link.rect()`, flip y (P5); destination exactly like v2:
   prefer `link.action()` (`PdfAction::Uri` / `PdfAction::LocalDestination`),
   fall back to `link.destination()`; y from `PdfDestinationViewSettings`
   variants (`SpecificCoordinatesAndZoom`, `FitPageHorizontallyToWindow`,
   `FitBoundsHorizontallyToWindow`), flipped with the **destination page's**
   height — note v2 flipped with the *source* page height; that is a v2 bug,
   use `doc.pages().get(dest_page)?.height()`. Skip links with neither dest
   nor uri.
2. Re-export `LinkBox` from `lib.rs` under the feature flag.
3. Fixture test in `render.rs::tests`:
   `internal_links_fixture_yields_a_page_two_destination` — page 0 of
   `internal-links.pdf` has ≥1 link with `dest == Some((1, _))`; rect within
   page bounds; `single-page.pdf` yields an empty list (not an error).

### 1.2 Shell: state + hit test

Files: `shell/src/gui/session.rs`, `pdf_view.rs`, `mod.rs`, `overlay.rs`.

1. `PdfSession` gains `pub links: HashMap<u32, Vec<md_pdf::LinkBox>>`
   (mirror of `chars` — same population pattern). In
   `pdf_view::ensure_tiles`, where chars are loaded per visible page, also
   load links (`load_page_links`, idempotent, gated like `load_page_chars`).
2. `PdfSession::link_at(&self, page: u32, pt: (f32, f32)) -> Option<&LinkBox>`
   — point-in-rect over `links[&page]`, topmost-last like `annotation_at`.
3. `pdf_view.rs` `PdfCanvas::update`: add
   `ButtonPressed(mouse::Button::Right)` → publish new
   `Message::PdfRightClick { tab, pos, viewport }`.
   `mouse_interaction`: check `link_at` **before** the text check → `Pointer`.
4. `mod.rs` `Message::PdfRightClick` handler:
   - `page_at_point` → `link_at`. No link: ignore.
   - `uri` link: `self.status = format!("link: {uri}")`, return (P7).
   - internal link: build the preview (1.3) and open the overlay.
5. Left-click navigation: at the *top* of `pdf_mouse_down`, after computing
   `(page, pt)`: if `link_at` hits an internal link →
   `session.record_jump(); session.go_to_page(dest_page);` plus, when
   `dest_y` is `Some`, add `dest_y * zoom` to the scroll (clamped);
   `ensure_tiles`; status `"→ p. N · alt+left returns"`; **return before**
   annotation picking / selection anchoring. A URI link on left-click:
   `open::that(uri)`? No — do not add a dependency silently; show the URI in
   the status line and leave opening to Phase 5 backlog (note it).

### 1.3 Shell: the popup itself

1. New overlay variant:
   ```rust
   PdfLinkPreview { page: u32, image: iced::widget::image::Handle, width: u32, height: u32 }
   ```
   `kernel_name()` → `"pdf-link-preview"`, `title()` → `"Reference"`,
   `input_mut` → it has no input: give it an empty-string arm by storing a
   dead `String` OR (cleaner) change `input_mut` to return
   `Option<&mut String>` — prefer the latter; fix the two call sites
   (`overlay_raw_input` backspace/typing arms become no-ops on `None`).
2. Rendering the preview image: in the handler, call
   `renderer.render_page(abs, dest_page, scale)` (already exists) with
   `scale = (520.0 / page_width_pts).min(2.0)`; build
   `image::Handle::from_rgba`. If `dest_y` is `Some`, no cropping in v1 —
   the whole page renders and `esc` closes; cropping to a band around
   `dest_y` (v2's `center_ratio`) is a noted refinement.
3. `overlay::view`: the current card assumes input+rows. Add a match arm
   *before* building `input_line`: for `PdfLinkPreview`, return a centered
   `container(iced::widget::image(handle).width(540))` card with the same
   border style, title `Reference — p. N`, and a hint line `esc closes`.
4. `esc` already routes to `overlay.close` (modal fence). Enter
   (`overlay.confirm`) should **navigate**: `record_jump` + `go_to_page` like
   1.2.5, then close — "peek, then commit".
5. Append to `SMOKE.md`:
   `15. internal-links.pdf: right-click the link → popup shows page 2; esc closes; left-click navigates; alt+left returns.`

### 1.4 Tests (`shell/tests/pdf_links.rs`, copy the harness from `pdf_toc.rs`)

- Always-on: `right_click_on_nothing_is_inert` (fake pdf; PdfRightClick →
  no overlay, no panic).
- pdfium-gated, fixture `internal-links.pdf`:
  - `right_click_on_an_internal_link_opens_the_preview_and_esc_closes` —
    find the link rect via the engine, project its center to viewport px
    (use `placed_pages` like `annotations_wiring.rs` does), send
    `PdfRightClick`, assert `Overlay::PdfLinkPreview { page: 1, .. }`, press
    `escape`, overlay gone.
  - `left_click_navigates_and_alt_left_returns` — click center of link rect,
    assert `current_page() == 1`, `alt+left` → page 0.
  - `enter_in_preview_navigates`.

**Done when:** gate + smoke item 15 pass.

---

## Phase 2 — Vault file-browser left panel (user item 2)

**Behavior:** `ctrl+b` toggles a left panel showing the vault tree (folders
collapsible, files clickable). Clicking a file opens it in the focused pane
(same path as quick-open). State (open/closed, expanded dirs) survives
restart. v2 reference: `native/src/views/sidebar.rs` (derives the tree from a
flat path list — reuse that idea).

### 2.1 Data

1. `scan_vault` (in `gui/mod.rs`) already returns sorted vault-relative file
   paths and skips dotted entries. Derive the tree *in the view* from this
   flat list (v2's approach, ~60 lines): immediate children of a prefix =
   unique first segments; dirs sort before files, case-insensitive.
   Implement as a pure function in a new `shell/src/gui/file_tree.rs`:
   ```rust
   pub struct TreeRow { pub label: String, pub rel_path: String, pub is_dir: bool, pub depth: u16 }
   /// Flatten the visible portion of the tree given the expanded set.
   pub fn visible_rows(files: &[String], expanded: &BTreeSet<String>) -> Vec<TreeRow>
   ```
   Unit-test this pure function directly (no GUI): nesting, expansion,
   ordering, empty vault.
2. Shell state: `tree_open: bool`, `tree_expanded: BTreeSet<String>`;
   refresh `self.files = scan_vault(&self.vault_root)` whenever the panel
   opens and after `editor.save` / `open_document` (cheap).

### 2.2 Commands and messages

1. `defaults.rs`: `spec("workspace.toggle-files", "Toggle File Panel",
   "Workspace", vec![bind(workspace_scope, Chord::ctrl('b'), …)])` — check
   `SHORTCUTS.md` first; as of writing `ctrl+b` is free. Scope: the same
   scope `workspace.split-right` uses (panel is workspace chrome, must work
   from md *and* pdf focus — verify which scope that binding uses and match it).
2. Messages: `TreeFileClicked(String)`, `TreeDirToggled(String)`.
   Handlers: file → `self.open_document(&rel)`; dir → toggle in
   `tree_expanded`, `save_session()`.
3. `run_command` arm flips `tree_open` and calls `save_session()`.

### 2.3 View

In `Shell::view`, wrap the workspace: when `tree_open`,
`row![ panel, workspace_column ]` where `panel` is a
`scrollable(column(rows))` inside a `container` with `width(240)`, themed
like the overlay card. Each row: `mouse_area(text(...))` with indent
`"  ".repeat(depth)` and `▸/▾` markers for dirs, `on_press` mapping to the
messages above. Highlight the focused document's row (compare to the focused
tab's `rel_path`).

### 2.4 Persistence

`SessionSnapshot` (in `gui/snapshot.rs`) gains
```rust
#[serde(default)] pub tree_open: bool,
#[serde(default)] pub tree_expanded: Vec<String>,
```
(`#[serde(default)]` keeps old snapshots loadable — restore must degrade,
never refuse). Capture/restore alongside the existing fields.

### 2.5 Tests (`shell/tests/file_tree.rs`)

- Pure: `visible_rows` cases (see 2.1).
- Windowless: `ctrl_b_toggles_and_persists` (toggle, drop shell, new shell →
  still open); `clicking_a_file_row_opens_it_in_the_focused_pane` (send
  `TreeFileClicked`, assert workspace has the tab — copy assertions from
  `shell_key_routing.rs`); `ctrl_b_works_from_pdf_focus` (BUG-A style check).
- Smoke item 16: panel toggles, folders expand, click opens, state survives
  relaunch.

---

## Phase 3 — Theme tokens on v2's palette + branding (user items 4 & 5)

### 3.1 Token system (plan M2 "theme system on tokens")

1. New `shell/src/gui/tokens.rs`:
   ```rust
   pub struct Tokens {
       pub bg_primary: Color, pub bg_secondary: Color, pub bg_tertiary: Color,
       pub bg_surface: Color, pub border: Color, pub border_subtle: Color,
       pub text_primary: Color, pub text_secondary: Color, pub text_muted: Color,
       pub accent: Color, pub accent_secondary: Color,
       pub danger: Color, pub success: Color, pub warning: Color,
       pub sel_tint: Color, pub highlight_default: Color,
   }
   pub fn dark() -> &'static Tokens
   ```
   Values = v2's premium dark theme (`native/src/theme.rs`), the user's
   preferred design:
   bg_primary `#0d0e10`, bg_secondary `#181a1d`, bg_tertiary `#23262b`,
   bg_surface `#334b47`, border `#45484e`, border_subtle `#1d2024`,
   text_primary `#e3e5ed`, text_secondary `#a9abb2`, text_muted `#9d9ea3`,
   accent `#b1ccc6`, accent_secondary `#cde8e2`, danger `#ee7d77`,
   success `#d9f2d2`, warning `#bfdad4`. Selection tint: accent at α 0.30
   (replaces the hardcoded blue in the tint plan); highlight_default stays
   `#ffd866` (annotation color compatibility — stored per annotation).
2. Migrate consumers: `editor_canvas.rs::palette` (BG/TEXT/MARKER/HEADING…)
   becomes thin aliases reading `tokens::dark()` so call sites stay valid;
   overlay card colors, tab strip, status bar, file panel likewise. Grep for
   `Color::from_rgb` in `shell/src` — after this phase the only literals
   left should be in `tokens.rs`.
3. `Shell::theme` builds `iced::Theme::custom` from the tokens (background =
   bg_primary, text = text_primary, primary = accent) so stock widgets match.
4. Light/high-contrast themes and a settings toggle are **not** in scope —
   the token struct makes them a follow-up.
5. Smoke item 17: visually compare against v2 (`images/*.png` screenshots) —
   panels/overlays/status read as the same family.

### 3.2 Window icon + desktop entry (user item 5)

1. Icon: in `run()` (`gui/mod.rs`), window settings gain
   `icon: iced::window::icon::from_file_data(include_bytes!("../../../../md-editor.png"), None).ok()`
   (path from `shell/src/gui/` to the repo root — verify with
   `include_bytes!` compile error if wrong; the file is `md-editor.png` at
   repo root). Requires iced's `image` feature — already enabled.
2. Desktop entry installer: port `native/src/platform/desktop_integration.rs`
   to `shell/src/desktop.rs`. Keep its structure: functions take the home
   dir as a parameter (`install_with_home(home: &Path)`) so tests run against
   a tempdir; embed the icon with `include_bytes!`; write
   `~/.local/share/applications/md-editor.desktop` (Exec=md-shell %f, Icon=md-editor)
   and the hicolor icon. CLI: `--install-desktop` / `--uninstall-desktop` in
   `main.rs` next to `--demo`. Linux-only: on other OSes print "not supported"
   and exit 0. Typed errors (no `String`).
3. Tests: `desktop.rs` unit tests over a tempdir (files created, uninstall
   removes them, second install is idempotent).

---

## Phase 4 — The bespoke study tracker (user item 3) — **ask first**

v2's tracker is not a generic timer: it is a phases/projects/gates/reading
plan board driven by a JSON config (`TrackerConfig { PHASES, PROJECTS,
GATES, READING }`) plus a session log (`StudySession { date, hours,
activity_type, phase, notes }`) and a KV store (`TrackerKv`) — see
`core/src/domain/session.rs`, `core/src/tracker.rs`,
`core/src/database/tracker_repository.rs`, UI in
`native/src/views/tracker.rs` (~1200 lines), screenshot
`images/study_tracker.png`.

**Before implementing, ask the user (do not guess):**
1. Which v2 tracker views are load-bearing: the session log + add/edit form?
   the phase/project board? gates? the reading list? all four?
2. Should the config stay a user-editable JSON (and where — `<vault>/.md-editor/tracker.json`?),
   or become UI-managed?
3. Is the tracker per-vault or global (v2 was app-global)?
4. Open as a *document tab* (a peer pane, like the plan's tracker-as-engine
   diagram suggests) or as a dedicated panel/overlay?

**Implementation sketch once answered** (sized for the likely answers):
1. Storage in the vault sidecar: new migrations component `"tracker"` with
   `study_sessions` and `tracker_kv` tables mirroring v2's repository;
   `vault/src/tracker.rs` service with typed errors; CRUD + `total_hours()`.
   Port v2's repository tests.
2. Surface: if "document tab", the kernel already has spare `EditorKind`
   variants (`Image`, `Graph`) — add `Tracker`, open via command
   `tracker.open` (palette, no chord), render a form/list view in a new
   `gui/tracker_view.rs`; sessions list + add-entry row first, board second.
3. The JSON config parses with serde into the v2 shapes (copy the structs);
   missing file = empty config, never an error.
4. Windowless tests: add/edit/delete session round-trips the sidecar;
   `tracker.open` from md and pdf focus (BUG-C: documents are peers).

---

## Phase 5 — Backlog (after the parity items, roughly in this order)

Each is independently shippable; specs are intentionally shorter — expand
into the handoff before starting one.

1. ✅ **Async tile + glyph worker.** Move `render_tile`/`page_chars`/
   `page_links` calls off the update thread: one worker thread owning a
   channel of engine `TileKey`/page requests (the `RenderQueue` cancellation
   semantics already fit), results returned via
   `Task::perform`/`Subscription`. Removes the 0.5 cap and the first-scroll
   hitch on huge PDFs. Gate: scrolling a 500-page doc (CI's `--large`
   fixture) never blocks input > 16 ms (manual check + a timing assertion in
   a worker unit test).
2. ✅ **Backlinks overlay** (plan §3.4 — service exists in `vault/src/links.rs`):
   command `note.backlinks` lists referrers of the focused note via
   `LinkGraph`; reuse the overlay list pattern; enter opens the referrer.
3. ✅ **Annotation niceties** (handoff item 7 leftovers): color cycling
   (`pdf.highlight-color` rotating a small token palette, stored per
   annotation — schema already has `color`), copy-selection-to-clipboard
   (`ctrl+c` in pdf scope via `iced::clipboard::write`), linked-note creation
   (`pdf.annotation-link-note`: create `<stem>-notes.md`, store
   `linked_note`, open it), orphan report (palette command listing
   `known_documents` rows whose hash no longer matches any vault file).
4. ✅ **Editing ergonomics bundle** (plan §3.2/M3) — engine-side, one PR each,
   all property-tested in `editor`: auto-pairs; smart list continuation +
   renumbering; checkbox toggle (`ctrl+enter`); table cell `tab` navigation +
   reflow; smart paste (URL over selection → link); heading cycle
   (`ctrl+1..6`). Every one is a `Command` through the bus, undo-coalescing
   rules decided per command (see `undo.rs` doc comments).
5. ✅ **Whitespace-elastic `pdf.find`** (P6): in `select::find`, treat any
   whitespace run in the needle as matching ≥0 whitespace/line-break gap in
   the stream; pure change + synthetic tests; removes the multi-word limit.
6. ✅ **Settings UI surface** (plan M2) — render `keymap.json` + theme choice;
   low urgency, the files work today.
7. ✅ **URI links open in browser** (from 1.2.5) — decide on the `open` crate
   (new dependency: record an ADR-style decision-log entry) or print-only.
   Landed with the ADR (0102) written *after* the code — the §0.2 ADR-first
   rule exists so this is not repeated.

---

## Phase 6 — Course correction: restore the bar (2026-06-12; do in this order)

**Why this phase exists.** Phases 1–5 and the UX overhaul landed real features,
but the quality bar drifted while they did: `gui/mod.rs` grew to 4 838 lines
(the god-file disease the master plan's §1 names as the reason v3 exists),
the workspace was handed off with a known-red test and a paragraph explaining
it, renderer features shipped with paint-only geometry and "approximate"
hit-testing, a dependency ADR was written after the code, and theme state went
into a global atomic. None of this is rolled back — the features stay — but
**no new feature work happens on the markdown renderer or the shell until
6.0–6.3 are done.**

**Re-sequenced 2026-06-13:** the user ordered a Typora-grade live editing
experience (Phase 7). Phases 6.4 and 6.5 are **superseded** by Phase 7.2 and
7.5 — doing typographic hierarchy and exact hit-testing on the monospace
grid, then redoing them on shaped text, would be the same work twice. After
6.3, go straight to Phase 7.0. Phase 6.6 keeps only its non-renderer items.

Every sub-phase below ends with the §0.3 gate and a handoff row, like any
other unit of work. Sub-phases are sized for one session each.

### 6.0 Make the tree green and committed (do first, small)

1. **Fix the red tracker test** — the decision is made, do not re-litigate:
   transient command outcomes are **toasts** (P14;
   `UX_OVERHAUL_PLAN.md` §6.1). `gui/mod.rs:1311` still writes
   `self.status = "tracker: session logged manually"` while the path the test
   exercises reports via toast; unify them:
   - the manual-log handler reports exactly once, via
     `self.success_toast("tracker: session logged manually")`; delete the
     status write (position pill untouched — P7).
   - expose toasts to tests: `pub fn toasts(&self) -> &[Toast]` on `Shell`
     (and public read access to `Toast::message`/`kind` if not already).
   - `shell/tests/tracker_wiring.rs::tracker_manual_log_and_delete` asserts
     the toast message instead of `shell.status()`.
2. Run the full gate (§0.3) — both feature configs.
3. **Commit.** The working tree carries ~3 200 uncommitted lines across 27
   files; the handoff snapshot currently describes an unreachable state.
   Commit first, then update the snapshot (§0.2 truthfulness rule).

**Done when:** gate fully green, zero known-failing tests, tree committed,
handoff snapshot re-taken from the commit.

### 6.1 v3 size budgets in CI

Port v2's ratchet idea (`scripts/check-budget.sh` + `budgets.toml`) to v3:

1. `budgets.toml` — `[file_budgets]` mapping the frozen files (§0.2) to
   their current line counts; a global `hard_limit = 700` for everything
   else.
2. `scripts/size-budget.sh` — fails when an unlisted `v3/**/*.rs` file exceeds
   the hard limit or a listed file exceeds its ceiling; prints the offender.
   When you shrink a frozen file, lower its ceiling in the same PR.
3. Wire into the `v3` job in `.github/workflows/quality.yml` next to the
   shortcuts-freshness check.
4. Prove the script fires by injecting a violation and watching it fail
   (the `ARCHITECTURE_RULES.md` verification practice), then remove the
   injection.

### 6.2 Decompose `gui/mod.rs` (mechanical, behavior-frozen)

**Completed, 2026-06-13.** Landed modules:
`toast.rs`, `status.rs`, `session_persist.rs`, `commands_file.rs`,
`commands_md.rs`, `commands_pdf.rs`, `commands_pdf_annotations.rs`,
`commands_pdf_nav.rs`, `commands_settings.rs`, `chrome.rs`,
`pdf_input.rs`, `pdf_worker_events.rs`, `stores.rs`, `input.rs`,
`chrome_context.rs`, and `chrome_panels.rs`. All are ≤700 lines.
`gui/mod.rs` is 1,479. Buffer formatting/editing moved to
`editor/src/buffer/formatting.rs`, `buffer/edit_ops.rs`, and
`buffer/typing.rs`, reducing `buffer.rs` to 833.

Target shape — names may flex to what the code wants, sizes may not
(each new file ≤ 700):

```
gui/mod.rs            Shell struct, update() routing, view() assembly only
gui/commands_file.rs  file.* / vault.* / workspace.* handlers
gui/commands_md.rs    editor.* handlers incl. formatting dispatch
gui/commands_pdf.rs   pdf.* handlers
gui/chrome.rs         menu bar, tab strip, pane scaffolding views
gui/toast.rs          Toast type, queue, view_toasts
gui/status.rs         two-segment status bar
```

Rules of engagement:
- **Pure moves.** Use `impl Shell { … }` blocks in the new files (Rust allows
  inherent impls split across files in one crate) so call sites do not churn.
  No behavior edits in a move commit — if you spot a bug while moving, note
  it in the handoff and fix it in a separate commit after the move lands.
- One extraction = one commit; the routing suites must be green after each;
  lower the `mod.rs` ceiling in `budgets.toml` in the same commit.
- Same treatment for `editor/src/buffer.rs` (1 911): the ergonomics
  operations (auto-pairs, list continuation/renumbering, table nav, heading
  set/cycle) move to `editor/src/edit_ops.rs` built on the buffer's public
  command surface; their tests move with them.

**Done when:** `gui/mod.rs` ≤ 1 500 lines (routing + state only), every
extracted file ≤ 700, all ceilings lowered, suites green throughout.

### 6.3 Golden draw-plan snapshots for the markdown renderer

The renderer's quality gaps shipped invisibly because nothing in CI sees what
it paints (P10). Mirror PDF Phase 0.1 — pure plans, then a golden corpus:

1. Extract the geometry from `editor_canvas.rs` paint functions into a pure
   plan: `pub(crate) fn line_plan(…) -> Vec<PaintOp>` where `PaintOp` is a
   toolkit-free enum (text run: content/x/y/size/color-role; rect; asset
   placement: kind/rect). `paint_line`/`paint_block_asset`/
   `paint_inline_preview` become iteration over ops. This also starts paying
   the 755-line budget down.
2. Fixture document (checked in under `shell/tests/fixtures/golden.md`):
   h1–h3 headings, plain + wrapped paragraph, bullet list with checkbox,
   table, fenced code, inline math, multi-line display math, an image
   reference, a wikilink — plus the caret parked on a styled line so one line
   is in revealed state.
3. `shell/tests/editor_draw_plan.rs`: render the plan for the whole
   fixture at a fixed wrap width, serialize ops line-by-line to text, compare
   against a checked-in snapshot file with plain `assert_eq!` (no `insta` —
   matches the repo's self-verifying-suite philosophy). A renderer change ⇒
   a reviewable snapshot diff in the PR.
4. Keep the BUG-B golden gate honest: caret enter/leave on the fixture doc
   still damages ≤ 2 lines.

**Done when:** paint fns contain no geometry beyond iterating plans; the
snapshot exists and a deliberate one-token style change produces exactly the
expected diff (verify once, revert).

### 6.4 ~~Typographic hierarchy through the measure phase~~ — superseded

Superseded 2026-06-13 by **Phase 7.2**: heading scale built on the monospace
grid would be redone immediately after the shaped-text swap (7.0). The
principle stands and moved with it — hierarchy is *measurement*, never paint
decoration.

### 6.5 ~~Exact click-to-caret on rendered lines~~ — superseded

Superseded 2026-06-13 by **Phase 7.5**: the round-trip property test is
specified there, against the shaped layout that paint and hit-testing will
share. "Approximate" still ends — it ends on the right substrate.

### 6.6 Non-renderer polish ✅

- ~~**Theme state off the global atomic**~~ done (2026-06-13). Theme choice
  is a `Shell` field persisted in the snapshot; views and canvases receive
  immutable token references. `tokens::set_light_theme` and
  `USE_LIGHT_THEME` are deleted. `session_restore` proves two shells can
  hold different themes concurrently and the chosen theme round-trips.
- Moved to Phase 7: tables as measured cells (7.3), list hanging indent
  (7.2), interactive checkboxes (7.5), async assets (7.4), proportional
  font (7.0 — now mandatory, not a spike option), CJK/emoji/bidi (7.6),
  p95 keypress bench (7.6).

## Phase 7 — Typora-grade live editor (user-ordered 2026-06-13)

**The order.** The user's verdict on the live markdown editor: the rendered
artifacts are clanky, and the editing experience must be comparable to
Typora. This is a product bar, not a polish list. Concretely, Typora-grade
means:

- prose in a proportional reading font with real text shaping; headings
  visibly larger; comfortable vertical rhythm;
- syntax markers **fully hidden** when the caret is elsewhere — no
  reserved-width gaps in the middle of words;
- moving the caret into a line/block reveals its source *in place*,
  smoothly; leaving re-renders it — and layout never corrupts during the
  transition;
- tables, fenced code, display math, and images render as real blocks;
  caret inside a block shows that block's source;
- assets never make the layout jump; checkboxes and links are clickable;
- typing stays sacred: p95 keypress→frame < 8 ms (master plan pillar 1)
  through all of it.

**Prerequisites:** Phase 6.0–6.3 complete. The golden draw-plan corpus (6.3)
is the safety net for this rebuild — every Phase 7 step regenerates it
deliberately, and the diff is the review. The §0.2 hard rules apply to every
step, especially measure-phase ownership and the size budgets.

**Why this is a program, not a polish pass.** Two of the renderer's founding
contracts are structurally incapable of the Typora bar, and each gets
re-decided by ADR *before* code (§0.2 rule):

1. The shell measures on a **monospace column grid** (`MonoMeasurer`,
   `wrap_columns`; `editor_canvas.rs` says it itself: "columns advance,
   pixels don't"). No grid, no Typora typography. → ADR-0104, Phase 7.0.
2. Conceal is **reserved-width** (markers keep their columns, painted or
   not — master plan §3.2). That guarantees layout stability but reads as
   gaps punched into prose. → ADR-0105, Phase 7.1.

What does *not* change: the three-phase layout protocol, the height
sum-tree, the damage contract, the Styler/Measurer seam, the parser, the
buffer, the kernel. The seams were built for exactly this swap — use them.
If a step seems to require bypassing a seam, stop and re-read §0.2.

### 7.0 Shaped text measurement ✅ (ADR-0104; replaces the mono grid)

ADR-0104 is drafted as **proposed** with the candidates and criteria; the
spike resolves it to accepted with measured numbers.

1. **Time-boxed spike (≤ 2 days):** measure + paint + hit-test one
   paragraph and one heading with both candidates:
   (a) `cosmic-text` directly (it is iced's own shaper — pin the same
   version iced resolves, check `v3/Cargo.lock`);
   (b) iced's `advanced::text::Paragraph` API.
   Selection criteria, in order: one geometry source usable by measure,
   paint, *and* hit-test (no parallel math — that is how "approximate"
   happened); cost of measuring a 100k-line document (if a full pass is too
   slow, an estimate-then-refine strategy is acceptable **only** if refined
   heights flow through the normal `Damage`/height-tree path and converge);
   grapheme-cluster and IME behavior. Default if tied: (a), because the
   measurer must be constructible without an iced renderer in windowless
   tests.
2. `ShapedMeasurer` in `shell/src/gui/` implements the engine's
   `Measurer` trait: visual rows + per-row heights from shaped runs, plus
   `caret_to_point` / `point_to_caret` on the same shaped layout, exposed
   for paint plans and hit-testing.
3. `MonoMeasurer` survives **only** as a test measurer for engine-level
   suites (cheap, deterministic). Shell tests run `ShapedMeasurer` with an
   embedded test font so geometry is byte-reproducible in CI — never the
   host's font stack.
4. Wire it through `EditorDocument` (the `Measurer` is already injected);
   regenerate the 6.3 golden snapshot once; record p95 keypress and
   open-document timings for the large fixture in the handoff row.

**Done when:** the editor paints shaped proportional text; BUG-B suite
green; hit-tests resolve through the shaped layout; timings recorded. (Done: 13.1s for 100k lines, 192µs p95 keypress).

### 7.1 Conceal v2 — true hide, measured reveal ✅ (ADR-0105; Done)

ADR-0105 (accepted — the direction is user-ordered) retires reserved-width:

1. Conceal state becomes a **measure input**, not a paint trick. A
   concealed line is styled *and measured* without its marker glyphs; when
   the caret enters, the line is re-styled and re-measured in revealed
   form; the height tree shifts subsequent offsets; paint damage = the
   revealed line + the shifted region.
2. `Styler::layout_stable()` is retired. Its replacement invariant: every
   conceal transition goes through remeasure *before* paint (debug-assert
   in `set_conceal` / `EditorDocument::apply`, the same place the old
   assert lived).
3. **BUG-B gate v2** (update `editor/tests/bug_b_layout_reflow.rs`): the
   contract was never "geometry must not change" — it is "offsets are never
   stale, content never overlaps". Assert: (a) styled-damage from a caret
   move ≤ 2 lines, plus a correct `shifted_from`; (b) after any transition,
   offsets equal a from-scratch layout of the same content+conceal state
   (differential); (c) a caret-motion storm (random walks over the golden
   fixture, 8 seeds) never produces overlap or a stale offset.
4. Reveal granularity v1 is the **line** (and the **block** for block
   constructs, 7.3). Element-level reveal — only the span under the caret
   shows its markers, Typora's exact behavior — is 7.6 refinement, built on
   the same mechanism (a reveal-set is just a finer style key).

**Done when:** no reserved gaps anywhere; the storm test is green; the
golden diff shows concealed lines tightening (review it consciously). (Done: all tests pass, golden snapshot updated and verified).

### 7.2 Reading typography & rhythm ✅ (subsumes old 6.4)

On shaped text, set the type system once, in tokens/metrics — not per
widget:

1. Per-`LineKind` scale table (h1–h6, body, code, quote) and spacing rhythm
   (paragraph spacing, blockquote bar + indent, list hang-indent from real
   glyph advance, hr). Final values come from a side-by-side review against
   Typora's defaults — match the *feel*, not necessarily the pixels.
2. Heading-edit reflow test: editing a paragraph into `# heading` shifts
   subsequent offsets through the height tree.
3. Regenerate goldens deliberately.

### 7.3 Blocks render as units (implemented; manual smoke pending)

The parser's block states already group lines; expose
`EditorDocument::reveal_range(caret) -> Range<usize>` (engine, tested) so
the reveal unit for block constructs is the block:

1. **Fenced code:** background panel + monospace runs first (structure
   now); syntax highlighting is a separate step behind its own dependency
   ADR (default candidate `syntect`) — do not fold a highlighter in
   silently.
2. **Tables:** real measured columns (widths from shaped cell content),
   padded cells, header rule. Caret inside ⇒ the whole table reveals as
   source.
3. **Display math / images:** already render via the TeX renderer and asset
   pipeline; move them onto the same block-reveal contract (caret inside a
   `$$` block reveals the TeX source in place — today's per-line behavior
   unifies with tables/code).

**Done when:** the golden fixture's table/code/math sections render as
blocks, reveal as blocks, and click-into-block lands the caret at the
nearest source char (hit-test through the shaped layout).

### 7.4 Assets never pop the layout (implemented; manual smoke pending)

1. Asset discovery/render moves off the document-load path onto the
   `gui/worker.rs` pattern (one queue, results routed by path).
2. Last-known asset dimensions are cached in the sidecar (new migrations
   component `asset_sizes`, append-only ladder as always) so a reopened
   document measures correctly *immediately*; the async result refines
   through the normal remeasure path — when the cached size still matches,
   zero visual movement.
3. Tests: reopen-with-cache ⇒ no second reflow; a genuinely resized image ⇒
   one reflow through `Damage`, offsets correct (differential check).

### 7.5 Interaction exactness (implemented; manual smoke pending)

1. Round-trip property test (`shell/tests/editor_hit_testing.rs`): for
   every char of the golden fixture, in concealed *and* revealed states:
   plan-position → hit-test → same char. One shared geometry path is what
   makes this pass; if it fails, fix the divergence, never add a fudge
   constant.
2. Clickable checkboxes: click toggles `[ ]`↔`[x]` through the existing
   engine formatting command — a command through the bus, like every
   interaction (mouse-coverage test updates itself).
3. Links/wikilinks: plain click positions the caret (it's an editor);
   **ctrl+click follows** (wikilink → open note; URI → ADR-0102 path).
   Typora's rule; pin it in a test. Hover: I-beam over text, pointer +
   underline over links with ctrl held.

### 7.6 Smoothness & refinements ✅

- ~~Element-level reveal granularity (the full Typora behavior)~~ done
  (2026-06-13). `ConcealMode::Partial(display_range)` + `reveals_at(col)` are
  the finer style key the 7.1 note anticipated. `EditorDocument::conceal_at`
  reveals only the inline element the caret sits in for a lone paragraph
  (clicking an inline `$math$` no longer un-renders an adjacent `**bold**`);
  block constructs and image-bearing paragraphs keep whole-line reveal. The
  styler keeps a marker only when it falls inside the revealed range; paint and
  the measurer ask `reveals_at` per element instead of `== Concealed`. Also
  fixed the trailing-marker caret-placement bug (`display_col_to_source` snaps
  a click at the visual line end to the true source end). Pinned in
  `editor::document` tests + the re-pinned golden draw plan.
- ~~Conceal/caret/scroll motion polish~~ done (2026-06-13). Markdown wheel
  and page scrolling ease over 120 ms; caret and newly revealed marker glyphs
  fade in over 90 ms. Motion ticks exist only while a transition is active.
  Persisted `reduce_motion` shell state disables both and snaps any pending
  scroll to its target. Full old/new layout blending is deliberately excluded:
  retaining stale pre-reflow geometry would violate the one-current-layout
  contract; only glyph alpha transitions.
- ~~Fenced-code syntax highlighting~~ done per ADR-0106. Incremental lexer
  state and semantic roles live in `editor`; shell maps roles to theme
  colors and never parses syntax in the paint path. Geometry-invariance,
  convergence, known-language paint, and unknown-language fallback are
  test-pinned.
- ~~CJK/emoji and mixed-direction bidi round-trip properties on shaped
  runs~~ done in `editor_hit_testing.rs`.
- ~~p95 keypress→frame bench in CI (master plan §6's promise, finally
  honored — a coarse timing assertion in a quiet job beats nothing)~~ done in
  `shell/tests/keypress_bench.rs`: drives `MdSession::apply` over a 5k-line
  document (incremental parse + restyle + shaped remeasure of the touched
  range), asserts p95 < 16ms. Runs in the existing `cargo test --workspace`
  CI step; ~0.4ms locally, generous margin against shared-runner variance.
- ~~Baseline mouse selection and clipboard editing~~ done. Markdown canvas
  drag-selects through shaped hit-testing in either direction and across
  lines; `ctrl+c` preserves selection, `ctrl+x` deletes through
  `EditorCommand` and remains undoable. Windowless shell regressions cover
  forward/reverse multi-line selection plus copy/cut/undo.
- ~~Broad legibility pass~~ done (automated slice, 2026-06-13). Prose is
  17 px on a 27 px baseline; Markdown uses a centered reading column capped at
  840 px with 28 px minimum margins instead of stretching across wide panes.
  Blockquotes gain a quiet bar and 22 px hanging inset. Measure, paint, caret,
  selection, hit-test, tables, and assets share the same content bounds.
  Wide-pane centering and quote geometry are test-pinned; golden regenerated.
  Real-vault visual review remains in smoke item 27.

### Typora-parity acceptance checklist

Run after each sub-phase; all must hold by the end of 7.6. Append these to
`docs/SMOKE.md` as their sub-phases land:

```
27. Open a real note: headings are visibly larger, prose is proportional, no gaps where ** or # hide.
28. Click into a bold word: markers appear in place; the line reflows only itself; click away: they vanish.
29. Caret-walk an entire document end to end: no overlap, no jumping, no stale lines (the Bug-B feel test).
30. Table renders as a grid; click inside: source appears; edit a cell; click away: grid re-renders.
31. Fenced code shows as a mono block; display math renders; click into math: TeX source in place.
32. Reopen a document with images: layout is identical instantly (no pop when assets load).
33. Click a checkbox: it toggles (undo undoes it). Ctrl+click a wikilink: the note opens.
34. Type fast in a 5k-line document: zero perceptible lag, undo always undoes.
35. Drag-select Markdown forward and backward across lines; ctrl+c copies; ctrl+x cuts; ctrl+z restores.
36. Toggle Reduced motion in Settings; Markdown scroll/reveal motion becomes immediate and stays disabled after restart.
37. Widen the Markdown pane past 1200 px: prose remains centered in a readable column; headings and quotes retain clear hierarchy.
```

## Appendix A — Test harness recipes

**Windowless shell test** (the standard pattern — copy from
`shell/tests/pdf_toc.rs`): build `Shell::new(default_registry()?,
registry.keymap()?, tempdir)`, drive with `shell.update(Message::Key(...))`
via the `press`/`type_text` helpers, assert on `shell.status()`,
`shell.overlay()`, `shell.focused_pdf()`, `shell.workspace()`. Fixtures are
copied into the tempdir vault. pdfium tests skip when the library is absent.

**Engine geometry test:** synthetic `CharBox` grids via the `line_of` helper
in `pdf/src/select.rs::tests` — pin semantics there first, then confirm
against real fixtures (Phase 0.2 suite).

**Sidecar test:** open stores on `<tempdir>/.md-editor/sidecar.db`; components
cohabit (annotations + sessions + index); never share a connection across
threads.

## Appendix B — Updating the ledger

After each phase: add a status-board row to `HANDOFF.md` (file paths +
test names in the "Where" column), append decision-log entries (dated,
newest last) for anything a future agent would otherwise re-litigate, and
refresh the verification snapshot (test counts from the gate run).
