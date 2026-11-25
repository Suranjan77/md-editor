# Code Health Plan

**Goal:** make MD Editor cheaper and safer to change: smaller units, typed errors, real
logging, an enforced quality gate, and tests where the risk is. No behaviour changes except
where a workstream says so.

**Date:** 2026-11-23 · **Status:** in progress — W1, W2, W4, W9 done (2026-11-24); W3, W5 and W10 (native install, Arch Linux only) done 2026-11-24; see §5. Companion to the
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

### W10 — Native install on Arch Linux, portability dropped (M)

**Decision (2026-11-24):** MD Editor no longer needs to run as a portable folder. It becomes
a normally installed program. **Scope: Arch Linux only for now** — Windows and macOS
packaging are out of scope; existing Windows code and CI stay as they are but get no new
work. This reverses *Pillar 2: Zero-Configuration Portability* (wiki *Architecture and
Philosophy*).

**Where things live (XDG)**

| What | Location |
| :--- | :--- |
| Settings DB (study history, annotations, sidecars) | `$XDG_DATA_HOME/md-editor/` (default `~/.local/share/md-editor/`) |
| Log (`md-editor.log`, W4) | `$XDG_STATE_HOME/md-editor/` (default `~/.local/state/md-editor/`) |
| Binary | `/usr/bin/md-editor` |
| PDFium | `/usr/lib/md-editor/libpdfium.so` |
| Desktop entry, icons | `/usr/share/applications/md-editor.desktop`, `/usr/share/icons/hicolor/<size>/apps/md-editor.png` |
| Licence | `/usr/share/licenses/md-editor/LICENSE` |

- `core/src/state.rs::data_dir` stops preferring the executable's directory and the
  writability probe goes; resolution is `MD_EDITOR_DATA_DIR` (kept, for tests and throwaway
  profiles) → `$XDG_DATA_HOME/md-editor` → `~/.local/share/md-editor`. Add
  `state::log_dir()` (`$XDG_STATE_HOME` → `~/.local/state`) and point the W4 logger at it.
  Relative `XDG_*` values are ignored, per the spec (already done for `XDG_DATA_HOME`).
- **One-time migration, the other way round:** if the XDG DB does not exist but a
  `md_editor_settings.sqlite` sits beside the executable, copy it (with `-wal`/`-shm`) into
  `~/.local/share/md-editor/` and leave the original in place. Delete today's "migrate back
  to portable" path (`migrate_legacy_db` from the per-user location).
- `pdf.rs::bind_pdfium` candidates: `/usr/lib/md-editor/` (found as `../lib/md-editor/`
  relative to the exe, so a `/usr/local` or `$pkgdir` prefix also works), then the exe dir
  and `exe/resources` (dev builds), then the system loader path.

**Packaging**

- A `PKGBUILD` in `packaging/arch/`, published to the AUR as `md-editor` (builds from a
  release tag) — and optionally `md-editor-git` (builds `main`).
  - `depends`: `gcc-libs`, `glibc`, `fontconfig`, `freetype2`, `libxkbcommon`, `wayland`,
    `vulkan-icd-loader` (check with `namcap` and `ldd` on the built binary).
  - `makedepends`: `rust` (or `rustup`), `cargo`.
  - `build()` runs `cargo build --release --locked` with the W1 PDFium pin, so the
    checksum-verified library is what gets installed. (Alternative: depend on the AUR
    `pdfium-binaries` package instead of bundling — decide when writing the PKGBUILD;
    bundling keeps the tested version.)
  - `package()` installs the files in the table above; the desktop entry declares
    `MimeType=text/markdown;application/pdf;` and `Exec=md-editor %F`.
  - `check()` runs `cargo test --workspace --locked`.
- The self-installer goes: `--install` / `--uninstall` and `install_linux_desktop_entry*`
  are removed from `main.rs` (pacman owns desktop integration and the icon/desktop caches
  are refreshed by pacman hooks). A path argument still opens a file or vault.
- CI: a job that builds the package in an `archlinux:latest` container with `makepkg` and
  runs `namcap` on the PKGBUILD and the package; the release workflow attaches the built
  `.pkg.tar.zst` to tagged releases alongside (or instead of) the Linux tarball.

**Tests and docs**

- Unit tests: data/log dir resolution with `XDG_*` and `HOME` overrides (absolute,
  relative, unset); migration from a beside-exe DB (copied once, sidecars included,
  original kept, no-op when the target exists); `bind_pdfium` candidate order.
- Release Checklist: replace *First-run portability check* with `makepkg -si` → first run
  (data in `~/.local/share/md-editor/`, log in `~/.local/state/md-editor/`) → upgrade over
  an existing install (data kept) → `pacman -R md-editor` (data kept, desktop entry and
  icons gone) → app appears in the launcher and opens `.md` files from the file manager.
- Rewrite the portability sections in README, wiki *Architecture and Philosophy* (Pillar
  2), *Core Services* §3, *Developer Guide* §5 and *Release Checklist*, and the user guide's
  "travels as one portable folder" line. Installation docs lead with the AUR.

