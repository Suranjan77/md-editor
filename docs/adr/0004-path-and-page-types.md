# ADR-0004: Typed Paths And PDF Page Units

- Status: Accepted
- Date: 2026-06-07
- Owners: core domain and PDF maintainers

## Context

Plain strings and integers currently represent absolute paths, vault-relative
paths, 0-based PDF indexes, and 1-based labels. Unit confusion can navigate to
wrong pages or escape vault boundaries.

## Decision

Introduce boundary newtypes incrementally:

- `VaultPath`: validated vault-relative path.
- `AbsPath`: validated absolute filesystem path.
- `PageIndex`: 0-based internal PDF index.
- `PageNumber`: non-zero 1-based UI/link number.

Service APIs use typed values at ownership boundaries. Conversion occurs once,
near input parsing or display. Internal hot loops may retain primitives until
profiling supports migration.

Variable names remain explicit even before newtypes reach a call site:
`vault_path`, `abs_path`, `page_index`, and `page_number`.

## Consequences

- Invalid unit combinations become compile-time or construction-time errors.
- Call sites pay explicit conversion cost and migration effort.
- Cross-platform path validation needs dedicated tests.
- Link format remains stable while internal representation improves.

## Alternatives Considered

- Naming convention only: rejected because compiler cannot enforce it.
- Immediate whole-codebase conversion: rejected due to churn and hot-path risk.
- Canonicalize every path: rejected because nonexistent targets and platform
  semantics require distinct validation policies.

## Enforcement

- API review rejects ambiguous new path/page parameters.
- Tests cover index/number conversion and traversal rejection.
- `docs/ARCHITECTURE_RULES.md` defines naming and ownership.

## Supersedes

None.
