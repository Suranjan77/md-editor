# Architecture

MD Editor is a native desktop application built as a small Cargo workspace of
focused crates. The guiding rule is a hard separation between **toolkit-agnostic
engines** (pure logic, no UI dependency) and a single **shell** crate that wires
them to the [iced](https://iced.rs) GUI toolkit. This boundary is recorded in
[ADR-0100](adr/0100-v3-toolkit-iced-default.md) and enforced in CI by
[`scripts/architecture-check.sh`](../scripts/architecture-check.sh).

## Why this shape

Coupling UI state to application logic is the failure mode that earlier
iterations of this project kept hitting: keyboard shortcuts captured by the
wrong surface, documents that could only render in one layout, undo history
silently lost. The engines are therefore designed so those bugs are
*unrepresentable*, and they are testable headlessly — without a window, a GPU,
or a compositor. The shell is the only place a toolkit type may appear.

## Workspace layout

```
                       +------------------+
                       |     md-shell     |   iced GUI, the only binary.
                       |  (binary: md-editor)  Composes everything below.
                       +--------+---------+
                                |
        +------------+----------+-----------+------------+
        |            |          |           |            |
   +----v----+  +----v----+ +---v-----+ +---v----+
   |md-kernel|  |md-editor| | md-vault| | md-pdf |
   +---------+  +---------+ +---------+ +--------+
    workspace    text        files,      tiles,
    model:       buffer +    index,       render
    panes,       layout +    search,      queue,
    focus,       undo +      annotations, pdfium
    commands,    parse        tracker,    (feature)
    keymap                    links
```

- The **engines never depend on each other** in production code. Composition is
  the shell's job. (One exception: `md-pdf` has a *dev-only* dependency on
  `md-vault` for an integration test that proves the PDF-text → search-index
  seam; it lives under `[dev-dependencies]` and never ships.)
- Only `md-shell` depends on `iced`. The engine crates do not name `iced` or
  `winit` anywhere — code or manifest.

## The crates

### `md-kernel` — the workspace model (UI-free)

The kernel owns *what the workspace is* without knowing how it is drawn. Four
pillars, each removing a class of bug by construction:

| Module | Responsibility |
|---|---|
| [`command`](../kernel/src/command.rs) | Every action is a registered `CommandSpec`. The keymap, command palette, menus, and the shortcuts doc are all *generated* from this registry — never hand-maintained, so they cannot drift. |
| [`input`](../kernel/src/input.rs) | One declarative `Keymap` with scope-stack resolution derived from focus, plus static conflict detection at startup. A binding that two scopes would both claim is a hard error before the window opens. |
| [`pane`](../kernel/src/pane.rs) | The `PaneTree`: documents open as tabs in panes; a split is a layout choice, never a precondition. Any document type can open in any pane. |
| [`focus`](../kernel/src/focus.rs) | Exactly one focused editor; all input flows through it first. |

`workspace.rs` ties these into the `Workspace` aggregate the shell drives.

### `md-editor` — the text engine

A toolkit-agnostic Markdown editing engine:

- [`buffer`](../editor/src/buffer.rs) — the rope-backed text buffer; all
  mutations go through edit operations so history stays coherent.
- [`undo`](../editor/src/undo.rs) — an undo **tree**, not a linear stack: editing
  after an undo branches history instead of discarding the redo path.
- [`parse`](../editor/src/parse.rs) / [`style`](../editor/src/style.rs) /
  [`syntax`](../editor/src/syntax.rs) — incremental Markdown parsing and styling
  ([ADR-0101](adr/0101-v3-incremental-parser.md)).
- [`height_tree`](../editor/src/height_tree.rs) / [`layout`](../editor/src/layout.rs)
  — a height sum-tree and the 3-phase layout protocol that keep document
  measurement logarithmic, so editing a 5,000-line file stays responsive.

The engine emits geometry and draw intent; it never calls a renderer.

### `md-vault` — local-first persistence and indexing

The vault is any folder the user opens. This crate owns everything stored beside
their notes:

