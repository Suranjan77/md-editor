# ADR-0003: Concrete Repository Boundaries

- Status: Accepted
- Date: 2026-06-07
- Owners: core persistence maintainers

## Context

Native code issues SQL and core exposes a public database mutex. Schema setup
and row mapping are difficult to evolve while presentation code knows
infrastructure details.

## Decision

SQLite access moves behind concrete core repositories grouped by persisted
concept: settings, tracker, PDF documents, annotations, and search. SQL row
mapping stays beside queries. Application service methods own transaction
boundaries spanning repositories.

Start with concrete structs. Introduce traits only when multiple implementations
or a useful test fake exists. Core returns typed errors; native adds user-facing
context.

## Consequences

- Native eventually removes its `rusqlite` dependency.
- Schema and query changes remain localized.
- Service methods may initially forward to legacy storage during migration.
- Repository APIs must avoid becoming generic table wrappers.

## Alternatives Considered

- Public connection pool/mutex: rejected because callers can bypass invariants.
- ORM adoption: rejected because migration needs no new dependency or model
  rewrite.
- Trait per repository immediately: rejected as ceremony without proven seam.

## Enforcement

- Views cannot import `rusqlite`.
- Native direct SQL is reported as migration debt.
- After repository migration, any native `rusqlite` use becomes a hard failure.
- Migration tests cover fresh and upgraded databases.

## Supersedes

None.
