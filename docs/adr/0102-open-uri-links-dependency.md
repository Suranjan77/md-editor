# ADR-0102: URI links open in browser via `open` crate

- Status: accepted (2026-06-12)
- Scope: v3 only (`v3/` workspace).
- Note: recorded *after* the dependency had already shipped. That ordering
  violates the ADR-before-code rule since added to
  `development/IMPLEMENTATION_PLAN.md` §0.2; the decision itself stands.

## Context

`development/IMPLEMENTATION_PLAN.md` Phase 1.2.5 / backlog item 7 deferred the decision on opening external URI links from PDFs in the system browser.
The standard library has no cross-platform way to launch the default browser; a small dependency that shells out to the platform opener (`xdg-open`, `open`, `start`) is needed for Linux, macOS, and Windows.

## Decision

Add the `open` crate (version 5) as a dependency of `md-shell`.
When a user left-clicks a URI link in a PDF, the shell will spawn a thread to call `open::that(uri)` asynchronously.

## Consequences

- The `open` crate is extremely lightweight (only depends on `std` and executes shell commands like `xdg-open` or `open` depending on target OS).
- No blocking of the iced main thread: launching the URI asynchronously prevents any GUI freeze.
- Provides native external URL link navigation from within the PDF reader.
