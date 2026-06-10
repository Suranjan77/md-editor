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
| P1.T1 CODING_STANDARDS.md | ⬜ | |
| P1.T2 ARCHITECTURE_RULES.md + enforcement | ⬜ | |
| P1.T3 ADRs | ⬜ | |
| P1.T4 TESTING.md | ⬜ | |
| P1.T5 unwrap ratchet | ⬜ | |

## Phase 2 — Core Unification

| Task | Status | Notes |
|---|---|---|
| P2.T1 domain extraction from types.rs | ⬜ | |
| P2.T2 VaultService consolidation | ⬜ | |
| P2.T3 state.rs dissolution | ⬜ | |
| P2.T4 TrackerService | ⬜ | |
| P2.T5 SearchService + indexer | ⬜ | |
| P2.T6 legacy deletion + API freeze | ⬜ | |

## Phases 3–10 + UX track

Not started except where noted below.

| Task | Status | Notes |
|---|---|---|
| UXA.T2 design tokens | ⬜ | |

## Standing backlog / findings

- Old docs/ tree was deleted in the working tree before this overhaul began (user action); replacement docs are written fresh by Phase 1 tasks.
- `native/src/main.rs` carries a large `#![allow(...)]` clippy block — candidates for burn-down in Phase 1's lint budget work.
- `features/pdf/` already exists and is substantial (update.rs 1,873 lines) — Phase 5's "three legacy views" coexist with it; the P5.T1 audit must map all four.
