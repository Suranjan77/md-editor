# V3 UX Overhaul Plan — from keyboard tool to GUI application

> Status ledger: `docs/V3_HANDOFF.md` (update it after every unit of work).
> Sibling plan: `docs/V3_IMPLEMENTATION_PLAN.md` (its §0 working rules, §0.4
> pitfalls register, and verification gate apply to **every** step here —
> reread them first; they are not repeated in full).
>
> **Status (updated 2026-06-13):** phases 0–6 complete. Tracker feedback is
> unified on toasts; theme state is owned by each `Shell`, persisted, and
> passed explicitly to views and canvases.

## Why this plan exists

User verdict (2026-06-12): v3 is architecturally sound and feature-complete
through Phase 5, but it "feels like using a terminal based pdf/markdown
reader/editor". Concretely, with no chord knowledge a new user cannot:

- see or navigate the vault (the file panel exists but starts closed, and
  nothing on screen says `ctrl+b`);
- split the view, open the tracker, or open a PDF's TOC;
- discover *any* shortcut from inside the app (`docs/V3_SHORTCUTS.md` is a
  file in the repo, not a surface in the product);
- do anything with the mouse beyond clicking tabs, tree rows, and overlay
  rows that are already on screen.

v2 (`native/`, REFERENCE ONLY — never modify) had all of this: a toolbar of
icon buttons with tooltips, a welcome screen, a file sidebar with
create/delete, toasts, confirm modals, docked TOC/annotations/backlinks
panels. **v2's GUI is the floor, not the ceiling** ("or better" — user).

## North star and acceptance bar

**The no-keyboard test:** every item of `docs/V3_SMOKE.md` except literal
text entry can be completed with the mouse alone. When that's true, this
plan is done. Each phase below adds its own mouse-driven smoke items as it
lands.

## Ground rules (additions on top of V3_IMPLEMENTATION_PLAN §0.2)

1. **Buttons emit commands, never behavior.** Add exactly one new message,
   `Message::RunCommand(CommandId)`, whose handler is one line:
   `self.run_command(cmd)`. Every clickable element routes through it. If a
   click needs behavior no command has, *register a new command first*
   (`v3/kernel/src/defaults.rs`) — the palette, the shortcuts doc, the help
   overlay, and the keymap-override file must always tell the whole truth.
   Widgets still bind **no keys** (BUG-A discipline; pitfall P9).
2. **The discoverability invariant (CI-enforced from Phase 1 on):** every
   registered command id appears in the menu or a context-local control model, or in an explicit
   `MOUSE_EXEMPT` list in the test (e.g. `overlay.confirm`). A windowless
   test walks the pure menu model and asserts coverage. New command ⇒ the
   test forces you to place it somewhere the mouse can reach.
3. **Pure view models.** Windowless suites cannot see paint (pitfall P10).
   Menus, toolbars, panels: compute *what to show* (rows, labels, enabled
   flags, target commands) in pure functions like `file_tree::visible_rows`
   and `paint.rs` already do; the iced layer only renders the model. Tests
   target the model.
4. **Tokens only.** No raw hex/`Color::from_rgb` in views — everything reads
   `gui/tokens.rs`. New semantic roles (hover, focus-ring, toast levels) are
   added to `Tokens` once, not improvised per widget.
5. **Overlay layering (pitfall P1):** anything floating above canvases
   (menus, popovers, toasts, modals) is a later child of the existing
   `stack![]` in `Shell::view`, never painted in the same canvas layer.
6. **State that should survive a restart** (sidebar widths, panel
   visibility, dock choices) goes in `gui/snapshot.rs` with `#[serde(default)]`
   — restore degrades, never refuses.
7. **One phase per session/agent, gate after every numbered step.** Land
   value incrementally; never hold the tree hostage to a half-built menu
   system.

## Phase index (rough order of value ÷ cost)

