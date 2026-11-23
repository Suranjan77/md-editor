# Page override — Handwriting window

## Chrome

- The ink window uses the same `Toolbar` component and tokens as the shell (it has its own
  ~30 literal sizes today). Tool selection is a `Segmented` of `IconButton`s (pen,
  highlighter, eraser, lasso) with the active tool filled (MASTER §4.6 filled = selected).
- The colour palette is a vertical rail of 28px swatches with a 2px `focus.ring` on the
  active one; each swatch has a tooltip naming the colour (not colour-only).
- Unsaved state: title prefix "●" (kept) **and** a `status.warning` "Unsaved" chip in the
  toolbar; Save is the single primary button.

## Paper

Paper is content, not chrome: it follows the user's paper choice, not the UI theme.
Paper colours: light paper `#FAFAF7` with ruling `#D6D3CB`; dark paper `#16181D` with ruling
`#2B2F38`. Selection (lasso) uses `accent` @ 20% fill + dashed `accent` outline.

## Ink colours

From the content palette (MASTER §2.4), with black/white swapped to stay visible on the
chosen paper. A colour is never re-mapped when the paper changes after strokes exist.

## Radial menu

Ring backgrounds `bg.raised` @ 95%, active slot `accent.muted`, labels `caption`. Under
reduced motion it appears without the scale-in.
