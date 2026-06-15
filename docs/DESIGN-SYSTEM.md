# Quiet Vault — Design System

> **This document supersedes the older `docs/DESIGN.md` for all new surfaces.** When the two
> disagree, this file (and the reference implementation `docs/quiet-vault-reference.html`)
> wins.

> Dark-only design system for the **M** markdown/PDF study vault.
> Concept: **document-first, not panel-first.** The classic IDE chrome is dissolved — the note is the hero; everything else is a quiet, retractable rail or a summoned overlay. Keyboard- and command-palette-centric.
>
> Reference implementation: `docs/quiet-vault-reference.html`. This file is the source of truth for any new surface (PDF viewer, Welcome, Settings, overlays, etc.). Match it exactly.

---

## 0. Principles (read first)

1. **The note is the hero.** Center it as a ~750px sheet of dark paper with generous margins. Chrome shrinks; content dominates.
2. **No menu bar.** Global actions live in the **command spine** (the centered command bar, ⌘K) and the command palette. Don't reintroduce a `File · Edit · View` menu strip.
3. **Rails, not panels.** Left (files) and right (study tracker) are quiet, retractable rails — not heavy bordered panels. They collapse for pure focus (⌘B / ⌘⇧T).
4. **Calm & minimal.** Neutral near-black grays, one purple accent used sparingly, a single teal secondary for wikilinks. **No gradients as decoration** (the only gradients allowed are the metallic logo and thin progress fills). No emoji. No rounded-corner-with-left-accent-bar cards.
5. **Headings read by weight & size, never color.** Color is reserved for links, accents, and syntax.
6. **Keyboard-first, low chrome.** Surface actions through the palette + hover tooltips, not dense toolbars.
7. **Quiet motion.** A blinking caret, a soft palette fade, an overlay dim. Everything else is still. Respect `reduce motion`.

---

## 1. Color tokens

All values are literal hex / rgba. **Inline styles only** — there is no CSS variable layer; repeat these literals.

### Surfaces (darkest → lightest)
| Token | Value | Use |
|---|---|---|
| `bg/rail` | `#0a0a0d` | Left & right rails, code-block bg, deepest insets |
| `bg/chrome` | `#0c0c10` | Top bar, status bar |
| `bg/canvas` | `#0e0e12` | App canvas + editor sheet background |
| `bg/inset` | `#0b0b0f` | Math block, quiet inset panels |
| `surface/1` | `#101014` | Stat cards, reading rows, palette footer |
| `surface/2` | `#15151b` · `#16161c` | Table header, command bar, chips, inputs |
| `surface/3` | `#1a1a21` | Active tab pill |
| `surface/code-inline` | `#1c1c23` | Inline `code` background |
| `surface/palette` | `#141419` | Command palette body |

### Borders (always hairline)
| Token | Value |
|---|---|
| `border/faint` | `rgba(255,255,255,0.05)` |
| `border/hairline` | `rgba(255,255,255,0.06)` |
| `border/strong` | `rgba(255,255,255,0.07)` |
| `border/overlay` | `rgba(255,255,255,0.09)` – `rgba(255,255,255,0.10)` |

### Text
| Token | Value | Use |
|---|---|---|
| `text/title` | `#f2f2f5` | Headings, big numbers |
| `text/primary` | `#e8e8ec` | UI primary, active items |
| `text/body` | `#c9c9d0` | Editor body copy |
| `text/secondary` | `#a8a8b0` · `#9a9aa2` | Secondary labels, table cells |
| `text/muted` | `#6b6b73` · `#5c5c64` | Captions, meta, section labels |
| `text/faint` | `#33333a` · `#4a4a52` | Separators (`·`), disabled |

### Accent — purple (primary)
| Token | Value | Use |
|---|---|---|
| `accent` | `#bd93f9` | Active file, focus, links, caret, dirty dot, checkboxes, log hours |
| `accent/bgDim` | `rgba(189,147,249,0.10)` | Active row background, command highlight |
| `accent/borderDim` | `rgba(189,147,249,0.14)` | Accent card borders |
| `accent/borderHi` | `rgba(189,147,249,0.35)` | Focus/hover border on command bar |
| `accent/fill` | `linear-gradient(90deg,#9b7fd4,#bd93f9)` | Progress-bar fill only |
| `accent/onLilac` | `#8a7fb0` | Muted purple label text |
| `selection` | `rgba(189,147,249,0.28)` | `::selection` |

