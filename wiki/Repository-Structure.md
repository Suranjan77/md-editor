# Repository Structure

A Cargo workspace (Rust edition 2024) with two crates: **`md-editor-core`**, the headless
engine, and **`md-editor-native`**, the Iced desktop app. Core never depends on native.

> **Finding your way.** Every source file opens with a one-line `//!` summary, so
> `grep -rm1 '^//!' core/src native/src` prints a live map of the code. Tests for a module
> `foo.rs` live in `foo/tests.rs`.

```
md-editor/
├── core/                 md-editor-core: vault, index, search, SQLite, PDF reader
├── native/               md-editor-native: the Iced app (shell, editor widget, viewers, graph, ink)
│   └── shaders/          WGSL for the knowledge-graph GPU layer
├── wiki/                 This documentation (authoritative)
├── docs/                 Design-system spec (design-system/) and open-items.md
├── tests-fixtures/       Sample PDFs used by tests; showcase/ holds the README demo paper
├── images/               README screenshots and GIF (scripts/readme-media.sh)
├── .github/workflows/    CI: quality gate (ci.yml) and Windows/Linux release builds
├── packaging/arch/       PKGBUILDs (AUR md-editor, md-editor-git) and the desktop entry
├── vendor/winit/         Vendored winit 0.30.13 plus Wayland tablet input, patched in via [patch.crates-io]
├── scripts/              clippy-ratchet.sh (clippy gate), build-arch-package.sh (package from the tree), readme-media.sh (README images)
├── clippy-baseline.txt   Allowed clippy warning count; only goes down
├── clippy.toml, deny.toml  Lint config for tests; licence/ban policy for cargo-deny
├── CLAUDE.md             Orientation for AI coding agents (also useful to humans)
└── md-editor.png/.ico    App icons, embedded at build time
```

`target/` holds build output and the cached PDFium library; it is git-ignored.

---

## `core/` — md-editor-core

| Path | Responsibility |
| :--- | :--- |
| `build.rs`, `build_pdfium.rs` | Download (pinned release, SHA-256 verified), cache and copy the platform PDFium library. |
| `src/lib.rs` | Module declarations; re-exports `AppState`. |
| `src/state.rs` | `AppState`: the SQLite connection, schema DDL, WAL pragmas, `user_version` migrations, DB path resolution, vault generation and publication locks. |
| `src/state/pdf_sidecar.rs` | Portable PDF highlights: `<vault>/.md-editor/annotations/<id>.jsonl`, written on every change and merged three-way with SQLite on vault and PDF open. |
| `src/state/tracker_sidecar.rs` | Portable Study Tracker sessions: `<vault>/.md-editor/study-sessions.jsonl`, written on save/delete and three-way merged with SQLite on vault open/index. |
| `src/state/pdf_store.rs` | PDF sidecar persistence on `AppState`: document scoping (vault-scoped identity), annotations, references, text cache. |
| `src/config.rs` | `get_sys_config` / `set_sys_config` over `settings`, and `read_startup_value` for use before `AppState` exists. |
| `src/citations.rs` | BibTeX `.bib` parser paired with vault `.pdf`s (plus standalone PDF heuristics) for `@` inline citations. |
| `src/daily.rs` | Daily Desk note synthesis (`daily/YYYY-MM-DD.md`): unchecked task rollover from prior daily notes and Spaced Synthesis Brief from `[!question]` / `[!claim]` callouts, plus the last study day's sessions. |
| `src/math_eval.rs` | Pure-Rust recursive-descent mathematical expression evaluator (supporting LaTeX `\frac`, `\sqrt`, trig/log/constants). |
| `src/vault.rs` | The vault API: indexing for a generation, open/save/create/delete, `read_vault_image`. |
| `src/vault/backlinks.rs` | `search_vault`, incoming and mixed (note + PDF annotation) backlinks. |
| `src/vault/indexing.rs` | `index_vault_for_generation`: one walk, then only changed notes and PDFs re-read and re-indexed (`vault_file_meta`). |
| `src/vault/rename.rs` | Renaming files and folders while rewriting links, search rows and PDF sidecars. |
| `src/vault/relink.rs` | The link fixes a rename needs: wikilinks and Markdown links/images to the moved entry, and relative links out of moved notes. |
| `src/vault/files.rs` | Path jailing (`resolve_vault_path_checked`), the atomic `write_file` / `write_bytes`, file-type checks, tree listing. Tests in `files/tests.rs`. |
| `src/vault/graph.rs` | `get_graph_snapshot`: knowledge-graph nodes and edges. See [Knowledge Graph](Knowledge-Graph.md). |
| `src/file_index.rs` | Wikilink parsing (`[[target\|alias]]`), resolution (exact path, then shortest matching basename), outgoing and incoming maps. |
| `src/file_index/code_spans.rs` | Byte ranges of fenced code and inline code spans, where `[[...]]` is not a link. |
| `src/pdf.rs` | PDF data types, `compute_provisional_id`, render-scale buckets, `bind_pdfium`. |
| `src/pdf/renderer.rs` | `PdfRenderer`, the handle the app talks to. |
| `src/pdf/geometry.rs` | Page space → view space: the transform PDFium renders with, so text, search and link rects line up with the bitmap whatever the page's rotation or crop box. |
| `src/pdf/worker.rs` | The PDFium worker thread: 3-document LRU cache, priority and standard queues, one handler per command (render, text, links, search, previews). |
| `src/pdf/toc.rs` | Page-text extraction and outline recovery: embedded bookmarks, printed contents pages, dot leaders. Tests in `toc/tests.rs`. |
| `src/pdf/toc_synth.rs` | The typographic fallback: synthesizing an outline from heading-like lines. |
| `src/references.rs` | Pure resolver from page text to equation, figure, table and section references. Never touches PDFium. |
| `src/tracker.rs` | `StudySession` and tracker key/value persistence. |
| `src/attachments.rs` | Images pasted into notes: attachments folder, naming, SHA-256 dedupe, import, relative Markdown links. |
| `src/types.rs` | Plain data for the UI: `FileEntry`, `SearchResult(s)`, backlinks, `GraphSnapshot`. |
| `src/{pdf,state,vault}/error.rs` | The typed errors of each layer: `PdfError`, `StateError`, `VaultError`. |
| `src/massive_tests.rs` | Combinatorial and stress tests. |
| `examples/` | `dump_refs`, `bench_refs`: reference-resolver diagnostics on a real PDF. |

