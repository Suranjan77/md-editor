# Repository Structure

A Cargo workspace (Rust edition 2024) with two crates: **`md-editor-core`**, the headless
engine, and **`md-editor-native`**, the Iced desktop app. Core never depends on native.

> **Finding your way.** Every source file opens with a one-line `//!` summary, so
> `grep -rm1 '^//!' core/src native/src` prints a live map of the code. Tests for a module
> `foo.rs` live in `foo/tests.rs`.

```
md-editor/
├── core/                 md-editor-core: vault, index, search, SQLite, PDF and PPTX readers
├── native/               md-editor-native: the Iced app (shell, editor widget, viewers, graph, ink)
│   └── shaders/          WGSL for the knowledge-graph GPU layer
├── wiki/                 This documentation (authoritative)
├── docs/                 The Field Handbook PDF and its HTML sources (a periodic snapshot)
├── tests-fixtures/       Sample PDFs and a generated .pptx used by tests
├── images/               README screenshots
├── .github/workflows/    CI: quality gate (ci.yml) and Windows/Linux release builds
├── packaging/arch/       PKGBUILDs (AUR md-editor, md-editor-git) and the desktop entry
├── scripts/              clippy-ratchet.sh (clippy gate), build-arch-package.sh (package from the tree)
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
| `src/state/pdf_store.rs` | PDF sidecar persistence on `AppState`: document scoping (vault-scoped identity), annotations, references, text cache. |
| `src/config.rs` | `get_sys_config` / `set_sys_config` over `settings`, and `read_startup_value` for use before `AppState` exists. |
| `src/vault.rs` | The vault API: indexing for a generation, open/save/create/delete, `read_vault_image`. |
| `src/vault/backlinks.rs` | `search_vault`, incoming and mixed (note + PDF annotation) backlinks. |
| `src/vault/rename.rs` | Renaming files and folders while rewriting wikilinks, search rows and PDF sidecars. |
| `src/vault/files.rs` | Path jailing (`resolve_vault_path_checked`), the atomic `write_file` / `write_bytes`, file-type checks, tree listing. Tests in `files/tests.rs`. |
| `src/vault/graph.rs` | `get_graph_snapshot`: knowledge-graph nodes and edges. See [Knowledge Graph](Knowledge-Graph.md). |
| `src/file_index.rs` | Wikilink parsing (`[[target\|alias]]`), resolution (exact path, then shortest matching basename), outgoing and incoming maps. |
| `src/pdf.rs` | PDF data types, `compute_provisional_id`, render-scale buckets, `bind_pdfium`. |
| `src/pdf/renderer.rs` | `PdfRenderer`, the handle the app talks to. |
| `src/pdf/worker.rs` | The PDFium worker thread: 3-document LRU cache, priority and standard queues, one handler per command (render, text, links, search, previews). |
| `src/pdf/toc.rs` | Page-text extraction and outline recovery: embedded bookmarks, printed contents pages, dot leaders. Tests in `toc/tests.rs`. |
| `src/pdf/toc_synth.rs` | The typographic fallback: synthesizing an outline from heading-like lines. |
| `src/references.rs` | Pure resolver from page text to equation, figure, table and section references. Never touches PDFium. |
| `src/pptx.rs`, `src/pptx/` | Read-only PowerPoint reader. `model` is the resolved slide model; `package`, `xml`, `theme`, `color`, `fill`, `text`, `placeholder`, `geometry`, `numbering`, `table_style`, `slide` and `slide_content` (tables and text bodies) do the reading and inheritance. See [Core Services](Core-Services.md#8-powerpoint-presentations-pptxrs-pptx). |
| `src/tracker.rs` | `StudySession` and tracker key/value persistence. |
| `src/attachments.rs` | Images pasted into notes: attachments folder, naming, SHA-256 dedupe, import, relative Markdown links. |
| `src/types.rs` | Plain data for the UI: `FileEntry`, `SearchResult(s)`, backlinks, `GraphSnapshot`. |
| `src/massive_tests.rs` | Combinatorial and stress tests. |
| `examples/` | `dump_refs`, `bench_refs`: reference-resolver diagnostics on a real PDF. |

## `native/` — md-editor-native

### Shell (`src/app/`, `main.rs`)

| Path | Responsibility |
| :--- | :--- |
| `main.rs` | CLI (a path to open), logging init, window size restore, the iced daemon. |
| `build.rs` | Embeds the Windows icon and version resource. |
| `logging.rs` | File logger behind `log` (`md-editor.log`, rotated 1 MB × 3, level from `MD_EDITOR_LOG`) and the panic hook. |
| `app/mod.rs` | `MdEditor` state, `boot`, `title`, `update`, and `update_inner`, which routes each message family to a handler below. |
| `app/{pdf,graph,ink,search,editor,vault,shortcuts,shell}.rs` | One `update_<family>` handler each; see [Native Desktop GUI](Native-Desktop-GUI.md#2-messages-and-where-they-are-handled-messagesrs-app). |
| `app/pdf.rs` | `update_pdf`: a router only; each `PdfMessage` goes to a handler in the three files below or to `PdfPane::update`. |
| `app/pdf_render.rs` | PDF load results, zoom and fit-to-width, rendered/failed pages, scrolling, page links, TOC. |
| `app/pdf_nav.rs` | PDF link clicks and previews, PDF search results, document id → highlights and references, jumping to an annotation. |
| `app/pdf_select.rs` | PDF text selection and copy, highlights, quick notes, linked notes, orphan report. |
| `app/pdf_pages.rs` | Opening PDFs, page render scheduling and eviction, available width, `navigate_pdf_page`. |
| `app/paste.rs` | Ctrl+V in a note, "Insert image…", and files dropped on the window: images become attachments, vault files become links, drops on the sidebar are imported (never over an existing file). |
| `app/tabs_handler.rs` | `update_tabs`: activating, closing, reopening and reordering document tabs, split-view pairing, per-vault tab persistence. |
| `app/settings_handler.rs` | `update_settings`, plus restoring and persisting theme, scale, motion, editor typography, attachments and recent vaults. |
| `app/vault_entries.rs` | Creating, renaming and deleting vault entries; name-modal validation. |
| `app/vault_links.rs` | Sidebar clicks, internal / external / `pdf://` link routing, sidebar keyboard navigation. |
| `app/documents.rs` | Opening, saving, parking documents (notes, decks, images); session and layout persistence. |
| `app/subscription.rs` | Keyboard, timers, window events, pen input, the vault watcher. |
| `app/view.rs` | `view(window)`: the shell window (toolbar, tab strip, sidebar, main pane, side panels, status bar) and the ink window. |
| `app/overlays.rs` | Layers above the shell: drop hint, tab overflow, context menus (one builder per target), vault search, palette, dialogs, PDF link preview, toasts; `anchor_context_menu`. |
| `app/links.rs` | Wikilink, relative-link and heading-anchor resolution. |
| `messages.rs` | `Message` and its families (`VaultMessage`, `TabsMessage`, `SettingsMessage`, …), `Shortcut`, `TrackerTab`. |
| `shortcuts.rs` | The shortcut registry: one table feeding key handling, menus, tooltips, the palette and the Settings shortcut list. |
| `tabs.rs`, `tabs/tests.rs` | `Tabs`: strip order, active tab, MRU, reopen stack, duplicate-name labels, per-tab view state, session JSON. |
| `platform.rs` | Per-OS shims: opening external links, reading the clipboard for paste (arboard; test double in tests). |
| `size_lint.rs` | Test-only code-health guardrail: modules over ~800 non-test lines and functions over 150 lines fail, with a ratchet list for today's offenders. |
| `test_support.rs` | Test-only: pins the data dir to a temp folder for every test that builds the app; registers the bundled fonts for headless renders. |
| `e2e/` | Headless end-to-end tests: `mod.rs` (harness, invariants), `scenarios.rs` (data safety), `flows.rs` (UI flows), `workspace.rs` (tabs, Settings applied live, excluded folders, context menus, focus return, image picker and drops), `monkey.rs` (seeded random sessions), `telemetry.rs`. |

