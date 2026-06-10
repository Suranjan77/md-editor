# ADR-0101: v3 markdown parsing — in-house incremental parser; tree-sitter re-openable

- Status: accepted, explicitly re-openable (2026-06-10)
- Scope: v3 editor engine.

## Context

V3 plan §3.2 calls for a 2-week tree-sitter-markdown spike vs evolving the in-house
parser, judged on incrementality, inline-extension support (wikilinks, math, citations),
and error tolerance. The spike has not been run.

## Decision

Proceed with the **in-house incremental block parser** direction (explicit block-state
entry/exit for fences/math/tables, forward re-parse to convergence) because:
1. The layout protocol (style/measure/paint, ADR-quality contract in `md3-editor`) is
   parser-agnostic — the style phase consumes "(line text, block state, conceal mode)"
   regardless of who computes block state.
2. Inline extensions (wikilinks `[[..]]`, citations `@key`, `$math$`) are first-class
   product features; tree-sitter-markdown's extension story would dominate the spike and
   needs hands-on evaluation that shouldn't block kernel/layout work.

The spike remains scheduled before the style phase is built in earnest (handoff §deferred
item 3). Switching costs are contained to the styler implementation.

## Consequences

- `md3-editor` defines `Styler` as a trait; a tree-sitter-backed styler is a drop-in.
- Conceal correctness does not depend on this decision (plan §3.2: "conceal works with
  either").
