# UI And UX Improvement Roadmap

This roadmap turns the completed research workspace into a polished,
keyboard-friendly, visually coherent application. It is intentionally sized for
a multi-month effort by a team of about ten developers.

## Operating Rules

- Build UI work with test coverage first where practical: failing `iced_test`
  flow, implementation, regression coverage, then cleanup.
- Preserve existing architecture boundaries: `native/views` composes UI and
  wires messages, `native/app.rs` coordinates state, renderer code stays
  viewport-bounded.
- Keep markdown document mutations behind `EditorCommand`; UI workflows must
  dispatch commands instead of editing buffers directly.
- Keep markdown parsing in `native/src/editor/highlight.rs`; visual polish must
  not add parser rules to renderers.
- Keep all major interactions keyboard-accessible before declaring UX complete.
- Add loading, empty, disabled, conflict, and error states for every user-facing
  workflow changed by a milestone.
- Add accessibility acceptance checks for focus order, keyboard traversal,
  visible labels or tooltips, contrast, scalable text, and reduced-motion
  behavior. Do not claim screen-reader support beyond what current Iced
  accessibility APIs can actually expose.
- Keep performance budgets explicit: no full-document hot-path scans, no
  widget-per-PDF-rect overlays, no unbounded search/status updates.
- Prefer stable, dense research-tool UI over marketing-style surfaces:
  no decorative hero layouts, no nested cards, no one-hue palette, no oversized
  type inside tool panels.
- Avoid generic AI-generated visual patterns: no glassmorphism, frosted panels,
  blur-heavy overlays, gradient blobs, floating orb backgrounds, excessive
  glow, random pastel gradients, oversized rounded cards, decorative mockup
  frames, fake 3D depth, or stock SaaS dashboard composition. Visual decisions
  must come from the app's research/editor workflow, not trend defaults.
- Record design decisions in this roadmap or related docs when they create
  reusable UI conventions.
- Update `docs/HANDOFF.md` after meaningful implementation work.
- Run `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` before handoff for code changes.

## Team Lanes

- Product UX: workflow maps, information architecture, user journeys, copy, and
  acceptance criteria.
- Visual System: theme tokens, typography, spacing, icons, colors, panels, and
  component states.
- Editor Experience: markdown editing affordances, cursor/selection feedback,
  block controls, source/render clarity, and inline diagnostics.
- PDF Experience: page controls, annotation ergonomics, selection feedback,
  zoom/navigation, and reading state.
- Research Workflow: citation, excerpt, linked-note, backlinks, outline, and
  search workflows.
- Navigation And Command: shortcuts, command palette, focus model, history, and
  quick switchers.
- Accessibility: focus order, keyboard-only operation, contrast, labels, motion,
  and scalable UI.
- Performance UX: perceived speed, progress feedback, cancellation, cache
  visibility, and large-vault behavior.
- QA Automation: `iced_test` flows, visual regression fixtures, smoke scripts,
  and release checklists.
- Documentation: user guides, in-app terminology, screenshots, and workflow
  examples.

## Non-Goals

- Do not change markdown syntax, parser semantics, or PDF file storage as part
  of visual polish.
- Do not replace Iced, PDFium, SQLite, or the editor renderer unless a separate
  architecture roadmap approves it.
- Do not add cloud sync, collaboration, AI features, or account systems.
- Do not make a marketing landing screen; first screen stays a usable vault or
  vault-opening workflow.

## Milestone 0: UX Baseline And Test Harness

- Audit current app screens: vault opener, sidebar, editor, PDF viewer, split
  mode, global search, command palette, annotation sidebar, citation palette,
  tracker, modals, and error states.
- Create `docs/UI_UX_AUDIT.md` with component names, states, message paths,
  keyboard paths, current style source, and missing tests.
- Add `iced_test` helpers for opening representative app states: empty vault,
  large markdown file, active PDF, split research mode, active search, active
  modal, and annotation-heavy PDF.