| Phase | What | Size |
|---|---|---|
| 0 | Discoverability triage: tree open by default, welcome pane, shortcuts/help overlay, status-bar split | S |
| 1 | Menu bar + contextual icon set + `RunCommand` plumbing | M |
| 2 | File sidebar → real file manager (create/rename/delete, context menu, resizable, vault picker) | L |
| 3 | Mouse-driven panes/tabs: close buttons, draggable split ratios, tab overflow | M |
| 4 | PDF reading chrome: pane toolbar, docked TOC, annotations sidebar, color picker, async worker | L |
| 5 | Markdown chrome: formatting toolbar, find/replace bar, outline + backlinks panels | L |
| 6 | Feedback & polish: toasts, confirm modals, hover/focus states, settings UI, light theme | M |

---

## Phase 0 — Discoverability triage ✅ complete 2026-06-12

### 0.1 File tree opens by default

`gui/mod.rs`: `tree_open: true` in `Shell::new` **when no session snapshot
exists** (first run); a saved session keeps the user's choice (snapshot field
already exists: `tree_open`). Test: fresh tempdir vault ⇒ `tree_open()`;
restored session with `tree_open: false` stays closed.

### 0.2 The empty pane becomes a welcome surface

Today: `text("ctrl+p to open a file · ctrl+shift+p for commands")`
(`pane_view`, `gui/mod.rs`). Replace with a column of real buttons (v2
reference: `native/src/views/welcome.rs`):

- "Open File…" → `RunCommand(file.quick-open)`
- "Browse Vault" → `RunCommand(workspace.toggle-files)`
- "Command Palette" → `RunCommand(palette.open)`
- "Keyboard Shortcuts" → `RunCommand(help.shortcuts)` (next step)
- each with its chord rendered as a dim badge next to it (registry lookup —
  `registry.get(id)` exposes bindings; format via the same code
  `--dump-shortcuts` uses).

Pure model: `fn welcome_rows(registry) -> Vec<(label, CommandId, Option<String /*chord*/>)>`,
tested windowlessly; the view just renders it.

### 0.3 `help.shortcuts` — the app explains itself

