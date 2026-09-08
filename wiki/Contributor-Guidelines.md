# Contributor Guidelines

Thank you for contributing to MD Editor! To maintain code quality, crash resilience, and consistent design aesthetics, all contributors are expected to follow the principles outlined in this guide.

---

## 1. Architectural Invariants for Contributors

### Rule 1: Maintain Strict Crate Decoupling
- Never import `iced`, `winit`, or graphical rendering crates into `md-editor-core`.
- `md-editor-core` must remain completely headless, deterministic, and testable without a windowing server or GPU.

### Rule 2: Adhere to Design Tokens
- Never introduce hardcoded dimensions, padding, or colors in view modules.
- Use spacing tokens (`SPACE_0` through `SPACE_6`), typography scale tokens (`TEXT_XS` through `TEXT_LG`), and corner radii from [`native/src/theme.rs`](file:///home/sur/repo/md-editor/native/src/theme.rs).
- Animations must exclusively use the easing curve `Easing::EaseOutQuad` and durations (`FAST`, `PANEL`, `OVERLAY`) from [`native/src/motion.rs`](file:///home/sur/repo/md-editor/native/src/motion.rs).

### Rule 3: Enforce Viewport Culling
- In [`native/src/editor/renderer.rs`](file:///home/sur/repo/md-editor/native/src/editor/renderer.rs), never iterate across all lines in a document during a draw or layout pass.
- Always clamp processing strictly between `first_line` and `last_line` as returned by `HeightTree::find_line_at_y()`.

### Rule 4: PDFs are Immutable
- Never write to or mutate user `.pdf` files.
- All annotations, highlights, bookmarks, and cross-references must be persisted externally in the SQLite database (`pdf_annotations`, `pdf_references`).

### Rule 5: Maintain Durability Invariants
- Never replace the atomic save sequence (`write_file()` writing to sibling temporary file, calling `sync_all()`, and atomically renaming) with direct truncation or in-place overwriting.
- Document buffer undo histories must continue to survive document navigation via the 32-note LRU cache.

### Rule 6: Zero Idle CPU
- Ensure that `iced::window::frames()` is subscribed **only** while animations are actively transitioning (`motion.is_animating()`).
- A settled application window must yield 0% CPU consumption.

---

## 2. Testing Requirements

### Adding Tests for Bug Fixes
- Any bug fix in the Markdown parser, undo grouping, or auto-pairing must be accompanied by a regression test in [`native/src/editor/buffer.rs`](file:///home/sur/repo/md-editor/native/src/editor/buffer.rs) or [`native/src/editor/highlight.rs`](file:///home/sur/repo/md-editor/native/src/editor/highlight.rs).
- Any change to wikilink parsing or resolving must be validated against the combinatorial stress suite in [`core/src/massive_tests.rs`](file:///home/sur/repo/md-editor/core/src/massive_tests.rs).

---

## 3. Pre-Pull Request Verification

Before opening a pull request, run the complete verification suite locally:

```bash
# 1. Format check
cargo fmt --check

# 2. Workspace typecheck
cargo check --workspace

# 3. Full test suite execution
cargo test --workspace

# 4. Lint check
cargo clippy --workspace -- -D warnings
```

Ensure all automated tests pass and that your code introduces no new warnings.
