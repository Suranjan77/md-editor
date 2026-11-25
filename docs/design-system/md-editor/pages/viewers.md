# Page override — PDF and image viewers

## Shared viewer toolbar

One `Toolbar` (viewer variant) for PDFs and images, docked at the **top** of the pane
(today the PDF toolbar sits at the bottom with text glyphs):

`[outline]  ·  [page input] / N  ·  [− zoom % +] [fit width] [fit page]  ·  [find] [more ⋯]`

- All controls are `IconButton`s with tooltips and shortcuts; zoom percentage is a
  click-to-edit field with tabular numerals.
- Page indicator is an editable field (type a page, Enter jumps).

## Canvas

- Viewer ground: `bg.canvas` in dark, `#E2E8F0` (light `bg.overlay`) in light, so white pages
  read as paper in both themes. Page gap 16px; page shadow = elevation 1.
- Loading pages: a `bg.raised` placeholder at the page's real size (no layout jump) with a
  subtle shimmer that is disabled under reduced motion.
- Load error: centred `EmptyState` with the reason (password-protected, unreadable) and an
  action (Retry / Show in folder). Replaces plain text in `theme::WARNING`.

## Annotations

- Highlight colours come from the **content palette** (MASTER §2.4), 35% alpha, unchanged
  by UI theme.
- Focused annotation: 2px `focus.ring` outline (not a hard-coded teal).
- Selection popover (after text selection): `Popover` with Highlight (colour swatches),
  Copy, Add note, Link note — replaces the right-click-only discovery.
- Reference link preview: `Popover` card, elevation 1, max 420px wide, Escape closes.