- Add deterministic layout smoke tests first. Add screenshot fixtures only if
  project tooling can keep them stable across Linux CI font/render differences.
- Define baseline accessibility checklist and add focused tests for tab order,
  default focus, modal trapping, disabled controls, and shortcut dispatch.
- Define UX quality gates for every later milestone: no overlapping text, no
  unlabeled icon-only controls, clear loading/error states, keyboard path
  present, and no noticeable layout jump during common interactions.
- Deliverable: audit doc, reusable UI test helpers, first accessibility
  checklist, and first layout regression tests.

## Milestone 1: Information Architecture And App Shell

- Redesign the app shell around clear work zones: vault navigation, active
  document, reference pane, workflow sidebar, command/status surface.
- Define consistent panel placement for editor-only, PDF-only, split research,
  synced reading, and search-heavy modes.
- Add panel persistence rules for width, collapsed state, active sidebar tab,
  and last focused pane.
- Replace ad hoc toolbar grouping with stable command groups: file, edit,
  navigation, view, research, annotation, search.
- Add status bar model for save state, indexing/search progress, PDF page/zoom,
  active pane, and background errors.
- Add tests covering layout mode transitions, panel persistence, command
  availability, and active-pane indicators.
- Deliverable: app-shell specification and tested layout state model before
  large visual restyling begins.

## Milestone 2: Visual Design System

- Define design tokens for spacing, typography scale, border radius, colors,
  focus rings, shadows, divider strength, and semantic states.
- Move reusable colors and component metrics into `native/src/theme.rs` or
  narrow view-local style helpers; avoid scattering raw colors and dimensions.
- Create light, dark, and high-contrast themes with WCAG-oriented color pairs.
- Standardize components: icon button, text button, segmented control, tabs,
  toolbar group, sidebar row, tree row, search result, modal, toast/status item,
  empty state, and error state.
- Define anti-pattern examples in the design tokens doc: forbidden background
  effects, corner radii, glow/shadow usage, gradient usage, empty-state art, and
  card density. Every new screen should pass this review before implementation.
- Replace inconsistent inline styling with shared helpers or style modules
  matching existing Iced patterns.
- Add hover, pressed, focused, selected, disabled, loading, warning, and error
  variants for shared controls.
- Add visual regression coverage for component states and representative
  screens across desktop and narrow window sizes.
- Deliverable: style token map, component-state matrix, and migration checklist
  for existing views.

## Milestone 3: Keyboard And Command Model

- Define one command registry covering menu commands, command palette entries,
  toolbar actions, shortcuts, context menus, and disabled-state reasons.
- Keep command registry UI-facing only. Mutating editor actions still route
  through `EditorCommand`; PDF navigation still routes through existing PDF link
  and navigation helpers.
- Add discoverable keyboard shortcuts for navigation, pane switching, search,
  citation insertion, annotation creation, outline/backlinks, and sidebar
  visibility.
- Add shortcut conflict detection and tests for command routing by active pane.
- Make command palette results context-aware, ranked, and grouped by workflow.
- Add palette actions for changing layout mode, toggling sidebars, opening
  recent files, jumping to headings/pages, and running annotation workflows.
- Add keyboard-only tests for top research workflows from vault open to quote
  insertion, linked-note creation, and return navigation.
- Deliverable: command registry API, shortcut conflict report, and generated
  command-palette inputs for existing actions.

## Milestone 4: Editor UX Polish

- Improve editor visual affordances for cursor, selection, active line, search
  matches, link hover, inline `pdf://` targets, code blocks, tables, math, and
  images.
- Add lightweight block controls for headings, checkboxes, tables, code blocks,
  quote blocks, and PDF citation blocks without disrupting typing flow.
- Add source/render clarity for markdown syntax: hidden markers must remain
  predictable, selectable, and reversible.
- Add contextual inline actions for links, backlinks, citations, task lists, and
  broken references.
- Improve find-in-file UX with result count, wrap status, no-result state,
  replace-ready extension point, and active match visibility.
