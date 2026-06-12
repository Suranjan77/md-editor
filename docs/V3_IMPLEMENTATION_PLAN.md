# V3 Implementation Plan — remaining work, step by step

> Audience: an agent (or human) who has **not** read the rest of this repo.
> Follow phases in order. Phase 0 exists because completed features kept
> shipping with paint-level bugs the test suite could not see; do not skip it.
> Companion documents: `docs/V3_HANDOFF.md` (execution ledger — update it
> after every phase), `docs/V3_GROUND_UP_PLAN.md` (the master plan, cited as
> "plan §…"), `docs/V3_SHORTCUTS.md` (generated — never edit by hand).

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
- Every user action is a registered command (`v3/kernel/src/defaults.rs`).
  New command = new `spec(...)` entry there + handler arm in
  `gui/mod.rs::run_command` + regenerate the shortcuts doc (see gate below).
- New SQLite tables go through `vault::migrations` with a **new** component
  name or a new numbered step. Never edit an existing migration (a
  fingerprint test will fail if you do).
- The decision log in `V3_HANDOFF.md` is append-only, newest last.

### 0.3 The verification gate (run after every unit of work)

```bash
cd v3
cargo fmt --all                                            # then commit no diff
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features md3-shell/pdfium -- -D warnings
cargo test  --workspace
cargo test  --workspace --features md3-shell/pdfium
cargo run -p md3-shell -- --demo                           # must end all-ok
cargo run -q -p md3-shell -- --dump-shortcuts > ../docs/V3_SHORTCUTS.md
git diff --exit-code ../docs/V3_SHORTCUTS.md               # fresh ⇒ no diff (commit it if you added commands)
```

pdfium-gated tests must **skip, not fail** when `libpdfium` is absent
(pattern: `let Some(renderer) = renderer() else { eprintln!("skipping…"); return; }`).

A task is *done* when: the gate passes, new behavior has tests at the lowest
layer that can express it, the manual smoke checklist (`docs/V3_SMOKE.md`,
created in Phase 0) passes for touched surfaces, and `V3_HANDOFF.md` has a
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
| P7 | status line | `run_command` ends with `sync_status()`, which **overwrites** `self.status` with the caret/page pill. | A handler whose message must survive (guidance, "no matches") ends with `return Task::none();` before the fallthrough. Grep `"pdf.find"` arm for the pattern. |
| P8 | borrows | `self.focused_pdf_mut()` borrows all of `self`; touching `self.status`/`self.vault_root` while it lives won't compile. | Clone `let root = self.vault_root.clone();` *before* taking the session; write `self.status` *after* the last session use. |
| P9 | keymap | A chord claimed by any reachable scope never falls through to raw input. Raw-input matches in `pdf_raw_input`/`editor_raw_input` ignore modifiers. | Check `docs/V3_SHORTCUTS.md` before picking a chord. Bind in the narrowest scope that works. Overlay scope is a modal fence — while an overlay is open only Overlay+Global resolve. |
| P10 | tests vs paint | The windowless suites drive `Shell::update` and never call `draw()`. They **cannot see paint bugs** (P1 was invisible to 200 green tests). | Paint geometry must live in pure "paint plan" functions (Phase 0.1) with unit tests; toolkit-level effects are covered by the manual smoke checklist only. |
| P11 | fixtures | The generated fixtures are *friendly* (uniform boxes, generous leading) — they missed P3 entirely. | Hostile fixtures exist after Phase 0.2 (`tight-leading.pdf`, `two-column.pdf`). Run selection/search tests against them, not just `single-page.pdf`. |
| P12 | feature configs | Code/tests gated on `pdfium` rot silently in the other config. | Always run the gate's two clippy + two test invocations. Imports used only by gated tests need `#[cfg(feature = "pdfium")]`. |

---

## Phase 0 — Hardening the shipped surfaces (do first, in this order)

Rationale: three user-visible selection bugs shipped while the suite was
green. Each sub-phase converts one class of invisible bug into a visible one.

### 0.1 Extract pure paint plans from the PDF canvases

**Goal:** the geometry that `draw()` paints becomes a pure function with unit
tests, so "state says selected but nothing would draw" fails in CI.

Files: `v3/shell/src/gui/pdf_view.rs`, new `v3/shell/src/gui/paint.rs`,
new test `v3/shell/tests/pdf_paint_plan.rs`.

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
new test `v3/pdf/tests/selection_real_glyphs.rs`.

1. In `gen-fixtures.py` add (copy the style of `fixture_single_page`):
   - `tight-leading.pdf` — one page, 6 lines, font size 12 with **leading 12**
     (`text_stream(..., size=12, leading=12)`), so loose boxes overlap.
   - `two-column.pdf` — one page, two `text_stream` calls at x=72 and x=320,
     same y values, 4 lines each.
   Regenerate (`python3 scripts/gen-fixtures.py`), update the README table,
   commit the new PDFs.
2. Tests (pdfium-gated, copy the `renderer()` helper from
   `v3/pdf/tests/fts_bridge.rs`): for each of `single-page.pdf`,
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

File: `v3/pdf/tests/selection_real_glyphs.rs` (extend), possibly
`v3/pdf/src/render.rs`.

`page_chars` flips y with `page.height()`. For `/Rotate 90/270` pdfium
reports post-rotation dimensions, but glyph boxes may not match the flip.
1. Write the test first: for each page of `rotated-pages.pdf`, run the
   bounds-check + whole-page-drag assertions from 0.2. 
2. If red: the fix belongs in `render.rs::page_chars` only (P5 — flip at the
   boundary). Likely shape: use `page.width()`/`page.height()` *after*
   rotation consistently for both the box flip and `page_size` (tiles already
   use the same source, so selection and paint stay aligned).
