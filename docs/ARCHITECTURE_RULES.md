# Architecture Rules

The enforced boundaries of the workspace. For the wider picture and rationale,
see [ARCHITECTURE.md](ARCHITECTURE.md). These rules are machine-checked by
[`scripts/architecture-check.sh`](../scripts/architecture-check.sh) and
[`scripts/size-budget.sh`](../scripts/size-budget.sh), run in CI by
[`.github/workflows/quality.yml`](../.github/workflows/quality.yml). If a rule
here and a script disagree, fix the script — the rule wins.

## Crate graph

```
md-shell (iced GUI, the only binary)
   ├── md-kernel    workspace model: panes, focus, commands, keymap
   ├── md-editor    text buffer, layout, undo, Markdown parse/style
   ├── md-vault     files, index, search, annotations, tracker, links
   └── md-pdf       tiles, render queue, pdfium (feature-gated)
```

- The engine crates — `md-kernel`, `md-editor`, `md-vault`, `md-pdf` — are
  **toolkit-agnostic**. They must not reference `iced` or `winit` in code or in
  their `Cargo.toml`. *(enforced — ADR-0100)*
- Engines **do not depend on each other** in production code; the shell composes
  them. The single allowed exception is `md-pdf`'s `[dev-dependencies]` use of
  `md-vault` for the PDF-text → search-index integration test. *(enforced)*
- Only `md-shell` knows about `iced`, windowing, file dialogs, or any other
  platform/UI concern.

## Inside the engines

- **`md-kernel`** holds the UI-free workspace model. Every user action is a
  registered command; the keymap, palette, menus, and `docs/SHORTCUTS.md` are
  generated from the registry, never hand-written. The keymap detects binding
  conflicts at startup and refuses to launch on a clash.
- **`md-editor`** is a pure text engine. All buffer mutations go through edit
  operations so undo/redo stays coherent; whole-buffer `set_text` is reserved
  for file load. Layout and measurement stay logarithmic — no full-document
  scans on hot paths.
- **`md-vault`** owns all persistence. Per-vault state lives under
  `<vault>/.md-editor/` and is rebuildable; it is never mixed into user content.
  New APIs use typed errors (`thiserror`), not stringly-typed `Result<_, String>`.
- **`md-pdf`** keeps the pure tile/cache/queue logic free of PDFium so it builds
  and tests without the native library; rasterization sits behind the `pdfium`
  feature.

## Inside the shell

- `shell/src/gui/` is the iced view layer: chrome, overlays, the editor and PDF
  canvases, and the study tracker. It translates toolkit events into kernel
  chords/commands and renders engine output — it does not reimplement engine
  logic.
- The shell owns all file-format parsing (session snapshots, keymap overrides)
  so the kernel and engines stay serde-free.
- No `iced` types leak back into the engines; data flows engine → shell → screen.

## Budgets (ratchets)

[`budgets.toml`](../budgets.toml) freezes current module sizes as ceilings.
[`scripts/size-budget.sh`](../scripts/size-budget.sh) fails CI if any `.rs` file
exceeds its ceiling (or the 700-line hard limit when unlisted). Ceilings may
only **decrease**: when you shrink a file, lower its number in the same PR.

The `unwrap()`/`expect()` ban in production code is enforced directly by clippy
(`unwrap_used` / `expect_used` are set to `deny` in the workspace lints), so a
violation fails `cargo clippy -D warnings`.

## Verifying a rule fires

The enforcement scripts are only trustworthy if they actually fail. When you add
or change a rule, prove it by temporarily injecting a violation (e.g. add
`use iced::Color;` to an engine crate) and confirming the check exits non-zero
before relying on it.
