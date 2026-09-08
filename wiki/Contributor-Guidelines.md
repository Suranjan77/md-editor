# Contributor Guidelines

Thank you for contributing to MD Editor. These rules are the ones that are expensive to
rediscover — each exists because something concrete broke without it.

---

## 1. Architectural Invariants

### Rule 1: Keep the crates decoupled

- Never import `iced`, `winit`, `ratex-render`, or any GUI or text-shaping crate into
  `md-editor-core`.
- Core must stay headless, deterministic, and testable without a windowing server or GPU. Its
  56 tests run in CI containers with no display.

### Rule 2: Use the design tokens

- No hardcoded dimensions, padding, font sizes, or hex colours in view modules.
- Sizes come from `TEXT_XS` … `TEXT_DISPLAY`, gaps from `SPACE_0` … `SPACE_6`, radii from
  `RADIUS_SM` / `RADIUS_MD` / `RADIUS_LG`, and colours from the palette — all in
  `native/src/theme.rs`.
- Animations use `Easing::EaseOutQuad` and one of `FAST`, `PANEL`, `OVERLAY` from
  `native/src/motion.rs`. A new duration needs a reason, not a preference.
- A panel's animated open width must match the width it actually lays out at, or the clip
  that produces the slide crops it permanently.

### Rule 3: Keep the draw pass proportional to what is visible

- In `native/src/editor/renderer.rs`, the **draw** pass must never iterate the whole document.
  Clamp strictly between `visible_start` and `visible_end` from
  `HeightTree::find_line_at_y()`, and stop once the y position passes the viewport.
- The **layout** pass does walk every line, but every line must be a cache hit unless
  something about it actually changed. If you add state that affects a line's height, add it
  to `line_hash` or `resource_hash` — otherwise you have silently made layout O(N) shaping
  calls per frame, and debug builds will crawl.
- When adding a block type, update block-range tracking, height measurement, draw metadata,
  and hit testing **together**. They are one contract split across four call sites.

### Rule 4: PDFs are immutable

- Never write to a user's `.pdf`.
- Highlights, notes, bookmarks, and cross-references belong in `pdf_annotations` and
  `pdf_references`, keyed by the content-derived `document_id`.
- Only ever create one `Pdfium` binding per process, and reach it only through
  `core/src/pdf.rs`.

### Rule 5: Do not weaken durability

- Never replace the atomic save sequence — sibling temp file, permission copy, `sync_all`,
  atomic rename, parent-directory sync — with a direct truncating write.
- Navigation must keep flushing the active buffer before switching, and must keep **refusing
  to navigate** when that write fails.
- Undo histories must keep surviving navigation through the 32-document retained registry,
  and a parked buffer must keep being discarded when the file changed on disk.

### Rule 6: Zero idle CPU

- Arm the per-frame subscription **only** while `motion.is_animating()` is true.
- The same discipline applies to every other subscription: the toast timer, autosave poll,
  highlight debounce, and search debounce each return `Subscription::none()` when they have
  nothing pending. A new always-on timer is a regression even if it looks cheap.

### Rule 7: Async results clean up before they are gated

- In every PDF result handler, remove the page from its pending set **before** checking
  `render_generation`. Returning early on a stale generation without cleaning up strands the
  page in a permanent loading state.
- Any code that increments `render_generation` must clear the previous generation's pending
  sets.
- Document-level artifacts — resolved references, the TOC — are gated on `document_id` or the
  document path, **never** on `render_generation`, because their scans routinely outlive a
  zoom change.
- The same idea applies to markdown: every highlight task carries a generation id, and a
  result whose generation no longer matches is discarded.

The full set of PDF rules is in [PDF Viewer Internals](PDF-Viewer-Internals.md#10-reliability-rules).

---

## 2. Testing Requirements

- A bug fix in the markdown parser, undo grouping, or auto-pairing needs a regression test in
  `native/src/editor/buffer.rs` or `native/src/editor/highlight.rs`.
- A change to wikilink parsing or resolution must be validated against the combinatorial
  suite in `core/src/massive_tests.rs`.
- A change to the reference resolver's output for the same input **must bump**
  `references::RESOLVER_VERSION`, or users will be served stale cached results.
- A change to PDF scroll or page geometry needs coverage in the page-slot tests in
  `native/src/app.rs`.
- New core functionality belongs behind a headless test. If it cannot be tested without a
  window, it probably belongs in `md-editor-native`.

---

## 3. Documentation

**This wiki is the only documentation tree.** There is no `docs/` directory; anything that
would have gone there goes here instead.

- A change that alters behaviour described on a wiki page updates that page in the same pull
  request.
- Every claim on these pages should be checkable against the file named beside it. Prefer
  naming the constant or function over paraphrasing its value, so a rename breaks a search
  rather than rotting quietly.
- Record *why* alongside *what*. Most of the non-obvious code in this repository already
  carries that reasoning in module comments; keep the wiki consistent with them.

---

## 4. Pre-Pull-Request Verification

CI builds and packages, but does **not** run the tests or the lints. Run them locally:

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace           # 158 tests, all passing
cargo clippy --workspace
```

`cargo fmt --check` and `cargo test` must pass cleanly.

**Clippy is not yet warning-clean**: the tree currently carries 34 warnings (2 in core, 32 in
native), mostly `too_many_arguments`, `redundant_guard`, and `if_same_then_else`. So
`-D warnings` does not pass on a fresh checkout. The rule is therefore *do not add new ones*:
compare the count before and after your change, and fix any warning your own code introduces.

Then, if your change touches saving, navigation, animation, or the PDF pane, walk the
relevant items in the [Release Checklist](Release-Checklist.md) — particularly the durability
checks, which no automated test fully replaces.