New command `help.shortcuts` ("Keyboard Shortcuts", category Help, chord
`ctrl+/`; check `docs/V3_SHORTCUTS.md` for conflicts first, P9). Handler
opens a new list overlay variant `Overlay::Help` whose rows come straight
from `registry.specs()`: `title · category` left, chord right; typing
filters (reuse the palette's subsequence matcher); **enter runs the
command** (it's a palette that teaches chords). The scrollable list +
clamp + `snap_selected` machinery from `gui/overlay.rs` is shared — this is
one new `list_rows` arm and one confirm arm. Suite: open → has ≥ every
default command row; filter narrows; enter on a row runs it (assert
`last_command`).

### 0.4 Status bar stops eating messages (makes pitfall P7 structural)

Split the bottom bar into left **message** segment (transient: command
echo, errors, "N chars selected") and right **position** segment (Ln/Col or
`p. N/M · zoom% · §`), two fields in `Shell` instead of one `status`
string. `sync_status` only ever writes the right segment, so handlers no
longer need the `return Task::none()` dance to keep a message visible —
delete those early returns and the P7 register entry's workaround note.
Tests: the routing suite asserts messages survive `sync_status` and the
pill still updates. (Touches many `self.status =` sites — mechanical, do
it as its own commit.)

Phase 0 smoke additions: fresh vault shows the tree and a welcome pane with
working buttons; `ctrl+/` lists every shortcut and enter runs one.

---

## Phase 1 — Menu bar chrome ✅ complete 2026-06-12

v2 reference: `native/src/views/toolbar.rs`, `native/src/views/icons.rs`.

Direction correction (user, 2026-06-12): global icon toolbar removed because
it duplicated the menu bar. Icon controls belong on local surfaces where
their context is clear, starting with the floating PDF control bar.

### 1.1 Plumbing

- `Message::RunCommand(CommandId)` (+ the one-line handler).
- `Message::TabCloseClicked(TabId)` → `ws.focus_tab(tab)` then
  `run_command(workspace.close-tab)` (pull into Phase 3 if time is short).
- New commands this phase introduces (all in `defaults.rs`, then regenerate
  the shortcuts doc):
  - `pdf.zoom-in` (`ctrl+=`, pdf scope), `pdf.zoom-out` (`ctrl+-`, pdf
    scope) — handler: `session.set_zoom(session.zoom * 1.25 / 0.8)` +
    `ensure_tiles` (clamping already lives in `set_zoom`, 0.25–6.0).
  - `help.shortcuts` if Phase 0 didn't land it.

### 1.2 Icon set

Port v2's canvas-drawn icons (`native/src/views/icons.rs` — Folder, File,
Search, Command, Split, ListTree, Clock, Chevrons, Trash, X, …) into
`v3/shell/src/gui/icons.rs`. It is self-contained iced-canvas drawing, no
font/asset dependency; colors become token parameters. This is the one
place straight porting (not redesign) is right.

### 1.3 Global toolbar — superseded and removed

User direction removed this row because it duplicated the menu bar. Keep
`icons.rs` for context-local controls; do not restore a second global
command surface.

Historical design, rejected 2026-06-12 because it duplicated the menu:

One row, height ~38 px, `bg_secondary`, under the menu bar (or standalone
until 1.4 lands):

- always: sidebar toggle (`workspace.toggle-files`), quick-open
  (`file.quick-open`), vault search (`search.global`), palette
  (`palette.open`), split (`workspace.split-right`), tracker
  (`workspace.toggle-tracker`), help (`help.shortcuts`)
- focused-markdown group: save (`editor.save`, shows dirty accent when
  buffer dirty), undo/redo, find (`editor.find`)
- focused-pdf group: zoom out / `NN %` (click → `pdf.zoom-input`) / zoom
  in, back/forward (`pdf.back`/`pdf.forward`), go-to-page
  (`pdf.go-to-page`), find (`pdf.find`), TOC (`pdf.toc`)

Every button: `tooltip(button(icon), "<Title> · <chord>", Bottom)` —
title + chord from `registry.get(id)`; never hardcode either. Pure model
Proposed pure model was
`fn toolbar_model(registry, focused_kind, dirty) -> Vec<ToolGroup>`
(id, icon, enabled, active) — windowless tests assert the pdf group appears
iff a PDF is focused, and that every model entry resolves to a registered
command. Handlers for unfocused-surface clicks already no-op gracefully
(`focused_md_mut()`/`focused_pdf_mut()` return `None`) — the model should
*disable* (dim) them anyway.

### 1.4 Menu bar

iced has no native menu widget. Two options; pick the first unless it
fights back hard (then ADR the second):

1. **In-house anchored popover** (recommended; consistent with the
   overlay pattern, no new dependency): menu bar is a row of flat buttons
   (File · Edit · View · PDF · Help); clicking one sets
   `open_menu: Option<MenuId>` on `Shell`; the open menu renders as a
   later `stack![]` child — full-window transparent `mouse_area` (click
   anywhere = close) with the dropdown column positioned under the title
   via a top-padded container. Items: label left, chord right, dim when
   disabled; click → `RunCommand` + close. Esc closes (route
   `overlay.close` when a menu is open, or treat menu as a lightweight
   overlay scope — decide and pin in the handoff decision log).
2. `iced_aw`'s menu widget (new dependency: record an ADR; we own less
   code but import their bugs and their release cadence).

Menu **model is pure data** in `gui/menu.rs`:

```rust
pub struct MenuItem { pub command: CommandId, pub enabled: bool }
pub fn menu_model(registry, focused_kind, ...) -> Vec<(/*title*/ &str, Vec<MenuItem>)>
```

