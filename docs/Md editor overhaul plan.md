# MD-Editor Overhaul Plan (pdf-improv → v2)

**Audience:** Any contributor, including low-capability LLM agents working one task at a time.
**Branch baseline:** `pdf-improv` as of June 2026 (~42k LOC Rust, workspace = `core` + `native`).
**Read this whole header before doing ANY task. Then do exactly one task, verify it, and stop.**

---

## 0. How To Use This Plan (rules for agents)

1. **One task per session.** Each task below has an ID like `P2.T4`. Do only that task. Do not "also fix" unrelated things you notice — file them as new tasks in `PLAN-NOTES.md` instead.
2. **Definition of Done (DoD) is mandatory.** A task is complete only when every DoD checkbox passes. If you cannot make one pass, stop and write what blocked you in `PLAN-NOTES.md`.
3. **Always run before handoff:**
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ./scripts/architecture-check.sh
   ```
4. **Never** edit generated files, `Cargo.lock` by hand, or anything in `target/`.
5. **Commit format:** `phaseN(scope): summary` e.g. `phase2(core): move tracker state into core::tracker`. One logical change per commit.
6. **Phases are strictly ordered.** Do not start Phase N+1 tasks until Phase N's exit criteria (listed at end of each phase) are all green. Tasks *within* a phase marked `[parallel-ok]` may be done in any order; others must be done in listed order.
7. **Terminology contract (do not violate):**
   - `page_index` = 0-based PDF page; `page_number` = 1-based label.
   - `vault_path` = vault-relative path; `abs_path` = absolute filesystem path.
   - All Markdown buffer mutations go through `EditorCommand`; `buffer.set_text` only for file load / whole-buffer transactions.
   - Parsing lives in the parser modules; the renderer never gains parsing rules.
   - No `unwrap()`/`expect()` outside `#[cfg(test)]` code; use `?`, `Result`, or documented invariants.

---

## 1. Current-State Assessment (verified against the repo)

**Architecture problems**
- `core` contains two parallel, half-finished architectures: legacy flat files (`vault.rs` 565 lines, `state.rs` 373, `file_index.rs`, `tracker.rs`, `types.rs`) AND a newer layered layout (`domain/`, `application/`, `database/`, `vault/`, plus an empty `infrastructure/mod.rs`). Both are live; callers mix them.
- `native` carries almost all business logic. `app/update.rs` (2,442 lines) is a god-reducer; `app/view.rs` (1,131), `editor/renderer/widget.rs` (2,264), `views/tracker.rs` (1,176), `views/pdf_annotations.rs` (1,098) are god-files.
- A `features/` layer exists (`shell.rs`, `editor.rs`, `search.rs`, `workspace.rs`, `system.rs`, `tracker.rs`, `overlays.rs`) but `update.rs`/`effects.rs`/`view.rs` still centralize everything — another half-finished migration.
- Three PDF view modules overlap: `pdf_viewer.rs`, `interactive_pdf.rs`, `pdf_annotations.rs` (~3,000 lines combined) with duplicated geometry/selection logic.

**Hygiene problems**
- Debris committed to repo root and src: `dummy.rs`, `dummy_test.rs`, `test_scroll.rs`, `massive_tests.rs` (623-line grab-bag), `dummy.pdf`, `update.patch`, `issue_ui.png`, `skills-lock.json`, `ORIGINAL_REQUEST.md`.
- 244 `unwrap()` occurrences across `.rs` files despite the AGENTS.md rule.
- `.github/workflows/quality.yml` is **syntactically broken** (a stray `n` and an interleaved "Check budget" step around the architecture-metrics step) — CI for that job cannot be trusted.
- AGENTS.md references `docs/CODING_STANDARDS.md` and `docs/ARCHITECTURE_RULES.md`, which do not exist.
- Tests are concentrated in two mega-files (`core/src/massive_tests.rs`, `native/src/app/model_tests.rs` 2,702 lines) instead of living beside the code they test. No integration-test directory, no PDF fixture corpus beyond `dummy.pdf`.

**Product gaps (breadth)** — from README claims vs code: no undo history persistence, no multi-tab/split editing model abstraction, no plugin/extension story, no export (HTML/PDF), PDF annotations are sidecar-only with no export/import, search has no index persistence strategy beyond SQLite basics, no telemetry-free crash reporting, no packaging (only a Windows build workflow + `.desktop` file), no accessibility pass, no i18n.

**Verdict:** Do **not** rewrite from scratch. The editor/renderer/pdfium plumbing is the hard-won part. Instead: stabilize → unify the two half-architectures → split god-files → harden → extend features → release engineering. A staged overhaul preserves working behavior while every phase ends in a shippable state.

---

## 2. Target Architecture (end state)

