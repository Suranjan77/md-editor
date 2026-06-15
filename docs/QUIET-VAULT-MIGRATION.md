# Quiet Vault — UI Migration Plan

> Phased plan to move the current `md-editor` UI to the **dark-only, document-first
> "Quiet Vault"** design. The design contract is `docs/DESIGN-SYSTEM.md`; the pixel-exact
> reference is `docs/quiet-vault-reference.html`. This document is the *engineering* plan:
> what to change, in which files, in what order.

## Implementation status

- ✅ **Phase 0** — design docs + vendored OFL fonts landed.
- ✅ **Phase 1** — tokens & palette (purple accent, teal wikilinks, calm syntax).
- ✅ **Phase 2** — Hanken Grotesk + Geist Mono embedded via `gui::fonts`.
- ✅ **Phase 3** — 752px editor sheet / 40px margins.
- ✅ **Phase 4** — command spine replaces the menu bar (`gui::command_spine`).
- ✅ **Phase 5** — quiet rails (`bg_rail`) + tracker is a ledger (timer removed from UI).
- ✅ **Phase 6** — overlay scrim + command-palette archetype for the shared list panel.
- ✅ **Phase 7** — status bar restyle (partial: container/typography; content unchanged).
- 🔶 **Phase 8** — partial: Settings/Close line icons added; remaining cleanup below.

**Remaining polish (follow-ups):** restyle the per-variant overlay cards (Confirm,
NameInput, Settings, PDF dialogs) to `surface_palette`; Welcome screen + toast restyle;
active-line raw-marker reveal tuning; full dead-code removal of the (now UI-unreachable)
menu, timer, and Light-theme plumbing plus their tests; `qvfade`/`qvdim` motion gated on
reduce-motion. The unreachable plumbing is retained for now so existing behavior tests
stay green.

## Goals & guardrails

- Match `docs/quiet-vault-reference.html` exactly — no invented colors, fonts, or chrome.
- Each phase **compiles, passes tests, and is independently reviewable**.
- No behavior is silently dropped: removed chrome (menu bar, timer) must have an equivalent
  path (command palette / ledger).
- Tech stack unchanged: Rust + Iced. All GUI lives in the `shell` crate
  (`shell/src/gui/**`); `kernel`/`editor`/`vault`/`pdf` stay UI-free.

## Current → target gap (verified)

| Area | Current | Target |
|---|---|---|
| Accent | teal `0x4fd1b5` | purple `#bd93f9`; teal `#67c6b0` = wikilinks only |
| Theme | Dark + Light token sets | **Dark only** |
| Syntax | keyword→red, fn→teal, … | keyword purple, fn `#e0b04a`, string `#8fbf7f`, type `#67c6b0`, self/param `#cf8f6a`, comment `#5a5a62`, base `#c4c4cc` |
| Fonts | Iced system defaults | embedded Hanken Grotesk + Geist Mono |
| Top chrome | `File·Edit·View·PDF·Help` menu bar | no menu bar → command spine (⌘K) |
| Editor width | `MAX_READING_WIDTH = 840`, margin `28` | ~`752` / `40` |
| Left rail | file tree, 240w | quiet rail `#0a0a0d`, 258w, row h30 |
| Right rail | tracker, 360w, **Start/Stop Timer** | ledger rail `#0a0a0d`, 308w, **no live timer** |
| Overlays | centered modals | command-palette archetype (scrim+blur, 640px `#141419`, r14) |
| Status bar | 13px muted | h26 `#0c0c10`, Geist Mono 11 |

## Key files (map)

- Tokens: `shell/src/gui/tokens.rs` (`DARK_TOKENS`/`LIGHT_TOKENS`, `for_name`, `Shell::tokens()`)
- Markdown color mapping: `shell/src/gui/editor_canvas/palette.rs`
- Markdown paint roles/fonts: `shell/src/gui/paint.rs`, `shell/src/gui/editor_canvas.rs`
  (`font_role_font` ~`:614`, sheet constants ~`:23–27`, `geometry.rs`)
