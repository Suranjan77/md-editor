# Contributing to MD Editor

Thanks for your interest in improving MD Editor. This guide covers the
essentials; the linked docs go deeper.

## Getting started

```bash
git clone <your-fork-url>
cd md-editor
cargo run -- <a-vault-folder>
```

The default build runs without PDF rasterization. To work on PDF features, build
with the `pdfium` feature and a discoverable `libpdfium` — see the
[README](README.md#build-from-source).

## Project layout

MD Editor is a Cargo workspace of toolkit-agnostic **engine** crates
(`md-kernel`, `md-editor`, `md-vault`, `md-pdf`) and a single **shell** crate
(`md-shell`) that wires them to the iced GUI. The engine/shell boundary is the
core architectural rule — read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and
[docs/ARCHITECTURE_RULES.md](docs/ARCHITECTURE_RULES.md) before making changes.

## Before you open a PR

Run the full gate (or `just check`):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/architecture-check.sh
./scripts/size-budget.sh
```

Key conventions ([docs/CODING_STANDARDS.md](docs/CODING_STANDARDS.md)):

- No `unwrap()`/`expect()` in production code (enforced by clippy `deny`).
- Typed errors with `thiserror`; no new stringly-typed errors.
- Markdown parsing stays in `md-editor`; the shell renders, it does not parse.
- Modules stay under the size budgets in [`budgets.toml`](budgets.toml);
  ceilings only go down.

If you change keyboard commands, regenerate the shortcuts reference:

```bash
just shortcuts   # or: cargo run -q -p md-shell -- --dump-shortcuts > docs/SHORTCUTS.md
```

## Testing

The engines are tested headlessly, so most of the suite runs anywhere. PDF
rasterization tests skip when no `libpdfium` is present. See
[docs/TESTING.md](docs/TESTING.md).

## Architectural decisions

Significant decisions are recorded as ADRs in [docs/adr/](docs/adr/). To overturn
one, add a superseding ADR rather than editing the original.

## License

By contributing, you agree that your contributions are licensed under the
project's [MIT License](LICENSE).