- Add tests for typing stability, selection preservation, search navigation,
  inline action dispatch, and large-document scroll performance.
- Deliverable: editor interaction spec proving typing, selection, and undo/redo
  behavior remain unchanged except where explicitly designed.

## Milestone 5: PDF Reading And Annotation UX

- Redesign PDF toolbar around page navigation, zoom, selection mode,
  annotation mode, search, TOC, and linked-note actions.
- Add visible reading state: current page, page count, zoom mode, active search
  hit, pending render status, and document load errors.
- Improve text selection feedback, copied-text confirmation, and quote
  insertion affordance.
- Add annotation creation flows for highlight, underline, strike, area note,
  free note, tag, color, and resolved/unresolved status.
- Add keyboard-first PDF annotation commands and annotation-sidebar shortcuts.
- Add tests for page navigation, zoom controls, selection-to-citation,
  annotation creation, filter changes, and stale render/error states.
- Deliverable: PDF toolbar and annotation interaction model with exact disabled
  reasons for no PDF, no selection, unsupported text layer, and missing note.

## Milestone 6: Split Research Workflow UX

- Make split mode feel deliberate: clear active pane, synchronized context,
  follow-citation affordance, and return-to-origin navigation.
- Add synced research mode with explicit source PDF, active markdown note,
  current citation target, and linked-note state.
- Add companion-note surfacing so users can see or switch the note paired with a
  PDF without losing reading position.
- Improve drag/resizer behavior, minimum pane sizes, collapsed reference pane,
  and narrow-window fallback.
- Add cross-pane toasts/status messages for quote inserted, note linked,
  backlink opened, missing target, and repair available.
- Add tests for split layout persistence, pane focus, scroll retention,
  synced-note opening, and narrow-window behavior.
- Deliverable: split-mode state diagram covering focus, scroll target, active
  pane, companion note, and navigation history.

## Milestone 7: Search, Outline, And Navigation UX

- Redesign global search as a searchable command surface for files, headings,
  PDF text, annotations, quick notes, and backlinks.
- Add source filters, result grouping, ranking explanations where helpful,
  streaming status, cancellation, and stale-result suppression UI.
- Improve search previews with highlighted match context, PDF page labels,
  annotation metadata, and linked-note hints.
- Merge markdown outline and PDF TOC into one navigator with icons, hierarchy,
  current-position tracking, and empty/error states.
- Add quick switcher for recent files, open PDFs, headings, pages, and
  backlinks.
- Add tests for streaming search states, source toggles, result activation,
  outline/TOC navigation, and keyboard-only result selection.
- Deliverable: unified search and navigator spec with stale-result, capped-work,
  and cancellation states explicitly documented.

## Milestone 8: Onboarding, Empty States, And Recovery

- Replace bare empty screens with task-focused states for no vault, empty
  vault, no file selected, unsupported file, no PDF text, no backlinks, no
  search results, and no annotations.
- Add first-run vault opener flow with recent vaults, portable-settings note,
  and PDFium availability diagnostics.
- Add recoverable error UX for missing files, moved PDFs, stale annotations,
  failed saves, corrupt PDFs, indexing failure, and database migration failure.
- Add nonblocking notifications for background indexing, PDF cache population,
  repair completion, and export completion.
- Add confirmation and undo affordances for destructive or broad operations.
- Add tests for all empty/error/recovery states and confirmation flows.
- Deliverable: recovery-state matrix mapping each failure to user-visible copy,
  retry/repair action, persistence behavior, and test coverage.

## Milestone 9: Accessibility And Inclusive UX

- Make complete keyboard traversal possible for app shell, editor controls, PDF
  controls, sidebars, modals, palettes, and context menus.
- Add accessible names and roles for icon-only controls where Iced supports
  them; otherwise add visible labels or tooltips with deterministic text.
- Validate contrast for text, focus rings, selected rows, annotations, search
  highlights, disabled controls, and PDF overlay colors.