```
core/                          # zero UI deps; pure logic + persistence
  src/domain/                  # value types: ids, paths, pdf_page, note, annotation, session
  src/application/             # services: VaultService, SearchService, PdfService,
                               #           TrackerService, AnnotationService, LinkGraphService
  src/infrastructure/          # fs watcher, pdfium adapter, sqlite adapter, config store
  src/lib.rs                   # re-exports a stable API surface only

native/                        # iced shell only: views, widgets, messages, theming
  src/app/                     # Model (thin), Message routing, subscriptions
  src/features/<name>/         # per-feature: model.rs, update.rs, view.rs, messages.rs
       editor/ pdf/ search/ workspace/ tracker/ overlays/ shell/
  src/editor/                  # the custom text widget (buffer, parser, renderer) — a library
  src/widgets/                 # reusable iced widgets (toast, palette, modal, status bar)
  src/theme.rs

docs/                          # ARCHITECTURE.md, CODING_STANDARDS.md, ARCHITECTURE_RULES.md,
                               # ADR/ (architecture decision records), TESTING.md, RELEASING.md
tests-fixtures/pdf/            # small CC0 PDFs covering: outline, links, CJK, rotated pages,
                               # encrypted, malformed, 500+ pages
xtask/                         # cargo-xtask for packaging, fixtures, release checks
```

**Dependency rule (enforced by `scripts/architecture-check.sh`, to be extended):**
`native` → `core` only. Inside core: `application` → `domain` + `infrastructure`; `domain` depends on nothing; `infrastructure` → `domain`. Inside native: `features/*` → `app` messages + `core`; features never import each other directly.

---

## 3. Phase Overview

| Phase | Name | Goal | Ends shippable? |
|---|---|---|---|
| 0 | Baseline & Safety Net | CI fixed, repo cleaned, behavior snapshot tests | Yes |
| 1 | Documentation & Guardrails | Docs that AGENTS.md promises; lint budgets; ADRs | Yes |
| 2 | Core Unification | One architecture in `core`; legacy files dissolved | Yes |
| 3 | Native Decomposition | Kill god-files; complete the `features/` migration | Yes |
| 4 | Editor Engine Hardening | Buffer/parser/renderer correctness, fuzzing, perf budgets | Yes |
| 5 | PDF Subsystem Overhaul | Unify 3 PDF views; robust rendering pipeline; annotation model v2 | Yes |
| 6 | Search & Knowledge Graph | Persistent index, incremental updates, backlinks/graph service | Yes |
| 7 | Breadth Features I | Export (HTML/PDF), session restore, multi-pane, settings UI | Yes |
| 8 | Breadth Features II | Annotation import/export, citations v2, tracker analytics, vault sync-safety | Yes |
| 9 | Quality: a11y, i18n, perf | Keyboard-complete UI, screen-reader labels, locale scaffolding, perf CI | Yes |
| 10 | Release Engineering | Installers (win/mac/linux), auto-update strategy, crash handling, v2.0 | Yes |

Estimated effort: each phase is 1–3 weeks of focused agent work; tasks are sized ≤ half a day each.

---

# PHASE 0 — Baseline & Safety Net

**Goal:** make the repo trustworthy: green CI, no debris, and characterization tests that freeze current behavior so later refactors are provably safe.

### P0.T1 — Fix `quality.yml`
- Open `.github/workflows/quality.yml`. The architecture job has a corrupted block: a stray line beginning with `n      - name: Check budget` interleaved with the `architecture-metrics` step.
- Rewrite the job so steps are, in order: checkout → install Rust → `./scripts/architecture-check.sh` → `./scripts/architecture-metrics.sh` → `./scripts/check-budget.sh`, each as its own well-formed step.
- Validate locally: `python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/quality.yml'))"`.
- **DoD:** YAML parses; CI run on a draft PR is green or fails only on pre-existing code issues (record those in `PLAN-NOTES.md`).

### P0.T2 — Repo debris removal `[parallel-ok]`
- Delete: `update.patch`, `issue_ui.png`, `dummy.pdf` (root), `skills-lock.json`, `ORIGINAL_REQUEST.md` (move its content into `docs/HISTORY.md` first), `native/src/dummy.rs`, `native/src/dummy_test.rs`, `native/src/test_scroll.rs`, `core` references to them.
- Search for module declarations (`mod dummy;`, `mod test_scroll;` etc.) in `lib.rs`/`main.rs`/`bin.rs` and remove.
- If any deleted test exercised real behavior, move that test (not the file) into the module it tests under `#[cfg(test)]`.
- **DoD:** `git grep -l "dummy\|test_scroll"` returns nothing outside docs; full check suite passes.

