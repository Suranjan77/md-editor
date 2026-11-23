# Code Health Plan

**Goal:** make MD Editor cheaper and safer to change: smaller units, typed errors, real
logging, an enforced quality gate, and tests where the risk is. No behaviour changes except
where a workstream says so.

**Date:** 2026-11-23 · **Status:** proposed. Companion to the
[UI overhaul](ui-overhaul-plan.md), [bug discovery](e2e-bug-discovery-plan.md) and
[tabs & images](feature-plan-tabs-and-images.md) plans.

---

## 1. Baseline (measured on `main` at `4c85bc6`)

| Metric | Value | Note |
| :--- | :--- | :--- |
| Non-test Rust | ~42,200 lines | after this session's module splits |
| Largest functions | `app/pdf.rs::update_pdf` **884** lines · `pdf/renderer.rs::PdfRenderer::new` (worker loop) **636** · `app/view.rs::shell_view` **456** · `app/vault.rs::update_vault` **453** · `views/icons.rs::draw` 332 · `views/pdf_viewer.rs::{toolbar, view_continuous}` 264 / 254 | functions over ~150 lines are where bugs hide |
| `DocBuffer` | one `impl` of ~870 lines | editing, movement, undo, formatting, lists all together |
| `Message` | **168** variants in one flat enum | routed to handlers by name prefix with a `_ => Task::none()` catch-all that silently drops anything misrouted |
| `MdEditor` | 20 fields | sub-states exist but most handlers live on the shell |
| Errors | **78** functions return `Result<_, String>` | no error kinds; callers cannot tell "not found" from "permission denied" |
| Logging | **34** `eprintln!`/`println!`, **1** use of `log` | nothing persisted; field reports have no trail |
| Panics in non-test code | core: 7 `unwrap`/`expect`, all guarded invariants · native: 14 `unwrap`, 17 `expect` | native ones need an audit |
| `unsafe` | 30 sites, all in `native/src/platform.rs` and `native/src/ink/pen_win.rs` (Win32) | core has none |
| Clippy | 10 warnings (baseline); 6 `#[allow(clippy::too_many_arguments)]` | view builders take 9–13 arguments |
| CI | builds release packages only — **no fmt, clippy, tests or audit** | see wiki *Developer Guide* §CI |
| Untested modules (no `#[test]` in or beside them) | `pdf/toc.rs` 1,003 · `pdf/renderer.rs` 940 · `pptx/slide.rs` 910 · `app/pdf.rs` 892 · `views/gpu_graph.rs` 807 · `app/vault.rs` 602 · `ink/canvas.rs` 588 · `app/pdf_pages.rs` 586 · `app/documents.rs` 425 · `vault/files.rs` 405 | some are covered indirectly (`pdf/tests.rs`, `vault/tests.rs`); app/* will be covered by e2e scenarios |
| Duplicate dependency versions | ~15 (bitflags, hashbrown, rustix, read-fonts, png…) | all pulled in by iced/wgpu; not actionable locally |

---

## 2. Workstreams

### W1 — Quality gate in CI (S) · do first

A `ci.yml` workflow on every push and PR, Linux and Windows:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets` with a **count ratchet** (fails if warnings exceed
   the committed baseline file; the baseline only goes down)
3. `cargo test --workspace` (core PDF tests need PDFium: cache `target/pdfium/`)
4. `cargo test -p md-editor-native e2e` (headless; uses tiny-skia when no GPU), uploading
   `target/e2e/` as an artifact
5. `cargo audit` and `cargo deny check licenses bans` (with a `deny.toml` allowing the
   current licence set: MIT/Apache/BSD/ISC/Zlib/OFL/GPL-3.0 for the app)
6. Pin PDFium to a release compatible with `pdfium-render` 0.9.1 with `PDFIUM_SHA256`, so
   CI and releases are reproducible (today it floats on `latest`)

**Done when:** a PR that breaks formatting, adds a clippy warning, fails a test or adds a
vulnerable or badly licensed crate cannot go green.

### W2 — Workspace lints (S)

Add `[workspace.lints]` to `Cargo.toml` and `lints.workspace = true` in both crates:

- `rust`: `unsafe_op_in_unsafe_fn = "deny"`, `missing_debug_implementations = "warn"` (core)
- core: `#![forbid(unsafe_code)]` in `lib.rs` (it has none today; keep it that way)
- clippy: `unwrap_used = "warn"` in core and in `native/src/app/`, `dbg_macro`, `todo`,
  `print_stdout`, `print_stderr = "warn"` (drives W4), `large_enum_variant`,
  `needless_pass_by_value`
- Every `unsafe` block in `platform.rs` / `pen_win.rs` gets a `// SAFETY:` comment
  (`clippy::undocumented_unsafe_blocks = "deny"` for those files)

### W3 — Typed errors (M)

- Core gets one error type per module with `thiserror` (already in the tree via iced):
  `VaultError { NotFound, OutsideVault, PermissionDenied, Io, InvalidName, … }`,
  `PdfError`, `StateError`, `PptxError`.
- Public core functions return them; `Display` keeps today's user-facing wording, so toasts
  read the same.
- Native matches on kinds where behaviour should differ (e.g. *permission denied* on save →
  persistent error toast with Retry; *not found* on open → remove from tabs/recents).
- Migrate module by module (vault → state → pdf → pptx); the 78 `Result<_, String>` count is
  the progress metric.

### W4 — Logging and diagnostics (S)

- `log` facade + a small file logger (rotating, in the data dir: `md-editor.log`, 1 MB ×3);
  level from `MD_EDITOR_LOG` (default `info`).
- Replace the 34 `eprintln!`/`println!` (CLI install output excepted).
- Panic hook: log the panic with a backtrace, then the default hook. (Flushing unsaved
  buffers on panic is a data-safety item, out of scope here.)
- Key events logged at `info`: vault open/index timing, PDFium bind result and version,
  save failures, watcher errors. `debug`: per-message timing (reusing the e2e span
  wrapper idea) behind the env var.

### W5 — Message architecture (M)

Replace the flat 168-variant `Message` and prefix routing with nested enums:

```rust
enum Message {
    Pdf(PdfMessage), Graph(GraphMessage), Ink(InkMessage), Search(SearchMessage),
    Editor(EditorMessage), Vault(VaultMessage), Shell(ShellMessage), Shortcut(Shortcut),
}
```

- `update_inner` becomes a 9-arm exhaustive match; each `update_<family>` matches its own
  enum **exhaustively**, deleting the silent `_ => Task::none()` catch-alls (a misrouted or
  forgotten message becomes a compile error).
- Views map child messages with `.map(Message::Pdf)`, so a view can only emit its family.
- Mechanical, one family per PR, e2e suite green after each.

### W6 — Decompose the giants (M)

| Function | Split into |
| :--- | :--- |
| `update_pdf` (884) | `pdf_render.rs` (load, render, evict, fit), `pdf_select.rs` (selection, highlights, quick notes, linked notes), `pdf_nav.rs` (links, references, previews, page navigation); each handler ≤ 150 lines |
| `PdfRenderer::new` worker (636) | a `Worker` struct owning the document cache and queues, one method per `PdfCommand`; `new` only spawns it |
| `shell_view` (456) | one function per region (toolbar, sidebar, main pane, side panels, overlays) — lands naturally with UI overhaul Phase 2 |
| `update_vault` (453) | vault lifecycle, tree/selection, entry operations (create/rename/delete), name modals |
| `DocBuffer` impl (870) | `buffer/{mod, edit, movement, undo, format, lists}.rs` with `impl DocBuffer` blocks per concern |
| `views/icons.rs::draw` (332) | deleted by the UI overhaul (Phosphor SVGs) |
| View builders with 9–13 args | take a small `…Props` struct; removes all `too_many_arguments` allows |

Rule after W6: no function over 150 lines and no module over ~800 lines outside tests,
checked by a test that scans the tree (same pattern as the UI token lint).

### W7 — State ownership (M)

Move logic that touches only one sub-state onto it (`PdfPane`, `SearchState`,
`VaultState`, `GraphState`, `InkState` get `update` methods for their own messages, as
`TrackerState` and `UiState` already do). The shell keeps only cross-cutting effects
(opening documents, saving, backlinks). Target: each `app/<family>.rs` handler is mostly
delegation plus cross-pane glue.

### W8 — Tests where the risk is (M, ongoing)

| Module | Plan |
| :--- | :--- |
| `pdf/toc.rs` (1,003, untested directly) | unit tests for leader-line parsing, numbered-heading depth, delta learning, using text fixtures (no PDFium needed) |
| `pptx/slide.rs` (910) | fixture-driven tests per element kind (pictures, tables, groups, placeholders) extending `pptx/tests.rs` |
| `pdf/renderer.rs` | command-level tests through the `Worker` struct after W6 (render, search, cancellation, stale generations) |
| `app/*` | covered by the e2e scenario suite ([bug discovery plan](e2e-bug-discovery-plan.md) S1–S17) |
| `ink/canvas.rs`, `views/gpu_graph.rs` | snapshot tests through the e2e screenshot path |
| `vault/files.rs` | property tests for path jailing (`..`, symlinks, absolute, Windows separators) |

Add `cargo llvm-cov` to a nightly CI job and publish the report; no hard coverage target,
but new modules must ship with tests.

### W9 — Panic and unsafe audit (S)

- Native's 31 `unwrap`/`expect`: replace those on I/O, user input or async results; keep
  invariant ones with a message stating the invariant.
- Confine `unsafe` to `platform.rs` and `ink/pen_win.rs` (already true); document each
  block (W2).

---

## 3. Order

| Step | Workstreams | Why |
| :--- | :--- | :--- |
| 1 | W1 CI gate, W2 lints | Everything after is protected and measured |
| 2 | W4 logging, W9 audit | Small, immediate field value |
| 3 | W5 message architecture | Mechanical, best done before the UI overhaul touches every view |
| 4 | W6 decomposition, W7 ownership | Alongside UI overhaul Phases 1–2 (`shell_view` is rewritten there anyway) |
| 5 | W3 typed errors | Before the tabs feature, which needs "not found" vs other errors |
| 6 | W8 tests | Continuous; each W6 split lands with tests for what it extracted |

## 4. Definition of done (whole plan)

- CI gate green and required on `main`; clippy at 0 or every remaining allow justified.
- No `Result<_, String>` in core's public API; no `eprintln!` outside the CLI installer.
- `Message` routing exhaustive; no silent catch-alls.
- No function > 150 lines, no non-test module > ~800 lines (enforced by test).
- Every module over 300 lines has tests in or beside it, or is listed as e2e-covered.