**Done when:** `makepkg -si` (and an AUR helper) installs MD Editor, it launches from the app
launcher, nothing is written beside the executable, `namcap` is clean, and an existing
portable user's settings, highlights and study history appear after upgrading.

---

## 3. Order

| Step | Workstreams | Why |
| :--- | :--- | :--- |
| 1 | W1 CI gate, W2 lints | Everything after is protected and measured |
| 2 | W4 logging, W9 audit | Small, immediate field value |
| 2b | W10 native install (Arch Linux) | Settles where data and logs live before more code depends on it; the package rides on the W1 PDFium pin |
| 3 | W5 message architecture | Mechanical, best done before the UI overhaul touches every view |
| 4 | W6 decomposition, W7 ownership | Alongside UI overhaul Phases 1–2 (`shell_view` is rewritten there anyway) |
| 5 | W3 typed errors | Before the tabs feature, which needs "not found" vs other errors |
| 6 | W8 tests | Continuous; each W6 split lands with tests for what it extracted |

## 4. Definition of done (whole plan)

- CI gate green and required on `main`; clippy at 0 or every remaining allow justified.
- No `Result<_, String>` in core's public API; no `eprintln!` in non-test code.
- Installed on Arch Linux via a PKGBUILD/AUR package; nothing written beside the executable.
- `Message` routing exhaustive; no silent catch-alls.
- No function > 150 lines, no non-test module > ~800 lines (enforced by test).
- Every module over 300 lines has tests in or beside it, or is listed as e2e-covered.

## 5. Progress

**2026-11-24 — W1, W2, W4, W9 landed.**

- W1: `.github/workflows/ci.yml` (fmt; clippy ratchet on Linux, plain clippy on Windows;
  tests incl. e2e with `target/e2e/` uploaded; `cargo audit`; `cargo deny check licenses
  bans` with `deny.toml`). `scripts/clippy-ratchet.sh` + `clippy-baseline.txt` (5, all
  `too_many_arguments` → W6 Props structs). PDFium pinned to `chromium/8165` with
  per-platform SHA-256 in `core/build_pdfium.rs`. Audit went from 18 advisories to 0:
  `cargo update`, `rusqlite` 0.39 → 0.43 (bundled SQLite CVE), `zip` 8 → 9.
- W2: `[workspace.lints]` + `clippy.toml`; core `forbid(unsafe_code)`,
  `missing_debug_implementations`, `unwrap_used`; `unwrap_used` in `native/src/app/`;
  `undocumented_unsafe_blocks` denied in `platform.rs` / `ink/pen_win.rs`.
  `needless_pass_by_value` was **not** enabled: it flags the Props-by-value pattern W6
  prescribes and iced's by-value events.
- W4: `native/src/logging.rs` (rotating file log, `MD_EDITOR_LOG`, panic hook); all
  `eprintln!`/`println!` outside the CLI installer, examples and tests replaced; PDFium
  bind, vault index timing and save failures logged.
- W9: native non-test `unwrap`s removed (sidebar); remaining `expect`s state their
  invariant.
- Licence settled: `GPL-3.0-or-later` (workspace `license`, `deny.toml`, README); it was
  `MIT` in `Cargo.toml` while `LICENSE` was GPL-3.0.

**2026-11-24 — W10 landed (Arch Linux).**

- `state.rs`: `data_dir()` → `$XDG_DATA_HOME/md-editor`, new `log_dir()` →
  `$XDG_STATE_HOME/md-editor` (logger uses it); beside-exe probe and the reverse migration
  removed; `portable_db_path()` feeds the one-time copy into the XDG location.
- `pdf.rs::pdfium_candidates`: `<exe>/../lib/md-editor/` first.
- `main.rs`: `--install`/`--uninstall` and the desktop-entry writer removed.
- `packaging/arch/`: `md-editor` (tagged release; checksum `SKIP` until `v1.0.0` is tagged)
  and `md-editor-git` PKGBUILDs + `.SRCINFO`, desktop entry. `PDFIUM_ARCHIVE` lets the
  PKGBUILD supply the pinned archive as a declared source.
- `scripts/build-arch-package.sh` and the CI `arch-package` job (makepkg incl. `check()`,
  `namcap`, artifact upload).
- Not done: `directories` crate (hand-rolled XDG is enough on Linux-only); publishing to the
  AUR and tagging `v1.0.0` are owner actions.

**2026-11-24 — W5 landed.**

- `Message` is now `Vault | Graph | Shell | Editor | Pdf | Search | Ink | KeyboardShortcut`,
  each wrapping a family enum (`VaultMessage`, …) with `From` impls. Variant names lost
  their family prefix (`PdfLoaded` → `PdfMessage::Loaded`). Families follow the old router
  grouping, so e.g. tracker and presentation messages are `ShellMessage`.
- `update_inner` is an eight-arm exhaustive match; every `update_<family>` takes its own
  enum and matches it exhaustively; `update_shortcuts` takes `Shortcut`. All nine
  `_ => Task::none()` catch-alls are gone. Removing them surfaced one unhandled shape,
  `VaultOpened(None)` (folder picker cancelled), now an explicit no-op.