### P0.T3 — Test relocation scaffolding
- Create `core/tests/` and `native/tests/` integration dirs (empty `smoke.rs` each asserting crates link).
- Split `core/src/massive_tests.rs` by subject: tests about vault → `core/src/vault/` modules' `#[cfg(test)]`, tracker tests → `tracker.rs`, etc. Delete `massive_tests.rs` when empty. Do **not** change any assertion.
- **DoD:** test count before == after (`cargo test --workspace -- --list | wc -l` recorded in commit message); `massive_tests.rs` gone.

### P0.T4 — Characterization tests for the reducer
- In `native/src/app/`, add `characterization_tests.rs` (cfg(test)): for the 15 most important `Message` variants (file open, save, search open/next, PDF open, PDF page nav, annotation create, tracker start/stop, command palette open, toast show, toc click, backlink click, theme toggle, vault open, editor insert), construct a default `Model`, apply the message via `update`, and snapshot-assert the resulting model fields that change (use `insta` crate, add as dev-dependency).
- These tests define "current behavior is correct by definition" for Phase 2–3 refactors.
- **DoD:** ≥15 snapshot tests committed with accepted snapshots; suite green.

### P0.T5 — PDF fixture corpus `[parallel-ok]`
- Create `tests-fixtures/pdf/` containing small, license-clean PDFs generated by a script `scripts/gen-fixtures.py` (use `reportlab` or raw PDF bytes) covering: 1-page text, multi-page with outline/bookmarks, internal links, rotated page, Unicode/CJK text, an intentionally truncated/corrupt file, and a programmatically generated 500-page doc. Commit the generator, and commit the small outputs (<200 KB each) except the 500-page one (generated in CI).
- **DoD:** `python3 scripts/gen-fixtures.py` regenerates fixtures deterministically; README in the dir documents each file's purpose.

### P0.T6 — `PLAN-NOTES.md` + task ledger
- Create `PLAN-NOTES.md` at repo root with a table: Task ID | Status | Commit | Notes. Seed with Phase 0 tasks. Every future task updates this file.
- **DoD:** file exists, lists P0 tasks with statuses.

**Phase 0 exit criteria:** CI fully green on the branch; no debris files; characterization snapshots in place; fixtures available; ledger started.

---

# PHASE 1 — Documentation & Guardrails

**Goal:** write the rulebook that AGENTS.md already references, and turn rules into machine checks so weak agents can't drift.

### P1.T1 — `docs/CODING_STANDARDS.md`
Write concrete standards (each with a good/bad code example): error handling (`thiserror` in core, `anyhow` only at binary edge — add deps), no `unwrap` policy + the `// INVARIANT:` escape hatch format, naming conventions (the page_index/page_number and vault_path/abs_path contracts), module size soft limit 400 lines / hard limit 700, function limit 75 lines, public API documentation requirement (`#![warn(missing_docs)]` on core), logging via `tracing` (add dep; replace `println!`/`eprintln!`).
- **DoD:** doc exists; `clippy.toml` updated with `too-many-lines` style guidance where supported; AGENTS.md link resolves.

### P1.T2 — `docs/ARCHITECTURE_RULES.md` + enforce
- Document the dependency rules from §2 of this plan. Extend `scripts/architecture-check.sh` to enforce: (a) `core` has no `iced`/`winit` imports; (b) `native/src/features/X` does not `use crate::features::Y` for X≠Y; (c) file line budgets via `scripts/check-budget.sh` with a committed `budgets.toml` listing current sizes as ceilings (ratchet: numbers may only go down).
- **DoD:** script fails if you add a forbidden import (prove with a temporary commit you then revert); budgets file committed.

### P1.T3 — ADR scaffold `[parallel-ok]`
- `docs/adr/0001-keep-iced.md`, `0002-pdfium-rendering.md`, `0003-sqlite-sidecar-annotations.md`, `0004-elm-architecture-with-features.md`. Each: Context / Decision / Consequences, ~1 page. These freeze decisions so future agents don't relitigate them.
- **DoD:** four ADRs committed; `docs/adr/README.md` index.

### P1.T4 — `docs/TESTING.md` `[parallel-ok]`
- Define the test pyramid for this repo: unit (beside code), integration (`*/tests/`), characterization snapshots, renderer golden-image tests (Phase 4), PDF fixture tests (Phase 5), and the commands to run each. Document `insta` snapshot workflow.
- **DoD:** doc exists and commands in it actually run.

### P1.T5 — Unwrap ratchet
- Add `scripts/unwrap-budget.sh`: counts `unwrap()`/`expect(` outside `#[cfg(test)]` blocks (a simple grep excluding files whose path contains `tests` plus `// INVARIANT:`-annotated lines is acceptable v1). Record current count as ceiling in `budgets.toml`. Wire into quality.yml.
- **DoD:** CI fails if count increases; current count documented.

**Phase 1 exit criteria:** all docs AGENTS.md references exist; architecture/budget/unwrap checks run in CI and pass.

---

# PHASE 2 — Core Unification