- Fonts/app builder: `shell/src/gui/welcome.rs` (`iced::application(...)` ~`:61`),
  `BOLD` consts in `chrome.rs:4`, `tracker_view.rs:16`, `tracker_widgets.rs:7`
- Top-level layout: `shell/src/gui/mod.rs` (`view()` ~`:1190–1438`, menu mount `:1392`,
  file tree `:1196–1326`, status bar `:1378`)
- Menu bar: `shell/src/gui/menu.rs`
- Command registry/palette: `kernel/src/defaults.rs`, `shell/src/gui/overlay.rs`
- File tree: `shell/src/gui/file_tree.rs`
- Tracker: `shell/src/gui/tracker_view.rs`, `tracker_widgets.rs`
- Settings: `shell/src/gui/commands_settings.rs`
- Icons: `shell/src/gui/icons.rs`
- Status: `shell/src/gui/status.rs`
- Math asset color: `shell/src/gui/markdown_assets.rs` (~`:178`)

---

## Phase 0 — Foundations (this commit)

- Land `CLAUDE.md`, `docs/DESIGN-SYSTEM.md`, `docs/quiet-vault-reference.html`.
- Vendor OFL fonts under `assets/fonts/` (`HankenGrotesk-VariableFont_wght.ttf`,
  `GeistMono-VariableFont_wght.ttf` + `*-OFL.txt` + `README.md`).
- No source changes.

## Phase 1 — Tokens & palette (recolor)

**Files:** `tokens.rs`, `editor_canvas/palette.rs`, `commands_settings.rs`,
`markdown_assets.rs`.

1. Rewrite `DARK_TOKENS` to the Quiet Vault values (`DESIGN-SYSTEM.md §1`). Add fields the
   design needs that don't exist yet:
   - Surface ladder: `bg_rail #0a0a0d`, `bg_chrome #0c0c10`, `bg_canvas #0e0e12`,
     `bg_inset #0b0b0f`, `surface_1 #101014`, `surface_2 #16161c`, `surface_3 #1a1a21`,
     `surface_code_inline #1c1c23`, `surface_palette #141419`.
   - Borders: `border_faint .05`, `border_hairline .06`, `border_strong .07`,
     `border_overlay .10` (white alpha).
   - Accent: `accent #bd93f9`, `accent_bg_dim`, `accent_border_dim`, `accent_border_hi`,
     `selection rgba(189,147,249,.28)`.
   - New `wikilink #67c6b0` token (distinct from `accent`).
   - Semantic: `danger #e0735f`, `info #7fa8d8`, `success #6c8c7c`, `prio_high #e0b04a`,
     `prio_med #67c6b0`, `prio_low #6b6b73`.
   - Syntax: `syn_base #c4c4cc`, `syn_keyword #bd93f9`, `syn_function #e0b04a`,
     `syn_string #8fbf7f`, `syn_type #67c6b0`, `syn_param #cf8f6a`, `syn_comment #5a5a62`.