### State (`src/*_state.rs`, `*_pane.rs`)

| Path | Responsibility |
| :--- | :--- |
| `editor_state.rs` | `EditorPane`: buffer, highlighted lines, image and math caches, autosave timing, caret reveal, retained buffers. |
| `vault_state.rs` | Vault root, entries, sidebar selection and expansion, backlinks. |
| `ui_state.rs` | Modals, palette, toast queue, context menus, split view, panel widths, focus region, settings page, window size and scale. |
| `search_state.rs` | Query, flags, results, memoized in-document matches. |
| `tracker_state.rs` | Timer, sessions, tracker values, active tab. |
| `pdf_pane.rs` | PDF viewport, page caches, annotations, selection, render generation. |
| `pptx_pane.rs` | Open deck, load generation, zoom, scroll. |
| `graph_state.rs`, `graph_state/` | `GraphState`, with the force simulation in `physics` and initial placement in `layout`. |
| `ink/` | `InkState` and the handwriting engine; see [Handwritten Notes](Handwritten-Notes.md). |

### The Markdown editor (`src/editor/`)

| Path | Responsibility |
| :--- | :--- |
| `buffer/` | `DocBuffer` over `ropey::Rope`. `mod.rs`: state, command router, revision-based dirty tracking, line-ending preservation; `edit` (insert/delete/replace), `format` (inline wrappers, auto-pairing), `lists` (continuation, checkboxes), `movement` (grapheme-aware caret and selection), `undo` (transactions with inverse ops, run coalescing). |
| `highlight.rs`, `highlight/` | `Highlighted`: incremental Markdown → `StyledLine`s (block state machine here, `inline` spans, `code` via syntect). `reference.rs` is the old highlighter kept as a test oracle. |
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
| `renderer/draw/` | Painting: frame setup and culling, blocks, inline content, overlays, captions, primitives. |
| `renderer/testing.rs`, `tests.rs`, `properties.rs` | A recording renderer and `View` driver, behavioural tests, seeded property suites. |
| `render_preview.rs`, `render_snapshot_tests.rs`, `test_docs.rs` | Opt-in PNG previews and draw-call transcripts; the seeded test-document generator. |

