# V3 Ground-Up Rebuild — Retrospective (2026-06-10 → 2026-06-11)

> Source material: `docs/V3_HANDOFF.md` (status board + decisions log), git history on
> `pdf-improv`, and the working tree as of 2026-06-11.

## TL;DR

A plan scoped for 10–20 engineers over 12–18 months was executed by a single agent in
two days: ~15,300 lines of Rust across five crates (kernel, editor, vault, pdf, shell),
160 tests green workspace-wide (171 with pdfium), every M1/M2 quality gate met, and all
three named legacy bugs (BUG-A/B/C) dead by construction with regression suites pinning
them at multiple layers. The biggest process win was the testing strategy —
property/differential harnesses caught at least six real bugs before merge. The biggest
open risk at time of writing is administrative, not technical: the latest unit of work
(PDF reading UX, annotations v2) is verified per the handoff snapshot but sits
**uncommitted on `pdf-improv`** (~1,000 insertions / 1,500 deletions across 24 files,
plus 4 untracked files).

## What went well

**1. Dependency-order execution made everything testable headlessly.** Building
kernel → editor → vault/pdf → shell meant each engine shipped with its own test surface
before any GUI existed. The payoff shows in the routing suite: 15 tests drive
`gui::Shell::update` over a tempdir vault, windowlessly — BUG-A and BUG-C are pinned at
the shell layer without ever opening a window.

**2. Property and differential testing earned its cost several times over.** The
decisions log records concrete bugs that harnesses caught pre-merge, not hypothetically:

- CRLF merge/split underflow — line counts must be *measured* from the rope, never
  predicted, because ropey treats lone `\r` as a line break.
- Emoji-cluster splits from raw hit-test columns — led to boundary-snapping every
  selection endpoint inside the buffer rather than trusting callers.
- Mixed-paste undo coalescing destroying two steps with one undo.
- The inotify echo loop — the watcher consuming its own read events caused an infinite
  reindex, manifesting as a hung suite.
- Premature parser convergence when a line moved to/from index 0 (fixed by making
  classification pure on `(text, entry state)`).
- pdfium-render's `thread_safe` feature not actually serializing FFI — concurrent calls
  SIGSEGV; fixed with a process-wide mutex.

The differential pattern (treap vs. naive model over 4k ops, incremental parser vs. full
reparse over 2k edits, 8-seed × 500-command undo storms) is the single most reusable
practice from this effort.

**3. Bugs were killed architecturally, then pinned.** BUG-A became the "Overlay is a
modal fence" rule; BUG-B became the `layout_stable()` contract with reserved-width
conceal; BUG-C became documents-as-peers in the pane tree. Each is a structural
property, and each has a named regression suite so it cannot silently regress.

**4. The decision log is exemplary.** ~25 append-only entries with rationale, including
honest plan deviations: `(mtime, size)` change detection instead of the plan's
"mtime+hash" diff (with the reasoning — hashing defeats the cold-start gate — and an
explicit revisit condition), the skipped toolkit bake-off resolved via the plan's own
tie-breaker as ADR-0100, and search-operator semantics changed *with the test updated to
assert the new intent*, not just to pass.

**5. Seams held.** `Styler`/`Measurer`, `TextExtractor`, `ChangedSpan`, `DocLayout` —
engines stayed peers (vault never depends on pdf; the shell composes them), exactly as
plan §3 layering demanded. This is what made the interim-shell deletion cheap.

## What was bumpy

**Throwaway work happened, but paid for itself.** The interim plain-text `app`/`keys`
modules were built and then deleted once the `gui` module became the one shell. The
mitigating move — porting their test suites rather than dropping them — found two real
bugs (`editor.select-all` had no handler in `gui`; `Character(" ")` normalization made
`space` bindings unmatchable). Lesson: when deleting an interim implementation, its
tests are the part worth keeping.

**A suite existed that had never compiled against the final API.** The buffer storm
suite was written against an earlier `Command`/`apply` shape and only got ported on
06-11 — at which point it immediately caught the coalescing bug. A test that doesn't
build is worse than no test, because it reads as coverage. The v3 CI job now effectively
enforces that every test target builds on every push.

**Native FFI was the only place "correct by reading the docs" failed.** pdfium-render's
`thread_safe` feature promised more than it delivered; the truth only surfaced under a
parallel test run. The fix (engine-level mutex, safe under any caller topology rather
than by convention) is the right shape — but it's a reminder that the FFI boundary is
where property testing has to be supplemented with adversarial concurrency tests.

## Open risks and debt (carried forward deliberately)

- **Uncommitted branch state.** The handoff's verification snapshot ("160 tests
  green…") describes the working tree, not a commit. First action next session: commit
  `pdf-improv`.
- **Synchronous tile rendering in `update`.** Acceptable for 512 px tiles; the
  queue/cancellation semantics already live in the engine, so the async worker is a
  drop-in — but this is the most likely first performance complaint.
- **Annotations v2 has a store but no UI**, and the GUI's `SearchIndex` is in-memory per
  run — the persistent sidecar path decision is the gating call for the next session
  (handoff deferred item 7).
- **PDF UX gaps from plan §3.3:** TOC with section tracking, search overlay (`pdf.find`
  is routed but stubbed), text selection, back/forward history.
- **Collapsed decision points.** The toolkit bake-off and tree-sitter spike were
  resolved by default, recorded as re-openable ADRs (0100/0101). Fine for now; they
  should be consciously revisited at M3, not forgotten.
- Deferred item 10 (v2 hotfixes for BUG-A/B) is correctly flagged *ask the user first* —
  good scope hygiene, keep it.

## Practices to keep

1. Differential/property harnesses for every engine with a pure core — they found every
   subtle bug in this effort.
2. Append-only decision log with "revisit when X" conditions attached to deviations.
3. Named regression suites for root-caused legacy bugs, pinned at both engine and shell
   layers.
4. Engines expose policy, the shell composes — the `TextExtractor`-in-vault move is the
   template.
5. Port tests when deleting code; never drop them.

## Process tweak to adopt

Update the handoff's verification snapshot only *after* committing, so the snapshot
always describes a reachable state.
