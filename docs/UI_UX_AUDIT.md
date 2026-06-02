# UI/UX Baseline Audit

Last updated: 2026-06-02

## Purpose

Baseline audit for Milestone 0 in `docs/UI_UX_IMPROVEMENT_ROADMAP.md`.
Scope: current app surfaces, visible states, message paths, keyboard paths,
style sources, and missing tests before broader shell and visual-system work.

## Primary Screens

| Surface | Current component | States observed | Message paths | Keyboard paths | Style source | Missing coverage |
| --- | --- | --- | --- | --- | --- | --- |
| Vault opener | `views::welcome::view` | no vault, open vault CTA | `OpenVaultDialog` | `Ctrl+O` | `theme.rs`, `button::primary` | first-run diagnostics, recent vaults, default focus |
| App shell toolbar | `views::toolbar::view` | no file, active file, saved label, sidebar/toc/split/tracker toggles | `SidebarToggle`, `GlobalSearchOpen`, `CommandPaletteOpen`, `ToggleTOC`, `SplitViewToggle`, `TrackerToggle` | `Ctrl+P`, `Ctrl+F`, `Ctrl+O`, split/PDF shortcuts | `theme.rs`, inline container border | icon labels/tooltips, disabled reasons, active pane indicator |
| Sidebar | `views::sidebar::view` | visible/collapsed, selected path, expanded folders | `SidebarFileClicked`, `SidebarFolderToggled`, `SidebarToggle` | indirect through shortcuts and app focus | `theme.rs`, local button styles | tab order, tree traversal, empty vault state |
| Markdown editor | custom `editor::renderer::Editor` | active document, search bar, selection/cursor, links, tasks, media/math caches | `EditorCommand`, `EditorCommandNoScroll`, `EditorScrolled`, `EditorCheckboxToggle` | editor key handling, `Ctrl+S`, find, formatting shortcuts | `editor/renderer.rs`, `theme.rs` | layout smoke fixtures, focus order with surrounding panels |
| PDF viewer | `views::pdf_viewer::{toolbar, view_continuous}` | no PDF, loading placeholders, page/zoom/search/annotation controls, selection | PDF navigation/render/search/annotation messages | PDF page/search/zoom shortcuts, copy selection | `theme.rs`, local toolbar styles, `interactive_pdf.rs` | stale render/error states, disabled reasons, keyboard annotation flow |
| Split research mode | `MdEditor::view` row composition | markdown + PDF, resizer, active panel state | `SplitViewToggle`, drag messages, navigation history | split shortcut, cross-pane navigation shortcuts | inline split divider/container styles | layout transition fixture, pane focus persistence, narrow width behavior |
| Global search | `views::search::view` | hidden, empty, searching, error, source toggles, PDF status | `SearchQueryChanged`, `GlobalSearch*`, result clicks | open, Enter/arrow flows partially covered | `theme.rs`, local result styles | cancellation affordance, keyboard-only result selection |
| Command palette | `views::command_palette::view` | hidden, filtered commands, grouped workflow labels | `CommandPalette*`, `KeyboardShortcut` | `Ctrl+P`, Enter through app dispatcher | `theme.rs` | generated registry source, default focus test |
| Citation palette | `views::citation_palette::view` | hidden, selection/annotation/search-hit items, Enter first result | `CitationPalette*`, PDF insert messages | shortcut open, Enter first result | `theme.rs` | empty state, focus traversal beyond input |
| Annotation sidebar | `views::pdf_annotations::view` | no annotations, filters, focused row, cite/link/tag/status actions | `Pdf*Annotation*`, `PdfInsertAnnotationLink` | palette/shortcuts only | `theme.rs`, local button styles | keyboard sidebar shortcuts, large-list update bounds |
| TOC/backlinks sidebars | `views::toc::view`, `views::backlinks::view` | hidden, empty, combined markdown/PDF entries, backlinks | `TocEntryClicked`, `BacklinkClicked` | command/search flows | `theme.rs` | keyboard traversal, active-position tracking |
| Tracker | `views::tracker::view` | dashboard, running session, curriculum/config tabs | tracker messages | toolbar toggle and shortcuts | `theme.rs`, local styles | focus order, reduced-motion/progress accessibility |
| Modals/context menu | `views::modals::view` | create/rename/delete/link-note/tag/template/context menu | `NameModal*`, `Pdf*`, file operations | Enter/Escape app handling | `theme.rs`, local button styles | modal focus trap, destructive confirmation matrix |
| Toast/status | `views::toast::view`, ad hoc strings | transient success/error text | set by app update branches | none | `theme.rs` | queueing, reduced motion, background error surface |

## Baseline Accessibility Checklist

- Keyboard path exists for opening vault, opening command/search/citation
  palettes, saving, PDF navigation, split toggle, and cross-pane back/forward.
- Default focus exists for global search, PDF search, and citation palette
  inputs through explicit widget IDs.
- Modal Escape/Enter dispatch exists in app update handling.
- Empty/error states exist for TOC, backlinks, global search, and several PDF
  loading paths.
- Icon-only toolbar buttons need visible labels, tooltips, or accessible names
  where current Iced APIs permit.
- Focus order and modal trapping still need app-level smoke coverage.
- Contrast and high-contrast theme checks are not yet automated.
- Reduced-motion behavior is not yet documented or tested.

## Test Harness Baseline

- Added app-level fixture helpers in `native/src/app.rs` tests for no-vault,
  markdown, PDF, split research, global search, command palette, active modal,
  and annotation-heavy PDF states.
- Added first smoke tests that render representative app states through
  `iced_test::simulator` and assert stable user-visible labels.
- Extended fixtures for large markdown, active file search, and narrow split
  mode through `iced_test::Simulator::with_size`.
- Added keyboard-path smoke tests for command palette, citation palette,
  search, TOC, focus mode, and Escape overlay priority. Current `iced_test`
  APIs can simulate key events, but they do not expose enough focus-tree state
  for complete app-level tab-order assertions.
- Added deterministic layout smoke coverage for primary shell text labels,
  asserting active path, save status, and PDF page status do not overlap in
  markdown, PDF, and narrow split layouts.
- Extracted app focus targets into testable `FocusTarget` mappings and added a
  deterministic command-palette input ID. Command and citation palettes now
  expose rendered input IDs that match their default-focus tasks.
- Added rendered focus-target checks for file search, global search, PDF
  search, command palette, and citation palette input IDs.
- Added large-annotation filter baseline counter coverage. Current annotation
  filtering is an explicit linear pass over annotations, with stable filtered
  ordering by page and creation order.
- Added `docs/UI_UX_RELEASE_CHECKLIST.md` and linked it from launch/features
  docs. Checklist covers layout overlap, keyboard traps, label coverage,
  contrast, stale loading states, reduced motion, and platform pass notes.
- Added Milestone 1 app-shell fixture coverage for derived modes, active panes,
  command groups, and narrow-window persistence collapse behavior.
- Added shell persistence coverage proving saved sidebar/workflow state, split
  ratio, reference width, and active pane round-trip through config.
- Added initial app-shell status coverage for dirty/saved state, PDF page/zoom,
  search progress, active pane, and toast/error priority.

## Next Audit Work

- Add modal focus-trap coverage when current Iced test APIs expose enough
  focus traversal state, or after introducing an app-level focus model.
- Continue Milestone 1 by rendering unified active-pane/status UI and migrating
  away from remaining fixed sidebar width assumptions.