### Views and viewers (`src/views/`, `src/slides/`)

| Path | Responsibility |
| :--- | :--- |
| `views/sidebar.rs`, `toolbar.rs`, `welcome.rs`, `toast.rs`, `settings.rs` | Shell chrome and the Settings page (`Ctrl+,`). |
| `views/viewer_toolbar.rs` | The shared viewer toolbar for PDFs, slides and images: outline, page field, zoom field, fit, find. |
| `views/modals.rs`, `link_note_picker.rs`, `command_palette.rs` | Dialogs and the `Ctrl+P` palette (registry and fuzzy ranking via `fuzzy.rs`). |
| `views/search.rs`, `toc.rs`, `backlinks.rs` | Side panels. |
| `views/pdf_viewer.rs`, `interactive_pdf.rs` | The PDF pane and the per-page widget. See [PDF Viewer Internals](PDF-Viewer-Internals.md). |
| `views/pptx_viewer.rs`, `slides/` | The slide viewer; `slides/` prepares decks off the UI thread and draws slides on layered canvases. |
| `views/graph.rs`, `graph/`, `gpu_graph.rs` (+`gpu_graph/pipeline.rs`) | The knowledge graph: toolbar, inspector, label overlay, and the GPU layer (`gpu_graph.rs` handles pointer interaction; `pipeline.rs` holds the wgpu instance layouts and render pipeline). |
| `views/ink.rs` | The handwriting window's toolbar and palette. |
| `views/tracker.rs`, `tracker/` | The study tracker: dashboard in `tracker.rs`, the other tabs in `tracker/sections.rs`, the JSON config model in `tracker/config.rs`. |
| `design/` | The design system ([MASTER](../docs/design-system/md-editor/MASTER.md)): `palette` (Dark/Light token tables), `type_scale`, `metrics` (spacing, sizes, panel widths), `motion` (panel animations, springs, reduced motion), `fonts` (bundled Inter, JetBrains Mono, Source Serif); `mod.rs` holds the runtime theme, scale and typography settings. Guardrail tests in `design/tests.rs`; `design/setting.rs` makes those settings per-thread under `cargo test`. |
| `ui/` | Component library built only from `design` tokens: buttons, fields, menus, dialogs, toasts, panels, tab strip, status bar, tree and list rows, icons (Phosphor SVGs in `assets/icons/`). `ui/gallery.rs` renders every component in both themes at several scales; `ui/context_anchor.rs` stamps the pointer position onto context-menu messages. |
| `pdf_notes.rs`, `search.rs` | Linked-note helpers for PDF highlights; in-note line matching. |

---

## Crate Dependencies

| Crate | Runtime | Build |
| :--- | :--- | :--- |
| core | `rusqlite` (bundled SQLite), `pdfium-render`, `image`, `regex`, `serde`/`serde_json`, `sha2`, `uuid`, `zip` (read-only, deflate), `roxmltree` | `ureq`, `flate2`, `tar`, `sha2` (PDFium download) |
| native | `md-editor-core`, `iced` 0.14 (canvas, image, advanced, tokio), `ropey`, `syntect`, `ratex-*`, `unicode-linebreak`, `unicode-segmentation`, `rfd`, `notify`, `tokio`, `chrono`, `image`, `regex`, `serde`, `uuid`, `bytemuck`; Windows: `windows-sys` | Windows: `winresource` |

### Dependency Rules

1. **No cycles.** `md-editor-core` never depends on `md-editor-native`.
2. **No UI in core.** Core takes no GUI or text-shaping dependency (`iced`, `winit`,
   `ratex-render`).
3. **FFI containment.** `pdfium-render` is used only under `core/src/pdf/`; exactly one
   `Pdfium` binding exists per process. Win32 calls stay in `native/src/platform.rs` and
   `native/src/ink/pen_win.rs`.