### Secondary & semantic
| Token | Value | Use |
|---|---|---|
| `wikilink` | `#67c6b0` (border `rgba(103,198,176,0.32)`) | `[[wikilinks]]` only — distinct from purple md-links |
| `danger` | `#e0735f` · `#cf7a68` · `#e06b5e` | PDF icon, delete affordances |
| `info` | `#7fa8d8` | Image-file icon |
| `success` | `#6c8c7c` | "saved" status |
| `prio/high` | `#e0b04a` | Reading priority — high |
| `prio/med` | `#67c6b0` | Reading priority — medium |
| `prio/low` | `#6b6b73` | Reading priority — low |

### Code syntax palette (calm, few hues)
| Role | Value |
|---|---|
| base | `#c4c4cc` |
| keyword | `#bd93f9` |
| function | `#e0b04a` |
| string | `#8fbf7f` |
| type / constant | `#67c6b0` |
| self / param | `#cf8f6a` |
| comment | `#5a5a62` |

---

## 2. Typography

Two families. Load via Google Fonts in `<helmet>`:
`Hanken+Grotesk:wght@400;500;600;700` and `Geist+Mono:wght@400;500`.

- **`Hanken Grotesk`** — all UI and editor body. Warm humanist sans; calm at reading sizes. **Never Inter/Roboto/Arial.**
- **`Geist Mono`** — code, numerals you want monospaced (timers, hours, dates, keycaps, line/col), technical labels.

### Type scale
| Role | Size / weight / tracking / line-height | Color |
|---|---|---|
| Editor H1 | 34 / 700 / -0.8px / 1.15 | `text/title` |
| Editor H2 (rendered) | 25 / 700 / -0.4px | `text/title` |
| H3–H6 | step down by weight & size only (≈21 → 16, 700→600). **No color.** |
| Body | 16.5 / 400 / 1.78 | `text/body` |
| Inline `code` | 14 mono, bg `surface/code-inline`, text `#d9c7f5`, pad `2px 6px`, radius 5 |
| Code block | 13 mono / 1.65 | per syntax palette |
| UI default | 14 / 400–500 | `text/primary` |
| UI small | 12 – 12.5 | secondary |
| Caption / meta | 11 – 12 | muted |
| Section label | 11 / 600 / 0.7px / UPPERCASE | `text/muted` |
| Micro (status, keycaps) | 10 – 11 mono | muted |

**Min sizes:** never below 11px UI / 13px reading body. Hit targets ≥ 28px (rail rows 30, buttons 26–34).

---

## 3. Spacing, radius, sizing

- **Radius scale:** `5` (inline code, keycaps) · `7` (tree rows, icon buttons) · `8` (chips, tabs, buttons) · `9` (command bar, list rows) · `10–12` (cards, code, table) · `14` (command palette).
- **Fixed heights:** top bar `48` · center tab strip `40` · tracker tab strip `38` · status bar `26` · tree/list row `30` · chips & tabs `28` · icon buttons `26–30` · primary buttons `34`.
- **Rail widths:** left files `258` · right tracker `308`. Both toggle to `0` (unmounted via `sc-if`).
- **Editor sheet:** `max-width:752px; margin:0 auto; padding:56px 40px 220px;` (big bottom pad so the last block can scroll to center).
- **Gaps:** prefer flex/grid `gap`. Section blocks `margin-bottom:22px`. Row gaps `8–13`.

### ⚠ Layout rule (non-negotiable)
Every flex child that scrolls **must** carry `min-height:0` (and its column parent too), or it grows to content height and won't scroll. Pattern:
```
root: height:100vh; display:flex; flex-direction:column; overflow:hidden
 └ middle row: flex:1; min-height:0; display:flex
     └ center col: flex:1; min-width:0; min-height:0; display:flex; flex-direction:column
         └ scroll area: flex:1; min-height:0; overflow-y:auto
```

---

## 4. Iconography

