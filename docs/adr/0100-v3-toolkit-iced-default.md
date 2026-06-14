# ADR-0100: v3 toolkit — iced by default; bake-off collapsed

- Status: accepted (2026-06-10)
- Scope: v3 only (`v3/` workspace). v2's ADR-0001 is unaffected.

## Context

V3 plan §3.5 calls for a 3-week bake-off (iced vs gpui vs egui) building the same
vertical slice. This execution has no team to run three parallel slices, and the plan
itself specifies the tie-breaker: **"Default if scores tie: stay on iced."**

## Decision

Adopt the default: **iced** is the v3 shell toolkit. The real insurance policy from the
plan is kept in full force: the editor engine (`md-editor`) and the workspace kernel
(`md-kernel`) are **toolkit-agnostic by construction** — no iced types in their APIs;
the editor emits draw commands, the kernel consumes abstract `Chord`s and produces
`CommandId`s. Only `md-shell` may depend on a UI toolkit.

## Consequences

- A later gpui/egui evaluation is a shell-only port; the bake-off can still be run
  honestly when staffing allows, against working kernel/editor crates.
- CI enforces the boundary the same way v2's architecture-check does: kernel/editor/
  vault/pdf crates must not depend on iced/winit (checked in scripts/architecture-check.sh).