**Goal:** one architecture in `core`. Legacy `vault.rs`, `state.rs`, `file_index.rs`, `tracker.rs`, `types.rs` dissolve into `domain/` + `application/` + `infrastructure/`. `native` consumes only the new API.

**Method for every move task:** (1) create new module, (2) move code verbatim, (3) leave `pub use` re-export at old path, (4) run characterization tests, (5) migrate callers, (6) delete re-export. Never combine "move" and "improve" in one commit.

### P2.T1 — Domain extraction from `types.rs`
- Inspect `core/src/types.rs` (137 lines). Move each type into `domain/`: note/file types → `domain/note.rs`; tracker types → `domain/session.rs`; search hit types → `domain/search.rs`. Keep `types.rs` as pure re-exports, marked `#[deprecated(note = "import from domain::*")]` where attribute placement allows.
- **DoD:** clippy green; no behavior change (snapshots untouched).

### P2.T2 — `VaultService` consolidation
- `core/src/vault.rs` (565 lines, legacy) overlaps `core/src/vault/` (paths, search, reference_repair) and the stub `application/vault_service.rs` (45 lines). Plan: move file-tree scan/CRUD from `vault.rs` into `vault/fs.rs` (infrastructure-flavored, pure std::fs); make `application/vault_service.rs` the only public entry (open vault, list tree, create/delete/rename note, resolve `vault_path`↔`abs_path`). `vault.rs` becomes re-exports, then dies in P2.T6.
- Add unit tests: tempdir-based vault with nested folders, unicode filenames, symlink (skipped on Windows), rename collision.
- **DoD:** `native` compiles against `VaultService` only for vault ops (grep proves no `use md_editor_core::vault::vault::` legacy paths); new tests pass.

### P2.T3 — `state.rs` dissolution
- `core/src/state.rs` (373 lines) is app-level state living in core. Classify each item: persistence/config → `infrastructure/config_store.rs` (absorb `config.rs`); per-vault runtime state (open files, index status) → owned by services; UI-ish state (selected tab etc., if any) → moves to `native` model in Phase 3 (leave a `// PHASE3:` marker + re-export for now).
- **DoD:** `state.rs` ≤ re-exports + PHASE3-marked leftovers; ledger lists exactly what remains and why.

### P2.T4 — `TrackerService`
- Merge `core/src/tracker.rs` (49 lines) and `database/tracker_repository.rs` behind `application/tracker_service.rs`: start/stop session, daily totals, study gates. Service owns the repository; repository stays private to core.
- **DoD:** `native/src/views/tracker.rs` and `features/tracker.rs` call only the service; repository type no longer exported from core.

### P2.T5 — `file_index.rs` → `infrastructure/indexer.rs` + `SearchService`
- Move indexing state machine into `infrastructure/indexer.rs`. Flesh out `application/search_service.rs` (currently 20 lines) as the façade over `vault/search.rs` (741 lines — move under `application/search/` or keep as private engine module) and `database/search_repository.rs`.
- Define the public API now (it will gain persistence in Phase 6): `index_vault`, `index_file`, `remove_file`, `query(scope: ActiveFile|Vault|Pdf, text) -> Vec<SearchHit>`, `progress() -> IndexProgress`.
- **DoD:** native's `search.rs` + `features/search.rs` compile against `SearchService` only; old paths gone.

### P2.T6 — Legacy file deletion + API freeze
- Delete `vault.rs`, `state.rs` (if emptied), `file_index.rs`, `tracker.rs`, `types.rs` re-export shells. Curate `core/src/lib.rs` to export exactly: `domain::*` types, the five services, config store, and error types. Enable `#![warn(missing_docs)]` in core and document every public item (rote one-liners acceptable).
- **DoD:** `cargo doc -p md-editor-core` builds with no missing-docs warnings; characterization snapshots still pass unchanged.

**Phase 2 exit criteria:** `core/src` contains only `domain/ application/ infrastructure/ database/ lib.rs` (database may later fold into infrastructure — record as ADR if kept); native imports nothing legacy; all tests green.

---

# PHASE 3 — Native Decomposition

**Goal:** dismantle `app/update.rs` (2,442), `app/view.rs` (1,131), `app/effects.rs` (934) into completed feature modules. The `features/` directory becomes real: each feature owns model+update+view+messages.

### P3.T1 — Message audit & routing table
- Read `native/src/messages.rs` (121 lines — likely a top-level enum) and the big match in `update.rs`. Produce `docs/MESSAGES.md`: a table of every Message variant → owning feature (editor/pdf/search/workspace/tracker/overlays/shell/system). This table is the contract for the splits below. Get the table committed before moving code.
- **DoD:** every variant classified; no "misc" bucket larger than 5 variants.

