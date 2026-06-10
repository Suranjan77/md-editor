# ADR-0004: Elm architecture with feature modules

Status: Accepted · Date: 2026-06-10

## Context

iced imposes Model/Message/update/view (Elm architecture). With one flat model
and one giant reducer this degraded into god-files (`app/update.rs` 2,442
lines). Two partial migrations coexisted: a flat `Message` enum and a
`features/` layer. A decision was needed on the end state before Phase 3
completes the decomposition.

## Decision

Keep the Elm architecture, organized by feature:

- One top-level `Message` enum whose variants wrap per-feature enums
  (`Message::Pdf(PdfMessage)` etc. — already in place).
- Each `features/<name>/` owns its model slice, messages, update, and view;
  the root reducer only routes.
- Features never import each other; rare cross-feature effects route through
  the root as explicit events (`AppEvent`, Phase 3) — enforced by
  `architecture-check.sh` with a shrink-only allowlist.
- Async work returns `Task<Message>` from feature updates; no hidden
  side-channels.

## Consequences

- Message flow stays auditable (`docs/MESSAGES.md` planned in P3.T1) and
  feature code reviewable in isolation; budgets force the god-files to shrink.
- Some boilerplate (wrapping/unwrapping messages) is accepted as the cost of
  isolation.
- Shared UI state (focus, panels, overlays) belongs to `shell`/`overlays`
  features, not to whichever feature touched it last.