Suggested contents (labels come from the registry — only grouping lives
here): **File** quick-open · save · close tab · quit; **Edit** undo · redo ·
select-all · find; **View** toggle files · toggle tracker · split right ·
next tab · shortcuts help; **PDF** toc · find · go-to-page · zoom in/out ·
back/forward · highlight · note · export annotations.

### 1.5 The discoverability invariant test (ground rule 2)

`v3/shell/tests/mouse_coverage.rs`: every id in `registry.specs()` is in
`menu_model` ∪ context-local controls ∪ `MOUSE_EXEMPT` (exempt: `overlay.close`,
`overlay.confirm`, and nothing else without a written reason next to it).
This is the test that keeps the GUI honest forever.

Phase 1 smoke: split a view, open the tracker, open a TOC, change zoom, and
read every shortcut — mouse only, fresh vault, no docs.

---

## Phase 2 — File sidebar → file manager ✅ complete 2026-06-12

v2 reference: `native/src/views/sidebar.rs`, `modals.rs`, `welcome.rs`.

### 2.1 Vault file operations as commands

New commands (Workspace scope, palette + menu + context-menu reachable):
`file.new-note`, `file.new-folder`, `file.rename`, `file.delete`. Handlers
prompt via new input overlays (`Overlay::NameInput { purpose, input }` —
reuses the raw-input path), then go through **md3-vault**:

- create: `atomic_save` an empty note + targeted `sync_paths`; open it.
- rename/move: fs rename, then `LinkGraph::rename_file` + `rewrite_links`
  + `atomic_save` per referrer (the vault service exists and is tested —
  `v3/vault/src/links.rs`; this finally wires link repair end to end),
  re-sync index, update open sessions' `rel_path`/`DocumentId` mapping
  (decide: keep `DocumentId` stable across rename — pin in decision log).
- delete: confirm modal first (Phase 6 modal or a minimal inline one),
  remove, re-sync, close affected tabs (kernel `collapse_empty_panes`).
- PDFs: annotations survive by content hash (`annotations_survive_rename`
  already pins it) — say so in the confirm copy.

Tests at the vault layer exist; shell tests drive the overlay flow over a
tempdir (create → appears in tree + index; rename a linked note → referrer
text rewritten; delete → tab closed, index row gone).

### 2.2 Tree affordances

Header row: vault name + icon buttons (new note, new folder, collapse-all,
refresh). Row hover: background `bg_tertiary`; row right-click → context
menu (anchored popover from 1.4): Open, Open in Split, Rename, Delete, New
Note Here, New Folder Here. File-type icons (md/pdf) from `icons.rs`; dirty
dot on open-dirty files (sessions know).

### 2.3 Resizable + persistent sidebar

Drag handle on the tree's right edge (a 6 px `mouse_area` strip; on-drag
message updates `tree_width: f32`, clamp 160–480). Persist `tree_width` in
the snapshot. Same machinery gets reused by Phase 3 split dividers and the
Phase 4 panels — build it as a small reusable `gui/drag.rs` helper.

### 2.4 Vault picker / welcome window

`md3-shell` with no/invalid vault arg currently exits(?) — give it v2's
welcome flow instead: recent-vaults list (stored via
`directories::ProjectDirs` config, same crate the tracker already uses),
"Open Vault…" (`rfd` file dialog — new dependency, record the decision; v2
used native dialogs too), "Create Vault…". Out: changing vaults mid-run
(quit-and-reopen is fine at this stage; note it).

Implementation note: no/invalid vault startup opens an in-app
welcome window instead of opening an OS dialog automatically. It shows Open
Vault, Create Vault, and recent-vault actions; native folder selection opens
only after a click. `vault.open` exposes the same picker from the File menu
and relaunches into the selected vault.

---

## Phase 3 — Panes & tabs you can drive with a mouse ✅ complete 2026-06-12

### 3.1 Tab strip

- Per-tab `×` button (`Message::TabCloseClicked`), middle-click closes too
  (iced `mouse_area::on_middle_press`).
