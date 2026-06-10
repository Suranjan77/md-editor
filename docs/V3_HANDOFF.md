# V3 Handoff — execution state of docs/V3_GROUND_UP_PLAN.md

> **Read this first when resuming v3 work.** Updated after every completed unit of work.
> Sibling ledgers: `PLAN-NOTES.md` (v2 incremental plan), `docs/V3_GROUND_UP_PLAN.md` (the master plan).

## Ground rules for this execution

The plan is written for 10–20 engineers over 12–18 months. Execution here is by a single
agent, so the plan's *decision points* are collapsed to their stated defaults and the
*architecture* is built in dependency order (kernel → editor → vault/pdf → shell). Every
"squad deliverable" becomes a crate/module with its quality gate expressed as tests —
especially the three named regression suites (BUG-A/B/C) that M1's gate requires.

- v3 lives in `v3/` as an **independent cargo workspace** (root workspace excludes it).
  v2 (`core/`, `native/`) is untouched and remains the shipping app.
- Toolkit: the 3-week bake-off (plan §3.5) cannot be run here; plan's own tie-breaker
  applies — **stay on iced**, editor engine stays toolkit-agnostic via draw commands.
  Recorded as ADR-0100.
- Parser: tree-sitter spike deferred; in-house incremental block parser direction kept,
  re-openable. Recorded as ADR-0101.
- v3 crates: `unwrap`/`expect` banned outside tests (no escape hatch yet), typed errors
  only (`Result<_, String>` banned), `#![deny(warnings)]` not used (CI uses `-D warnings`).

## Status board

| Plan item | Plan ref | Status | Where |
|---|---|---|---|
| v3 workspace scaffold | §5 M0 | ✅ | `v3/` — independent workspace, root excludes it; clippy denies unwrap/expect workspace-wide |
| ADR-0100 toolkit decision | §3.5 | ✅ | `docs/adr/0100-v3-toolkit-iced-default.md` — iced by default, engines toolkit-agnostic; boundary enforced in architecture-check.sh (proven by injection) |
| ADR-0101 parser decision | §3.2 | ✅ | `docs/adr/0101-v3-incremental-parser.md` — in-house incremental, re-openable; `Styler` trait is the seam |
| Kernel: CommandRegistry + CommandBus | §3.1 | ✅ | `v3/kernel/src/command.rs` — duplicate/foreign-binding rejection, subsequence palette, FIFO bus |
| Kernel: InputRouter (scoped keymap, conflict CI) | §3.1 | ✅ | `v3/kernel/src/input.rs` — chord parse/display, scope stack, innermost-wins, **Overlay = modal fence** (only Overlay+Global reachable under a modal), static conflict detection, user override API |
| Kernel: PaneTree + tabs + DocumentStore | §3.1 | ✅ | `v3/kernel/src/pane.rs` — split tree, tab dedup per document, empty-pane collapse, doc dedup by path, `Layout` view for the shell |
| Kernel: FocusModel (single focus owner) | §3.1 | ✅ | `v3/kernel/src/focus.rs` (invariant maintained by Workspace) |
| Kernel: Workspace façade | §3.1 | ✅ | `v3/kernel/src/workspace.rs` — `scope_stack()` *derived* per call; `handle_key()` is the one keystroke entry point; doc GC on tab close |
| **BUG-A regression suite** (keymap scoping + conflict enumeration) | §5 M1 gate | ✅ | `v3/kernel/tests/bug_a_keymap_scoping.rs` (7 tests incl. modal fence + the exact v2 split scenario) |
| **BUG-C regression suite** (PDF standalone in a tab) | §5 M1 gate | ✅ | `v3/kernel/tests/bug_c_documents_are_peers.rs` (5 tests) |
| Editor: height sum-tree (O(log n) offsets) | §3.2 | ✅ | `v3/editor/src/height_tree.rs` — implicit treap w/ subtree sums, deterministic priorities, differential-tested vs naive model (4k random ops) |
| Editor: 3-phase layout protocol (style/measure/paint) | §3.2 | ✅ | `v3/editor/src/layout.rs` — `Styler`/`Measurer` traits, `Damage { repaint, shifted_from }`, offsets never cached per line, viewport-bounded paint |
| Editor: layout-stable conceal contract | §3.2 | ✅ | `Styler::layout_stable()` + debug assert in `set_conceal`; reserved-width strategy demonstrated in tests |
| **BUG-B regression suite** (height change reflows; damage ≤ affected lines) | §5 M1 gate | ✅ | `v3/editor/tests/bug_b_layout_reflow.rs` (6 tests incl. "caret motion damages ≤ 2 lines" golden gate) |
| Vault: typed errors + atomic save | §3.4 | ✅ | `v3/vault/` — `VaultError` (thiserror), temp+fsync+rename save (watcher/index deferred) |
| PDF: tile cache + render queue (pure logic) | §3.3 | ✅ | `v3/pdf/src/tile.rs` — 1.4^n zoom buckets (never-upscale>1.4× proven by sweep test), byte-budget LRU w/ eviction reporting, cancellable queue (pdfium wiring deferred) |
| Shell: registry-generated keymap/palette dump | §3.1 | ✅ | `v3/shell/` — startup conflict check exits non-zero; `--dump-shortcuts` generates `docs/V3_SHORTCUTS.md`; `--demo` walks BUG-A/C on the live kernel |
| CI: v3 job in quality workflow | §6 | ✅ | `.github/workflows/quality.yml` `v3` job: fmt, clippy -D warnings, tests, demo, generated-doc freshness diff |

