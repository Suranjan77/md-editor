# Testing

The engines are designed to be tested headlessly — without a window, GPU, or
compositor — so the bulk of the suite runs everywhere, including CI. The only
local-only tier is the optional GUI smoke ([GUI_TESTING.md](GUI_TESTING.md)).

## Test layers

| Layer | Where | Run with |
|---|---|---|
| Unit tests | `#[cfg(test)]` beside the code | `cargo test --workspace` |
| Engine integration tests | `kernel/tests/`, `editor/tests/`, `vault/tests/`, `pdf/tests/` | `cargo test --workspace` |
| Regression suites (BUG-A/B/C) | `kernel/tests/bug_a_*`, `editor/tests/bug_b_*`, `kernel/tests/bug_c_*` | `cargo test --workspace` |
| Shell behavior tests | `shell/tests/*` — drive the real `gui::Shell` with semantic messages | `cargo test -p md-shell` |
| Golden draw-plan snapshot | `shell/tests/editor_draw_plan.rs` vs its fixture | `cargo test -p md-shell` (regenerate with `UPDATE_EXPECT=1`) |
| PDF rasterization tests | `pdf/tests/*`, `--features pdfium` | `cargo test -p md-pdf --features pdfium` |

### Regression suites

`BUG-A` (stolen shortcuts), `BUG-B` (layout reflow on reveal), and `BUG-C`
(documents forced into split view) are named suites that pin specific historical
bugs as unrepresentable. The kernel demo walks the same scenarios on the live
kernel:

```bash
cargo run -p md-shell -- --demo
```

### Shell behavior tests

`md-shell` is exercised through its real update loop: tests construct
`Shell::new(...)` and feed semantic messages (`RunCommand`, `Key`,
`PaneCommand`, `TreeFileClicked`, …) over a throwaway vault directory — the
equivalent of a DOM-level UI test, and where behavior coverage belongs.

## PDF tests and PDFium

The pure tile/cache/queue logic in `md-pdf` tests without any native library.
Tests that need real rasterization are gated behind the `pdfium` feature. The
first such build downloads a pinned, checksum-verified library for the current
platform and caches it under `target/pdfium`. For an offline build, point
`PDFIUM_LIB_DIR` at a directory containing a compatible platform library:

```bash
cargo test -p md-pdf --features pdfium
cargo test -p md-shell --features pdfium
```

## Fixtures

- `tests-fixtures/pdf/` — a small, committed, license-clean PDF corpus; see its
  [README](../tests-fixtures/pdf/README.md). Regenerate deterministically with
  `python3 scripts/gen-fixtures.py` (add `--large` for the 500-page stress
  document, which is gitignored and generated in CI).

## Conventions

- Tests needing a real filesystem use unique tempdirs (e.g. via `tempfile`) and
  clean up after themselves.
- Tests are exempt from the `unwrap`/`expect` ban via a file-level
  `#![allow(...)]`; production code is not.

## Full pre-handoff gate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/architecture-check.sh
./scripts/size-budget.sh
```

Or, with [`just`](../justfile): `just check`.
