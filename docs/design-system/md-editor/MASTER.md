# MD Editor Design System — Master

The single source of truth for how MD Editor looks, moves and behaves. Page files in
`pages/` override this file for one surface; where they are silent, this file applies.

**Status:** proposed (2026-11-23). Implemented by the plan in
[`docs/ui-overhaul-plan.md`](../../ui-overhaul-plan.md). Until then `native/src/theme.rs`
and `motion.rs` describe what ships.

**Provenance.** Generated with the UI/UX Pro Max skill (v2.15.0):
`--design-system` for "note-taking knowledge management research desktop productivity" and
"markdown editor dark mode focus tool" (dials: variance 2–3, motion 3, density 7), plus
`--domain product|color|typography|ux|icons` searches. Both design-system runs and the
product-domain match for *Knowledge Base/Documentation* converged on **Minimalism & Swiss
Style**. The generator's landing-page *patterns* were discarded (this is a desktop tool,
not a marketing page). Palettes were adapted from its *Developer Tool / IDE* and teal
results; every pair below was re-measured for WCAG contrast, and adjustments are marked.
The skill has no Rust/Iced stack profile, so its guidance is applied through Iced's own
widgets.

---

## 1. Principles

1. **Content first.** The note, the PDF, the page of ink are the interface. Chrome recedes:
   neutral surfaces, one accent, no decoration that does not carry meaning.
2. **Calm, not empty.** Swiss restraint — grid, hierarchy by size and weight, generous but
   deliberate whitespace — with dense-enough controls for a daily tool (density 7/10).
3. **One way to do each thing.** One button system, one icon family, one type scale, one
   motion vocabulary. A new variant needs a reason, not a preference.
4. **Every state is designed.** Rest, hover, pressed, focused, selected, disabled,
   loading, empty, and error exist for every component before it ships.
5. **Tokens, never literals.** Views name roles (`fg.secondary`, `space.4`), never values.
6. **Dark and light are designed together**, and contrast is measured in both.

---

## 2. Colour

Two layers: a **primitive** ramp (never used by views directly) and **semantic roles**
(the only thing views reference). Both themes define every role.

### 2.1 Semantic roles — Dark (default)

| Role | Hex | Use | Contrast (measured) |
| :--- | :--- | :--- | :--- |
| `bg.canvas` | `#0B1120` | Window ground, editor page | — |
| `bg.surface` | `#111827` | Sidebar, panels, toolbars | — |
| `bg.raised` | `#1B2336` | Cards, inputs, hovered rows, menus | — |
| `bg.overlay` | `#272F42` | Selected rows, pressed controls, popovers | — |
| `bg.scrim` | `#020617` @ 60% | Behind modals | measured per composition |
| `border.subtle` | `#1E293B` | Dividers (decorative) | exempt |
| `border.default` | `#334155` | Panel and card edges | 1.7:1 on surface (decorative) |
| `border.control` | `#64748B` | Input and control boundaries | **3.7:1** on surface, 3.3:1 on raised |
| `fg.primary` | `#F8FAFC` | Body and titles | 17.0:1 surface · 12.8:1 overlay |
| `fg.secondary` | `#CBD5E1` | Labels, metadata | 12.0:1 surface · 9.0:1 overlay |
| `fg.tertiary` | `#94A3B8` | Captions, placeholders, disabled-adjacent | 6.9:1 surface · 5.2:1 overlay |
| `accent` | `#2DD4BF` | Primary actions, active state, links | 9.5:1 surface |
| `on-accent` | `#042F2E` | Text/icons on `accent` fills | 7.8:1 |
| `accent.muted` | `#2DD4BF` @ 16% | Selected backgrounds, active tab underlay | — |
| `focus.ring` | `#5EEAD4` | Keyboard focus, 2px | 12.0:1 surface |
| `highlight` | `#F59E0B` | Search matches, attention (not errors) | 8.3:1 surface |
| `status.danger` | `#F87171` | Errors, destructive | 6.4:1 surface · 4.8:1 overlay |
| `status.success` | `#4ADE80` | Saved, completed | 10.2:1 surface |
| `status.warning` | `#FBBF24` | Unsaved, degraded | 10.6:1 surface |
| `status.info` | `#60A5FA` | Neutral notices | 7.0:1 surface |