Statuses: ✅ done · 🔶 partial · ⬜ not started · ❌ blocked

**Verification snapshot (2026-06-10):** v3 — 47 tests green, clippy `-D warnings`
clean, fmt clean, `md3-shell --demo` all-ok; guardrail scripts green incl. new
v3 toolkit-boundary rule (fires on injected `use iced::…`, verified). v2 suite
unaffected (root workspace excludes `v3/`).

## Deliberately deferred (next sessions, in order)

1. **Shell UI on iced** — wire kernel Workspace + InputRouter into a real window
   (M1's "dogfood-internal" gate needs this). Kernel is UI-free by design so this is
   additive.
2. **Rope buffer + undo tree port** — plan keeps ropey + `EditorCommand` discipline from
   v2 (`native/src/editor/buffer.rs` is the quarry); persistent undo is new.
3. **Incremental parser** (ADR-0101 spike) + style phase fed by it.
4. **Vault watcher** (`notify`, 500 ms debounce) + FTS5 index port from v2 core.
5. **pdfium wiring** for the tile renderer (cache/queue logic lands UI-free first).
6. v2 hotfixes for BUG-A/BUG-B (plan §9.2) — *not ordered by user; ask before doing.*

## Decisions made during execution

(append-only log; newest last)

- 2026-06-10: v3 placed in-repo at `v3/` rather than a new repo — single-user project,
  one history, plan §8 "legacy reference" satisfied by directory boundary instead.
- 2026-06-10: **Overlay scope is a modal fence**, not just the innermost scope: while an
  overlay is open, resolution consults only Overlay then Global. Plain innermost-wins
  would let unbound chords fall through to the editor underneath — reintroducing the
  BUG-A failure shape from the other direction. Test-pinned in bug_a suite.
- 2026-06-10: kernel keymap-file parsing (user remap JSON) deliberately left to the
  shell; kernel exposes `Keymap::apply_override` so the kernel stays serde-free.
- 2026-06-10: `HeightTree` is an implicit treap (not a Fenwick tree) because lines are
  inserted/removed, not just updated; priorities from a seeded xorshift so tree shape is
  deterministic and tests are reproducible.
- 2026-06-10: tile cache keeps a single over-budget tile rather than evicting it
  (something must be displayable); eviction reports keys so the shell owns pixmap drops.
- 2026-06-10: `Mods` is four bools, not bitflags — avoids a dependency; revisit only if
  chord matching shows up in profiles (it won't).