- Tab strip becomes horizontally scrollable on overflow
  (`scrollable::Direction::Horizontal` — pattern already in
  `tracker_view.rs:622`).
- `+`-style "open file" button at the strip's end → `file.quick-open`.

### 3.2 Draggable split ratios

The kernel already has `split_with_ratio` (clamped 0.05–0.95) and ratios
render via `FillPortion`. Add a divider strip (6 px) between split children
in `layout_view`; drag emits `Message::SplitRatioDragged { node_path, ratio }`.
Kernel needs one new API: `PaneTree::set_ratio(path_or_id, f32)` — add it
with unit tests in `v3/kernel` (find the split node; same clamps). Ratios
already persist in snapshots (`split_with_ratio` restore path is live).
Cursor: `ResizingHorizontally/Vertically` on hover.

### 3.3 Pane header affordances

In each pane's top-right corner: split-right, split-down, close-pane icon
buttons (`workspace.split-right`, new `workspace.split-down` — the kernel
supports `SplitAxis::Vertical` already; new `workspace.close-pane` =
close all tabs in pane via existing close machinery). Update the mouse
coverage list.

### 3.4 (Stretch) drag tabs between panes

Kernel `move_tab(tab, target_pane)` + dedup rules (a document already open
in the target pane = focus it, kernel tab-dedup logic exists). GUI
drag-and-drop in iced is manual (track press→move→release with a floating
ghost on the stack). Skip unless the rest of the phase lands early; the
split buttons + "Open in Split" context item cover the need.

Deferred as specified: direct tab drag remains stretch work. Core Phase 3
ships tab close/middle-close, overflow scrolling, quick-open button,
split-right/down, close-pane, and persistent draggable split ratios.

---

## Phase 4 — PDF reading chrome ✅ complete 2026-06-12

Landed as a floating bottom-of-view control bar (user direction correction,
§4.1 note) plus docked TOC/annotations panels, selection context menus, and
the async worker. See the handoff status board.

v2 reference: `native/src/views/pdf_viewer.rs`, `toc.rs`,
`pdf_annotations.rs`, `interactive_pdf.rs`.

### 4.1 Per-pane PDF toolbar

A slim bar above the PDF canvas when the pane shows a PDF (not the global
toolbar — per pane, so split PDFs each get one): page `N / M` (click N →
`pdf.go-to-page`), prev/next page, zoom −/%/+, **fit-width / fit-page**
(new commands `pdf.fit-width`, `pdf.fit-page`; pure math from
`DocLayout` page sizes ÷ viewport — add `DocLayout::zoom_for_fit_width
(viewport_w)` etc. in `v3/pdf/src/scroll.rs` with unit tests), find, TOC,
back/forward.

Direction correction (user, 2026-06-12): controls render as a floating bar
at the bottom of each PDF view, not a top pane toolbar. Initial bar includes
previous/next page, page input, zoom −/%/+, find, and TOC; commands target
the bar's tab before execution so split PDFs remain independent.

### 4.2 TOC as a docked panel (the modal stays for quick-jump)

`pdf.toc-panel` toggles a right-docked panel (like the tracker) listing the
outline: depth-indented, scrollable, **current section highlighted live**
(`PdfSession::current_section` already computes it — extend to return the
entry index), click jumps (`record_jump` first, like the overlay confirm
arm). Width-resizable via `gui/drag.rs`. Persist open/width per snapshot.
The ctrl+t overlay remains (keyboard quick-jump); both read the same
session outline.

### 4.3 Annotations sidebar

`pdf.annotations-panel`: right-docked list of the document's annotations
(quads → page + first words via `range_selection` text already stored;
note preview; color swatch). Click scrolls to the annotation (page-point →
scroll math same as `jump_to_pdf_match`); trash icon deletes (existing
removal path); note icon opens the note overlay; **color cycling** lands
here (`pdf.highlight-color` rotating a small token palette — schema's
`color` column is live, impl plan P5.3).

### 4.4 Selection context menu