## `native/` — md-editor-native

### Shell (`src/app/`, `main.rs`)

| Path | Responsibility |
| :--- | :--- |
| `main.rs` | CLI (a path to open), logging init, window size restore, the iced daemon. |
| `build.rs` | Embeds the Windows icon and version resource. |
| `logging.rs` | File logger behind `log` (`md-editor.log`, rotated 1 MB × 3, per-target levels from `MD_EDITOR_LOG`, long lines truncated, repeats collapsed) and the panic hook. |
| `app/mod.rs` | `MdEditor` state, `boot`, `title`, `update`, and `update_inner`, which routes each message family to a handler below. |
| `app/{pdf,graph,ink,search,editor,vault,shortcuts,shell}.rs` | One `update_<family>` handler each; see [Native Desktop GUI](Native-Desktop-GUI.md#2-messages-and-where-they-are-handled-messagesrs-app). |
| `app/graph_pinboards.rs` | Named per-vault graph Pinboards: modal state, saving, restoring, and deleting layouts. |
| `app/pdf.rs` | `update_pdf`: a router only; each `PdfMessage` goes to a handler in the three files below or to `PdfPane::update`. |
| `app/pdf_render.rs` | PDF load results, zoom and fit-to-width, rendered/failed pages, scrolling, page links, TOC. |
| `app/pdf_nav.rs` | PDF link clicks and previews, PDF search results, document id → highlights and references, jumping to an annotation. |
| `app/pdf_select.rs` | PDF text selection and copy, highlights, quick notes, linked notes, orphan report. |
| `app/pdf_ink.rs` | Non-destructive PDF margin ink handlers: pen/eraser toggle, color selection, commit, erase, and undo. |
| `app/pdf_pages.rs` | Opening PDFs, page render scheduling and eviction, available width, `navigate_pdf_page`. |
| `app/paste.rs` | Ctrl+V in a note, "Insert image…", and files dropped on the window: images become attachments, vault files become links, drops on the sidebar are imported (never over an existing file). |
| `app/tabs_handler.rs` | `update_tabs`: activating, closing, reopening and reordering document tabs, per-pane, per-vault tab persistence and restoring the panes. |
| `app/workspace.rs` | The two-pane workspace: `PaneDocs`, swapping a pane's state in, routing `FromPane` / `ToPane` messages, focus, `for_each_pane`. |
| `app/panes.rs` | Pane commands: split, unsplit, focus the other pane, move a tab across, open to the side, one file per pane. |
| `app/pane_view.rs` | The panes area: each pane's tab strip and sheet from its own state, the divider, the empty pane. |
| `app/desks.rs` | `update_desks`: saving, switching (flushing dirty buffers in all panes first), and deleting per-vault saved desks. |
| `app/settings_handler.rs` | `update_settings`, plus restoring and persisting theme, scale, motion, editor typography, attachments and recent vaults. |
| `app/synthesis.rs` | Opt-in inline math evaluation (`EvaluateMath`) and fenced code block execution (`RunCodeBlock` / `CodeBlockFinished`). |
| `app/vault_entries.rs` | Creating, renaming and deleting vault entries; name-modal validation. |
| `app/vault_links.rs` | Sidebar clicks, internal / external / `pdf://` link routing, sidebar keyboard navigation. |
| `app/documents.rs` | Opening, saving, parking documents (notes, PDFs, images); session and layout persistence. |
| `app/subscription.rs` | Keyboard, timers, window events, pen input, the vault watcher. |
| `app/view.rs` | `view(window)`: the shell window (activity rail, sidebar, tab strip, document sheet, side panels, status bar) and the ink window. |
| `app/overlays.rs` | Layers above the shell: drop hint, tab overflow, context menus (one builder per target), vault search, palette, dialogs, PDF link preview, toasts; `anchor_context_menu`. |
| `app/links.rs` | Wikilink, relative-link and heading-anchor resolution. |
| `app/completion.rs` | The `[[` / `/` completion popup: opened and narrowed after each editor command, accepted as one undo step. |
| `messages.rs` | `Message` and its families (`VaultMessage`, `TabsMessage`, `SettingsMessage`, …), `Shortcut`, `TrackerTab`. |
| `shortcuts.rs` | The shortcut registry: one table feeding key handling, menus, tooltips, the palette and the Settings shortcut list. |
| `tabs.rs`, `tabs/tests.rs` | `Tabs`: strip order, active tab, MRU, reopen stack, duplicate-name labels, per-tab view state, session JSON. |
| `desks.rs`, `desks/tests.rs` | `Desk` and `DeskStore`: per-vault saved workspace layouts (`desks:<vault_root>`), JSON persistence, rename and delete path propagation. |
| `platform.rs` | Per-OS shims: opening external links, reading the clipboard for paste (arboard; test double in tests). |
| `size_lint.rs` | Test-only code-health guardrail: modules over ~800 non-test lines and functions over 150 lines fail, with a ratchet list for offenders (empty since W6). |
| `test_support.rs` | Test-only: pins the data dir to a temp folder for every test that builds the app; registers the bundled fonts for headless renders. |
| `e2e/` | Headless end-to-end tests: `mod.rs` (harness, invariants), `scenarios.rs` (data safety), `flows.rs` (UI flows), `panes.rs` (two-pane workspace), `desks.rs` (saved desks, Daily Desk, study-tracker sidecar sync), `synthesis.rs` (Wave 4/5 E2E: `@` citations, `#heading` completion, `EvaluateMath`, `RunCodeBlock`, PDF margin ink), `workspace.rs` (tabs, Settings applied live, excluded folders, context menus, focus return, image picker and drops), `monkey.rs` (seeded random sessions), `showcase.rs` (README media, `#[ignore]`d), `telemetry.rs`. |

### State (`src/*_state.rs`, `*_pane.rs`)

| Path | Responsibility |
| :--- | :--- |
| `editor_state.rs` | `EditorPane`: buffer, highlighted lines, image and math caches, autosave timing, caret reveal, retained buffers. |
| `vault_state.rs` | Vault root, entries, sidebar selection and expansion, backlinks. |
| `ui_state.rs` | Modals, palette, toast queue, context menus, the split divider, panel widths, focus region, settings page, window size and scale. |
| `search_state.rs` | Query, flags, results, memoized in-document matches. |
| `tracker_state.rs` | Timer, sessions, tracker values, active tab. |
| `study_context.rs` | What a running study session worked on (time per document, pages, words, highlights), summarized into its notes. |
| `pdf_pane.rs` | PDF viewport, page caches, annotations, selection, render generation. |
| `graph_state.rs`, `graph_state/` | `GraphState`, with the force simulation in `physics`, initial placement in `layout`, filtering plus read-only queries (stats, backlinks, related, hubs, epistemic edge lookup, node cycling) in `queries`, and named per-vault layouts in `pinboards`. |
| `ink/` | `InkState` and the handwriting engine (`raster.rs` draws a page as a picture for a note, `svg.rs` writes/reads dual-mode `.ink.svg`); see [Handwritten Notes](Handwritten-Notes.md). |

### The Markdown editor (`src/editor/`)

| Path | Responsibility |
| :--- | :--- |
| `buffer/` | `DocBuffer` over `ropey::Rope`. `mod.rs`: state, command router, revision-based dirty tracking, line-ending preservation; `edit` (insert/delete/replace), `format` (inline wrappers, auto-pairing), `lists` (continuation, checkboxes), `movement` (grapheme-aware caret and selection), `undo` (transactions with inverse ops, run coalescing), `words` (word motion and deletion, word/line selection, block indent and outdent). |
| `highlight.rs`, `highlight/` | `Highlighted`: incremental Markdown → `StyledLine`s (block state machine here, `inline` spans, `code` via syntect). `reference.rs` is the old highlighter kept as a test oracle; `callout` maps `> [!type]` to a colour family. |
| `layout_tree.rs` | `HeightTree`, a Fenwick tree over line heights. |
| `layout_cache.rs` | `LineHeightCache` keys so lines are remeasured only when needed. |
| `renderer/mod.rs` | The `Editor` widget, `State`, `LayoutKey`, `MathRender`; `Widget` methods delegate to the modules below. |
| `renderer/metrics.rs` | The vertical rhythm and every constant shared by layout, painting and hit testing. |
| `renderer/measure.rs` | Shaping, font metrics, memoized word offsets and widths, `span_font()`. |
| `renderer/spans.rs` | Which spans reveal their Markdown source for a given caret column. |
| `renderer/flow.rs` | `Flow`, inline layout of one line: UAX #14 breaking, rows, baselines, column ⇄ x. The only wrapper. |
| `renderer/layout.rs` | Line body heights, collapsing margins, filling the height tree. |
| `renderer/caret.rs` | Caret boxes, hit testing with affinity, visual up/down. |
| `renderer/glide.rs`, `reveal.rs` | Caret glide and blink; answering the app's reveal request. |
| `renderer/selection.rs`, `scroll.rs` | Selection shape and text; per-block horizontal scroll and scrollbars. |
| `renderer/events.rs` | Mouse and keyboard; every edit leaves as an `EditorCommand`. |
| `renderer/draw/` | Painting: frame setup and culling, blocks, inline content, overlays, captions, line numbers (`gutter.rs`), primitives. |
| `renderer/testing.rs`, `tests.rs`, `properties.rs` | A recording renderer and `View` driver, behavioural tests, seeded property suites. |
| `render_preview.rs`, `render_snapshot_tests.rs`, `test_docs.rs` | Opt-in PNG previews and draw-call transcripts; the seeded test-document generator. |

### Views and viewers (`src/views/`)

| Path | Responsibility |
| :--- | :--- |
| `views/sidebar.rs`, `rail.rs`, `breadcrumbs.rs`, `welcome.rs`, `toast.rs`, `settings.rs` | Shell chrome (file tree, activity rail, status-bar breadcrumbs) and the Settings page (`Ctrl+,`). |
| `views/viewer_toolbar.rs` | The shared viewer toolbar for PDFs and images: outline, page field, zoom field, fit, find. |
| `views/modals.rs`, `link_note_picker.rs`, `command_palette.rs` | Dialogs and the `Ctrl+P` palette (registry and fuzzy ranking via `fuzzy.rs`). |
| `views/search.rs`, `toc.rs`, `backlinks.rs` | Side panels. |
| `views/pdf_viewer.rs`, `views/pdf_ink.rs`, `interactive_pdf.rs` | The PDF pane, per-page margin ink toolbar/canvas overlay, and the per-page widget. See [PDF Viewer Internals](PDF-Viewer-Internals.md). |
| `views/graph.rs`, `graph/`, `gpu_graph.rs` (+`gpu_graph/pipeline.rs`) | The knowledge graph: toolbar, inspector, label overlay, and the GPU layer (`gpu_graph.rs` handles pointer interaction; `pipeline.rs` holds the wgpu instance layouts and render pipeline). |
| `views/ink.rs` | The handwriting window's toolbar and palette. |
| `views/tracker.rs`, `tracker/` | The study tracker: dashboard in `tracker.rs`, the other tabs in `tracker/sections.rs`, the JSON config model in `tracker/config.rs`. |
| `design/` | The design system ([MASTER](../docs/design-system/md-editor/MASTER.md)): `palette` (Dark/Light token tables), `type_scale`, `metrics` (spacing, sizes, panel widths), `motion` (panel animations, springs, reduced motion), `fonts` (bundled Inter, JetBrains Mono, Source Serif); `mod.rs` holds the runtime theme, scale and typography settings. Guardrail tests in `design/tests.rs`; `design/setting.rs` makes those settings per-thread under `cargo test`. |
| `ui/` | Component library built only from `design` tokens: buttons, fields, menus, dialogs, toasts, panels, tab strip, status bar, tree and list rows, icons (Phosphor SVGs in `assets/icons/`). `ui/gallery.rs` renders every component in both themes at several scales; `ui/context_anchor.rs` stamps the pointer position onto context-menu messages. |
| `pdf_notes.rs`, `search.rs` | Linked-note helpers for PDF highlights; in-note line matching, regex replacement expansion. |
| `autocomplete.rs`, `views/completion.rs` | Completion triggers and suggestions (vault files and `#heading` anchors for `[[`, `.bib`/PDF citations for `@`, block templates for `/`), and the popup drawn under the caret. |
| `code_runner.rs` | Fenced code block runner (`sh`, `bash`, `python`), run only on request and not sandboxed: the block runs in its own process group, which a 5s timeout kills; 64 KB output cap; idempotent `> [!output]` callout insertion. |
| `raster.rs` | Decoding image files for the image viewer and images in notes: raster formats by content, SVG through `resvg` at 2× density. |

---

## Crate Dependencies

| Crate | Runtime | Build |
| :--- | :--- | :--- |
| core | `rusqlite` (bundled SQLite), `pdfium-render`, `image`, `regex`, `serde`/`serde_json`, `sha2`, `uuid`, `log`, `thiserror` | `ureq`, `flate2`, `tar`, `sha2` (PDFium download) |
| native | `md-editor-core`, `iced` 0.14 (canvas, image, advanced, tokio, svg), `iced_program`, `ropey`, `syntect`, `ratex-*`, `unicode-linebreak`, `unicode-segmentation`, `rfd`, `arboard` (clipboard images and file lists), `resvg`, `notify`, `tokio`, `chrono`, `image`, `regex`, `serde`, `uuid`, `bytemuck`, `log`; Linux: `libc` (killing a code block's process group), `winit` (the vendored copy, for Wayland tablet input); Windows: `windows-sys`. Tests: `iced_test`, `iced_debug`, `iced_beacon` | Windows: `winresource` |

### Dependency Rules

1. **No cycles.** `md-editor-core` never depends on `md-editor-native`.
2. **No UI in core.** Core takes no GUI or text-shaping dependency (`iced`, `winit`,
   `ratex-render`).
3. **FFI containment.** `pdfium-render` is used only under `core/src/pdf/`; exactly one
   `Pdfium` binding exists per process. Win32 calls stay in `native/src/platform.rs` and
   `native/src/ink/pen_win.rs`; the winit tablet hook is used only in
   `native/src/ink/pen_wayland.rs`.
4. **Vendored winit.** `vendor/winit` replaces crates.io winit for the whole build, so it
   must stay at the exact version iced resolves to. Every change to it is also kept as a
   patch in `vendor/winit/md-editor-patches/`: when iced moves to a newer winit, vendor the
   new release and reapply the patches (or drop the ones upstream has absorbed).
