# ADR-0001: Inward Dependency Direction

- Status: Accepted
- Date: 2026-06-07
- Owners: core and native maintainers

## Context

Application currently has two crates but several responsibilities remain mixed.
Native code performs SQL, `AppState` exposes infrastructure, and large UI
modules coordinate domain work. Restructuring needs stable direction before
files move.

## Decision

Dependencies point inward:

```text
presentation -> feature coordination -> application services -> domain
                                                        ^
                                                        |
                                              infrastructure adapters
```

`core` cannot depend on `native`, Iced, or native presentation modules. Views
compose UI and messages only; they cannot use SQLite or PDFium directly.
Infrastructure implements application needs without leaking concrete handles
into presentation code.

Module boundaries precede new crate boundaries. Crates split only after module
edges stabilize.

## Consequences

- Domain and application logic become testable without Iced.
- Infrastructure replacement and migration become localized.
- Temporary adapters and forwarding APIs may exist during extraction.
- Cross-boundary changes require explicit service APIs.

## Alternatives Considered

- Four immediate crates: rejected because unstable boundaries would create
  churn and circular pressure.
- Keep service locator: rejected because public concrete handles preserve
  coupling.
- Generic event bus: rejected because it hides dependencies and flow.

## Enforcement

- `scripts/architecture-check.sh`
- `.github/workflows/quality.yml`
- `docs/ARCHITECTURE_RULES.md`

## Supersedes

None.