- [`atomic`](../vault/src/atomic.rs) — crash-safe saves (write-temp-then-rename).
- [`index`](../vault/src/index.rs) / `search` — a SQLite FTS index over Markdown
  and extracted PDF text.
- [`annotations`](../vault/src/annotations.rs) — PDF highlights and notes stored
  in a sidecar database keyed by content hash, so they survive file
  rename/move ([ADR-0003](adr/0003-sqlite-sidecar-annotations.md)).
- [`links`](../vault/src/links.rs) — wikilink graph: backlinks, outlinks,
  broken-link queries, and rename repair.
- [`tracker`](../vault/src/tracker.rs), [`session`](../vault/src/session.rs),
  [`watcher`](../vault/src/watcher.rs) — study-tracker storage, session
  snapshots, and filesystem change notification.

Per-vault state lives under `<vault>/.md-editor/` (search index, annotations,
session, keymap overrides). It is plain SQLite/JSON the app can rebuild — never
mixed into the user's content.

### `md-pdf` — the PDF engine

- [`tile`](../pdf/src/tile.rs) / [`render`](../pdf/src/render.rs) — tile
  addressing and a byte-budget LRU cache feeding a cancellable render queue, so
  scrolling a 500-page document only renders what is visible plus a small
  preload.
- [`outline`](../pdf/src/outline.rs), [`select`](../pdf/src/select.rs),
  [`scroll`](../pdf/src/scroll.rs) — bookmarks, text selection geometry, and
  scroll math.

Actual page rasterization uses PDFium ([ADR-0002](adr/0002-pdfium-rendering.md))
behind the optional `pdfium` feature, so the pure tile logic builds and tests on
machines without the native library.

### `md-shell` — the application

The only crate that knows about iced. It:

- composes the kernel and engines and runs the Elm-style update loop
  ([ADR-0004](adr/0004-elm-architecture-with-features.md));
- translates toolkit events into kernel `Chord`s and `CommandId`s;
- owns all file-format parsing (session snapshots, keymap-override files), so
  the kernel and engines stay serde-free;
- renders the editor and PDF canvases, chrome, overlays, and the study tracker;
- generates the keymap, palette, and `docs/SHORTCUTS.md` from the command
  registry.

`shell/src/gui/` holds the iced view layer; `headless.rs` exposes the CLI modes
CI relies on (`--demo`, `--dump-shortcuts`, `--palette`).

## How an input event flows

```
key press
   │  (iced KeyEvent)
   ▼
shell/gui/keys.rs ──► kernel Chord
   │
   ▼
kernel input router ── resolves against the focused editor's scope stack
   │
   ▼
CommandId ──► CommandBus ──► shell command handler
   │
   ▼
engine call (buffer edit / pane split / vault save / pdf scroll)
   │
   ▼
shell rebuilds the affected view ──► iced repaints
```

Because resolution is driven by *focus and scope* rather than which surface
happens to be visible, the same physical key does the right thing in each
context (e.g. `Ctrl+Z` undoes in a Markdown pane but opens the page/zoom input
in a focused PDF pane).

## Enforced boundaries

[`scripts/architecture-check.sh`](../scripts/architecture-check.sh) fails CI if:

1. any engine crate (`kernel`, `editor`, `vault`, `pdf`) names `iced`/`winit` in
   code or its manifest;
2. an engine's production code imports a sibling engine crate.

Module size is ratcheted by
[`scripts/size-budget.sh`](../scripts/size-budget.sh) against
[`budgets.toml`](../budgets.toml): ceilings may only go down. See
[ARCHITECTURE_RULES.md](ARCHITECTURE_RULES.md) for the full rule list and
[CODING_STANDARDS.md](CODING_STANDARDS.md) for the conventions.

## Decision records

Architectural decisions are logged as ADRs in [`docs/adr/`](adr/). Start with
[ADR-0100](adr/0100-v3-toolkit-iced-default.md) (iced + the engine/shell split),
then [ADR-0101](adr/0101-v3-incremental-parser.md) (incremental parser),
[ADR-0002](adr/0002-pdfium-rendering.md) (PDFium), and
[ADR-0003](adr/0003-sqlite-sidecar-annotations.md) (sidecar annotations).