### P3.T2 — Feature skeleton
- Convert each `features/*.rs` into `features/<name>/{mod.rs, model.rs, messages.rs, update.rs, view.rs}`. Define `pub enum <Name>Msg` per the routing table and a top-level `Message::<Name>(<Name>Msg)` wrapper. Add a temporary compatibility layer translating old flat variants to wrapped ones so the app keeps compiling during migration.
- **DoD:** app builds and runs; characterization tests adjusted only for message wrapping (snapshot values identical).

### P3.T3–T9 — Per-feature extraction (one task each, in this order: shell, workspace, editor, search, tracker, overlays, pdf)
For each feature `X`:
- Move `X`'s model fields out of `app/model.rs` into `features/X/model.rs` (Model holds `pub x: XModel`).
- Move `X`'s match arms from `app/update.rs` into `features/X/update.rs` as `pub fn update(model: &mut XModel, msg: XMsg, ctx: &mut AppCtx) -> Task<Message>` where `AppCtx` exposes core services + cross-feature event emission (define `AppEvent` enum for the rare cross-feature signals, dispatched by the thin root reducer).
- Move `X`'s view code from `app/view.rs` into `features/X/view.rs`; the corresponding `views/*.rs` files become widgets it composes (relocate to `widgets/` if reusable, else into the feature dir).
- Move `X`'s async work from `app/effects.rs` into the feature's `update.rs` as returned `Task`s.
- **DoD per feature:** root `update.rs` no longer mentions X's variants except one routing line; budgets ratchet down in `budgets.toml`; snapshots pass.

### P3.T10 — Root slimming
- After T3–T9: `app/update.rs` ≤ 150 lines (routing + AppEvent fan-out), `app/view.rs` ≤ 150 (layout shell composing feature views), `app/effects.rs` deleted or ≤ 100 (startup tasks only), `app/model.rs` ≤ 150. Remove the compatibility message layer. Update `docs/MESSAGES.md`.
- **DoD:** line counts verified by `check-budget.sh`; full suite + manual smoke (open vault, edit, save, open PDF, search, tracker) recorded in PLAN-NOTES.

### P3.T11 — `views/` → `widgets/` triage `[parallel-ok]`
- Reclassify remaining `views/*.rs`: pure-reusable (toast, modals, command_palette, icons, status_bar, toolbar, focus_button) → `widgets/`; feature-specific (sidebar, toc, backlinks, search, tracker, pdf_*, citation_palette, link_note_picker, welcome, diagnostics) → into their feature dirs. Delete `views/mod.rs` when empty.
- **DoD:** `native/src/views/` no longer exists; imports updated; architecture-check extended to forbid `widgets/ → features/` imports.

**Phase 3 exit criteria:** no file in `native/src` over 700 lines except `editor/renderer/widget.rs` (handled in Phase 4); features are self-contained; CI green.

---

# PHASE 4 — Editor Engine Hardening

**Goal:** make the custom editor (buffer/parser/renderer, ~10k lines) correct, fast, and regression-proof. This is the riskiest code; refactors here lean on new tests first.

### P4.T1 — Buffer property tests
- Add `proptest` dev-dep. For `editor/buffer/` (document, command, transaction, movement, formatting, table): properties — apply(insert/delete) then undo == original; redo == re-applied; any random command sequence keeps rope length consistent with reported length; cursor/selection always within bounds; grapheme-boundary safety for movement on strings containing emoji, CJK, combining marks, CRLF.
- **DoD:** ≥8 properties, 256 cases each, green; any found bug fixed in its own commit with a minimized regression test.

### P4.T2 — Parser conformance suite
- Build `editor/parser/tests/conformance.rs` using a curated subset (~150 cases) of CommonMark spec examples relevant to supported syntax (headings, emphasis, links, code fences, blockquotes, lists/tasks, tables, math). Store cases as `.txt` pairs in `tests-fixtures/markdown/`. Where the editor intentionally diverges, mark the case `# DIVERGES: reason`.
- **DoD:** suite runs in CI; pass/diverge/fail counts in `docs/TESTING.md`; zero "fail" (either fix or formally diverge).

### P4.T3 — Split `renderer/widget.rs` (2,264 lines)
- Decompose along existing seams: event handling → `widget/events.rs` (keyboard/mouse/IME each ≤ 300), `widget/update_state.rs`, `widget/layout.rs` (delegating to measure/layout_cache), `widget/mod.rs` as the iced `Widget` impl wiring (≤ 300). Pure moves only; rely on P4.T1/T2 + characterization tests.
- **DoD:** no behavior change; budgets updated; each new file ≤ 400 lines.

### P4.T4 — Golden-image renderer tests
- Add a headless render path: a test-only function that lays out a document at fixed width/scale and dumps draw commands (not pixels — iced pixel capture is flaky) as a structured text snapshot (`insta`). Cases: heading+paragraph, code fence with highlight spans, table, wrapped long line with cursor mid-grapheme, math block, selection across blocks, scrolled viewport (asserting viewport-bounded layout: command count must not scale with document length — generate a 10k-line doc and assert draw-command count below a constant).
- **DoD:** ≥7 golden snapshots; the viewport-bound assertion passes (this enforces the AGENTS.md "no full-document hot-path scans" rule mechanically).