- Done with a parenthesis-aware rewrite of all 655 use sites; tests and clippy unchanged.
- Not yet: views still build `Message::Pdf(PdfMessage::…)` directly instead of returning
  `Element<PdfMessage>` and being `.map(Message::Pdf)`-ed, and sub-states (`UiState`,
  `TrackerState`, `PdfPane`, `PptxPane`, `SearchState`, `VaultState`, `EditorState`) still
  take `Message` with catch-alls. Both move with W7.

**2026-11-24 — W6 (part).** View builders with 9–13 arguments now take `ToolbarProps`,
`SearchProps`, `TrackerProps`, `ContinuousProps`; `InteractivePdf::new` takes a
`PageEvents` callback struct. **Clippy is at 0** (baseline file updated). The six
`#[allow(clippy::too_many_arguments)]` in the editor renderer (hot paths) remain, as do the
function splits (`update_pdf`, the PDFium worker, `update_vault`, `DocBuffer`).

**2026-11-24 — W3 landed.** Core's public API has no `Result<_, String>` left:
`vault::VaultError` (vault, files, graph, attachments; `is_not_found()`,
`is_permission_denied()`), `state::StateError` (state, tracker, config; `rusqlite` and
lock errors via `From`), `pdf::PdfError` (`Skipped`, `PasswordRequired`, `WorkerGone`,
`Other` — replaces the app's `err == "Skipped"` string check), `pptx::PptxError`. `Display`
keeps the old wording, so toasts read the same; each converts to `String` for the native
shell's message payloads (which must be `Clone`). `SaveOutcome::Failed` keeps the typed
`VaultError`. Still stringly: the PDFium worker's internal channels (typed at the
`PdfRenderer` boundary; restructured with W6) and `pub(super)` helpers.


**2026-11-24 — W6: `update_pdf` split.** `app/pdf.rs` is now only the router (~80 lines);
handlers moved to `app/pdf_render.rs` (load, zoom/fit, rendered pages, scrolling, TOC),
`app/pdf_nav.rs` (link clicks and previews, PDF search results, document id → highlights
and references, annotation jumps) and `app/pdf_select.rs` (selection, copy, highlights,
quick and linked notes, orphan report; the highlight helpers moved there from
`pdf_pages.rs`). `ZoomChanged` and `FitToWidth` share `apply_pdf_zoom`; the longest PDF
handler is now ~70 lines. One behaviour change: a failed `save_pdf_document` is logged
instead of silently ignored. Still to do in W6: the PDFium worker, `update_vault`,
`DocBuffer`, and the size-lint test.

**2026-11-24 — W6: worker, vault, `DocBuffer` and core splits.** The PDFium worker loop is
`core/src/pdf/worker.rs` (one handler per command); TOC synthesis is `pdf/toc_synth.rs`;
PDF sidecar queries moved to `state/pdf_store.rs`; `vault/backlinks.rs` and `vault/rename.rs`
split out of `vault.rs`; `pptx/slide_content.rs` out of `slide.rs`. `update_vault` delegates
to `app/vault_entries.rs` and `app/vault_links.rs`. `DocBuffer` is `editor/buffer/` (`edit`,
`format`, `lists`, `movement`, `undo`). Still to do in W6: the size-lint test (no function
> 150 lines, no non-test module > ~800 lines — `app/view.rs` is ~1100 lines today).
Along the way the subscription's keyboard listener passes its context flags through
`Subscription::with` (a capturing closure does not compile for `listen_with`), and the UI
gallery test holds the e2e lock, since a headless renderer created alongside an emulator
session crashed the test binary.

**2026-11-25 — W6 done: size lint at zero.** The size lint (`native/src/size_lint.rs`) now
passes with both ratchet lists empty: no non-test module over ~800 lines and no function
over 150 lines in `core` or `native`. The 17 functions and 3 modules on the list at landing
were split: the shell, search, graph and subscription handlers (`update_shell` stays an
exhaustive router with no catch-alls); `UiState::update`, the status bar and tab strip;
the PDF page list (`view_continuous` → `empty_viewer`, `rendered_page`,
`placeholder_page`, `ContinuousProps::overlay_rects`); the GPU graph (wgpu side in
`views/gpu_graph/pipeline.rs`, input handling in `commit_view` / `on_left_press` /
`on_cursor_moved`); graph filtering and insights in `graph_state/queries.rs` and the
initial layout in `layout_island` / `scatter_islands`; ink pen input in `ink/input.rs`
and the canvas `draw` in three passes; slide paragraph layout; and the highlighter
(`parse_inline_spans` walks a table of inline parsers, `highlight_line` and
`highlight_with_state` delegate per block kind). The frozen test oracle
`editor/highlight/reference.rs` is excluded from the lint, like the other test-only files.
No behaviour changes; the oracle and incremental-highlight tests and a 3000-case renderer
property run pass.
