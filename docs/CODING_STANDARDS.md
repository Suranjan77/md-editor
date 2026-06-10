# Coding Standards

Referenced by `AGENTS.md`. Enforced where possible by `scripts/architecture-check.sh`,
`scripts/check-budget.sh`, `scripts/unwrap-budget.sh`, and clippy (`-D warnings`).
Keep this doc concrete: every rule has a good/bad example.

## 1. Error handling

- **core**: typed errors with `thiserror`-style enums (or hand-rolled `Display` impls);
  never stringly-typed errors in *new* APIs. Existing `Result<_, String>` functions are
  legacy — migrate when touched, don't extend.
- **native**: errors surface to users via the toast/status system; binaries may flatten
  errors at the very edge.

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

Ratcheted by `scripts/unwrap-budget.sh` (ceiling in `budgets.toml`, currently 8 —
may only go down). Tests are exempt.

Escape hatch: if an invariant genuinely guarantees safety, document it **on the same
line or the line above** with `// INVARIANT:` (the budget script skips such lines):

```rust
// BAD
let first = items.first().unwrap();

// GOOD
let Some(first) = items.first() else { return Task::none() };

// ACCEPTABLE (rare)
// INVARIANT: layout is rebuilt whenever page_sizes changes, so index < len.
let size = page_sizes[index].unwrap();
```

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

All Markdown buffer mutations go through `EditorCommand` so undo/redo stays coherent.
`buffer.set_text` is allowed only for file load or an explicit whole-buffer
transaction. Enforced by `architecture-check.sh` (`.set_text(` ban in production code).

## 5. Parsing stays in the parser

Markdown parsing rules live in `native/src/editor/highlight.rs` + `editor/parser/`.
The renderer consumes `StyledLine`s; it never inspects raw Markdown. Enforced by
`architecture-check.sh`.

## 6. Module and function size

- Module: **soft limit 400 lines, hard limit 700**. Existing oversized files are
  frozen at their current size in `budgets.toml` (`[file_budgets]`) and may only
  shrink. New files must respect the hard limit.
- Function: soft limit **75 lines**. If you need a comment like `// part 3: ...`,
  extract a function instead.

## 7. Public API documentation

`core`'s public items get doc comments (rote one-liners acceptable). Phase 2 ends
with `#![warn(missing_docs)]` on the core crate; until then, document everything you
touch.

## 8. Logging

No `println!`/`eprintln!` in library code. Use the diagnostics view / toast system for
user-visible state; `tracing` is planned (Phase 10 crash handling) — when it lands,
all ad-hoc prints go through it.

## 9. Formatting and lints

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace` must pass before handoff.
- Don't add `#[allow(...)]`; burn down the existing allow-list in `native/src/main.rs`
  when touching related code (each removal its own commit).
- Keep unrelated formatting churn out of feature commits.

## 10. UI styling (from UX-A onward)

Views consume design tokens (`native/src/design/`) — no raw `Color::from_rgb`,
ad-hoc paddings, or magic durations in feature views. Raw-color count is ratcheted
in `budgets.toml` (`raw_color_ceiling`, target 0).