### P4.T5 — Performance budgets
- Add `criterion` benches: keypress-to-layout on 1k/10k/100k-line docs; full reparse of 1 MB file; search-highlight pass. Record baselines in `docs/PERF.md`. Add `scripts/perf-smoke.sh` (runs reduced benches, fails on >25% regression vs committed baseline JSON) — wire as a manual/weekly CI job, not per-PR.
- **DoD:** baselines committed; smoke script works locally.

### P4.T6 — Editor gaps backlog (implement top 3)
- From a quick audit, implement in separate commits: (1) IME/composition correctness pass (verify preedit rendering and commit; add tests around `unicode-segmentation` boundaries), (2) multi-cursor *internal model readiness* — refactor selection to `Vec<Selection>` with len-1 behavior identical (UI later), (3) configurable soft-wrap width / line numbers toggle plumbed through settings.
- **DoD:** each item has tests; snapshots updated deliberately with reviewer note in commit body.

**Phase 4 exit criteria:** property+conformance+golden suites in CI; widget.rs split; perf baselines exist; zero clippy warnings.

---

# PHASE 5 — PDF Subsystem Overhaul

**Goal:** one coherent PDF feature replacing the `pdf_viewer.rs` / `interactive_pdf.rs` / `pdf_annotations.rs` trio; rendering pipeline robust against bad files; annotation model v2.

### P5.T1 — PDF code audit map
- Produce `docs/PDF-AUDIT.md`: for each of the three modules, list responsibilities (page raster cache, scroll math, hit-testing, selection, link handling, highlight overlay, note popovers, search overlay) and mark duplicates. Decide the target layout: `features/pdf/{model,update,view,messages}.rs` + `features/pdf/widgets/{page_canvas.rs, selection.rs, overlay.rs, toc_panel.rs}` + core-side `application/pdf_service.rs` (already 252 lines) + `infrastructure/pdfium.rs`.
- **DoD:** audit doc committed with a per-function destination table.

### P5.T2 — Core `PdfService` v2
- Consolidate all pdfium calls in core (`infrastructure/pdfium.rs`), exposing: open(abs_path) → DocHandle; page_count; render_page(page_index, scale) → RGBA image; text_in_rect; search(text) → Vec<(page_index, rects)>; outline() → tree; links(page_index). All fallible (`Result<_, PdfError>` via `thiserror`), no panics on the corrupt fixture. Add a worker-thread render queue with cancellation (render requests for pages no longer visible are dropped) — pdfium is already `thread_safe` feature-enabled.
- Tests against the Phase 0 fixture corpus, including: corrupt file returns error (no panic), 500-page doc renders page 250 in <100 ms after open (loose assert, CI-tolerant), CJK text extraction non-empty.
- **DoD:** native no longer imports `pdfium_render` directly (architecture-check rule added); fixture tests pass.

### P5.T3 — View unification
- Rebuild the viewer in `features/pdf/` using PdfService: continuous scroll with virtualized pages (render visible ±1 only; placeholder boxes elsewhere), zoom (fit-width/fit-page/percent), TOC panel, internal-link jumps, selection + copy. Migrate logic from the three legacy files function-by-function per the P5.T1 table; delete legacy files at the end.
- **DoD:** legacy trio deleted; manual smoke checklist (12 items: open, scroll fast, zoom, toc jump, link jump, select across columns, copy, search next/prev, resize window, dpi change, corrupt file error toast, reopen restores page) recorded as passed in PLAN-NOTES; budgets: no pdf file >500 lines.

### P5.T4 — Annotation model v2
- Define in `domain/annotation.rs`: Highlight {id, doc_sha256, page_index, quads: Vec<Rect>, color, created_at, note: Option<String>, linked_note: Option<vault_path>}. Key by document SHA-256 (use existing `sha2` + `integrity.rs` logic) so renamed/moved PDFs keep annotations. Migration: write `database/migrations/` runner (numbered SQL files) and migrate existing sidecar rows; keep a backup table.
- `AnnotationService`: CRUD, list_by_doc, list_by_page, attach/detach note link. Debounced persistence stays (M10 requirement).
- **DoD:** migration tested round-trip on a fixture DB committed under `tests-fixtures/db/`; old API removed.

### P5.T5 — Annotation UI v2
- Highlight via selection → color picker popover; margin indicators per page; side panel listing annotations for the open doc (click → scroll to quad); "link to note" flow using existing `link_note_picker` widget.
- **DoD:** snapshot tests for the panel view fn; smoke checklist passed.

