# Coding Standards

Conventions for contributing code. Enforced where possible by
[`scripts/architecture-check.sh`](../scripts/architecture-check.sh),
[`scripts/size-budget.sh`](../scripts/size-budget.sh), and clippy
(`-D warnings`, with `unwrap_used`/`expect_used` denied workspace-wide). Keep
this doc concrete: every rule has a good/bad example.

## 1. Error handling

- **Engines** (`md-kernel`, `md-editor`, `md-vault`, `md-pdf`): typed errors with
  `thiserror` enums; never stringly-typed errors in *new* APIs. Existing
  `Result<_, String>` functions are legacy — migrate when touched, don't extend.
- **Shell**: errors surface to users via the toast/status system; the binary may
  flatten errors at the very edge (`main`).

```rust
// BAD: stringly-typed, swallows the cause
pub fn open(path: &str) -> Result<Doc, String> {
    std::fs::read(path).map_err(|e| e.to_string()).map(Doc::parse)?
}

// GOOD: typed, preserves the source
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("failed to read {path}: {source}")]
    Io { path: String, #[source] source: std::io::Error },
    #[error("not a valid PDF: {0}")]
    Malformed(String),
}
pub fn open(path: &Path) -> Result<Doc, PdfError> { /* ... */ }
```

## 2. No `unwrap()` / `expect()` in production code

The workspace lints set `unwrap_used` and `expect_used` to `deny`, so
`cargo clippy --workspace -- -D warnings` fails on either. Tests opt out with a
file-level `#![allow(clippy::unwrap_used, clippy::expect_used)]`.

```rust
// BAD
let first = items.first().unwrap();

// GOOD
let Some(first) = items.first() else { return Task::none() };
```

When an invariant genuinely guarantees safety, prefer restructuring so the
unwrap disappears; if it truly cannot, document the invariant on the line above
and scope an `#[allow]` as narrowly as possible.

## 3. Naming contracts (do not violate)

| Name | Meaning |
|---|---|
| `page_index` | 0-based PDF page (internal) |
| `page_number` | 1-based page label (UI, links) |
| `vault_path` | vault-relative path (`notes/a.md`) |
| `abs_path` | absolute filesystem path |

```rust
// BAD: which is it?
fn go_to(page: u16) { ... }

// GOOD
fn go_to(page_index: u16) { ... }
fn format_label(page_number: u16) -> String { format!("p. {page_number}") }
```

## 4. Buffer mutation discipline

All Markdown buffer mutations go through the editor's edit operations so
undo/redo stays coherent. Whole-buffer `set_text` is allowed only for file load
or an explicit whole-buffer transaction.

## 5. Parsing stays in the editor engine

Markdown parsing and styling live in `md-editor` (`parse.rs`, `style.rs`,
`syntax.rs`). The shell's renderer consumes styled output; it never re-parses
raw Markdown.

## 6. Module and function size

- Module: **soft limit 400 lines, hard limit 700**. Existing oversized files are
  frozen at their current size in [`budgets.toml`](../budgets.toml)
  (`[file_budgets]`) and may only shrink. New files must respect the hard limit.
- Function: soft limit **75 lines**. If you need a comment like `// part 3: ...`,
  extract a function instead.

## 7. Public API documentation

Engine crates' public items get doc comments (rote one-liners are acceptable).
Document everything you touch.

## 8. Logging

No `println!`/`eprintln!` in library code. Use the shell's toast/status system
for user-visible state. The CLI/headless entry points may print to stdout/stderr
at the edge.

## 9. Formatting and lints

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace` must pass before handoff.
- Avoid broad `#[allow(...)]`; scope any unavoidable allow to the narrowest item
  and explain why.
- Keep unrelated formatting churn out of feature commits.

## 10. UI styling

Shell views consume the design tokens in `shell/src/gui/tokens.rs` — no ad-hoc
palettes, paddings, or magic durations scattered through feature views.