*Adjusted from the skill's Developer-Tool palette: background darkened from `#0F172A` to
`#0B1120` for more surface separation; run-green accent replaced by the product's existing
teal family (brand continuity with the current `#B1CCC6`), lifted to `#2DD4BF` for 9.5:1
contrast; amber kept from the "markdown editor" run as the highlight colour.*

### 2.2 Semantic roles — Light

| Role | Hex | Contrast (measured) |
| :--- | :--- | :--- |
| `bg.canvas` | `#F8FAFC` | — |
| `bg.surface` | `#FFFFFF` | — |
| `bg.raised` | `#F1F5F9` | — |
| `bg.overlay` | `#E2E8F0` | — |
| `border.default` | `#CBD5E1` | decorative |
| `border.control` | `#64748B` | **4.8:1** on surface |
| `fg.primary` | `#0F172A` | 17.9:1 surface · 14.5:1 overlay |
| `fg.secondary` | `#334155` | 10.4:1 surface |
| `fg.tertiary` | `#475569` | 7.6:1 surface · 6.2:1 overlay |
| `accent` | `#0F766E` | 5.5:1 surface |
| `on-accent` | `#FFFFFF` | 5.5:1 |
| `focus.ring` | `#0D9488` | 3.7:1 surface (non-text, ≥3:1) |
| `highlight` | `#B45309` | 5.0:1 surface |
| `status.danger` | `#B91C1C` | 6.5:1 surface |
| `status.success` | `#15803D` | 5.0:1 surface |
| `status.warning` | `#A16207` | 4.9:1 surface |
| `status.info` | `#1D4ED8` | 6.7:1 surface |

**Light-theme rule:** coloured *text* (`accent`, `highlight`, `status.*`) goes on
`bg.canvas`, `bg.surface` or `bg.raised` only. On `bg.overlay` it drops to 4.0–4.4:1, so
selected rows use `fg.*` for text and show the colour in an icon or bar instead.

### 2.3 Colour rules

- Meaning never rides on colour alone: errors carry an icon and text; graph node kinds
  differ by shape; search matches differ from selection by fill *and* outline.
- `accent` marks the single primary action per view and the current location. It is not a
  decoration colour.
- `highlight` (amber) is for "found / look here", never for errors.
- Document content colours (ink pens, PDF highlight colours, graph categories) are a
  separate **content palette** (§2.4), so a user's yellow highlight never changes meaning
  when the UI theme changes.

### 2.4 Content palette (user-chosen colours)

Five annotation/ink hues, each with a dark and light rendering so they stay legible on both
paper and canvas: yellow `#FACC15`, green `#4ADE80`, blue `#60A5FA`, pink `#F472B6`,
orange `#FB923C` (dark); light renderings use the 600 step of the same hue. PDF
highlights draw at 35% alpha over the page in both themes (the page itself stays white).

---

## 3. Typography

Fonts are **bundled** (`include_bytes!` + `daemon.font()`), never left to the OS, so
measurement and rendering match on every platform. All three are OFL-licensed.

| Family | Use | Weights |
| :--- | :--- | :--- |
| **Inter** | All UI chrome; default document face | 400, 500, 600, 700 |
| **JetBrains Mono** | Code blocks, inline code, numeric data, shortcuts | 400, 600 |
| **Source Serif 4** *(optional)* | Alternative document face (Settings → Reading font) | 400, 600, italic |

*Inter is the skill's match for "developer tools, high-end productivity apps"; the mono
pairing follows its Inter + JetBrains Mono tri-stack. Source Serif 4 answers the skill's
long-form-reading serif results with a face built for screens.*

### 3.1 UI type roles

Minimum UI size is **12px** (skill: text < 12px is an anti-pattern). Numbers in data
(timers, counts, page numbers) use tabular figures (`tnum`) or the mono face.

