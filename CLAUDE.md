# MD Editor

Native, local-first desktop workspace for Markdown notes, PDFs, presentations, a knowledge
graph and handwritten notes. Rust 2024, Iced 0.14, PDFium. GPL-3.0.

## Commands

```bash
cargo check --workspace --all-targets   # fast typecheck
cargo test --workspace                  # all tests (~435); core PDF tests need PDFium, fetched by build.rs
cargo fmt --all                         # the tree is rustfmt-clean; keep it that way
cargo test -p md-editor-native e2e -- --nocapture   # headless end-to-end runs; screenshots + telemetry in target/e2e/
cargo run -- path/to/vault-or-note.md   # launch (opens a window)
```

The first build downloads PDFium into `target/pdfium/` (network needed). If PDF tests fail
with an undefined `FPDF_*` symbol, a stale library is cached: delete
`target/debug/libpdfium.so` and rebuild.

## Map

- `core/` — headless engine, no UI deps: `vault.rs` (+`vault/`), `file_index.rs`,
  `state.rs` (SQLite), `pdf.rs` (+`pdf/` worker, TOC), `pptx.rs` (+`pptx/`), `references.rs`.
- `native/src/app/` — the shell. `mod.rs::update_inner` routes each `Message` family to
  `update_<family>` in `pdf.rs`, `graph.rs`, `ink.rs`, `search.rs`, `editor.rs`,
  `vault.rs`, `shortcuts.rs` or `shell.rs`. Shared machinery: `documents.rs`,
  `subscription.rs`, `view.rs`, `links.rs`.
- `native/src/editor/` — buffer, incremental highlighter, the custom editor widget.
- `native/src/views/` — stateless view builders. `*_state.rs` / `*_pane.rs` — sub-states.
- `native/src/ink/`, `graph_state.rs` — handwriting and the knowledge graph.

Every `.rs` file starts with a one-line `//!` summary: `grep -rm1 '^//!' core/src native/src`
is the fastest map. Tests for `foo.rs` live in `foo/tests.rs`.

## Where changes go

- New message: variant in `messages.rs`, arm in the matching `app/<family>.rs`. A variant
  whose name has no routed prefix also needs a pattern in `app/mod.rs::update_inner`.
- New shortcut: `Shortcut` in `messages.rs`, key in `app/subscription.rs`, handler in
  `app/shortcuts.rs`, palette entry in `views/command_palette.rs`.
- New file type in the vault: extension list and `is_supported_vault_path` in
  `core/src/vault.rs` / `vault/files.rs`, open path in `app/mod.rs::open_startup_file` and
  the sidebar/link handlers, icon in `views/sidebar.rs`.

## Rules that bite

- Core stays headless: never add `iced`/GUI deps to `core`.
- Writes go through `vault::write_file` (atomic, symlink- and permission-preserving).
  Notes autosave 400 ms after typing; switching documents flushes first. Never drop a dirty
  buffer.
- Sizes, colours and durations come from `native/src/theme.rs` and `motion.rs` — no literals
  in views.
- Editor renderer invariants (draw only visible lines, layout caching, one wrapper) are in
  `native/src/editor/CLAUDE.md`; core invariants in `core/CLAUDE.md`.
- The app is an iced daemon with two windows (shell + ink). Window events carry an id;
  scope handlers to `main_window` / `ink_window`.

## Docs

`wiki/` is authoritative; start at `wiki/Home.md` ("I want to change…" table). Plans in
progress: `docs/ui-overhaul-plan.md` (+ `docs/design-system/`) and
`docs/e2e-bug-discovery-plan.md`. When code
moves, update `wiki/Repository-Structure.md` in the same change. Cite files and symbols,
not line numbers. `docs/` holds the printable handbook, a dated snapshot.
