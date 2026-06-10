# ADR-0001: Keep iced as the UI toolkit

Status: Accepted · Date: 2026-06-10

## Context

The app is ~42k lines of Rust built on iced 0.14, including a fully custom
editor widget (buffer/parser/renderer, ~10k lines) using iced's advanced
widget APIs. Periodic frustration with iced (young a11y story, breaking
releases, limited text primitives) raises the question of switching (egui,
gpui, Tauri/webview, GTK/Qt bindings).

## Decision

Stay on iced. Upgrade versions opportunistically at phase boundaries only.

## Consequences

- The custom editor widget, the hardest-won asset, keeps working; a port would
  rewrite its event/layout/draw integration for ~zero user value.
- We accept iced's a11y limits; Phase 9 documents what's achievable (ADR-0006
  planned) instead of pretending a toolkit switch is on the table.
- Animation/motion (UX-C) is built as a small in-repo tween system on iced's
  frame/redraw scheduling rather than relying on toolkit-level animation.
- Any future toolkit discussion must come as a superseding ADR with a costed
  migration plan for the editor widget specifically.