- Custom **line icons**, `viewBox="0 0 16 16"`, `fill="none"`, `stroke="currentColor"`, `stroke-width:1.3–1.5`, rendered at 12–16px. Color via the parent's `color`.
- File-type icon colors: folder/md `#7a7a82` (active `#bd93f9`), pdf `#cf7a68`, image `#7fa8d8`.
- **No emoji, anywhere.** No filled/duotone icon sets.
- The brand **M**: 26px rounded tile, `linear-gradient(150deg,#2c2c30,#161618)`, inset top highlight + soft drop shadow; the letter is `background-clip:text` on `linear-gradient(160deg,#f4f4f6,#9a9aa2)` (metallic). This is the only place a metallic treatment appears.

---

## 5. Core components (recipes)

### Top bar (command spine) — h48, `bg/chrome`
`[ M vault switcher ▾ ] | [⊟ left toggle] [ ⌘K command bar (centered, max 560) ] [ today-log chip ] [⊟ right toggle] [⚙ settings]`
- **Command bar:** `surface/2`, hairline border, radius 9, magnifier + placeholder + `⌘K` keycap; hover → `accent/borderHi`. Click opens the palette. This replaces the menu bar.
- Icon buttons: 30px, transparent, hover `rgba(255,255,255,0.05)`; active rail toggle tinted `accent`.

### Left rail — files — w258, `bg/rail`
- Header: `FILES` section label + new-note (+) + collapse icons.
- Tree rows: h30, radius 7. Indent via `padding-left = 12 + depth·16` (folders) / `28 + depth·16` (files); folder caret absolutely placed at `10 + depth·16`, rotates 90° when open. Active row: `accent/bgDim` bg, `text/title`, accent icon; dirty = 6px accent dot trailing. Hover `rgba(255,255,255,0.04)`.
- Footer: a quiet "Backlinks · N" affordance.
- Context menu (to design): Open · Rename · Delete · New Note Here · New Folder Here — as a floating `surface/palette` card, radius 12, hairline border, 14px rows.

### Center — tab strip (h40) + editor
- Tabs are **calm pills**, not chrome: active = `surface/3` + `border/strong` + dirty dot; inactive = transparent, hover tint. Each has a tiny file-type glyph and an × (middle-click closes). `+` new-tab button. Right side: outline + split-right icon buttons.
- One pill per open doc; overflow scrolls horizontally.

### Editor — live in-place markdown
Render every block to its final form; **the caret's current line reveals raw markers**. Active-line treatment: subtle `accent/bgDim` background, mono markers at `rgba(189,147,249,0.55)`, then the rendered text, then a 2px accent caret with `qvblink`.
Block styles:
- **Bold** `700 #ececef` · *italic* normal-style emphasis · inline `code` (token above).
- **md link**: `accent`, 1px underline. **wikilink**: `wikilink` color + tinted underline. (Two link colors, by design.)
- **Blockquote**: `2.5px` left border `rgba(189,147,249,0.45)`, text `#9a9aa2`, not italic.
- **Task checkbox**: 18px, radius 5; unchecked border `rgba(255,255,255,0.22)`; checked = accent fill + `#0e0e12` check; done text struck + muted. Click toggles.
- **Code block**: `bg/rail`, hairline border, radius 10; 34px header with lang label (mono, muted) + copy icon; body padded 14×16.
- **Table**: hairline border radius 10; header row `surface/2`; cells 9×14, row separators `border/faint`.
- **Math**: inline & block rendered to images (LaTeX). Block = centered in `bg/inset` panel, radius 10.
- Images: scaled, aspect preserved.

### Right rail — Study Tracker (a **ledger**, not a stopwatch) — w308, `bg/rail`
Tabs: **Dashboard · Log · Projects · Gates · Reading · Config** (active = `surface/2` pill).
The tracker's job is logging daily work after the fact and tracking projects/gates/reading — **do not lead with a live running timer.**
- **Stat row**: two `surface/1` cards (e.g. Today `2.5h`, Streak `12 days`); the streak/number can take accent.
- **This Week**: 7 mini bars (today = solid `accent`, others = `rgba(189,147,249,0.30)`, empty = `rgba(255,255,255,0.06)`); goal caption.
- **Log (ledger)**: section label + week total; a dashed "Log hours for today…" add row (hover → accent); then entries `date(mono muted) · hours(mono accent) · note(truncated) · ×(delete, hover danger)`, separated by `border/faint`.
- **Projects**: name + `Phase n of m` (mono) + 5px progress track with `accent/fill`.
- **Gates**: 16px ring checkpoints (done = accent fill + dark check, struck label) + due date (mono).
- **Reading**: `surface/1` rows with a priority dot (`prio/*`), title (truncated) + meta.
- **Config** tab: a JSON editor (mono, `bg/rail`), stored globally across vaults.

