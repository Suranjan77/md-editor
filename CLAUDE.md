# MD Editor

Native, local-first desktop workspace for Markdown notes, PDFs, presentations, a knowledge
graph and handwritten notes. Rust 2024, Iced 0.14, PDFium. GPL-3.0-or-later. Ships as an
Arch Linux package (`packaging/arch/`); data and log live in XDG dirs, never beside the exe.

## Commands

```bash
cargo check --workspace --all-targets   # fast typecheck
cargo test --workspace                  # all tests (~435); core PDF tests need PDFium, fetched by build.rs
cargo fmt --all                         # the tree is rustfmt-clean; keep it that way
scripts/clippy-ratchet.sh               # clippy warnings must not exceed clippy-baseline.txt (CI gate)
cargo test -p md-editor-native e2e -- --nocapture   # headless end-to-end runs; screenshots + telemetry in target/e2e/
cargo run -- path/to/vault-or-note.md   # launch (opens a window)
scripts/build-arch-package.sh -si       # build + install the Arch package from the working tree
```

The first build downloads PDFium into `target/pdfium/` (network needed). If PDF tests fail
with an undefined `FPDF_*` symbol, a stale library is cached: delete
`target/debug/libpdfium.so` and rebuild.

## Map

- `core/` — headless engine, no UI deps: `vault.rs` (+`vault/`), `file_index.rs`,
  `state.rs` (SQLite), `pdf.rs` (+`pdf/` worker, TOC), `pptx.rs` (+`pptx/`), `references.rs`.
- `native/src/app/` — the shell. `mod.rs::update_inner` routes each `Message` family (`Message::Pdf(PdfMessage)`, …) to
  `update_<family>` in `pdf.rs`, `graph.rs`, `ink.rs`, `search.rs`, `editor.rs`,
  `vault.rs`, `tabs_handler.rs`, `settings_handler.rs`, `shortcuts.rs` or `shell.rs`.
  Shared machinery: `documents.rs`, `subscription.rs`, `view.rs`, `links.rs`.
- `native/src/editor/` — buffer, incremental highlighter, the custom editor widget.
- `native/src/views/` — stateless view builders. `*_state.rs` / `*_pane.rs` — sub-states.
- `native/src/design/` — design tokens and runtime theme/scale; `native/src/ui/` — the
  component library (gallery test in `ui/gallery.rs`); `tabs.rs` — document tabs.
- `native/src/ink/`, `graph_state.rs` — handwriting and the knowledge graph.

Every `.rs` file starts with a one-line `//!` summary: `grep -rm1 '^//!' core/src native/src`
is the fastest map. Tests for `foo.rs` live in `foo/tests.rs`.

## Where changes go

- New message: variant in the family enum in `messages.rs` (`PdfMessage`, `VaultMessage`, …),
  arm in that family's `app/<family>.rs` handler. Handlers match exhaustively, so the
  compiler finds the missing arm. A new family needs a `Message` variant and an arm in
  `app/mod.rs::update_inner`.
- New shortcut: `Shortcut` in `messages.rs`, an entry in the `shortcuts.rs` registry (which
  feeds the palette, tooltips and the Settings list), key in `app/subscription.rs`, handler
  in `app/shortcuts.rs`.
- New file type in the vault: extension list and `is_supported_vault_path` in
  `core/src/vault.rs` / `vault/files.rs`, open path in `app/mod.rs::open_startup_file` and
  the sidebar/link handlers, icon in `views/sidebar.rs`.

## Rules that bite

- Core stays headless: never add `iced`/GUI deps to `core`.
- Log with `log::{info, warn, error}`; `println!`/`eprintln!` are linted (CLI installer excepted).
- Writes go through `vault::write_file` (atomic, symlink- and permission-preserving, refuses
  read-only files); the editor saves via `vault::save_file_guarded`, which keeps an external
  edit as a conflict copy instead of overwriting it.
  Notes autosave 400 ms after typing; switching documents flushes first. Never drop a dirty
  buffer.
- Sizes, colours, type and durations come from `native/src/design/` (`metrics`, `palette`,
  `type_scale`, `motion`) — no literals in views. Build chrome from `native/src/ui/`
  components.
- Editor renderer invariants (draw only visible lines, layout caching, one wrapper) are in
  `native/src/editor/CLAUDE.md`; core invariants in `core/CLAUDE.md`.
- The app is an iced daemon with two windows (shell + ink). Window events carry an id;
  scope handlers to `main_window` / `ink_window`.

## Docs

`wiki/` is authoritative; start at `wiki/Home.md` ("I want to change…" table). Plans in
progress: `docs/ui-overhaul-plan.md` (+ `docs/design-system/`),
`docs/e2e-bug-discovery-plan.md`, `docs/code-health-plan.md`,
`docs/feature-plan-tabs-and-images.md`. When code
moves, update `wiki/Repository-Structure.md` in the same change. Cite files and symbols,
not line numbers. `docs/` holds the printable handbook, a dated snapshot.