2. **Dark only:** delete `LIGHT_TOKENS` + `light()`; `for_name()` always returns dark
   (keep the fn signature so call sites/session restore don't break). Drop the theme picker
   in `commands_settings.rs` (keep the Settings overlay; remove the theme control + the
   `SettingsThemeChanged` wiring or make it a no-op).
3. Update `palette.rs`:
   - `link()` → `accent` (purple); add/route `wikilink()` → new `wikilink` token (today it
     points at `accent_secondary`).
   - `syntax()` → map each `SyntaxRole` to the new `syn_*` fields (not `danger`/`success`).
   - Inline-code text → `#d9c7f5` on `surface_code_inline`.
4. `markdown_assets.rs`: replace the hardcoded math gray (~`:178`) with a token
   (`text_body`/`syn_base`).

**Verify:** `cargo build && cargo test`; launch and eyeball link vs wikilink colors, code
block syntax, inline code chip.

## Phase 2 — Fonts

**Files:** `welcome.rs` (app builder), `editor_canvas.rs` (`font_role_font`),
`chrome.rs`/`tracker_view.rs`/`tracker_widgets.rs` (`BOLD`).

1. Register fonts on `iced::application(...)`:
   `.font(include_bytes!(".../HankenGrotesk-VariableFont_wght.ttf"))`,
   `.font(include_bytes!(".../GeistMono-VariableFont_wght.ttf"))`,
   `.default_font(Font::with_name("Hanken Grotesk"))`.
2. `font_role_font()`: `Sans/SansBold/SansItalic` → `Hanken Grotesk` (weight/style set on
   the `Font`), `Mono` → `Geist Mono`.
3. `BOLD` consts → `Font::with_name("Hanken Grotesk")` + `Weight::Bold`.

**Verify:** confirm rendered glyphs are Hanken (UI/body) and Geist Mono (code, status, hours)
— not the system fallback. Check the exact variable-font family names Iced/cosmic-text
expose (`with_name` must match the font's name table).

## Phase 3 — Editor sheet

**Files:** `editor_canvas.rs` (constants ~`:23–27`), `geometry.rs`, `paint.rs`.

1. `MAX_READING_WIDTH 840 → 752`; `MIN_PAGE_MARGIN 28 → 40`. Centering via
   `content_width`/`content_left` is already correct.
2. Align the type scale to `DESIGN-SYSTEM.md §2` (H1 34/700/-0.8, rendered H2 25/700,
   body 16.5/1.78). Headings differ by **weight/size, not color**.
3. Active-line marker reveal: subtle `accent_bg_dim` line bg, mono markers at
   `rgba(189,147,249,0.55)`, 2px accent caret. (Confirm current active-line treatment and
   adjust to spec.)

**Verify:** sheet is ~752px and centered; long doc scrolls; active line reveals raw markers.

## Phase 4 — Remove menu bar, add command spine

**Files:** `mod.rs` (`view()` top), new `shell/src/gui/command_spine.rs`, `menu.rs`
(remove), command registry.

1. Pre-req: confirm every menu action in `menu.rs` exists as a command in
   `kernel/src/defaults.rs` and is reachable via `registry.palette()`. Add any missing
   palette entries (e.g. `app.quit`, vault.open) so nothing is orphaned.
2. Remove the menu bar mount (`mod.rs:1392`) and delete `menu.rs` + its `MenuId`/MenuOpen
   state, messages, and popover view.
3. Build `command_spine.rs` (h48, `bg_chrome`) per `DESIGN-SYSTEM.md §5 Top bar`:
   `[ brand M · vault name · note count · ▾ ]` (opens vault switcher / recents) · left-rail
   toggle (⌘B) · centered ⌘K command bar (max 560, opens palette) · today-log chip (opens
   tracker Dashboard) · right-rail toggle (⌘⇧T, accent when open) · settings gear (⌘,).
4. Mount the spine as the first row of `view()`’s column.

**Verify:** no menu bar; ⌘K opens palette; every former menu item runnable from palette;
rail toggles + settings work.

## Phase 5 — Rails restyle

**Files:** `mod.rs` (file tree `:1196–1326`), `file_tree.rs`, `tracker_view.rs`,
`tracker_widgets.rs`.

1. **Left rail (files):** bg `bg_rail`, width 258. Header `FILES` section label + new-note
   (+) + collapse. Rows h30 r7; indent `12 + depth·16` (folders) / `28 + depth·16` (files);
   folder caret at `10 + depth·16` rotating 90°. Active = `accent_bg_dim` bg + `text_title`
   + accent icon + 6px accent dirty dot. Hover `rgba(255,255,255,0.04)`. File-type icon
   colors: md/folder `#7a7a82`, pdf `#cf7a68`, image `#7fa8d8`. Footer "Backlinks · N".
2. **Right rail (tracker):** bg `bg_rail`, width 308, tab strip h38 (Dashboard · Log ·
   Projects · Gates · Reading · Config; active = `surface_2` pill). **Remove Start/Stop
   Timer** (`tracker_view.rs:529,539`) and any live-timer state/messages — the tracker is a
   **ledger**. Lead Dashboard with: two `surface_1` stat cards (Today / Streak), 7-bar
   This-Week chart (today solid accent), then Log ledger (dashed "Log hours for today…" add
   row → entries `date·hours·note·×`), Projects (progress `accent/fill`), Gates (16px rings),
   Reading (`surface_1` rows + priority dot), Config (mono JSON editor).

**Verify:** rails match widths/row heights; tracker has no timer; logging an entry works;
collapse/expand via toggles.

## Phase 6 — Overlays → command-palette archetype

**Files:** `overlay.rs` (all overlay views), motion helpers.

1. Shared chrome for every overlay (palette, quick open, vault search, find, PDF find,
   PDF TOC, backlinks, settings, name input, confirm/delete, go-to-page, annotation note,
   orphan report, link preview): scrim `rgba(6,6,9,0.55)` + ~2px blur; panel 640px
   `surface_palette`, `border_overlay`, radius 14, soft shadow. Center-top for
   command/search; centered for dialogs.
2. Rows h42: icon + name + group + keycap; active row `accent_bg_dim`. Footer hints
   (↑↓ navigate · ↵ run · "N commands").
3. Motion: `qvfade` panel-in (0.16s), `qvdim` scrim-in (0.12s); **gate on reduce-motion**
   (existing setting). Caret blink `qvblink` where a live input caret is shown.

**Verify:** all overlays share the look; reduce-motion disables fades.

## Phase 7 — Status bar, Welcome, Toasts

**Files:** `status.rs` + `mod.rs:1378`, `welcome.rs`, `toast.rs`.

1. Status bar h26, `bg_chrome`, Geist Mono 11: left `Ln n, Col n · Markdown`;
   right `N words · saved · UTF-8` (saved in `success`).
2. Welcome: centered, calm — brand **M**, Open Vault / Create Vault, Recent Vaults (last 8).
   Dark tokens (welcome already pins dark).
3. Toasts: bottom-right stack, `surface_2`, hairline border, radius 9–10, icon + message +
   ×, auto-dismiss ~4s; success/`danger` accents.

## Phase 8 — Icons & cleanup

**Files:** `icons.rs`, `chrome_panels.rs`, `tracker_view.rs`, dead code.

1. Add missing line icons (settings gear, chevron, ledger/log) in the existing 16-viewBox
   stroke-currentColor canvas style. Keep file-type icon colors from §4.
2. Replace literal `✕` glyphs (`chrome_panels.rs`, `tracker_view.rs`) with the `Close` icon.
3. Delete dead menu/light-theme code; run a no-emoji sweep (design forbids emoji).

---

## Per-phase checklist (gate every PR)

- [ ] `cargo build` (workspace) clean.
- [ ] `cargo test` (workspace) green.
- [ ] Keymap conflict check passes (CI-enforced).
- [ ] Manual smoke per `docs/SMOKE.md` (+ screenshot for visual phases).
- [ ] Hard-rules checklist from `docs/DESIGN-SYSTEM.md §9` satisfied:
  - [ ] Dark only; tokens only (no invented colors).
  - [ ] Headings by weight/size, not color.
  - [ ] One accent (purple) + teal wikilinks, used sparingly.
  - [ ] Hanken Grotesk + Geist Mono (no Inter/Roboto/Arial).
  - [ ] Line icons only; no emoji; no decorative gradients.
  - [ ] No menu bar; actions via command spine / palette.
  - [ ] Rails quiet & collapsible; ~750px note sheet is the hero.
  - [ ] Scroll containers carry `min-height:0` up the chain.
  - [ ] Overlays follow the command-palette archetype.
  - [ ] Tracker is a ledger, not a live timer.

## Sequencing notes

- Phases 1–3 are low-risk, mostly value swaps + new fields — do first; they make the app
  look ~80% correct with minimal structural change.
- Phase 4 is the only structural change (layout restructure + new module) — land it alone.
- Phases 5–8 are component-by-component restyles that can ship incrementally.
- `docs/DESIGN.md` is superseded by `docs/DESIGN-SYSTEM.md` (kept for history).