Right-click **on an active selection** → popover: Copy
(`pdf.copy-selection`, new command, `iced::clipboard::write`), Highlight,
Highlight + Note. Right-click elsewhere keeps meaning link-preview (that
check runs first today in `pdf_right_click`; selection check goes before
it — pin the precedence in a test).

### 4.5 Async tile + glyph worker (impl plan Phase 5.1, pulled in here)

The chrome makes render hitches *more* visible, and the find/TOC panels
want glyphs eagerly. Do the worker as specced in
`V3_IMPLEMENTATION_PLAN.md` Phase 5.1 (worker thread owning the pdfium
mutex side, results via `Task`/`Subscription`); removes the 200-page find
cap. Respect P4 (one worker = the serialization).

---

## Phase 5 — Markdown editing chrome ✅ complete 2026-06-12

Formatting toolbar (engine commands first, as specced), find/replace bar,
outline panel, and the earlier backlinks overlay all landed. See the handoff
status board.

### 5.1 Formatting toolbar group

Buttons: bold, italic, inline code, heading cycle, bullet list, checkbox,
wikilink. **These are engine commands first** (impl plan Phase 5.4
"editing ergonomics bundle": each is a `Command` through the bus with
undo-coalescing rules + property tests in `v3/editor`). The GUI buttons
land *with* their commands, one or two at a time — do not build dead
buttons ahead of the engine.

### 5.2 Find/replace bar

Replace the `Find` overlay for markdown with a docked bar under the
toolbar (input, match count, prev/next, replace, replace-all). Keyboard
entry stays `ctrl+f` (same command opens the bar). Engine: matches over
the rope (case-insensitive to start), replace = `Command::Replace` batches
through the existing undo machinery. The PDF find overlay is unaffected.

### 5.3 Outline panel for markdown

The parser already classifies heading lines (`LineKind` from
`MarkdownStyler`) — expose `EditorDocument::headings() -> Vec<(level,
text, line)>` (pure, tested in `v3/editor`), render as a docked panel,
click moves the caret + scrolls. Shares the docked-panel scaffolding from
4.2.

### 5.4 Backlinks panel

