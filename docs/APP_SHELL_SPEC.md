# App Shell Specification

Last updated: 2026-06-02

This document starts Milestone 1 of `docs/UI_UX_IMPROVEMENT_ROADMAP.md`.
It defines the app shell model before broader visual restyling.

## Work Zones

- Vault navigation: vault tree, selected file, folder expansion, recent/open
  vault entry points.
- Active document: markdown editor, PDF reader, image preview, or split
  markdown/PDF workspace.
- Reference pane: PDF companion surface in split research mode.
- Workflow sidebar: backlinks, annotations, outline/TOC, tracker.
- Command/status surface: toolbar, command palette, search overlays, status
  text, toast/background error surface.

## Layout Modes

`native/src/app_shell.rs` models these modes:

- `NoVault`: first screen, vault opener only.
- `EmptyVault`: vault is open but no indexable entries are present.
- `EditorOnly`: markdown document is the primary work surface.
- `PdfOnly`: PDF document is the primary work surface.
- `ImageOnly`: image preview is the primary work surface.
- `SplitResearch`: markdown and PDF are both open and split mode is requested.
- `SearchHeavy`: global search, command palette, or citation palette is active.

Search and palette overlays intentionally override document layout for command
availability, focus, and smoke-test expectations.

## Panel Persistence Rules

Persist:

- vault sidebar width and collapsed state;
- reference pane width and collapsed state;
- workflow sidebar width and collapsed state;
- split ratio;
- active workflow sidebar tab;
- last focused pane.

Clamp values before use:

- sidebar width: 180-360 px;
- reference pane width: 260-640 px;
- workflow sidebar width: 240-420 px;
- split ratio: 0.25-0.75.

When window width is below 720 px, reference and workflow sidebars collapse.
The active document remains primary.

## Active Pane Rules

- `NoVault` and `EmptyVault`: no active pane.
- `EditorOnly`: markdown pane active.
- `PdfOnly`: PDF pane active.
- `ImageOnly`: image pane active.
- `SplitResearch`: preserve last focused pane if it was PDF, otherwise default
  to markdown.
- `SearchHeavy`: preserve last focused pane because overlay commands should
  route back to the previous context.

## Command Groups

Stable command groups:

- File
- Edit
- Navigation
- View
- Research
- Annotation
- Search

Mode mapping:

- `NoVault` / `EmptyVault`: File, View.
- `EditorOnly`: File, Edit, Navigation, View, Research, Search.
- `PdfOnly`: File, Navigation, View, Annotation, Search.
- `ImageOnly`: File, Navigation, View.
- `SplitResearch` / `SearchHeavy`: all groups.

## Status Surface Model

Status bar fields now have an initial app-shell status snapshot:

- save state;
- indexing/search progress;
- PDF page and zoom;
- active pane;
- background errors.

Current implementation still renders these through toolbar, PDF toolbar,
search overlays, and toast strings. `MdEditor::view` derives the shell status
snapshot without changing rendered behavior, so active-pane/status UI can move
incrementally onto the model.

## Test Coverage

Initial tests live in `native/src/app_shell.rs` and cover:

- layout-mode derivation;
- split mode prerequisites;
- search/palette mode override;
- panel width clamping and narrow-window collapse;
- shell persistence serialization and lenient parsing;
- command-group availability by layout context;
- document visibility predicates used by `MdEditor::view`.
- status derivation for save state, search/PDF status, active pane, toast, and
  background errors.

`MdEditor::view` now derives an `AppShellState` snapshot, command-group set, and
uses shell predicates for no-vault, split research, PDF, and image layout branch
selection. App-level fixture tests verify mode, active pane, command-group, and
narrow-window persistence behavior.

Shell persistence now writes a compact `app_shell_persistence` config value and
restores sidebar collapsed state, workflow tab, split ratio, reference width,
and last focused pane at startup. User-driven sidebar/workflow toggles,
split-resizer completion, split-mode toggles, Show Usages, and active-pane
changes update the saved value.

App-level fixture tests verify shell status derivation for dirty markdown,
PDF page/zoom labels, search progress text, active pane, and toast/error
priority.

## Open Work

- Add active-pane indicator tests once UI renders the indicator.
- Render status fields through a unified status surface instead of separate
  toolbar/PDF/search/toast locations.