- Add scalable UI behavior for larger fonts, narrow windows, high DPI, and
  high-contrast themes.
- Add reduced-motion behavior for animated transitions, progress indicators,
  and scrolling effects.
- Add accessibility smoke tests and manual checklist in release docs.
- Deliverable: accessibility checklist checked against every primary screen and
  linked from release docs.

## Milestone 10: Performance And Perceived Speed

- Add lightweight placeholders, progress states, and cancellation for vault
  indexing, global search, PDF rendering, PDF text extraction, backlink
  indexing, and repair.
- Make background work state visible without blocking editor input.
- Add debounce and batching for UI state updates that can flood rendering:
  search streams, PDF page renders, annotation filters, status messages, and
  sidebar recomputation.
- Add performance budgets for layout transitions, typing latency, PDF scroll,
  global search streaming, and large annotation sidebars.
- Add regression tests or counters proving UI updates stay bounded by visible
  content or explicit caps.
- Add debug-only diagnostics panel for cache status, active tasks, queue depth,
  and recent background errors.
- Deliverable: measured performance budget table plus focused tests or counters
  for every hot UI path touched.

## Milestone 11: Documentation And Learnability

- Write concise user guides for vaults, markdown editing, PDF reading,
  citations, linked notes, annotations, search, backlinks, and portable mode.
- Add screenshot-backed workflow examples for research reading, quote
  collection, literature review, study tracking, and note repair.
- Add command/shortcut reference generated from the command registry once it
  exists.
- Add in-app help entry points that open local docs without blocking workflows.
- Keep terminology consistent across UI, docs, command palette, and error
  messages.
- Link user guides from README and release docs.
- Deliverable: documentation map tying every guide to UI entry points and
  command-palette terms.

## Milestone 12: Release UX Hardening

- Run full UX smoke pass on Windows, Linux, and macOS targets with light, dark,
  and high-contrast themes.
- Verify portable settings, PDFium diagnostics, first-run vault flow, recent
  vault restore, crash-safe save messaging, and migration recovery.
- Verify app icons, desktop integration, window sizing, DPI scaling, and file
  association expectations per platform.
- Add release checklist for UI regressions: layout overlap, keyboard traps,
  missing labels, stale loading states, broken shortcuts, and unreadable colors.
- Add visual-authenticity checklist: no glass panels, no decorative gradients,
  no generic SaaS cards, no unnecessary blur/glow, no placeholder-like copy, and
  no UI that looks disconnected from markdown/PDF research workflows.
- Freeze interaction and visual changes before final bug bash.
- Publish known UX limitations and next-version backlog.
- Deliverable: release UX signoff checklist with platform results and known
  limitations.

## Sequencing Notes

- Complete Milestone 0 before broad UI changes; otherwise regressions will be
  hard to distinguish from intended redesign.
- Complete Milestone 1 before Milestone 2 migration; component styling needs
  stable layout ownership.
- Complete Milestone 3 before user documentation; shortcuts and command names
  should come from one source of truth.
- Milestones 4 through 8 can run in parallel after Milestones 0 through 3, but
  each must preserve shared command, theme, and accessibility contracts.
- Milestones 9 through 12 should finish after major interaction changes settle.

## Acceptance Criteria

- Common workflows complete without mouse: open recent vault, open note, search,
  navigate to PDF citation, create annotation, insert citation, return.
- All primary screens have loading, empty, disabled, and error states.
- All shared controls use common style tokens and consistent state behavior.
- Split research mode preserves focus, scroll, pane widths, and navigation
  history predictably.
- Large markdown files, large PDFs, and large vault searches remain responsive.
- Accessibility checklist passes for focus, labels, contrast, keyboard access,
  and reduced motion.
- User docs cover every first-class workflow in `docs/FEATURES.md`.
- `docs/UI_UX_AUDIT.md`, release UX checklist, and command/shortcut reference
  stay current through final handoff.