`note.backlinks` (impl plan P5.2): right-docked list of referrers from
`LinkGraph`; click opens. Pulls the link graph into the shell's runtime
(today it's vault-tested only): build it on index sync, refresh on watcher
batches.

---

## Phase 6 — Feedback & polish ✅ complete 2026-06-13

Toasts, confirm modals, light/dark tokens, and keymap settings UI are
implemented. Tracker manual-log feedback uses the toast channel. Theme choice
is instance-local `Shell` state, persists through session restore, and no
global mutable theme state remains.

### 6.1 Toasts (v2 `toast.rs`)

Top-right stacked cards (info/success/error tokens), auto-dismiss ~4 s
(`Task::perform(sleep)` → `DismissToast(id)`), close button. Route: save
confirmations, export paths, errors that today vanish into the status
line. Status bar keeps the *position* pill (Phase 0.4); toasts take over
event messages.

### 6.2 Confirm modals

`Overlay::Confirm { message, on_confirm: CommandId }` (kernel fence
machinery already treats any overlay as modal): unsaved-changes on
close-tab/quit (buffer `is_dirty` + session save exists — today quit saves
the *session* but a dirty buffer's content loss is silent), file delete
(Phase 2 uses it). Enter/click Confirm, esc/click-away cancels.

### 6.3 Visual pass

- Hover/pressed states for every interactive row/button (tokens:
  `hover`, `pressed`, `focus_ring` — add to `Tokens` once).
- Scrollbar styling on all scrollables (thin, `border` color, hover
  accent).
- Consistent paddings/radii (pull the magic numbers scattered in
  `overlay.rs`/`mod.rs` into `tokens::metrics()`).
- Focused-pane accent border already exists; dim unfocused panes' tab
  strips one step.

### 6.4 Settings UI (impl plan P5.6)

A settings surface (overlay or tab): theme choice (dark/light), keymap
override list rendered from `<vault>/.md3/keymap.json` with add/remove
(writes the same file `settings.rs` already parses; conflicts validated
via the kernel's checker before save).

### 6.5 Light theme

Second `Tokens` value + `theme` choice persisted; everything already reads
tokens (rule 4), so this is data + a settings toggle, not a refactor.

---

## Appendix A — anchored popover recipe (menus, context menus)

One pattern serves 1.4, 2.2, 4.4: a full-window transparent layer that
closes on any click, with a positioned card on top.

```text
stack![
    base_ui,                                   // everything else
    mouse_area(Space::new(Fill, Fill))         // click-away closes
        .on_press(Message::PopoverDismissed),
    container(menu_card)                       // the dropdown
        .padding(Padding { top: anchor_y, left: anchor_x, .. })
]
```

Anchor coordinates: for the menu bar, accumulate x from the model (button
widths are fixed-ish — or capture via `mouse_area` press position); for
context menus, use the cursor position the triggering message already
carries (`pos` on the PDF mouse messages; add it to tree-row presses).
Remember P1: the popover is its own stack child, never a canvas fill. Keep
the open-popover state on `Shell` (`Option<PopoverState>`), and route esc
through the same close path as overlays so the kernel fence stays truthful
— decide whether a popover registers as a kernel overlay (recommended:
yes, `ws.open_overlay("menu")`) and pin it in the decision log.

## Appendix B — v2 reference map

| v2 file (`native/src/views/`) | v3 target | Phase |
|---|---|---|
| `welcome.rs` | welcome pane buttons / vault picker | 0.2 / 2.4 |
| `toolbar.rs` | global port removed; local controls use `icons.rs` | 1.3 / 4.1 |
| `icons.rs` | `gui/icons.rs` (straight port) | 1.2 |
| `sidebar.rs` | file tree header/context/resize | 2 |
| `modals.rs` | `Overlay::Confirm` | 6.2 |
| `toc.rs` | docked TOC panel | 4.2 |
| `pdf_annotations.rs` | annotations sidebar | 4.3 |
| `backlinks.rs` | backlinks panel | 5.4 |
| `toast.rs` | toast stack | 6.1 |
| `status_bar.rs` | two-segment status bar | 0.4 |

## Appendix C — new commands ledger (add to `defaults.rs`, regenerate doc)

| id | title | chord | scope | phase |
|---|---|---|---|---|
| `help.shortcuts` | Keyboard Shortcuts | `ctrl+/` | Workspace | 0.3 |
| `pdf.zoom-in` / `pdf.zoom-out` | Zoom In / Out | `ctrl+=` / `ctrl+-` | PDF | 1.1 |
| `file.new-note` / `file.new-folder` | New Note / Folder | — (palette/menu) | Workspace | 2.1 |
| `file.rename` / `file.delete` | Rename / Delete | — | Workspace | 2.1 |
| `workspace.split-down` | Split Down | — | Workspace | 3.3 |
| `workspace.close-pane` | Close Pane | — | Workspace | 3.3 |
| `pdf.fit-width` / `pdf.fit-page` | Fit Width / Fit Page | — | PDF | 4.1 |
| `pdf.toc-panel` | TOC Panel | — | PDF | 4.2 |
| `pdf.annotations-panel` | Annotations Panel | — | PDF | 4.3 |
| ~~`pdf.copy-selection`~~ | already landed (ctrl+c, pdf scope, 2026-06-12) | | | 4.4 |
| ~~`pdf.highlight-color`~~ | already landed (palette-only, 2026-06-12) | | | 4.3 |
| ~~`note.backlinks`~~ | already landed (ctrl+shift+b, md scope, 2026-06-12) — 5.4 only needs the *docked panel* form | | | 5.4 |

(Verify every chord against `docs/V3_SHORTCUTS.md` at the time of adding —
P9; the table above was checked against the doc as of 2026-06-12.)