**Phase 5 exit criteria:** single PDF feature; core owns pdfium; annotations keyed by content hash with migrations; fixtures exercised in CI.

---

# PHASE 6 — Search & Knowledge Graph

**Goal:** persistent, incremental search index and a real link-graph service powering backlinks/TOC-adjacent features.

### P6.T1 — Index persistence
- Move full-text into SQLite FTS5 (rusqlite bundled supports it): tables `files(file_id, vault_path, mtime, sha)`, `fts_content(file_id, body)`, `pdf_text(doc_sha, page_index, body)`. Indexer becomes incremental: on startup, diff mtimes/hashes; reindex changed only. Keep existing in-memory query path as fallback behind a config flag for one release.
- **DoD:** cold-start reindex skipped when nothing changed (test with tempdir vault); query results identical to legacy engine on a fixture vault (golden comparison test).

### P6.T2 — FS watcher
- Add `notify` crate in `infrastructure/watcher.rs`; debounce 500 ms; emit IndexEvents consumed by SearchService and the sidebar (file created/deleted/renamed externally updates tree + index). Windows/macOS/Linux behavior documented.
- **DoD:** integration test: create/modify/delete files in a temp vault, assert index converges within 2 s.

### P6.T3 — `LinkGraphService`
- Parse wiki/md links during indexing into `links(src_file_id, dst_vault_path, span)`. Service API: backlinks(file), outlinks(file), broken_links(), rename_repair(old, new) (absorb `vault/reference_repair.rs`). Wire `views→features` backlinks panel to it; add a "broken links" diagnostics view entry.
- **DoD:** rename a note in tests → all referring files rewritten; backlinks panel test green.

### P6.T4 — Search UX completion
- Unify three search modes behind one palette UI with scope toggle (file/vault/pdf); regex toggle; result preview with highlighted context; replace-in-file (with undo as single transaction via EditorCommand).
- **DoD:** replace-all undo restores exact original buffer (property test); snapshots for palette view.

**Phase 6 exit criteria:** FTS5 index persistent & incremental; watcher live; link graph powering backlinks + repair; legacy search engine removed (flag deleted).

---

# PHASE 7 — Breadth Features I

### P7.T1 — Session restore
- Persist via config store: open tabs, active file, scroll positions, PDF page/zoom per doc-sha, window size, sidebar widths. Restore on launch; "reopen closed tab" command.
- **DoD:** integration test serializes/deserializes session struct; manual smoke.

### P7.T2 — Multi-pane / split view hardening
- Formalize a `PaneLayout` model (binary split tree, max depth 3): editor|pdf|image in any pane; drag divider; keyboard pane focus cycling. (README shows split view exists — audit first; this task upgrades it to the tree model and fixes focus routing bugs found in audit, listed in PLAN-NOTES.)
- **DoD:** pane tree unit tests (split/close/focus invariants); messages routed by focused pane.

