# Plan Ledger

Tracks task status for `docs/Md editor overhaul plan.md` + UX addendum.
Statuses: ✅ done · 🔶 partial · ⬜ not started · ❌ blocked

## Phase 0 — Baseline & Safety Net

| Task | Status | Notes |
|---|---|---|
| P0.T1 fix quality.yml | ✅ | Corrupted step block rewritten (metrics → budget → unwrap-budget as separate steps); YAML validated with `yaml.safe_load`. |
| P0.T2 debris removal | ✅ | `git rm`: update.patch, my_diff.patch, diff.json, issue_ui.png, dummy.pdf, skills-lock.json, ORIGINAL_REQUEST.md (content preserved in docs/HISTORY.md), native/src/{dummy.rs,dummy_test.rs,test_scroll.rs}. None were declared as modules; no code changes needed. |
| P0.T3 test relocation | ✅ | massive_tests.rs split verbatim: 3 link-graph tests → file_index.rs (`link_graph_scale_tests`), config upserts → config.rs, tracker sessions/KV → tracker.rs, 3 vault tests → vault.rs (`vault_scale_tests`). No assertions changed. core/tests/smoke.rs added; native is bin-only so native/tests/smoke.rs uses CARGO_BIN_EXE link check (limitation recorded). Test modules will travel with their files when Phase 2 relocates them. |
| P0.T4 characterization tests | ✅ | `native/src/app/characterization_tests.rs`, 20 tests over shell/workspace/editor/search/overlays/tracker/pdf message variants. Plain asserts instead of insta (self-verifying, no snapshot-accept round — deliberate deviation). Theme toggle skipped: theme is process-global (`app_theme::set_active_theme`), unsafe to characterize across parallel tests; replaced with pdf-selection + window-resize coverage. `app/model_tests.rs` (2,702 lines) already acts as a broad characterization suite alongside. |
| P0.T5 PDF fixture corpus | ✅ | 6 committed fixtures + 500-page CI-only (`--large`, gitignored); validated with pdftotext incl. CJK extraction; byte-deterministic across runs. |
| P0.T6 ledger | ✅ | This file. |

## Phase 1 — Documentation & Guardrails

