# Copilot instructions

MD Editor is a native desktop Markdown + PDF workspace, built as a Cargo
workspace in Rust.

## Architecture

- Engine crates — `md-kernel`, `md-editor`, `md-vault`, `md-pdf` — are
  **toolkit-agnostic**: they must never reference `iced` or `winit`.
- `md-shell` is the only crate that knows the GUI toolkit; it composes the
  engines and builds the `md-editor` binary.
- See [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md) and
  [docs/ARCHITECTURE_RULES.md](../docs/ARCHITECTURE_RULES.md).

## Conventions

- No `unwrap()`/`expect()` in production code (clippy `deny`); typed errors with
  `thiserror`.
- All Markdown buffer mutations go through the editor's edit operations; Markdown
  parsing stays in `md-editor`, never in the shell renderer.
- Distinguish 0-based `page_index` from 1-based `page_number`.
- See [docs/CODING_STANDARDS.md](../docs/CODING_STANDARDS.md).

## Before opening a PR

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/architecture-check.sh
./scripts/size-budget.sh
```