### Status bar — h26, `bg/chrome`, mono 11
Left: `Ln n, Col n · Markdown`. Right: `N words · saved(success) · UTF-8`. Micro and quiet — this is not a toolbar.

### Command palette (overlay archetype)
- Scrim: `rgba(6,6,9,0.55)` + 2px blur, `qvdim` in. Panel: 640px, `surface/palette`, `border/overlay`, radius 14, big soft shadow, `qvfade` in. Top: search field with live caret + `esc` keycap. Rows h42: icon + name + group + keycap; active row `accent/bgDim`. Footer: ↑↓ navigate · ↵ run · "76 commands".
- **All overlays inherit this language** (Quick Open, Vault Search, Find, PDF TOC, Backlinks, Settings, Name Input, Confirm, Go-to-Page, Annotation Note, Orphaned Annotations, Link Preview). Center-top for command/search; centered for dialogs.

### Toasts (to design)
Bottom-right stack; `surface/2`, hairline border, radius 9–10; icon + message + ×; auto-dismiss ~4s. Success uses `success`, errors `danger`. Quiet, never modal.

---

## 6. Motion

Defined in `<helmet>` keyframes; everything subtle.
- `qvblink` 1.1s step-end — caret.
- `qvfade` 0.16s cubic-bezier(0.2,0.7,0.3,1) — palette/overlay panel in.
- `qvdim` 0.12s — scrim in.
- Hover transitions: instant or ≤120ms. **Reduce-motion** disables scroll animations and the caret blink.

---

## 7. Keyboard model (drives the UI)

`⌘⇧P` palette · `⌘P` quick open · `⌘⇧F` vault search · `⌘F` find in note · `⌘⇧B` backlinks · `⌘B` files rail · `⌘⇧T` tracker rail · `⌘,` settings · `⌘S` save · `⌘1–6` heading · `⌘Enter` checkbox · `⌘H` highlight (PDF) · `⌘N` annotation note (PDF) · `Alt+←/→` PDF jump history.
~76 commands total; many are palette-only. Keymaps are rebindable per-vault via JSON and **scope-aware** (markdown / PDF / overlay contexts).

---

## 8. Surfaces still to design (apply everything above)

- **PDF viewer + annotation** — continuous scroll; zoom/fit/go-to-page as overlays; TOC panel (mirror the outline tree); selection → highlight with 4-color cycle (yellow/pink/blue/green) + note; annotations list with swatch + note + delete; sidecar storage; jump history. Reuse rail + overlay language; the 4 highlight colors are the only place those hues appear.
- **Welcome / vault screen** — centered, calm: big **M**, "Open Vault" / "Create Vault", Recent Vaults (last 8). Same palette; no chrome.
- **Settings** (`⌘,`) — overlay; theme is **Dark only** for now (don't expose Light), reduce-motion toggle, keymap JSON.
- **File ops overlays** — Name Input, Delete/Confirm: centered dialogs in palette language.

---

## 9. Hard rules checklist (for any agent)

- [ ] Dark only. Tokens above — no invented colors; derive new ones in `oklch` from the accent if unavoidable.
- [ ] Headings differ by **weight/size, not color**.
- [ ] One accent (purple) + one secondary (teal wikilinks). Used sparingly.
- [ ] Hanken Grotesk + Geist Mono. No Inter/Roboto/Arial.
- [ ] Line icons only, no emoji, no gradient decoration.
- [ ] No menu bar — route global actions through the command spine / palette.
- [ ] Rails are quiet & collapsible; the note sheet is the hero (~750px).
- [ ] Every scroll container has `min-height:0` up its flex chain.
- [ ] Inline styles only (DC authoring); overlays follow the command-palette archetype.
- [ ] Study tracker is a **ledger + project/gate/reading tracker**, not a live timer.
