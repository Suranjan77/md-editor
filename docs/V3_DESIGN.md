# V3 design system

> Record of the 2026-06-14 design pass (user-directed). The app had drifted to a
> green-tinted, undesigned look with single-character "icons". This is the
> reference for the visual language going forward. Execution detail and the
> append-only decision log live in `docs/V3_HANDOFF.md`.

## Direction (user choices, 2026-06-14)

- **Refined teal, kept as lineage** — the study-app identity stays, but teal is
  *only* an accent now (links, active tab, selected file, focus, caret,
  selection). It is never a surface color.
- **Neutral, professional surfaces** — backgrounds are neutral dark gray
  (GitHub-dark family), not green-tinted. "Looks professional" was the bar.
- **Single theme** — no light mode, no theme switcher in settings. Dark is the
  only reachable theme (the light-theme *plumbing* removal is a tracked
  follow-up because it touches `session_restore` tests).
- **Real icons, not characters** — every chrome affordance uses the canvas icon
  system, not a Unicode glyph stand-in.

## Tokens (`v3/shell/src/gui/tokens.rs`)

`DARK_TOKENS` is the live palette. Neutral grays + one teal accent:

| token | hex | role |
|---|---|---|
| `bg_primary` | `#0d1117` | window / editor background |
| `bg_secondary` | `#161b22` | sidebar, panels, overlays |
| `bg_tertiary` | `#1f242c` | raised surfaces, code background |
| `bg_surface` | `#1a1f27` | overlay cards |
| `border` / `border_subtle` | `#2d333b` / `#21262d` | dividers |
| `text_primary` | `#e6edf3` | body text |
| `text_secondary` | `#9aa4af` | section headers, muted labels |
| `text_muted` | `#6e7681` | de-emphasized |
| `text_heading` | `#f0f4f8` | **Markdown headings (near-white, by weight not color)** |
| `accent` | `#4fd1b5` | teal: links, active tab, selected file, focus, caret |
| `accent_secondary` | `#7ee3cd` | wikilinks, hover accents |
| `danger` | `#ef7a72` | **destructive/error only** (delete, confirm-delete, error text, syntax keyword) |
| `success` | `#5cd6a0` | inline code, syntax strings |
| `warning` | `#e0b95e` | syntax numbers |
| `sel_tint` | `#4fd1b5` @ 0.18 | selection highlight |

Rule of thumb: **surfaces are neutral gray; the only saturated UI color is the
teal accent; `danger` coral is reserved for destructive/error semantics.**
Headings are *not* colored — they read as headings by size and weight.

`LIGHT_TOKENS` still exists but is unreachable from the UI (single-theme
decision). It is kept only so the theme plumbing (and its `session_restore`
round-trip test) compiles until that plumbing is removed.

## Icons (`v3/shell/src/gui/icons.rs`)

Icons are **drawn with `iced::canvas`** (vector line-art), not a font and not
emoji — dependency-free, theme-colored, crisp at any size. `icons::view(Icon,
color, size)` returns an `Element`. The `Icon` enum is the catalog.

Where they are used (all previously text/character stand-ins):

| surface | was | now |
|---|---|---|
| file rows | `MD ` / `PDF ` text | `Icon::File` / `Icon::Pdf` / `Icon::Folder` |
| tree header | `+N` `+F` `−` `↻` | `NewNote` `NewFolder` `Sidebar` `Refresh` |
| pane controls (top-right) | `⇥` `⇩` `×` | `Split` `SplitDown` `Close` |

Icons added in this pass: `NewNote`, `NewFolder`, `Pdf`, `Refresh`, `SplitDown`,
`Close` (and `Split` already existed). The active file/row tints its icon teal
via the `color` argument.

Still on text (intentional / follow-up): the Markdown formatting toolbar
(`B I Code H List Todo Link`) — `B`/`I` are conventional; the rest are
candidates for icons in a follow-up.

## Structural fixes folded into this pass

- **Headings** were painted with `tokens.danger` (the coral error color) →
  now `tokens.text_heading`. (`editor_canvas/palette.rs::heading`)
- **Table separator** (`|---|`) rendered a 1px dash *per cell*, reading as
  "floating dashes" in the gap between header and body → now one continuous
  full-width header rule. (`paint.rs`, paint-only; a height collapse was
  rejected because it would squish the source-revealed row.)
- **Vault header** and the **"MD Editor"** brand title no longer use coral
  (`text_secondary` and `accent` respectively).
- **Settings panel** lost the Light/Dark switcher; its coral section labels are
  now neutral.

## Verification

Golden draw-plan regenerated (`UPDATE_EXPECT=1`); `clippy -D warnings` clean;
size budget green (`gui/mod.rs` trimmed back under its 1441 ratchet);
`chrome` / `file_tree` / `discoverability` / `session_restore` suites pass.
Live look was confirmed on the COSMIC workstation (see
`docs/V3_GUI_TESTING.md` for why that check is local-only).

## Tracked follow-ups

1. Remove the light-theme plumbing entirely (`LIGHT_TOKENS`, `theme_name`,
   `SettingsThemeChanged`, the snapshot field) and update the `session_restore`
   theme round-trip test.
2. Icon-ify the Markdown formatting toolbar.
3. The `🗑` delete glyph in the annotations panel is still an emoji — give it a
   drawn `Trash` icon.