3. If green: record in the handoff decision log that rotation is covered.

### 0.4 Manual smoke checklist (`docs/V3_SMOKE.md`)

Toolkit-level bugs (P1) are invisible to every automated layer we have.
Create `docs/V3_SMOKE.md` with this exact list; run it (~5 min) whenever a
GUI surface changes, and extend it with every new feature:

```
Run: cd v3 && cargo run -p md3-shell --features pdfium -- <a real vault with a real-world PDF>
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

File: `v3/shell/src/gui/mod.rs::open_pdf_find`.

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

### 1.1 Engine: link extraction (`v3/pdf`)

1. In `v3/pdf/src/select.rs` *nothing changes.* Add to `v3/pdf/src/render.rs`:
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

Files: `v3/shell/src/gui/session.rs`, `pdf_view.rs`, `mod.rs`, `overlay.rs`.

1. `PdfSession` gains `pub links: HashMap<u32, Vec<md3_pdf::LinkBox>>`
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
5. Append to `V3_SMOKE.md`:
   `15. internal-links.pdf: right-click the link → popup shows page 2; esc closes; left-click navigates; alt+left returns.`

### 1.4 Tests (`v3/shell/tests/pdf_links.rs`, copy the harness from `pdf_toc.rs`)

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
   Implement as a pure function in a new `v3/shell/src/gui/file_tree.rs`:
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
   `V3_SHORTCUTS.md` first; as of writing `ctrl+b` is free. Scope: the same
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

### 2.5 Tests (`v3/shell/tests/file_tree.rs`)

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

1. New `v3/shell/src/gui/tokens.rs`:
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
   `Color::from_rgb` in `v3/shell/src` — after this phase the only literals
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
   (path from `v3/shell/src/gui/` to the repo root — verify with
   `include_bytes!` compile error if wrong; the file is `md-editor.png` at
   repo root). Requires iced's `image` feature — already enabled.
2. Desktop entry installer: port `native/src/platform/desktop_integration.rs`
   to `v3/shell/src/desktop.rs`. Keep its structure: functions take the home
   dir as a parameter (`install_with_home(home: &Path)`) so tests run against
   a tempdir; embed the icon with `include_bytes!`; write
   `~/.local/share/applications/md3.desktop` (Exec=md3-shell %f, Icon=md-editor)
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
2. Should the config stay a user-editable JSON (and where — `<vault>/.md3/tracker.json`?),
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

1. **Async tile + glyph worker.** Move `render_tile`/`page_chars`/
   `page_links` calls off the update thread: one worker thread owning a
   channel of engine `TileKey`/page requests (the `RenderQueue` cancellation
   semantics already fit), results returned via
   `Task::perform`/`Subscription`. Removes the 0.5 cap and the first-scroll
   hitch on huge PDFs. Gate: scrolling a 500-page doc (CI's `--large`
   fixture) never blocks input > 16 ms (manual check + a timing assertion in
   a worker unit test).
2. **Backlinks panel** (plan §3.4 — service exists in `vault/src/links.rs`):
   command `note.backlinks` lists referrers of the focused note via
   `LinkGraph`; reuse the overlay list pattern; enter opens the referrer.
3. **Annotation niceties** (handoff item 7 leftovers): color cycling
   (`pdf.highlight-color` rotating a small token palette, stored per
   annotation — schema already has `color`), copy-selection-to-clipboard
   (`ctrl+c` in pdf scope via `iced::clipboard::write`), linked-note creation
   (`pdf.annotation-link-note`: create `<stem>-notes.md`, store
   `linked_note`, open it), orphan report (palette command listing
   `known_documents` rows whose hash no longer matches any vault file).
4. **Editing ergonomics bundle** (plan §3.2/M3) — engine-side, one PR each,
   all property-tested in `v3/editor`: auto-pairs; smart list continuation +
   renumbering; checkbox toggle (`ctrl+enter`); table cell `tab` navigation +
   reflow; smart paste (URL over selection → link); heading cycle
   (`ctrl+1..6`). Every one is a `Command` through the bus, undo-coalescing
   rules decided per command (see `undo.rs` doc comments).
5. **Whitespace-elastic `pdf.find`** (P6): in `select::find`, treat any
   whitespace run in the needle as matching ≥0 whitespace/line-break gap in
   the stream; pure change + synthetic tests; removes the multi-word limit.
6. **Settings UI surface** (plan M2) — render `keymap.json` + theme choice;
   low urgency, the files work today.
7. **URI links open in browser** (from 1.2.5) — decide on the `open` crate
   (new dependency: record an ADR-style decision-log entry) or print-only.

---

## Appendix A — Test harness recipes

**Windowless shell test** (the standard pattern — copy from
`v3/shell/tests/pdf_toc.rs`): build `Shell::new(default_registry()?,
registry.keymap()?, tempdir)`, drive with `shell.update(Message::Key(...))`
via the `press`/`type_text` helpers, assert on `shell.status()`,
`shell.overlay()`, `shell.focused_pdf()`, `shell.workspace()`. Fixtures are
copied into the tempdir vault. pdfium tests skip when the library is absent.

**Engine geometry test:** synthetic `CharBox` grids via the `line_of` helper
in `v3/pdf/src/select.rs::tests` — pin semantics there first, then confirm
against real fixtures (Phase 0.2 suite).

**Sidecar test:** open stores on `<tempdir>/.md3/sidecar.db`; components
cohabit (annotations + sessions + index); never share a connection across
threads.

## Appendix B — Updating the ledger

After each phase: add a status-board row to `V3_HANDOFF.md` (file paths +
test names in the "Where" column), append decision-log entries (dated,
newest last) for anything a future agent would otherwise re-litigate, and
refresh the verification snapshot (test counts from the gate run).
