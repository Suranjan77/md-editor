# ADR-0002: Feature Reducers And Nested Messages

- Status: Accepted
- Date: 2026-06-07
- Owners: native application maintainers

## Context

`MdEditor` owns broad state and one flat message/update surface. Unrelated
features couple through shared fields and update arms, making isolated tests and
moves difficult.

## Decision

Extract cohesive feature state and nested feature messages incrementally.
Feature update functions own local transitions and return `Task` values or
typed effects. Top-level update routes messages and handles explicit
cross-feature outcomes.

Reducers do not perform database, filesystem, or PDFium work directly. Shared
source of truth remains singular. No reducer framework, macro-generated router,
or global event bus is introduced.

## Consequences

- Feature transitions can be tested without constructing full application.
- Message mapping adds small explicit boilerplate.
- During migration, flat and nested messages may coexist.
- Cross-feature effects become visible in types and review.

## Alternatives Considered

- Rewrite all messages at once: rejected due to review and regression risk.
- Observer/event bus: rejected because control flow becomes implicit.
- Keep one update function: rejected because ownership remains unclear.

## Enforcement

- Feature tests assert transition plus emitted task/effect.
- Review checks reducers for direct infrastructure calls.
- Top-level state and update size are tracked as migration metrics.

## Supersedes

None.
