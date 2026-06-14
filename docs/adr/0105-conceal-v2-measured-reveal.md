# ADR-0105: Conceal v2 — true hide with measured reveal (retires reserved-width)

- Status: accepted (2026-06-13)
- Scope: v3 editor engine conceal contract + shell renderer.
- Spec: `development/IMPLEMENTATION_PLAN.md` Phase 7.1. Supersedes the reserved-width
  strategy in `development/GROUND_UP_PLAN.md` §3.2 (note added there).

## Context

v3's original conceal contract was *layout stability by construction*:
concealed markers keep their reserved width ("columns advance, pixels
don't"), so revealing them on the active line can never reflow neighbors.
That was a deliberate over-correction against v2's Bug B (conceal changing
geometry as a styling side effect, with no reflow protocol).

It worked — BUG-B is dead — but the cost is visible: gaps punched into prose
wherever `**`, `#`, `$`, or link syntax hides. The user ordered a
Typora-grade editing experience (2026-06-13), and Typora truly hides
markers.

The structural insight: reserved width was a *tactic*. The thing that
actually kills Bug B is the three-phase layout protocol — height changes
flow through the height sum-tree with explicit damage, so offsets are never
stale. That protocol can absorb reveal-induced geometry changes the same way
it absorbs any edit.

## Decision

Conceal state becomes a **measure input**:

- A concealed line is styled *and measured* without its marker glyphs.
- Caret entering a line (or block construct — Phase 7.3) re-styles and
  re-measures it in revealed form; the height tree shifts subsequent
  offsets; damage = the revealed line + the shifted region.
- `Styler::layout_stable()` is retired. Replacement invariant
  (debug-asserted where the old assert lived): every conceal transition
  passes through remeasure before paint.
- **BUG-B gate v2** (`v3/editor/tests/bug_b_layout_reflow.rs`): the contract
  is "offsets never stale, content never overlaps, styled-damage from caret
  motion ≤ 2 lines with correct `shifted_from`" — verified differentially
  against a from-scratch layout, plus a seeded caret-motion storm test.
- Reveal granularity: line (and block for block constructs) in v1;
  element-level reveal (Typora's exact behavior) is a Phase 7.6 refinement
  on the same mechanism.

## Consequences

- Concealed prose renders tight — the single largest "clanky" artifact
  goes away.
- Revealing a line may rewrap it and shift everything below; that is now a
  *specified, tested* behavior instead of a forbidden one. Scroll anchoring
  must keep the caret line visually stable during reveal (shell concern,
  same anchoring used for PDF zoom).
- The cross-fade ambition from the master plan survives as Phase 7.6 motion
  polish, applied to the new mechanism.
- Any future styler must uphold the remeasure-before-paint invariant; there
  is no longer a `layout_stable()` escape hatch to hide behind.
