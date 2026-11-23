# Research Features Implementation Tracker

Last updated: 2026-07-19

## Goal

Add a cohesive research-navigation workspace built around the vault's existing
wikilink index. The release scope is an interactive knowledge graph plus
graph-derived tools that help users find related material and keep a vault
healthy.

## In scope

- Global graph of Markdown notes, PDFs, and unresolved wikilink targets.
- Local graph around the active note or PDF.
- Interactive canvas navigation: pan, zoom, hover, selection, and node dragging.
- Search and filters for PDFs, unresolved targets, and unconnected notes.
- Connection inspector with incoming links, outgoing links, and related notes.
- Vault-health summaries for unconnected notes, broken links, hubs, and
  connected components.
- Toolbar, keyboard, and command-palette entry points.
- Automatic graph refresh after index-affecting vault changes.
- User-facing feature documentation and regression tests.

## Status

| Area | Status | Notes |
| --- | --- | --- |
| Repository and architecture audit | Complete | Existing `FileIndex` is the graph source of truth. |
| Core graph snapshot DTOs/API | Complete | Deterministic Markdown/PDF/missing nodes, wikilink edges, aggregated PDF-annotation edges, vault-scoped persistence, and rename-safe paths are implemented. |
| Native graph state and layout | Complete | Cached deterministic layout, filters, local BFS, stats, retained node moves, and explainable related-note scoring are implemented. |
| Interactive graph view | Complete | Pan, zoom, fit, hover, selection, dragging, inspector, and vault-health views are implemented. |
| Application integration | Complete | Overlay priority, dirty-buffer-safe navigation, saved-document lifecycle, and debounced generation-safe background refresh are wired. |
| Toolbar and command palette | Complete | Graph and connections buttons, command entry, `Ctrl+G`, and Escape behavior are implemented. |
| Documentation | Complete | README and feature documentation describe behavior and saved-state boundaries. |
| Verification | Complete | Workspace check, all 212 serial tests, Clippy, and whitespace validation pass. |

## Remaining work

- Perform a manual cross-platform GUI smoke test before packaging, especially
  for large-vault layout, HiDPI canvas interaction, and toolbar fit at narrow
  window sizes.

No known implementation work remains. The smoke test is retained as a release
validation item because this environment does not provide representative
interactive displays or a large real-world vault.

## Design decisions

- Reuse `core::FileIndex`; do not maintain a second wikilink parser or graph.
- Keep graph data sorted and vault-relative so layouts are deterministic and
  platform-independent.
- Aggregate PDF-highlight links at the PDF-to-note level to avoid one node per
  annotation.
- Keep the graph as a central workspace view instead of another narrow side
  panel.
- Derive related notes and vault-health information from the graph so these
  features require no proprietary metadata or new persistence schema.
- Treat unsaved editor links as unsaved: the graph represents the last saved
  vault state.
- Publish vault indexes only when both the vault generation and content
  revision still match. A vault switch clears the previous search/link stores
  immediately, and a concurrent save/create/delete forces the background build
  to retry instead of overwriting newer state.

## Verification log

- Baseline `cargo check --workspace`: passed before implementation.
- Baseline `cargo test --workspace`: passed (177 tests) before implementation.
- Graph core `cargo check -p md-editor-core`: passed.
- Graph core `cargo test -p md-editor-core`: passed (65 tests after the
  immediate-create regression test was added).
- Graph-focused native tests: passed (layout, filters, related notes,
  transforms, zoom, hit testing, command discovery, and graph Escape behavior).
- Final `cargo check --workspace`: passed.
- Final `cargo test --workspace -- --test-threads=1`: passed (212 tests: 75
  core and 137 native).
- A first parallel full-suite run exposed an existing race in the Linux desktop
  installation test's millisecond-based temporary directory; that test passed
  in isolation and the serialized full suite passed.
- `cargo clippy --workspace --all-targets`: passed; the workspace still reports
  its existing non-fatal lint backlog, with no warnings in the new graph state
  or graph view modules.
- Final cross-vault concurrency audit: passed after generation/revision guards
  were added for index publication, graph snapshots, backlinks, and PDF
  identity scoping.
- `git diff --check`: passed.
- Repository-wide rustfmt has pre-existing formatting differences; the new
  graph modules are formatted, while workspace check/tests remain the primary
  regression gates.
