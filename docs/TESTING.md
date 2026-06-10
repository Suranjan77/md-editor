# Testing

## Test pyramid

| Layer | Where | Run with |
|---|---|---|
| Unit tests | `#[cfg(test)]` beside the code they test | `cargo test --workspace` |
| Integration tests | `core/tests/`, `native/tests/` | `cargo test --workspace` |
| Characterization tests | `native/src/app/characterization_tests.rs` | `cargo test -p md-editor-native characterization` |
| Scale/combinatoric tests | `*_scale_tests` modules (ex-`massive_tests.rs`) in `core/src/{file_index,config,tracker,vault}.rs` | `cargo test -p md-editor-core scale` |
| Renderer golden tests | planned (Phase 4: draw-command snapshots via `insta`) | — |
| PDF fixture tests | planned (Phase 5) against `tests-fixtures/pdf/` | — |

## Characterization tests

They freeze the **current** behavior of the root reducer — current behavior is
correct *by definition* for refactor phases (2–3). If a change breaks one:

- refactor commits must adapt mechanically (e.g. message wrapping) with values
  unchanged, or
- a deliberate behavior change must say so in the commit body.

They use plain asserts, not `insta`, so the suite self-verifies with no
snapshot-accept round.

## Fixtures

- `tests-fixtures/pdf/` — generated corpus; see its README. Regenerate with
  `python3 scripts/gen-fixtures.py` (add `--large` for the 500-page CI doc,
  which is gitignored).
- `tests-fixtures/markdown/` — planned (Phase 4 parser conformance suite).

## Conventions

- Tests needing a real filesystem use unique tempdirs under `target/` and clean
  up after themselves (see `unique_temp_dir` helpers).
- Tests are exempt from the unwrap budget; production code is not
  (`scripts/unwrap-budget.sh`).
- `native` is a binary-only crate: external integration tests cannot import its
  modules. `native/tests/smoke.rs` builds the binary via `CARGO_BIN_EXE`; real
  logic tests live in-crate.

## Full pre-handoff suite

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/architecture-check.sh
./scripts/check-budget.sh
./scripts/unwrap-budget.sh
```
