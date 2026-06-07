# UI/UX Release Checklist

Last updated: 2026-06-07

Use this checklist during release smoke passes and before closing UI/UX roadmap
milestones. Record platform-specific results in `docs/RELEASE_SIGNOFF.md` and
active blockers in `docs/HANDOFF.md`.

## Scope

Run against the primary app screens:

- Vault opener.
- App shell toolbar and sidebar.
- Markdown editor and file search.
- PDF viewer and PDF search.
- Split research mode.
- Global search.
- Command palette and citation palette.
- Annotation sidebar, TOC, backlinks, tracker, modals, and toast/status areas.

## Layout

- No overlapping text in toolbar, sidebars, overlays, modals, search panels, or
  PDF controls at desktop and narrow widths.
- Active path, save state, page/zoom state, and search status remain visible
  where expected.
- Long vault paths, long annotation text, and long search previews wrap or clip
  cleanly without covering adjacent controls.
- Split mode preserves usable minimum widths for markdown, PDF, divider, and
  sidebars.
- Empty/error/loading states do not shift surrounding controls unexpectedly.

## Keyboard

- Open recent or new vault without mouse where supported by current UI.
- Open command palette, global search, file search, PDF search, and citation
  palette with documented shortcuts.
- Escape closes the topmost modal/overlay first and does not trap the user.
- Enter submits modal or palette actions only when the active context expects
  submission.
- Sidebar, TOC, backlinks, annotation sidebar, and search results have a
  documented keyboard path or are listed as current limitations.
- No keyboard trap remains after closing modals, context menus, palettes, or
  search overlays.

## Labels

- Icon-only controls have visible labels, tooltips, or a documented limitation
  for current Iced accessibility support.
- Search filters, toggles, checkboxes, segmented controls, and destructive
  actions have deterministic user-visible text.
- Disabled or inert controls expose a visible reason where workflow-critical
  actions are unavailable.
- Command palette terms match toolbar labels, shortcut docs, and user guides.
- Error, empty, and recovery copy uses consistent nouns: vault, note, PDF,
  annotation, citation, backlink, and search result.

## Contrast

- Body text, muted text, selected rows, active tabs, focus rings, disabled
  controls, PDF overlays, annotation colors, and search highlights remain
  readable in light and dark themes.
- High-contrast theme gaps are recorded if not yet implemented.
- Warning/error/success status colors remain distinguishable without relying on
  color alone.
- PDF annotation overlay colors remain visible on bright and dark page content.
- Link, citation, and broken-reference affordances remain distinguishable from
  plain text.

## Loading And Recovery

- Vault indexing, global search, PDF search, PDF rendering, PDF text extraction,
  backlink refresh, and repair flows show nonblocking progress or explicit
  pending state.
- Stale loading states clear after success, cancellation, close, or error.
- Background errors remain discoverable without blocking editor input.
- Empty states exist for no vault, empty vault, no file selected, no PDF text,
  no search results, no backlinks, no annotations, and no TOC.
- Destructive or broad operations require confirmation and document undo or
  recovery behavior where available.

## Motion

- Animated or auto-scrolling behavior has a reduced-motion fallback or is
  recorded as a limitation.
- Progress indicators do not flash rapidly during search streams, PDF renders,
  annotation filter changes, or status updates.
- Programmatic scroll for citation/PDF navigation is predictable and does not
  fight manual scrolling.
- Toast/status transitions do not hide critical errors before the user can read
  them.

## Platform Pass

- Run layout, keyboard, contrast, loading, and motion checks on Windows, Linux,
  and macOS release targets.
- Verify window sizing, DPI scaling, portable settings, PDFium diagnostics,
  desktop integration, app icon, and recent-vault restore.
- Record known limitations with exact platform and date.

Linux release, installer, portable-mode, DPI, and headless search/recovery
checks passed on 2026-06-07. See `docs/RELEASE_SIGNOFF.md`. Windows and macOS
remain unverified.
