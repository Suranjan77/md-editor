# V3 Stabilization Report — 2026-06-12

This report records fixes made after testing v3 against a real study vault.
It supplements `V3_HANDOFF.md`; architecture remains governed by
`V3_GROUND_UP_PLAN.md` and `V3_UX_OVERHAUL_PLAN.md`.

## User-reported problems addressed

### Markdown editor geometry and rendering

- Soft wrapping now follows editor viewport width. Resize events update
  `EditorDocument` wrap width and remeasure line heights.
- Caret, selection, click hit-testing, and paint use the same wrapped
  monospace grid.
- Rendered assets participate in measure-before-paint layout:
  - image dimensions feed `MonoMeasurer`;
  - display-math dimensions feed `MonoMeasurer`;
  - height-tree offsets are recomputed after assets load;
  - following lines no longer paint through images or math.
- Inline math renders in text flow at the current x position instead of being
  centered over the whole line.
- Images preserve aspect ratio, use available width within size caps, and
  render centered as blocks.
- Multi-line `$$ ... $$` content is consolidated before Ratex parsing.
  Environments such as `\begin{align}`, `pmatrix`, `cases`, and related
  constructs therefore reach Ratex as one TeX unit.
- Display-math background panels were removed. Math now renders directly on
  editor background.
- Reading rhythm increased to 16 px text on a 28 px baseline. Paragraphs,
  headings, lists, quotes, and rules receive semantic trailing space.
- Markdown background uses lighter legacy-derived theme surface.

Primary code:

- `v3/shell/src/gui/editor_canvas.rs`
- `v3/shell/src/gui/session.rs`
- `v3/editor/src/layout.rs`
- `v3/editor/src/document.rs`
- `v3/editor/src/style.rs`

Regression coverage:

- rendered image height comes from loaded asset dimensions;
- inline math remains on one text row when space permits;
- multi-line `\begin{align} ... \end{align}` renders as one block;
- wrap-width changes reflow height-tree offsets.

### File browser startup

`Shell::new` now scans the vault immediately after session restore when file
tree is open. Users no longer need manual refresh before files appear.

Regression: `shell/tests/file_tree.rs::open_tree_is_populated_on_startup`.

### PDF behavior

- Fit-width and fit-page modes persist across split-pane resize and recompute
  zoom from new viewport dimensions. Manual zoom exits fit mode.
- Internal-reference previews use destination rectangles when available,
  cropping and enlarging referenced object instead of showing entire page.
- Page terminology follows project rule: `page_index` is zero-based engine
  state; `page_number` is one-based UI/link text.

Primary code:

- `v3/shell/src/gui/session.rs`
- `v3/shell/src/gui/pdf_view.rs`
- `v3/shell/src/gui/mod.rs`
- `v3/pdf/src/render.rs`

### Welcome-to-editor transition

Opening or creating vault from welcome surface now transitions through same
application flow instead of presenting welcome and editor as unrelated
applications. Welcome behavior and recent-vault handling remain in
`v3/shell/src/gui/welcome.rs` and `v3/shell/src/main.rs`.

### Toast runtime panic

Toast auto-dismiss uses `tokio::time::sleep`. Iced previously used default
thread-pool executor, so toast tasks panicked with:

```text
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

`md3-shell` now enables iced `tokio` feature, making iced executor own Tokio
runtime used by toast tasks. Decision recorded in ADR-0103.

## Verification

Run from `v3/`:

- `cargo fmt --all -- --check` — pass.
- `cargo clippy --workspace --all-targets -- -D warnings` — pass.
- `cargo check -p md3-shell --features pdfium` — pass.
- `cargo test -p md3-shell --lib` — 28 passed.
- `cargo test --workspace` — all reached suites pass except
  `shell/tests/tracker_wiring.rs::tracker_manual_log_and_delete`.

Tracker failure predates these renderer/runtime fixes. Test expects status-bar
text `"tracker: session logged manually"` while current feedback path uses a
toast and leaves status empty.

**Decision made (architect review, same day):** transient command outcomes
are toasts (impl-plan pitfall P14); the handler toasts once and the test
asserts `Shell::toasts()`. The fix is specced as impl-plan Phase 6.0 and is
the first unit of work — under the binary-gate rule (impl-plan §0.2) nothing
else lands before the workspace is green and committed.

## Remaining markdown work

> **This list is historical context, not a work queue.** It has been turned
> into step-by-step specs as `V3_IMPLEMENTATION_PLAN.md` Phase 6.3 (golden
> draw-plan snapshots) and **Phase 7 — Typora-grade live editor**
> (user-ordered 2026-06-13: shaped text measurement, true conceal, block
> rendering, stable assets, exact hit-testing). Work from that spec.

Current renderer is stabilized, not yet Obsidian/Typora-complete.

- Heading font sizes still use body size; only color/weight differ.
- Tables preserve source-row structure but are not yet rendered as polished
  cells with measured column widths.
- Lists and checkboxes need richer hanging-indent and interaction geometry.
- Concealed rich lines and revealed source lines still use different visual
  flow; click-to-caret mapping on rendered blocks is approximate.
- Asset discovery/rendering is synchronous during document load.
- No golden draw-command snapshot yet covers representative document with
  headings, paragraphs, table, image, inline math, display math, and active
  caret.
- Real-font measurement, proportional reading font, CJK width, emoji width,
  and bidi layout remain open.

Next renderer work should continue plan rule: parser semantics stay in
`v3/editor/src/parse.rs`/`style.rs`; shell renderer only measures and paints
semantic spans. Variable-height content must update height tree before paint,
and draw remains viewport-bounded.