| Role | Size / line | Weight | Use |
| :--- | :--- | :--- | :--- |
| `caption` | 12 / 16 | 400 | Metadata, counters, helper text |
| `label` | 12 / 16 | 500 | Section labels (sentence case, no all-caps tracking) |
| `body` | 13 / 20 | 400 | Default UI text, rows, menu items |
| `body.strong` | 13 / 20 | 600 | Emphasis within UI text |
| `title.sm` | 14 / 20 | 600 | Panel headers, dialog body headings |
| `title` | 16 / 24 | 600 | Dialog titles, view titles |
| `heading` | 20 / 28 | 600 | Settings sections, tracker headers |
| `display` | 32 / 40 | 700 | Welcome wordmark only |

Replaces today's `TEXT_XS` 8 / `TEXT_SM` 10 / `TEXT_BASE` 11 / `TEXT_MD` 14 / `TEXT_LG` 16 /
`TEXT_DISPLAY` 42.

### 3.2 Document typography

Owned by `pages/editor.md`: body 17px / 1.6, measure ≤ 72ch, heading scale ×1.25.

---

## 4. Space, size, shape

### 4.1 Spacing (4px grid)

| Token | px | Use |
| :--- | :--- | :--- |
| `space.0` | 0 | Flush |
| `space.px` | 1 | Hairlines only |
| `space.1` | 4 | Icon ↔ label, within a control |
| `space.2` | 8 | Between siblings in a control group; row padding (vertical) |
| `space.3` | 12 | Control padding (horizontal); between rows in a list group |
| `space.4` | 16 | Panel padding; between groups |
| `space.5` | 24 | Between sections |
| `space.6` | 32 | Between major regions; dialog padding |
| `space.7` | 48 | Empty-state breathing room |

Replaces the 2/6px steps, which sat off the 4px grid.

### 4.2 Control sizes

| Token | Height | Use |
| :--- | :--- | :--- |
| `control.sm` | 24 | Inline chips, toggles inside dense bars |
| `control.md` | 28 | Default buttons, inputs, toolbar items, list rows |
| `control.lg` | 36 | Primary dialog actions, welcome actions |

Pointer targets are never smaller than **24×24** (WCAG 2.2 AA target size); icon buttons
are 28×28 with a 16px glyph.

### 4.3 Radius

`radius.sm` 4 (chips, badges) · `radius.md` 6 (controls, rows) · `radius.lg` 10 (cards,
panels, popovers) · `radius.xl` 14 (dialogs) · `radius.full` (pills, avatars).

### 4.4 Elevation

Flat by default (Swiss). Elevation is used only for things that float.

| Level | Use | Treatment |
| :--- | :--- | :--- |
| 0 | Panes, panels | `border.default`, no shadow |
| 1 | Popovers, menus, tooltips | `bg.raised`, `border.default`, shadow 0/4/12 @ 30% |
| 2 | Dialogs, command palette | `bg.raised`, shadow 0/12/32 @ 40%, over `bg.scrim` |
| 3 | Toasts | `bg.overlay`, shadow 0/8/24 @ 40% |

### 4.5 Layers (z-order)

`base` → `sticky` (toolbars, find bar) → `popover` (menus, tooltips) → `scrim` → `dialog`
→ `toast`. Implemented as fixed `stack` order in `app/view.rs`; nothing else may stack.

### 4.6 Icons

