# ADR-0103: Use iced Tokio executor for shell async tasks

- Status: accepted (2026-06-12)
- Scope: v3 shell runtime.

## Context

Toast auto-dismiss is an iced `Task` using `tokio::time::sleep`. Iced default
native feature set selects its thread-pool executor, which does not provide a
Tokio reactor. First toast therefore panicked from an unnamed executor thread:

```text
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

## Decision

Enable iced `tokio` feature in `v3/shell/Cargo.toml`. Iced then selects its
Tokio backend and owns runtime used to poll shell tasks.

## Consequences

- Tokio timer APIs are valid inside iced tasks.
- No manually-created global runtime or timer thread is required.
- Runtime choice becomes deliberate shell configuration.
- Async work that needs cancellation or ordering should still use existing
  shell task/worker boundaries; enabling Tokio is not permission to move
  parser, layout, or pdfium work into ad hoc spawned tasks.