### P7.T3 — Export: HTML
- `application/export_service.rs`: Markdown → standalone HTML (single file, embedded CSS matching app theme, math rendered via the ratex pipeline to SVG or MathML, code highlighted via syntect's HTML output, images inlined as base64 with size cap + warning). Command-palette entry + file dialog.
- **DoD:** golden HTML snapshots for 3 fixture docs; opens correctly in a browser (manual note).

### P7.T4 — Export: PDF
- Markdown → PDF using a print-style pipeline: simplest robust route is HTML export + headless rendering being unavailable, so implement direct layout → PDF via `printpdf` or pdfium's page-generation if available; choose in an ADR (0005) after a 1-day spike, then implement headings/paragraphs/code/lists/tables/images with page breaks; math as embedded SVG raster fallback.
- **DoD:** ADR committed; 3 fixture docs export and open in a viewer; pagination test (no clipped lines) asserted via produced page count bounds.

### P7.T5 — Settings UI
- Replace ad-hoc config edits with a settings modal: theme, font size, wrap width, line numbers, autosave interval, PDF render scale cap, index exclusions (glob list). All backed by `ConfigStore` with live-apply where feasible.
- **DoD:** every setting round-trips (test); diagnostics view shows effective config.

**Phase 7 exit criteria:** restore/split/export/settings shipped; ADR-0005 recorded; CI green.

---

# PHASE 8 — Breadth Features II

### P8.T1 — Annotation export/import
- Export a doc's highlights+notes to (a) Markdown summary note in vault, (b) JSON. Import JSON (id-collision rules documented). Optional stretch: burn-in export to a copy of the PDF via pdfium annotation APIs — spike first, ADR if infeasible.
- **DoD:** round-trip JSON test; generated Markdown snapshot.

### P8.T2 — Citations v2
- Upgrade `citation_palette`: parse a BibTeX file in the vault (`biblatex` crate or minimal parser) → insert `[@key]`; hover/peek shows formatted reference; export pipeline renders a references section for cited keys.
- **DoD:** parser unit tests incl. malformed entries; export integration test.

### P8.T3 — Tracker analytics
- Weekly/monthly charts (iced canvas) for session time per project/tag; streaks; CSV export. Service-side aggregation queries with tests.
- **DoD:** aggregation unit tests against seeded fixture DB; view snapshot.

### P8.T4 — Vault safety
- Atomic saves (write temp + rename), conflict detection (mtime changed since load → non-destructive prompt with diff view), crash-recovery journal for unsaved buffers (the README's recovery window exists — audit, then back it with a tested journal format), `.trash/` soft-delete for vault file deletion with restore command.
- **DoD:** kill -9 during simulated edit loop recovers buffer (integration test using a child process); conflict prompt path tested.

**Phase 8 exit criteria:** all four shipped with tests; PLAN-NOTES smoke logs updated.

---

# PHASE 9 — Accessibility, i18n, Performance

### P9.T1 — Keyboard completeness
- Audit: every interactive element reachable and operable by keyboard; document the full shortcut map in `docs/SHORTCUTS.md`; add a shortcuts cheat-sheet overlay (`?` or palette). Fix gaps found (each its own commit).
- **DoD:** checklist in docs with all items ✅; cheat-sheet overlay snapshot test.

### P9.T2 — Screen-reader & contrast pass
- Use iced's accessibility support to label widgets where available (audit iced 0.14 a11y status first; if insufficient, record limits in ADR-0006 and do what's possible: focus order, announced toasts via OS notifications option, min contrast 4.5:1 verified for both themes with a small script checking theme.rs color pairs).
- **DoD:** contrast script in CI; ADR-0006 committed.

### P9.T3 — i18n scaffolding
- Introduce `fluent` (or `rust-i18n`) with `locales/en-US.ftl`; sweep all user-facing string literals into it (mechanical task — can be split alphabetically by feature into sub-tasks T3a–T3g for weak agents). No second language required yet; the point is the seam.
- **DoD:** `grep` for raw user-facing literals in view code returns < 20 documented exceptions; app runs identically.

### P9.T4 — Perf hardening
- Profile with the Phase 4 benches + a 5k-file vault fixture generator: startup < 1.5 s warm, index of 5k files < 10 s, memory < 400 MB with 3 PDFs open (record real numbers; budgets are targets — document actuals in `docs/PERF.md` and fix the worst offender found).
- **DoD:** PERF.md updated with measured table; ≥1 concrete optimization landed with before/after numbers.

**Phase 9 exit criteria:** shortcut map complete, contrast CI check, i18n seam in place, perf numbers documented.

---

# PHASE 10 — Release Engineering (v2.0)

### P10.T1 — `xtask` packaging
- `cargo xtask dist` builds: Windows (NSIS or MSIX — extend the existing windows-build workflow), macOS (.app + dmg, ad-hoc signed; notarization documented as manual step), Linux (AppImage + the existing .desktop). Bundles pdfium binaries per-platform (the core build.rs already fetches via ureq — verify license terms and pin checksums).
- **DoD:** CI artifacts produced for all three on tag builds; checksums published.

### P10.T2 — Crash handling & logs
- `tracing` file logs with rotation in the platform log dir; panic hook writing a crash report (no network — local-first); diagnostics view "open log folder" + "copy crash report".
- **DoD:** forced panic in debug command produces report; docs/RELEASING.md describes triage.

### P10.T3 — Update notification (no auto-update)
- On launch (opt-in setting, off by default): check GitHub releases API for newer tag; toast with link. Respect local-first ethos — document in README.
- **DoD:** mocked-response unit test; setting default off verified.

### P10.T4 — Release checklist & v2.0
- `docs/RELEASING.md`: version bump, changelog (keep `CHANGELOG.md` from Phase 0 onward — add retroactively now if missed), tag, CI dist, smoke matrix (the per-phase smoke checklists consolidated), publish. Cut `v2.0.0`.
- **DoD:** tagged release with artifacts; README screenshots/gif refreshed.

**Phase 10 exit criteria:** installable builds for 3 OSes, crash reporting, documented release process, v2.0 shipped.

---

## Appendix A — Standing Backlog (file new findings here, do not act on them mid-task)
- Decide fate of `core/src/database/` vs `infrastructure/` (ADR).
- `native/src/integrity.rs` + `core` hashing duplication — unify in Phase 5.
- `app_shell.rs` (671 lines) vs `features/shell` overlap — resolve in P3.T3.
- Evaluate iced upgrades opportunistically at phase boundaries only.

## Appendix B — Quick command card
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p md-editor-core
./scripts/architecture-check.sh && ./scripts/check-budget.sh && ./scripts/unwrap-budget.sh
python3 scripts/gen-fixtures.py
cargo insta review        # accept/reject snapshots
```