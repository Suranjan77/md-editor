# ADR-0104: Shaped proportional text measurement replaces the monospace grid

- Status: **proposed** (2026-06-13) — direction is decided (user order:
  Typora-grade editing); the *mechanism* is resolved by the Phase 7.0 spike,
  which updates this ADR to accepted with measured numbers.
- Scope: v3 markdown renderer (`v3/shell` measurer; engine seams unchanged).
- Spec: `development/IMPLEMENTATION_PLAN.md` Phase 7.0.

## Context

The v3 markdown editor measures and paints on a monospace column grid
(`MonoMeasurer` / `wrap_columns` in `v3/shell/src/gui/editor_canvas.rs`).
The user ordered a Typora-grade live editing experience (2026-06-13):
proportional reading typography, visibly scaled headings, and exact
caret/click geometry. A column grid is structurally incapable of that —
every advance is a lie for proportional glyphs, so hit-testing can only ever
be approximate and typography can only ever be decoration.

The engine was built for this swap: `Measurer` is an injected trait
(ADR-0101's Styler/Measurer seam), the height sum-tree consumes whatever
heights the measurer reports, and damage propagation is measurer-agnostic.

## Decision

Replace `MonoMeasurer` with a `ShapedMeasurer` in the shell that measures
real shaped text and exposes caret↔point mapping **from the same shaped
layout used for painting** — one geometry source for measure, paint, and
hit-test.

Candidates (spike resolves, criteria in priority order):

1. **`cosmic-text` directly** — iced's own shaper, pinned to the version
   iced resolves in `v3/Cargo.lock`. Default if the spike ties, because the
   measurer must be constructible in windowless tests without an iced
   renderer.
2. **iced `advanced::text::Paragraph`** — less code if its measurement API
   exposes per-run geometry usable outside `draw`.

Criteria: single geometry source for all three consumers; cost of measuring
a 100k-line document (estimate-then-refine is acceptable only if refined
heights flow through the normal `Damage`/height-tree path); grapheme-cluster
and IME behavior.

`MonoMeasurer` survives only as the engine-test measurer. Shell tests use
`ShapedMeasurer` with an embedded test font so CI geometry is reproducible.

## Consequences

- Typography (Phase 7.2), block rendering (7.3), and exact hit-testing
  (7.5) all become possible; "approximate" caret mapping ends.
- The 6.3 golden draw-plan corpus regenerates once at the swap; the diff is
  reviewed, not waved through.
- p95 keypress→frame < 8 ms (master plan pillar 1) must be re-verified and
  recorded in the handoff — shaping is not free.
- The engine (`v3/editor`) is untouched: this is a measurer implementation
  behind an existing seam. If the swap appears to require engine changes
  beyond the `Measurer` contract, that is a design smell — stop and revisit.