- **One family: Phosphor, regular weight** (the skill's primary icon library), bundled as
  SVG through iced's `svg` feature. Replaces the hand-drawn canvas icons in
  `views/icons.rs` and every Unicode glyph used as a control (`-`, `+`, `☰`, `⧉`, `⟳`, `✎`,
  `✕`, `●`).
- Sizes: `icon.sm` 14 · `icon.md` 16 (default) · `icon.lg` 20. Stroke weight never mixed
  within a surface; filled variants only for a *selected* state.
- Icon colour follows its text role; standalone icon controls need 3:1 against their
  background.

---

## 5. Motion

Keep the existing closed-form `Spring` and `EaseOutQuad` (they already meet "interruptible"
and "spring-physics"). Add duration tokens chosen by distance, and a reduced-motion mode.

| Token | Duration | Use |
| :--- | :--- | :--- |
| `motion.instant` | 0 | Reduced motion; state that must not lag |
| `motion.fast` | 100 ms | Hover, press, toggle, focus ring |
| `motion.base` | 160 ms | Panel open, tab switch, crossfade |
| `motion.slow` | 220 ms | Dialog/palette enter |
| exits | ×0.65 of enter | All exits (skill: exit faster than enter) |

Rules: animate opacity and offset only, never layout size (panel width animation is the
one exception and must stay clipped); at most two animated elements per view; every
animation is interruptible and never blocks input; with **Reduce motion** on, durations
become `instant`, the caret glide and smooth scroll jump, and the graph settles without
visible physics.

---

## 6. Components

Built once in `native/src/ui/` and reused everywhere. Each lists its required states.

| Component | Variants | States |
| :--- | :--- | :--- |
| `Button` | primary, secondary, ghost, danger; sm/md/lg; optional leading icon | rest, hover, pressed, focused, disabled, loading |
| `IconButton` | ghost, toggle (on/off) | same + selected; **always has a tooltip** with name and shortcut |
| `TextField` | default, search (leading icon, clear button) | rest, focused, invalid (message below), disabled |
| `Select` / `Segmented` | — | rest, hover, selected, focused |
| `Toggle` / `Checkbox` | — | on, off, mixed, disabled, focused |
| `ListRow` | file, result, command | rest, hover, selected, active (current doc), focused, drag-over |
| `TreeRow` | folder, file | + expanded/collapsed, indent guide, truncation with tooltip |
| `Tabs` | underline | rest, hover, selected, focused |
| `Panel` | sidebar, inspector | header (title + actions), body, empty state |
| `Card` | stat, info | — |
| `Dialog` | confirm, form, destructive | open/close motion, Escape, Enter = primary, focus trapped |
| `Popover` / `Menu` / `ContextMenu` | — | item hover, disabled item, separator, shortcut hint |
| `Tooltip` | — | delay 500 ms, instant when moving between tooltipped controls |
| `Toast` | info, success, error | error persists with action; others 4 s |
| `Kbd` | — | renders shortcuts (`Ctrl` `P`) in mono on `bg.raised` |
| `Badge` / `Chip` | neutral, accent, status | — |
| `ProgressBar` / `Spinner` | determinate, indeterminate | — |
| `EmptyState` | — | icon, one-line explanation, one action |
| `StatusBar` | — | save state, cursor, word count, background work |
| `Toolbar` | app, viewer, find | grouping with separators, overflow |
| `SplitHandle` | — | rest, hover, dragging; double-click resets |

---

## 7. Interaction & accessibility

- **Focus:** every interactive element shows a 2px `focus.ring` (offset 2px) when focused
  by keyboard. Tab order follows reading order: toolbar → sidebar → editor → side panels.
- **Keyboard:** every pointer action has a keyboard path (tree navigation with arrows, F2
  rename, Delete; palette for everything else; graph node pin/unpin without dragging).
- **Tooltips** on every icon-only control, including its shortcut.
- **Truncation:** single-line ellipsis plus a tooltip with the full text (file names,
  paths); never mid-word wrapping in rows.
- **Destructive actions:** confirmed in a `Dialog` with a `danger` primary button, and
  offer undo where the data allows (delete → toast "Undo" while the file is in the trash).
- **Feedback within 100 ms** for every press (press state), visible progress for anything
  over 1 s.
- **Text scaling:** Settings → *Interface size* scales the type and control tokens (90–130%).
- **Screen readers:** not achievable on Iced 0.14, which has no accessibility tree. Tracked
  as a dependency on Iced's AccessKit work; everything else above is in scope now.

---

## 8. Anti-patterns (do not)

- Literal colours, sizes, paddings or durations in `views/`, `app/` or `ui/` (enforced by a
  lint test).
- Unicode or emoji as icons.
- Text under 12px in the UI; all-caps micro labels.
- Two primary buttons in one view; colour-only status.
- Modal overlays for primary navigation (the tracker becomes a view, not an overlay).
- Animating layout properties other than the clipped panel widths; motion with no cause.
- Showing a control that does nothing (disabled controls look disabled and explain why in
  their tooltip).