| Task | Status | Notes |
|---|---|---|
| P1.T1 CODING_STANDARDS.md | ✅ | 10 concrete standards with good/bad examples. `thiserror` dep not added yet — added when Phase 2 introduces typed errors (no point depending on it unused). |
| P1.T2 ARCHITECTURE_RULES.md + enforcement | ✅ | architecture-check.sh extended: core↛winit, native↛pdfium_render, cross-feature import ban with shrink-only allowlist (3 pre-existing violations listed). Proven by injecting violations (core iced import + shell→tracker import both failed the script, then restored green). Added rg→grep fallback shim — script previously *silently passed* when ripgrep was missing. budgets.toml committed (file ceilings + unwrap + raw-color ratchets); check-budget.sh now pass/fail against it. |
| P1.T3 ADRs | ✅ | 0001 keep-iced, 0002 pdfium-via-core, 0003 sqlite-sidecar, 0004 elm-with-features + index. |
| P1.T4 TESTING.md | ✅ | Pyramid, characterization policy, fixture docs, pre-handoff commands. |
| P1.T5 unwrap ratchet | ✅ | scripts/unwrap-budget.sh; production count is **8** (plan's 244 figure included test code). Ceiling=8 in budgets.toml; wired into quality.yml. `// INVARIANT:` escape hatch honored. |

## Phase 2 — Core Unification

| Task | Status | Notes |
|---|---|---|
| P2.T1 domain extraction from types.rs | ✅ | types.rs **deleted** (went beyond plan): FileEntry→domain/note.rs, search types→domain/search.rs, Backlink*→domain/links.rs. All 94 native + 7 core callers migrated to `domain::`; transitional `pub use domain as types;` alias left in lib.rs (dies P2.T6). |
| P2.T2 VaultService consolidation | 🔶 | vault.rs fs/CRUD moved verbatim to vault/fs.rs; vault.rs is now mod decls + re-exports + scale tests (280 lines, was 821). VaultService unchanged (already the façade). Remaining: migrate native's direct `md_editor_core::vault::*` calls to VaultService; tempdir/unicode/symlink tests. |
| P2.T3 state.rs dissolution | 🔶 | Config-dir/portable/settings-db-path logic + tests moved to infrastructure/config_store.rs (absorbs config.rs; config.rs is a shim now). state.rs 373→~250 lines. Remaining: classify the PDF-repository delegation methods (Phase 5 will move them into PdfService/AnnotationService). |
| P2.T4 TrackerService | ✅ | application/tracker_service.rs is the only public entry; StudySession/TrackerKv live in domain/session.rs; tracker.rs free fns now pub(crate); all 4 native callers migrated (update.rs, startup.rs, features/tracker.rs, views/tracker.rs). |
| P2.T5 SearchService + indexer | 🔶 | file_index.rs → infrastructure/indexer.rs (was core-internal only; no native impact). SearchService façade exists. Remaining: IndexProgress/scope API from the plan (Phase 6 work anyway). |
| P2.T6 legacy deletion + API freeze | ⬜ | Blocked on T2/T3 leftovers; `#![warn(missing_docs)]` not yet enabled. |
| — | | `cargo check -p md-editor-core --all-targets` green after all moves. |

## Phases 3–10 + UX track

Not started except where noted below.

| Task | Status | Notes |
|---|---|---|
| UXA.T2 design tokens | ⬜ | |

## Known-bug register (root-caused 2026-06-10, user-reported)

| ID | Symptom | Root cause | Fix home |
|---|---|---|---|
| BUG-A | Ctrl+Z opens go-to-page/zoom input instead of undo | Global `iced::keyboard::listen()` in `app/subscription.rs` maps Ctrl+Z→`Shortcut::PdfZoomInput` unconditionally while the editor widget separately binds Ctrl+Z→`EditorCommand::Undo` (`renderer/widget.rs:2077`); both fire, no focus scoping | v2 hotfix: gate the PDF chord block on PDF-pane focus/`active_panel`. Structural: InputRouter (V3 plan §3.1) |
| BUG-B | Clicking a line expands it onto the next line; overflow text paints over the line below | Cursor entering a line reveals concealed markers → line re-wraps taller, but per-line layout cache (`layout_cache.rs`) has no document reflow protocol — subsequent line offsets stay stale → overdraw | v2 hotfix: invalidate subsequent offsets on height change. Structural: 3-phase layout + height sum-tree (V3 plan §3.2) |
| BUG-C | PDFs only open in split view, never standalone | No workspace model: `split_view_active`/`showing_pdf`/`active_panel`/two `active_path`s must be hand-synced; PDF-link open hardcodes `split_view_active = true` (`update.rs:110`) | Structural only: PaneTree workspace model (V3 plan §3.1) |

## V3 ground-up plan

`docs/V3_GROUND_UP_PLAN.md` — fresh-eyes, 10–20 dev / 12–18 month rebuild plan written
2026-06-10 at user request: workspace kernel (PaneTree/FocusModel/InputRouter/CommandBus),
3-phase editor layout protocol, tile-based PDF engine, watcher-driven vault core, toolkit
bake-off, 6 squads, milestones M0–M5. The incremental plan in this repo remains the
1–2 person path; the bug register above is the evidence base shared by both.

**Execution started 2026-06-10** in the `v3/` workspace — see `docs/V3_HANDOFF.md`
(the authoritative v3 ledger). M0 + the kernel/editor halves of M1 are built: all three
bugs are killed by construction in v3 and pinned by named regression suites
(bug_a/bug_b/bug_c test files). 47 tests, clippy/fmt clean, CI `v3` job added.

## Standing backlog / findings

- Old docs/ tree was deleted in the working tree before this overhaul began (user action); replacement docs are written fresh by Phase 1 tasks.
- `native/src/main.rs` carries a large `#![allow(...)]` clippy block — candidates for burn-down in Phase 1's lint budget work.
- `features/pdf/` already exists and is substantial (update.rs 1,873 lines) — Phase 5's "three legacy views" coexist with it; the P5.T1 audit must map all four.
