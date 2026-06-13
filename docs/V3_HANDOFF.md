# V3 Handoff — execution state of docs/V3_GROUND_UP_PLAN.md

> **Read this first when resuming v3 work.** Updated after every completed unit of work.
> Sibling ledgers: `PLAN-NOTES.md` (v2 incremental plan), `docs/V3_GROUND_UP_PLAN.md` (the master plan).
> **Next work is `docs/V3_IMPLEMENTATION_PLAN.md` Phase 7.6**, one
> refinement at a time. Course correction 6.0–6.3 and Typora-grade 7.0–7.5
> are implemented; manual smoke items 27–33 still gate full acceptance of
> 7.3–7.5. The §0.2 hard rules and §0.4 pitfalls register remain binding.

## Ground rules for this execution

The plan is written for 10–20 engineers over 12–18 months. Execution here is by a single
agent, so the plan's *decision points* are collapsed to their stated defaults and the
*architecture* is built in dependency order (kernel → editor → vault/pdf → shell). Every
"squad deliverable" becomes a crate/module with its quality gate expressed as tests —
especially the three named regression suites (BUG-A/B/C) that M1's gate requires.

- v3 lives in `v3/` as an **independent cargo workspace** (root workspace excludes it).
  v2 (`core/`, `native/`) is untouched and remains the shipping app.
- Toolkit: the 3-week bake-off (plan §3.5) cannot be run here; plan's own tie-breaker
  applies — **stay on iced**, editor engine stays toolkit-agnostic via draw commands.
  Recorded as ADR-0100.
- Parser: tree-sitter spike deferred; in-house incremental block parser direction kept,
  re-openable. Recorded as ADR-0101.
- v3 crates: `unwrap`/`expect` banned outside tests (no escape hatch yet), typed errors
  only (`Result<_, String>` banned), `#![deny(warnings)]` not used (CI uses `-D warnings`).

## Status board

| Plan item | Plan ref | Status | Where |
|---|---|---|---|
| v3 workspace scaffold | §5 M0 | ✅ | `v3/` — independent workspace, root excludes it; clippy denies unwrap/expect workspace-wide |
| ADR-0100 toolkit decision | §3.5 | ✅ | `docs/adr/0100-v3-toolkit-iced-default.md` — iced by default, engines toolkit-agnostic; boundary enforced in architecture-check.sh (proven by injection) |
| ADR-0101 parser decision | §3.2 | ✅ | `docs/adr/0101-v3-incremental-parser.md` — in-house incremental, re-openable; `Styler` trait is the seam |
| Kernel: CommandRegistry + CommandBus | §3.1 | ✅ | `v3/kernel/src/command.rs` — duplicate/foreign-binding rejection, subsequence palette, FIFO bus |
| Kernel: InputRouter (scoped keymap, conflict CI) | §3.1 | ✅ | `v3/kernel/src/input.rs` — chord parse/display, scope stack, innermost-wins, **Overlay = modal fence** (only Overlay+Global reachable under a modal), static conflict detection, user override API |
| Kernel: PaneTree + tabs + DocumentStore | §3.1 | ✅ | `v3/kernel/src/pane.rs` — split tree, tab dedup per document, empty-pane collapse, doc dedup by path, `Layout` view for the shell |
| Kernel: FocusModel (single focus owner) | §3.1 | ✅ | `v3/kernel/src/focus.rs` (invariant maintained by Workspace) |
| Kernel: Workspace façade | §3.1 | ✅ | `v3/kernel/src/workspace.rs` — `scope_stack()` *derived* per call; `handle_key()` is the one keystroke entry point; doc GC on tab close |
| **BUG-A regression suite** (keymap scoping + conflict enumeration) | §5 M1 gate | ✅ | `v3/kernel/tests/bug_a_keymap_scoping.rs` (7 tests incl. modal fence + the exact v2 split scenario) |
| **BUG-C regression suite** (PDF standalone in a tab) | §5 M1 gate | ✅ | `v3/kernel/tests/bug_c_documents_are_peers.rs` (5 tests) |
| Editor: height sum-tree (O(log n) offsets) | §3.2 | ✅ | `v3/editor/src/height_tree.rs` — implicit treap w/ subtree sums, deterministic priorities, differential-tested vs naive model (4k random ops) |
| Editor: 3-phase layout protocol (style/measure/paint) | §3.2 | ✅ | `v3/editor/src/layout.rs` — `Styler`/`Measurer` traits, `Damage { repaint, shifted_from }`, offsets never cached per line, viewport-bounded paint |
| Editor: layout-stable conceal contract | §3.2 | ✅ | `Styler::layout_stable()` + debug assert in `set_conceal`; reserved-width strategy demonstrated in tests |
| Editor: rope buffer + `Vec<Selection>` + branching undo | §3.2 | ✅ | `v3/editor/src/buffer.rs` (ropey, multi-cursor model day-one, grapheme-safe motion/deletion incl. emoji/CJK/CRLF) + `undo.rs` (`UndoTree` — editing after undo branches, never clears the future). Quality harness: `tests/buffer_undo_invariants.rs` (12 tests: 8-seed × 500-command storms w/ undo-to-root == identity, selections-in-bounds invariant, grapheme suites, multi-cursor edits, branch preservation) |
| Shell: markdown surface is a real buffer | §5 M1 | ✅ | typing/motion/selection via raw-input fallthrough (case-preserved via `KeyEvent.text`); ctrl+z/ctrl+shift+z/ctrl+a are real buffer commands; **ctrl+s saves through `md3-vault::atomic_save`** (and re-syncs the FTS index); sessions keyed by `DocumentId` (split panes share state by construction); dirty dot in tab strip, Ln/Col in status bar; content loads from disk on open |
| Shell: styled GUI (`gui` module) | §5 M1–M2 | ✅ | `v3/shell/src/gui/` — markdown paints through the engine's 3-phase layout on an iced canvas (`EditorCanvas` + `MonoMeasurer` grid; concealed markers keep reserved width, BUG-B end to end); PDF pane renders real pages behind the `pdfium` feature (placeholder otherwise); quick-open/palette/search/find/zoom/page overlays fed by the same single keystroke path; vault-rooted (`md3-shell <dir>`); FTS search composes the `TextExtractor` seam in the shell, as planned. Routing suite (15 tests) drives `gui::Shell::update` over a tempdir vault — BUG-A/C pinned at the shell layer, windowlessly |
| **BUG-B regression suite** (height change reflows; damage ≤ affected lines) | §5 M1 gate | ✅ | `v3/editor/tests/bug_b_layout_reflow.rs` (6 tests incl. "caret motion damages ≤ 2 lines" golden gate) |
| Editor: rope buffer + multi-cursor + grapheme safety | §3.2 | ✅ | `v3/editor/src/buffer.rs` — ropey, `Vec<Selection>` model (sorted/merged/non-empty, boundary-snapped), `ChangedSpan` buffer→layout bridge, `LayoutEngine::splice` consumer |
| Editor: undo tree (persistent-ready) | §3.2 | ✅ | `v3/editor/src/undo.rs` — branch-keeping tree, insert-run coalescing, save-point dirtiness, validated `UndoTreeSnapshot` for the sidecar |
| Editor: buffer property harness | §3.2/§6 | ✅ | `v3/editor/tests/buffer_properties.rs` — undo-to-root identity, selection invariants, grapheme alignment (ZWJ/flag/CJK/CRLF), buffer↔layout lockstep; caught 2 real CRLF/cluster bugs pre-merge |
| Editor: incremental block parser (ADR-0101) | §3.2 | ✅ | `v3/editor/src/parse.rs` — explicit entry/exit `BlockState` per line, forward reparse to convergence, returns invalidated range; differential-tested vs full reparse (2k random edits) |
| Editor: inline spans + production styler | §3.2 | ✅ | `v3/editor/src/style.rs` — `MarkdownStyler` (reserved-width conceal, `layout_stable() == true` by construction), char-offset `Span`s (emphasis/code/math/links/wikilinks/tables), spans always tile the source line |
| Editor: `EditorDocument` session | §3.2 | ✅ | `v3/editor/src/document.rs` — buffer + parser + layout behind one `apply()`; fence-typing cascade restyles, caret conceal-follow, merged `Damage`; "caret motion ≤ 2 lines" asserted end-to-end |
| Vault: typed errors + atomic save | §3.4 | ✅ | `v3/vault/` — `VaultError` (thiserror), temp+fsync+rename save |
| Vault: FTS5 incremental index | §3.4 | ✅ | `v3/vault/src/index.rs` — `(mtime, size)` diff (unchanged vault re-reads nothing, test-pinned), targeted `sync_paths` for watcher batches, quoted-token FTS queries (operator injection inert), root-relative paths |
| Vault: debounced fs watcher | §3.4 | ✅ | `v3/vault/src/watcher.rs` — `notify` + 500 ms quiet-window debounce thread, deduped batches; M2 "external edit converges < 2 s" gate test green |
| Vault: link graph + rename repair | §3.4 | ✅ | `v3/vault/src/links.rs` — regex-free wikilink extraction (alias/anchor aware), bidirectional graph, broken-link query, `rewrite_links` as a pure transaction (caller persists via atomic save) |
| PDF: tile cache + render queue (pure logic) | §3.3 | ✅ | `v3/pdf/src/tile.rs` — 1.4^n zoom buckets (never-upscale>1.4× proven by sweep test), byte-budget LRU w/ eviction reporting, cancellable queue |
| PDF: continuous-scroll geometry (pure) | §3.3 | ✅ | `v3/pdf/src/scroll.rs` — `DocLayout`: centered page strip from page sizes, cumulative offsets, `page_at`/`visible_pages` (partition-point), `visible_tiles` returning bucket-addressed `PlacedTile`s with display rects (virtualization: only viewport-intersecting tiles; ≤1.4× magnification by construction); zoom rebuild + caller re-anchoring; 9 unit tests |
| Shell: PDF reading UX (continuous scroll + tiles) | §3.3 / §5 M2 | ✅ | `gui/pdf_view.rs` + `PdfSession` v2 — page sheets + tiles painted on a canvas from `DocLayout` at real bounds; `ensure_tiles` drives the engine `RenderQueue`/`TileCache` (offscreen requests cancelled, evicted pixmaps dropped; synchronous render, worker thread deferred); wheel/pgup·pgdn/arrows/home/end scroll the strip, ←/→ jump pages, ctrl+g jumps, ctrl+z zoom re-anchors the current page across buckets; status pill `p. N/M · zoom%`. pdfium-gated suite `shell/tests/pdf_reading.rs` (4 tests over the multipage fixture) runs in CI |
| PDF: pdfium wiring (ADR-0002 re-affirmed) | §3.3 | ✅ | `v3/pdf/src/render.rs` behind the `pdfium` cargo feature — tile render (full page at bucket scale, sliced to 512 px grid), text extraction, typed errors incl. corrupt-PDF fixture test; FFI serialized by an engine-level mutex |
| Vault: PDF text → FTS bridge (`TextExtractor` seam) | §3.4 | ✅ | `SearchIndex::sync_with`/`sync_paths_with` take an optional extractor; PDFs share the `(mtime, size)` guard (no re-extraction, fake-extractor call-count tests); real-pdfium integration test in `v3/pdf/tests/fts_bridge.rs` (dev-dep only — production composition belongs to the shell) |
| **Annotations v2** (hash keys + migrations + export) | §3.3 / §5 M2 gate | ✅ | `v3/vault/src/annotations.rs` — `AnnotationStore` keyed by document SHA-256 (`document_hash`, streamed); numbered transactional migrations (`migrations(component, version)` table, sidecar-shared, append-only ladder pinned by fingerprint test); quads+color+note+linked-note CRUD; JSON export/import (serde) + Markdown summary; last-seen-path table for orphan reports. **M2 gate test green:** `v3/vault/tests/annotations_survive_rename.rs` (rename+move across sessions; edited bytes = new identity, old annotations reachable, never silently dropped). Shell wiring (highlight UI, persistent sidecar path) is a later session |
| PDF: text selection geometry (pure `select` + `page_chars`) | §3.3 | ✅ | `v3/pdf/src/select.rs` — line grouping by vertical overlap, caret positions from page points, per-line quads + text (synthetic-grid tests); `DocLayout::page_at_point`/`point_in_page` inverse hit-testing; `render.rs::page_chars` flips pdfium's bottom-left rects to top-left page points (fixture test drives the pure selector over real glyphs and cross-checks `extract_text`) |
| **Shell: annotations v2 wiring** (persistent sidecar + selection + highlight UI) | §3.3–3.4 / §5 M2 | ✅ | sidecar at `<vault>/.md3/sidecar.db`, one SQLite file shared by `SearchIndex` + `AnnotationStore` (disjoint tables; in-memory index fallback for read-only vaults) — the FTS index now persists across runs (cold start re-reads nothing, test-pinned); `record_document` + streamed SHA-256 on every PDF open (works without pdfium); drag selection on the canvas (`PdfMouseDown/Dragged/Up` → engine `select()` over cached `page_chars`, page-point state so scroll/zoom never invalidates it); `pdf.highlight` (ctrl+h) persists quads, `pdf.annotation-note` (ctrl+n) edits via overlay, delete key removes the picked highlight, `pdf.annotations-export` (palette) writes `<stem>-annotations.md` through `atomic_save` + index re-sync; annotation/selection tints painted from page points each frame. Suite: `shell/tests/annotations_wiring.rs` (4 always-on + 2 pdfium end-to-end incl. reopen-reloads-from-hash) |
| Vault: SessionStore + shared migrations runner | §3.4 / §5 M2 | ✅ | `v3/vault/src/migrations.rs` — the component-keyed ladder extracted from annotations (same `migrations` table, per-component versions); `v3/vault/src/session.rs` — single-row `session_state` holding the shell's opaque JSON snapshot (save/load/clear; cohabitation with annotations test-pinned) |
| Kernel: restore primitives | §3.1 | ✅ | `PaneTree::split_with_ratio` (ratio clamped 0.05–0.95, NaN→0.5; `split` delegates at 0.5) + `collapse_empty_panes` (hollow splits don't outlive their content; last pane survives) |
| **Shell: session restore** | §5 M2 | ✅ | `gui/snapshot.rs` — serde wire format (pane tree + per-path view state, all fields `#[serde(default)]` so restore degrades, never refuses); capture on open/close-tab/split/next-tab/tab-click/save/quit **and** on window close (`exit_on_close_request: false` → save → exit); restore in `Shell::new`: rebuild splits with saved ratios, skip vanished files, collapse hollow panes, reapply md caret+scroll / pdf zoom+scroll, refocus; **"resumed at p. N/M"** status when the focused doc is a PDF. Suite: `shell/tests/session_restore.rs` (5 always-on + pdfium "resumed at p. 3" E2E) |
| **Shell: settings v1 — user keymap overrides** | §3.1 | ✅ | `shell/src/settings.rs` — `<vault>/.md3/keymap.json` (`{scope, chord, command}` rows; `command: null` unbinds); command names resolved against the registry (ids stay `'static`, typos warn); bad rows/corrupt file warn and skip, never block startup; applied in `main` before the GUI. 5 tests incl. override-beats-default and scope isolation |
| Shell: registry-generated keymap/palette dump | §3.1 | ✅ | `v3/shell/` — startup conflict check exits non-zero; `--dump-shortcuts` generates `docs/V3_SHORTCUTS.md`; `--demo` walks BUG-A/C on the live kernel |
| **PDF selection/highlight paint fixes** (user-reported) | §3.3 | ✅ | (1) tints were invisible: iced_wgpu draws images after meshes *within a layer*, so same-frame `fill_rectangle` landed under the page tiles — `TintCanvas` now stacks above `PdfCanvas` (own layer, captures nothing); (2) quads landed on line 1: `select::lines()` any-overlap banding chained tightly-leaded loose boxes into one band — now requires >50% overlap of the smaller height (test-pinned); (3) hover I-beam over text / pointer over highlights via `mouse_interaction` (glyphs pre-load per visible page in `ensure_tiles`) |
| PDF: `select::find` + `range_selection` (pure search) | §3.3 | ✅ | `v3/pdf/src/select.rs` — case-insensitive char-stream match returning index ranges; `range_selection` (extracted from `select`) turns any range into quads+text; synthetic-grid tests |
| **Shell: PDF search overlay** (`pdf.find`) | §3.3 / §5 M2 | ✅ | `Overlay::PdfFind` — ctrl+f in pdf scope loads all pages' glyphs once, live-filters hits (`p. N · context` rows, capped 100), enter scrolls the match a third down the viewport and plants it as the live selection (tinted; `ctrl+h` chains). Suite: `shell/tests/pdf_find.rs` (2 always-on guard rails + pdfium e2e incl. case-insensitive needle + cross-page jump) |
| **PDF selection robustness** (user-reported, same-line drags) | §3.3 | ✅ | `joins_line` is now *vertical-center containment*: the interim ≥50%-overlap rule split visual lines at pdfium's degenerate (zero-height) synthesized space boxes, after which `position()` resolved every same-row caret into the first segment — same-line drags drew nothing while multi-line worked. `position()` also breaks vertical ties by x (`(dy, dx)` key), so same-row bands (two-column layouts) resolve under the cursor. Both pinned by synthetic-grid tests |
| PDF: outline extraction + section math | §3.3 | ✅ | `v3/pdf/src/outline.rs` (pure `OutlineEntry` + `section_at`, malformed-order tolerant) + `render.rs::outline()` (manual DFS over pdfium bookmarks w/ depth tags, cycle/size caps); fixture test over `multipage-outline.pdf` |
| **Shell: TOC overlay + section tracking** (`pdf.toc`) | §3.3 / §5 M2 | ✅ | ctrl+t — `Overlay::PdfToc` lists the outline (depth-indented, filterable; `toc_matches` shared by display rows and confirm so the row shown is the row picked), enter jumps; status pill appends `· § section` via `PdfSession::current_section`. Suite: `shell/tests/pdf_toc.rs` |
| **Shell: PDF back/forward jump history** | §3.3 | ✅ | alt+left / alt+right (`pdf.back`/`pdf.forward`; `Mods::ALT` added to the kernel) — jump-list grammar (new jump drops the forward branch, cap 64); positions stored in *points* (px ÷ zoom) so history survives zoom changes; recorded on go-to-page, TOC and find jumps. Covered in the pdf_toc e2e |
| CI: v3 job in quality workflow | §6 | ✅ | `.github/workflows/quality.yml` `v3` job: fmt, clippy -D warnings, tests, demo, generated-doc freshness diff |
| PDF geometry paint plans | Phase 0.1 | ✅ | `v3/shell/src/gui/paint.rs` — extracted `tint_plan` and `page_plan` to decouple geometry math from iced canvases; tested in `shell/tests/pdf_paint_plan.rs` |
| Hostile glyph selection tests | Phase 0.2 | ✅ | `v3/pdf/tests/selection_real_glyphs.rs` — added selection tests with real-glyph boxes on tight-leading and two-column layouts; verified same-line drag tie-breakers |
| Rotated pages selection audit | Phase 0.3 | ✅ | `v3/pdf/tests/selection_real_glyphs.rs` — verified selection bounds checks and whole-page drags for `/Rotate 90/180/270` |
| PDF search eager load cap | Phase 0.5 | ✅ | `v3/shell/src/gui/mod.rs` — capped eager page load at 200 pages to guard against UI freeze; added scale guard tests in `shell/tests/pdf_find.rs` |
| PDF internal-reference popup | Phase 1 | ✅ | Engine: `v3/pdf/src/select.rs`, `v3/pdf/src/render.rs` (page_links). Shell: `v3/shell/src/gui/session.rs` (link_at), `v3/shell/src/gui/pdf_view.rs` (load_page_links, RightClick), `v3/shell/src/gui/mod.rs` (PdfRightClick, pdf_mouse_down left-click navigation), `v3/shell/src/gui/overlay.rs` (Overlay::PdfLinkPreview); tested in `v3/shell/tests/pdf_links.rs` |
| File browser panel | Phase 2 | ✅ | `v3/shell/src/gui/file_tree.rs` — derived file tree from flat list; registered `workspace.toggle-files` hotkey; toggles open/collapsed state and handles file clicks; tested in `v3/shell/tests/file_tree.rs` |
| Theme tokens + branding | Phase 3 | ✅ | `v3/shell/src/gui/tokens.rs` — centralized v2 dark hex palette; updated editors/overlays/status/sidebar to read from tokens; wired application custom theme and window icon; desktop entry installer in `v3/shell/src/desktop.rs` |
| Study Tracker panel | Phase 4 | ✅ | `v3/vault/src/tracker.rs` (SQLite store) and `v3/shell/src/gui/tracker_view.rs` (curriculum, activities, log, config tabs); tested in `v3/vault/tests/tracker_store.rs` and `v3/shell/tests/tracker_wiring.rs` |
| **Overlay hit lists scroll** (user-reported: TOC panel unscrollable) | UX | ✅ | `gui/overlay.rs` — the 12-row display cap is gone; the full match set renders in a `scrollable` (capped 420 px) shared by every list overlay (palette/quick-open/search/pdf-find/toc); ↑/↓ clamp to the rows actually displayed and `snap_selected` keeps the row in view; vault search returns 50 hits. Suite: `shell/tests/overlay_list.rs` (4 tests). Stale `V3_SHORTCUTS.md` regenerated (toggle-files/toggle-tracker rows were missing) |
| Whitespace-elastic `pdf.find` | P5 backlog №5 | ✅ | `v3/pdf/src/select.rs::find` — needle whitespace matches ≥0 stream whitespace (a dropped `\r\n` wrap is zero chars wide), so multi-word needles match across line wraps; 3 new synthetic tests pin elastic/wrap/punctuation semantics. The P6 known-limitation is closed |
| **Backlinks panel** (`note.backlinks`) | P5 backlog №2 / §3.4 | ✅ | ctrl+shift+b on a focused note — `Overlay::Backlinks` lists referrers from `LinkGraph` (built fresh per call from the vault's notes), filterable, enter opens the referrer; md-scope chord is inert on PDFs (BUG-A discipline test-pinned). Suite: `shell/tests/backlinks.rs` (4 tests) |
| **Annotation niceties** (copy/color/linked-note/orphans) | P5 backlog №3 | ✅ | `pdf.copy-selection` (ctrl+c, pdf scope → `iced::clipboard::write`); `pdf.highlight-color` cycles a 4-entry palette via new `AnnotationStore::set_color` path (`HIGHLIGHT_PALETTE`, entry 0 = the default); `pdf.annotation-link-note` creates `<stem>-notes.md` through `atomic_save` + index sync, records it via new `AnnotationStore::set_linked_note`, opens it; `pdf.annotations-orphans` lists known docs whose hash matches no vault file (read-only `Overlay::OrphanReport`). Suite: `shell/tests/annotation_niceties.rs` (5 always-on tests) |
| **Async PDF worker** | P5 backlog №1 | ✅ | `gui/worker.rs` owns one FIFO worker thread for tile/glyph/link pdfium calls; shell subscription handshake installs its nonblocking submit handle, tracks in-flight work, routes results by absolute path, refreshes open find results incrementally, and keeps synchronous fallback for windowless tests. Production `pdf.find` queues every page instead of the 200-page stopgap. Suites: worker unit tests + `shell/tests/pdf_worker.rs` |
| **UX overhaul Phase 0: discoverability triage** | UX Phase 0 | ✅ | Fresh sessions open file tree; empty panes render registry-backed welcome buttons through `Message::RunCommand`; `help.shortcuts` (`ctrl+/`) opens filterable registry-backed help and runs selected commands; status bar has independent message and position segments. Suites: `shell/tests/discoverability.rs`, `file_tree.rs`, `session_restore.rs` |
| **UX overhaul Phase 1: menu chrome** | UX Phase 1 | ✅ | In-house anchored menu bar using kernel overlay fence; registry-derived labels/chords; every command menu-reachable or explicitly overlay-exempt via `shell/tests/mouse_coverage.rs`. Global icon toolbar was removed by user direction because it duplicated the menu. Suite: `shell/tests/chrome.rs` |
| **UX overhaul Phase 2: file manager + vault welcome** | UX Phase 2 | ✅ | `file.new-note/new-folder/rename/delete` use input/confirm overlays, atomic saves, link repair, index refresh, stable `DocumentId`, and affected-tab closure; tree header/hover/context menu/dirty markers; persistent 160–480 px drag width. No/invalid-arg startup shows an in-app welcome with Open/Create/recents; OS picker opens only after click. Suite: `shell/tests/file_tree.rs` |
| **Floating PDF controls & docked panels** | UX Phase 4 | ✅ | Bottom-centered per-PDF bar: previous/next page, page input, zoom −/%/+, find, TOC, fit-width/page. Resizable docked TOC and Annotations panels, and selection context menus (Copy, Highlight, Highlight + Note) fully wired. Suite: `shell/tests/chrome.rs` |
| **UX overhaul Phase 3: mouse-driven panes/tabs** | UX Phase 3 | ✅ | Per-tab close buttons and middle-click close; horizontal overflow strip plus quick-open button; per-pane split-right/split-down/close controls; `PaneTree::set_ratio` by split path and draggable 6 px dividers with persisted ratios. Direct tab drag remains optional stretch work. Suite: `kernel::pane` ratio tests + `shell/tests/chrome.rs` |
| **UX overhaul Phase 5: Markdown formatting toolbar** | UX Phase 5 | ✅ | Markdown formatting toolbar with bold, italic, inline code, heading cycle, bullet list, checkbox, and wikilink toggle commands fully implemented in the editor engine and wired to the GUI panel toolbar. Suite: `editor/tests/formatting.rs` |
| **UX overhaul Phase 5: Markdown outline panel** | UX Phase 5 | ✅ | Resizable docked Outline panel listing depth-indented headings extracted from the Markdown document, highlighting active heading, and jumping to heading on click. Suite: `shell/tests/markdown_outline.rs` |
| **UX overhaul Phase 5: Markdown find/replace bar** | UX Phase 5 | ✅ | Docked Find/Replace bar under the markdown editor toolbar supporting case-insensitive search, match counting, prev/next navigation, replacing individual matches, and transactional Replace All. Suite: `shell/tests/markdown_find_replace.rs` |
| **UX overhaul Phase 6: Feedback, Polish & settings** | UX Phase 6 | 🔶 | Toasts, confirm modals for unsaved changes/delete, light/dark tokens, and keymap settings UI landed. Tracker manual-log feedback is fixed and `tracker_wiring` is green; phase remains partial because light theme is a global atomic (`tokens::set_light_theme` — scheduled debt, Phase 6.6). Suites: `shell/tests/chrome.rs`, `session_restore.rs`, `tracker_wiring.rs` |
| **URI links open in browser** | P5 backlog №7 | ✅ | Added `open` dependency under ADR-0102, left-clicking a PDF external link opens it in browser asynchronously. Suite: `shell/tests/pdf_links.rs` |
| **User-reported stabilization pass** | UX / renderer | 🔶 | `docs/V3_STABILIZATION_2026-06-12.md` — startup file-tree population, markdown wrap and measure-before-paint asset geometry, inline math flow, larger typography/rhythm, transparent display math, consolidated multi-line LaTeX environments, PDF resize-fit/reference crop, welcome transition, and toast runtime fix. Core defects addressed; Typora/Obsidian-level markdown polish remains open (specced as impl-plan Phase 6.3 + Phase 7). |
| **Iced async runtime** | ADR-0103 | ✅ | `md3-shell` enables iced `tokio` feature so toast timer tasks run inside a Tokio reactor; fixes production panic from `tokio::time::sleep`. |
| **Course correction 6.0: green baseline** | Phase 6.0 | ✅ | Tracker manual-log success now queues one success toast and `tracker_wiring` asserts that channel. Full default + pdfium workspace gates green. |
| **Course correction 6.1: v3 size budgets** | Phase 6.1 | ✅ | `v3/budgets.toml` + `scripts/v3-budget.sh`, wired into CI. Hard limit 700; six pre-existing oversized files ratcheted. Enforcement proven by temporary hard-limit injection. |
| **Course correction 6.2: decomposition** | Phase 6.2 | ✅ | `gui/mod.rs` fell from 4,838 to **1,479**; `editor/src/buffer.rs` from 1,911 to **833**. Shell surface modules, `buffer/formatting.rs`, `buffer/edit_ops.rs`, and `buffer/typing.rs` keep extracted files ≤700 lines. Ratchets lowered in `v3/budgets.toml`; `scripts/v3-budget.sh` green. |
| **Phase 6.3 golden snapshots** | Phase 6.3 | ✅ | `editor_draw_plan.rs`, `paint.rs` — Paint geometry extracted; strict diff-based test verifies exact `PaintOp` stream against `golden.md` |
| **ADR-0104 typora grade** | Phase 7.0 | ✅ | `docs/adr/0104-v3-typora-grade.md` — layout engine refactored for shaped lines via `Measurer` trait |
| **Phase 7.1 hide/measured reveal** | Phase 7.1 | ✅ | `editor/src/document.rs`, `sync_conceal` — Conceal mode now true hide, layout stable invariant dropped |
| **Phase 7.2 typography** | Phase 7.2 | ✅ | `shell/src/gui/shaped_measurer.rs` — heading scales, rhythm, spacing |
| **Phase 7.3 blocks render as units** | Phase 7.3 | 🔶 | `paint.rs`, `editor/src/document.rs`, `session.rs` — measured table grid, fenced-code panel, unified block reveal, source/display offset mapping, shaped table hit-testing. Golden draw plan and BUG-B suites green; manual smoke items 30–31 pending. |
| **Phase 7.4 stable assets** | Phase 7.4 | 🔶 | `vault/src/asset_sizes.rs`, `gui/markdown_assets.rs`, `gui/worker.rs`, `gui/pdf_worker_events.rs` — sidecar dimensions, async image/math loading, cached first layout, remeasure only on changed dimensions. Store/worker/layout tests green; manual smoke item 32 pending. |
| **Phase 7.5 interaction exactness** | Phase 7.5 | 🔶 | `shaped_measurer.rs`, `editor_canvas.rs`, `editor_hit_testing.rs`, `markdown_interactions.rs` — one shaped geometry path for paint/caret/selection/hit-test, full golden round-trip, clickable checkbox through registered command, ctrl+click URI/wikilink, ctrl-hover pointer+underline. Manual smoke item 33 pending. |
| **Phase 7.6 refinements** | Phase 7.6 | 🔶 | CJK, emoji, and mixed-direction bidi shaped round-trip properties landed in `editor_hit_testing.rs`; caret geometry now follows bidi levels and ignores zero-width duplicate glyph records. ADR-0106 fixes fenced-code syntax ownership. **Inline-math vertical alignment fixed** (`paint.rs` — centered on the text row, not a `line_height`-tall box; golden re-pinned). **Table-cell text vertical overflow fixed** (`paint.rs` — centered in the cell box instead of overflowing into the next row; guard `table_cell_text_stays_within_its_cell_box`). **p95 keypress→layout CI bench landed** (`shell/tests/keypress_bench.rs` — 5k-line doc, p95 < 16ms, ~0.4ms locally). Remaining: element-level reveal, optional motion, fenced-code syntax implementation. |

Statuses: ✅ done · 🔶 partial · ⬜ not started · ❌ blocked

**Last committed verification snapshot (2026-06-13, Phase 6.2 in progress):** Phase 6.0–6.1
full gate was green in both feature configurations, kernel demo was green,
and shortcuts were fresh. Each Phase 6.2 extraction then passed fmt, the
relevant focused routing/editor suites, clippy `-D warnings` (both shell
feature configurations for PDF-facing moves), and the v3 size budget. Full
`md3-editor` suite after the buffer split: 48 unit tests plus all integration,
property, undo-storm, BUG-B, and formatting suites green. Local shell tests
used isolated `XDG_CONFIG_HOME` because tracker storage is app-global. A new
full dual-configuration workspace gate is still required when 6.2 reaches its
size targets.

## Deliberately deferred (next sessions, in order)

1. ~~Shell UI on iced~~ — done (see status board): the `gui` module is the one
   shell (the interim plain-text `app` module was deleted after its routing
   suite was ported).
2. ~~Rope buffer + undo tree port~~ — done (see status board).
3. ~~Incremental parser + style phase~~ — done (see status board).
4. ~~Vault watcher + FTS5 index~~ — done (see status board), plus link graph.
5. ~~pdfium wiring for the tile renderer~~ — done (see status board).
6. ~~PDF text → FTS index plumbing~~ — done (see status board); shell owns the
   production composition (`PdfiumExtractor` adapter lives in the bridge test as
   the reference shape).
7. ~~Shell wiring for annotations v2~~ — done (see status board): persistent
   sidecar, `record_document` on open, drag-select → highlight/note/delete/
   export. Still open on this surface: highlight color choices, creating a
   *linked note* from a highlight, an orphaned-annotation report UI
   (`known_documents` is ready), copy-selection-to-clipboard.
8. ~~PDF reading UX: continuous scroll + tiles~~ — done (see status board).
   Still open from plan §3.3: TOC with section tracking, PDF search overlay
   wiring (`pdf.find` is routed but stubbed), back/forward history, async
   tile worker.
9. ~~Session restore + settings~~ — done (see status board): layout + view
   state + focus across restarts, "resumed at p. N", keymap-override file.
   Settings *UI* (a rendered settings surface) is still plan-M2 open scope;
   so is a theme system on tokens.
10. v2 hotfixes for BUG-A/BUG-B (plan §9.2) — *not ordered by user; ask before doing.*
11. ~~PDF search overlay + TOC with section tracking + back/forward
    history~~ — done (see status board). Known pdf.find limitation:
    pdfium's `\r\n` chars are dropped from the char stream, so multi-word
    needles only match within a line (whitespace-elastic matching is a
    pure-engine refinement).
12. **User-ordered v2 parity (2026-06-12, see also agent memory
    `v3-feature-parity-expectations`):**
    (a) PDF internal-reference popup — right-click an internal link →
    modal showing the referenced item, esc closes
    (`tests-fixtures/pdf/internal-links.pdf` is ready);
    (b) left file-browser panel for the vault tree;
    (c) ~~the bespoke study tracker~~ — done (see status board);
    (d) colors: v2's palette is the preferred baseline ("or better") —
    fold into the plan-M2 theme-tokens work;
    (e) app icon branding — `md-editor.png` at repo root; v2 installed a
    desktop entry + icons (`native/src/main.rs --install`).
13. Remaining M2/M3 surfaces, in rough value order:
    editing-ergonomics bundle (plan §3.2 / M3); ~~link graph UI (backlinks
    panel)~~ — done (see status board); ~~annotation niceties (colors,
    linked notes, orphan report, copy-selection)~~ — done (see status
    board); ~~async tile worker~~ — done (see status board).
14. **GUI/UX overhaul (user-ordered 2026-06-12):** the app must stop
    feeling terminal-like; v2's mouse-first GUI is the floor. Full
    multi-phase program in `docs/V3_UX_OVERHAUL_PLAN.md` (menu chrome +
    shortcuts help first). Phases 0–6 and the full editing-ergonomics bundle (P5.4)
    are complete (auto-pairs, smart lists, table cell nav, smart paste, and heading cycles).
15. **Markdown live-preview quality:** stabilized against overlap and missing
    environment rendering, but heading scale, polished tables, interactive
    checkboxes, rich hit-testing, proportional-font shaping, and golden
    draw-command coverage remain. Historical detail in
    `docs/V3_STABILIZATION_2026-06-12.md`; the work is now **specced
    step-by-step as impl-plan Phase 6.3, then Phase 7** — work from that
    spec, not from the report's loose list.
16. ~~**Course correction (architect review, 2026-06-12):**~~ complete.
    impl-plan Phase 6, in order: green-and-committed tree (6.0), v3 size
    budgets in CI (6.1), `gui/mod.rs`/`buffer.rs` decomposition (6.2),
    renderer golden draw-plan snapshots (6.3). No new feature work on the
    renderer or shell before 6.0–6.3.
17. **Typora-grade live editor (user-ordered 2026-06-13):** the live
    markdown editor's rendered artifacts are clanky; the editing experience
    must be comparable to Typora. Specced as **impl-plan Phase 7**
    (7.0 shaped measurement → 7.1 conceal v2 → 7.2 typography → 7.3 block
    rendering → 7.4 stable assets → 7.5 interaction exactness → 7.6
    motion/refinements), governed by ADR-0104 (proposed, spike resolves)
    and ADR-0105 (accepted). Supersedes impl-plan 6.4/6.5.
18. ~~**Course correction Phase 6 execution (2026-06-13):**~~ complete.
    `gui/mod.rs` is 1,479 lines and `editor/src/buffer.rs` is 833; Phase 6.3
    goldens are green.

## Decisions made during execution

(append-only log; newest last)

- 2026-06-10: v3 placed in-repo at `v3/` rather than a new repo — single-user project,
  one history, plan §8 "legacy reference" satisfied by directory boundary instead.
- 2026-06-10: **Overlay scope is a modal fence**, not just the innermost scope: while an
  overlay is open, resolution consults only Overlay then Global. Plain innermost-wins
  would let unbound chords fall through to the editor underneath — reintroducing the
  BUG-A failure shape from the other direction. Test-pinned in bug_a suite.
- 2026-06-10: kernel keymap-file parsing (user remap JSON) deliberately left to the
  shell; kernel exposes `Keymap::apply_override` so the kernel stays serde-free.
- 2026-06-10: `HeightTree` is an implicit treap (not a Fenwick tree) because lines are
  inserted/removed, not just updated; priorities from a seeded xorshift so tree shape is
  deterministic and tests are reproducible.
- 2026-06-10: tile cache keeps a single over-budget tile rather than evicting it
  (something must be displayable); eviction reports keys so the shell owns pixmap drops.
- 2026-06-10: `Mods` is four bools, not bitflags — avoids a dependency; revisit only if
  chord matching shows up in profiles (it won't).
- 2026-06-10: buffer→layout bridge is a single `ChangedSpan` (first changed line +
  old/new line counts), not per-edit line deltas: undo replays ops in descending offset
  order, so per-edit line indices are only valid against intermediate rope states — a
  consumer patching from the final state would mis-index. Span extremes (min first line,
  min tail-below) are final-state-valid in both replay orders; property-tested in
  lockstep with `LayoutEngine::splice`.
- 2026-06-10: line-count effects of edits are *measured* from the rope after mutation,
  never predicted from the edited text: ropey treats lone `\r` as a line break, so edits
  adjacent to one can merge/split CRLF and break textual prediction (caught by the
  property suite as an underflow).
- 2026-06-10: every selection endpoint is snapped to a grapheme boundary inside the
  buffer (`replace_selections` / `line_col_to_offset`) rather than trusting callers —
  raw char cols from hit testing can split an emoji cluster (caught by property suite).
- 2026-06-10: undo coalescing = insert-runs only (same caret, uniform whitespace-ness,
  no newline); whitespace breaks the run so "hello world" is two undo steps. Deletes
  never coalesce — backspace granularity is per cluster, cheap to revisit.
- 2026-06-10: parser line-0 front-matter rule is encoded as a `BlockState::DocStart`
  *entry state*, not an index special-case: convergence compares entry states, so
  classification must stay pure on `(text, entry)` — an index-dependent rule let a line
  moving to/from line 0 converge prematurely with a stale parse (caught in test design).
- 2026-06-10: `Styler::style` now takes the block entry state — the plan's full style
  key (text, block state, conceal). `StyledLine` carries `LineKind` + `Span`s (char
  offsets); conceal stays paint-only (reserved width), so `MarkdownStyler` is
  layout-stable by construction, not by test luck.
- 2026-06-10: inline grammar is a pragmatic CommonMark subset (emphasis, code, inline
  math, links incl. one paren level, wikilinks, escapes); the invariant that *spans
  always tile the source line* is what paint correctness rests on, and is tested per
  kind. Conformance corpus tightening deferred to M3 per plan.
- 2026-06-11: pdfium wiring is a cargo **feature** (`pdfium`), not a separate crate —
  the pure tile logic always builds/tests; CI runs the feature too since
  `core/pdfium/libpdfium.so` is tracked in git. Tests skip (not fail) when no library
  binds, so contributor machines stay green.
- 2026-06-11: pdfium-render 0.9's `thread_safe` feature only makes handles Send+Sync;
  it does **not** serialize FFI calls — concurrent calls SIGSEGV/SIGTRAP (reproduced
  by the parallel test suite). v2 dodged this with a single worker thread; v3's
  `PdfRenderer` serializes every method through a process-wide mutex instead, so the
  engine is safe under any caller topology rather than by convention.
- 2026-06-11: tile rendering renders the whole page at bucket scale and slices the
  512 px tile out (correct-first); pdfium clip-rect rendering is the optimization
  path once profiles justify it (~4× zoom on very large pages).
- 2026-06-11: FTS queries are built by quoting each whitespace token (`"tok"*`) —
  FTS5 operators in user input (`NEAR(`, `AND`, unbalanced quotes) are inert by
  construction, test-pinned. Prefix matching falls out of the trailing `*`.
- 2026-06-11: index change detection is `(mtime_ns, size)`, not content hash — the
  plan says "mtime+hash diff" but hashing requires reading every file, which defeats
  the "cold start with no changes = no reindex" gate; `(mtime, size)` is what makes
  it cheap. Revisit only if a sync tool that preserves both while changing content
  shows up in practice.
- 2026-06-11: rename repair is split: `LinkGraph::rename_file` mutates the graph and
  *reports referrers*; `rewrite_links` is a pure text transform; the caller composes
  them with `atomic_save`. No fs access inside the link module keeps the
  "transactions" testable without a vault.
- 2026-06-11: the watcher **drops `Access` events** — consumers read files in
  response to a batch, and on inotify those reads emit open/close events, so
  forwarding them feeds the watcher its own echo: an infinite reindex loop
  (manifested as the vault test suite hanging; reproduced, fixed, suite re-run 3×).
- 2026-06-11: search semantics pinned: quoted tokens mean `AND`/`OR`/`NEAR` are
  *literal words* — `parsers AND` requires the word "and" in the body. The original
  test expected operator-stripping; the literal interpretation is the one the
  quoting design actually implements, and is now what the test asserts (with a
  positive case proving the literal match).
- 2026-06-11: PDF indexing goes through a `TextExtractor` trait *in vault* rather
  than a vault→pdf dependency — engines stay peers (plan §3 layering); the shell
  composes them. Failed extraction indexes an empty body so the `(mtime, size)` row
  lands and a corrupt file is not re-extracted every pass (call-count-tested).
- 2026-06-11: undo coalescing requires *uniform* inserted text — all whitespace
  or none. The old all-whitespace flag let a mixed insert (a paste like
  `" world"`) melt into the preceding typing run, so one undo destroyed both
  steps; caught when the buffer storm suite was ported to the final
  `Command`/`apply` API (it had never compiled against it). Pinned by
  `editing_after_undo_branches_instead_of_destroying_the_future`.
- 2026-06-11: the shell has **one** GUI: the `gui` module (canvas renderer on
  the 3-phase layout, PDF pane, vault-rooted overlays), promoted from the
  binary into the lib. The interim plain-text `app`/`keys` modules were
  deleted; their routing/normalization suites were ported, not dropped —
  `shell_key_routing.rs` now boots `gui::Shell` over a tempdir vault and
  opens files through the real quick-open flow. Two fixes fell out of the
  port: `editor.select-all` had no handler in `gui`, and `Character(" ")`
  normalized to `Key::Char(' ')` so `space` keymap bindings could never match.
- 2026-06-11: annotations live in **vault** (plan §3.4 lists annotations as
  vault-core), keyed by SHA-256 of the PDF bytes; `md3-vault` gains `sha2` +
  `serde`/`serde_json` (plan-mandated JSON export/import — hand-rolled JSON
  parsing was the worse option). Migrations are a per-component ladder
  (`migrations(component, version)`) so the FTS index can adopt the same
  table later; the shipped prefix is frozen by a fingerprint test — schema
  changes are new numbered entries, never edits. Edited bytes = a new
  document identity: old annotations stay reachable under the old hash
  (orphan-report material), re-binding across content edits is deliberately
  not guessed at.
- 2026-06-11: the sidecar is **one** SQLite file, `<vault>/.md3/sidecar.db`,
  shared by the FTS index and the annotation store — their tables are
  disjoint and the migration ladder is component-keyed precisely so they can
  cohabit; the dot-directory is invisible to every vault walk (index,
  quick-open, watcher) by the existing dot-skip rule. Index falls back to
  in-memory when the sidecar can't be created (read-only vault); annotations
  degrade to a status message instead — search has a useful degraded mode,
  silently-vanishing highlights do not.
- 2026-06-11: text-selection state lives in **page points anchored to one
  page**, not viewport px — scroll and zoom never invalidate a selection;
  the canvas projects quads per frame from `placed_pages` (same "stale
  viewport can under-render but never misdraw" property as tiles). The
  selection algorithm (line grouping, caret-between-glyphs, per-line union
  quads) is a pure md3-pdf module so its semantics are pinned by
  synthetic-grid tests, not by what pdfium happens to return.
- 2026-06-11: highlight interaction grammar: click picks the topmost
  (most-recent) annotation under the cursor; `ctrl+h` auto-picks the new
  highlight so `ctrl+n` (note) chains naturally; **delete** is raw input,
  not a keymap chord — it only means anything while a highlight is picked,
  and binding it globally in pdf scope would steal a key for a mode that
  rarely exists.
- 2026-06-11: `pdf.annotations-export` writes `<stem>-annotations.md` next
  to the PDF inside the vault (atomic save + targeted index re-sync) rather
  than a dialog — exports are vault citizens: searchable, linkable, plain
  files (plan pillar 5).
- 2026-06-11: the session snapshot is an **opaque JSON blob to the vault**
  (`SessionStore` stores a string) and a **serde type in the shell** — same
  split as keymap files: the vault owns durability and the single-sidecar
  guarantee, the shell owns every file format, the kernel stays serde-free.
  The shared migration runner moved to `vault::migrations` so annotations
  and sessions (and later the FTS schema) version independently in one
  `migrations` table.
- 2026-06-11: session save points are *event-edged* (open/close/split/
  tab-switch/save/quit/window-close), not debounced-continuous: scroll
  positions between two save points can be lost on kill-9, files cannot
  (crash journal is M3 scope). Window close goes through
  `exit_on_close_request: false` → save → `iced::exit()` so the X button is
  a save point too.
- 2026-06-11: restore *degrades, never refuses*: unknown snapshot fields are
  `#[serde(default)]`, vanished files are skipped, hollow splits collapse
  (`PaneTree::collapse_empty_panes`), an unparseable snapshot starts fresh
  with a status line. A saved session must never be able to brick startup.
- 2026-06-11: keymap-override rows resolve command *names* against the
  registry rather than minting new ids — `CommandId` stays `&'static str`,
  typos become warnings instead of dead bindings, and an override can only
  target a real command (palette/docs stay truthful).
- 2026-06-11: continuous-scroll geometry (`DocLayout`) lives in **md3-pdf**,
  not the shell — what's visible and which bucket-addressed tiles cover it is
  engine policy (mirrors the editor's layout-engine split); the shell only
  turns `PlacedTile`s into pixmaps and paint calls. `TILE_PX` moved to the
  always-built `tile.rs` so the pure half owns the grid constant. Painting
  reads geometry at the canvas's *real* bounds every frame (always correct);
  the session's stored viewport only steers which tiles get rendered, so a
  stale viewport can never misdraw, only under-render until the next event.
  Tile rendering is synchronous in `update` for now (512 px tiles are fast);
  the queue/cancellation semantics are already the engine's, so an async
  worker is a drop-in refinement, not a redesign.
- 2026-06-12: canvas overlays that must paint *over* `draw_image` content
  need their own render layer: iced_wgpu batches each layer as quads →
  meshes → images → text regardless of frame call order, so a
  `fill_rectangle` after `draw_image` in one frame still renders *under*
  the image. The shell's pattern is a `stack![]` of canvases (stack
  children after the first get `with_layer`); an inert overlay canvas
  (default `update`/`mouse_interaction`) lets all events fall through.
- 2026-06-12: `select::lines()` bands by *majority* vertical overlap
  (>50% of the smaller height), not any-overlap: pdfium loose boxes span
  full line height and overlap across tightly-leaded lines, which chained
  whole pages into one band (user-visible as selections painting on
  line 1). Synthetic fixtures had generous leading and missed it —
  regression test now uses a 14pt-box/12pt-step grid.
- 2026-06-12: pdf.find matches are *contiguous char-stream* matches:
  pdfium emits `\r\n` as control chars which `page_chars` drops, so a
  multi-word needle never matches across a line wrap. Acceptable for v1
  (single words dominate); whitespace-elastic matching is a pure-engine
  refinement when it itches.
- 2026-06-12: line banding is *vertical-center containment* (char center
  in band, or band center in char), not overlap ratios: real PDFs carry
  degenerate zero-height boxes for pdfium-synthesized spaces, and a
  ratio rule splits the line at each one — `position()` then resolved
  every same-row caret into the first segment, so same-line drags
  selected nothing (user-reported; multi-line drags worked, which was
  the tell). Caret resolution also tie-breaks vertically-equidistant
  bands by horizontal distance, which is what makes two-column rows
  resolve under the cursor.
- 2026-06-12: jump-history positions are stored in points (display px ÷
  zoom), not px or page indices: px goes stale across zoom changes and
  page indices lose the within-page offset; points re-anchor exactly at
  any zoom. History is per-session (not persisted in the snapshot) —
  a fresh run starts with empty history, deliberately.
- 2026-06-12: TOC depth is baked into the overlay rows as indentation at
  open time (a snapshot), while the session keeps raw `OutlineEntry`s —
  the status pill needs un-indented titles, and the overlay must not
  re-derive rows from a session that might change under it.
- 2026-06-12: pure paint plans (`tint_plan`, `page_plan`) decouple drawing coordinates from the toolkit canvas, making geometry assertions testable windowlessly.
- 2026-06-12: rotated pages selection audit: verified that bounds checks and whole-page drags for `/Rotate 90/180/270` pages are fully correct, meaning pdfium's post-rotation width/height dimensions align correctly with the character coordinate space and geometry flips.
- 2026-06-12: `pdf.find` cap: loading all glyphs synchronously can freeze large documents, so we cap eager load at 200 pages. This is a deliberate stopgap until the async tile/glyph worker is implemented (Phase 5.1).
- 2026-06-12: `LinkBox` is moved to `select.rs` and exported unconditionally from the `md3-pdf` engine so the shell crate builds successfully without the `pdfium` feature configuration.
- 2026-06-12: `PdfLinkPreview` overlay uses optional `input_mut()` returning `None` as it has no text input, and standardizes overlay input processing to prevent backspace/typing panics on non-input modals.
- 2026-06-12: PDF left-click link navigation runs at the top of `pdf_mouse_down` and navigates immediately if clicked inside a link bounding box, preventing selection or highlight picking on the same click.
- 2026-06-12: overlay hit lists render the *full* match set in one shared
  `scrollable` (stable widget id), not a `take(12)` window: the cap let ↓
  walk the selection past what the view showed while confirm resolved
  against the full list — enter picked rows the user never saw
  (user-reported on the TOC). Selection is clamped against
  `list_rows().len()` on every overlay keystroke (typing that narrows the
  list pulls the selection back in), and keyboard navigation issues
  `operation::snap_to` with relative offset `sel/(n−1)`, which keeps the
  selected row fully in view for any viewport ≥ one row tall.
- 2026-06-12: GUI/UX direction (user, 2026-06-12): the app reads as a
  terminal-style keyboard tool; v2's mouse-first GUI is the bar. The
  step-by-step program is `docs/V3_UX_OVERHAUL_PLAN.md` — next agents
  execute it phase by phase (menu chrome and a shortcuts/help
  surface first, since nothing is discoverable without knowing chords).
- 2026-06-12: `find` whitespace elasticity allows the *empty* gap on
  purpose: pdfium drops `\r\n` from the char stream, so the wrap the user
  reads as a space is zero characters wide — requiring ≥1 whitespace char
  would keep cross-wrap matches impossible, which was the whole bug.
  Greedy gap-skipping is safe because segments start with non-whitespace.
- 2026-06-12: the backlinks graph is built **fresh on every invocation**
  (read all vault notes), not cached: it mirrors quick-open's rescan
  philosophy — always correct, vault-sized cost, zero invalidation logic.
  A cached graph fed by watcher batches is the upgrade path when a real
  vault makes ctrl+shift+b feel slow, and `links.rs` already supports it.
- 2026-06-12: highlight colors are a fixed 4-entry cycle
  (`HIGHLIGHT_PALETTE`), not a color picker: one command, no new overlay,
  and entry 0 stays the historical default so existing annotations are
  already "on the cycle". The orphan report hashes every vault PDF on
  demand (same freshness-over-caching trade as backlinks; it's a
  palette-invoked report, not a hot path).
- 2026-06-12: `pdf.annotation-link-note` reuses an existing `linked_note`
  path when the annotation has one (open, don't re-create), and the
  sibling note is a vault citizen like the annotations export — created
  via `atomic_save`, indexed immediately, named `<stem>-notes.md`.
- 2026-06-12: one async PDF worker owns FIFO tile/glyph/link jobs. More
  workers add no throughput because pdfium is process-serialized; unbounded
  submission keeps the UI thread below the 16 ms input budget. Results route
  by absolute path, so closed-document results are harmlessly dropped.
- 2026-06-12: UX status is two fields: command/error messages on the left,
  document position on the right. `sync_status` can no longer erase
  guidance, closing pitfall P7 structurally.
- 2026-06-12: menu chrome is in-house, not `iced_aw`: five small anchored
  dropdowns need no dependency. Opening a menu enters the kernel's existing
  overlay scope, so Escape works through `overlay.close` and unbound editor
  shortcuts cannot leak through the menu.
- 2026-06-12: mouse discoverability is a CI invariant. Every registered
  command must appear in `menu_model` or a context-local control model; only
  `overlay.close` and `overlay.confirm` are exempt because they operate
  inside already-open modal surfaces.
- 2026-06-12: file rename keeps `DocumentId` stable. Kernel path lookup,
  markdown/PDF session paths, and snapshot paths move together; panes and
  shared editor state do not churn merely because a vault-relative name
  changed.
- 2026-06-12: vault selection uses `rfd` native folder dialogs only after an
  explicit Open/Create click. No/invalid startup arguments show the app-owned
  welcome; `vault.open` records recent paths and relaunches the selected vault.
- 2026-06-12: split dividers address nodes by root-relative boolean paths,
  not pane ids. Pane ids identify leaves; a divider belongs to an internal
  node. Paths serialize no new state and remain valid for the current layout
  frame; ratio writes retain the kernel's 0.05–0.95 clamp.
- 2026-06-12: global icon toolbar removed by user direction; menu remains the
  global command surface. Icons move to context-local controls, notably the
  bottom-floating PDF bar.
- 2026-06-12: rendered markdown assets are layout inputs, not paint-only
  decorations. Image/math dimensions enter `MonoMeasurer`, then
  `EditorDocument::remeasure()` updates height-tree offsets before paint.
  This restores the plan's style → measure → paint contract and prevents
  following lines from overlapping variable-height content.
- 2026-06-12: a fenced display-math block is rendered as one TeX unit.
  Session code groups parser-classified `MathContent` lines and stores the
  rendered asset on the first content line; continuation lines paint
  nothing. This supports Ratex environments without moving parsing rules
  into renderer.
- 2026-06-12: iced shell uses Tokio executor (ADR-0103). Iced's default
  thread pool cannot poll `tokio::time::sleep`; enabling iced `tokio`
  feature fixes toast auto-dismiss panic without a manually-owned runtime.
- 2026-06-12 (architect review — course correction): execution drifted from
  the plan's quality bar while UX phases 1–6 landed: `gui/mod.rs` reached
  4 838 lines (the god-file disease plan §1 cites as the reason v3 exists;
  v2's reducer was 2 400), a known-red test was handed off with prose instead
  of a fix, renderer geometry/hit-testing shipped "approximate" (violating
  the §3.2 measure-phase contract), ADR-0102 was written after its code, and
  theme state landed in a global atomic. Features are kept; the *practices*
  are corrected: new binding rules in impl-plan §0.2 (binary gate, size
  ratchets with frozen ceilings, measure-phase ownership, ADR-before-code,
  truthful handoff), pitfalls P13/P14 added, and remediation specced as
  impl-plan Phase 6. Renderer/shell feature work is frozen until 6.0–6.3
  are done.
- 2026-06-12: feedback-channel contract pinned (P14): transient command
  outcomes are **toasts**; the status-left segment is lightweight inline echo
  only; position pill stays right (P7). Consequence: the red
  `tracker_manual_log_and_delete` is fixed by making the manual-log handler
  toast once and asserting `Shell::toasts()` in the test (Phase 6.0) — not
  by restoring the status write.
- 2026-06-12: v3 adopts v2's size-ratchet practice (impl-plan Phase 6.1):
  `v3/budgets.toml` + `scripts/v3-budget.sh` in CI; frozen ceilings
  `gui/mod.rs` 4 838, `editor/buffer.rs` 1 911, `tracker_view.rs` 1 218,
  `editor_canvas.rs` 755; everything else hard-capped at 700. Ceilings only
  go down.
- 2026-06-12: document canon table added (impl-plan §0.2.1). Dated reports
  (`V3_STABILIZATION_*.md`) are historical records — work is always specced
  in the implementation plan, never executed from a report's bullet list.
  No new top-level docs without explicit user instruction.
- 2026-06-13 (user order): the live markdown editor must reach a
  **Typora-grade editing experience** — the rendered artifacts are clanky.
  Architect's diagnosis: two founding renderer contracts are structurally
  un-Typora and are re-decided by ADR rather than patched around: the
  monospace measuring grid (`MonoMeasurer`/`wrap_columns` — ADR-0104,
  shaped text measurement behind the existing `Measurer` seam) and
  reserved-width conceal ("columns advance, pixels don't" — ADR-0105,
  conceal becomes a measure input with reveal through the normal
  remeasure/damage path). The layout *protocol* (3 phases, height
  sum-tree, damage) is explicitly unchanged — it was always the real
  Bug-B kill; reserved width was a tactic. Program: impl-plan Phase 7;
  master plan §3.2 carries the supersession note.
- 2026-06-13: impl-plan Phases 6.4/6.5 superseded by 7.2/7.5 — typographic
  hierarchy and exact hit-testing will not be built twice (once on the mono
  grid, again on shaped text). Sequencing stands: 6.0–6.3 first (the 6.3
  golden corpus is the safety net for the Phase 7 rebuild), then 7.0.
- 2026-06-13: BUG-B gate v2 defined (with ADR-0105): the regression
  contract moves from "caret motion must not reflow" to "offsets never
  stale, content never overlaps, styled-damage ≤ 2 lines with correct
  shift" — asserted differentially against from-scratch layout plus a
  seeded caret-motion storm. `Styler::layout_stable()` retires in favor of
  a remeasure-before-paint invariant.
- 2026-06-13: tracker manual-log uses a dedicated toast-only helper. Existing
  `show_toast` callers still mirror messages into status because several
  established command contracts and tests depend on that behavior; migrating
  those channels is separate work, not folded into the Phase 6.0 test fix.
- 2026-06-13: Phase 6.1's complete file scan found two additional modules
  already above the 700-line hard limit: `kernel/src/pane.rs` (725) and
  `pdf/src/render.rs` (704). They receive frozen ceilings alongside the four
  review-listed files; omission from the review list was documentation drift,
  not permission to bypass the ratchet.
- 2026-06-13: Phase 6.2 uses split inherent `impl Shell` blocks and one
  extraction per commit. Shell behavior moved into `commands_file.rs`,
  `commands_md.rs`, `commands_pdf_annotations.rs`, `commands_pdf_nav.rs`,
  `pdf_input.rs`, `pdf_worker_events.rs`, `input.rs`, `stores.rs`,
  `session_persist.rs`, `status.rs`, `toast.rs`, `chrome_context.rs`, and
  `chrome_panels.rs`. This is mechanical ownership separation; public command
  routing and the single `Shell::on_key` path are unchanged.
- 2026-06-13: buffer formatting mutations moved to
  `editor/src/buffer/formatting.rs`, still reachable only through
  `Buffer::apply(Command)`. The extraction consolidated symmetric and
  asymmetric marker wrapping behind one private helper; the full editor
  property/undo/formatting suite pins equivalence. Current ratchets are
  `gui/mod.rs` 2,587 and `buffer.rs` 1,557; ceilings only decrease.
- 2026-06-13: Phase 6.2 exit gates reached. `gui/mod.rs` is 1,479 lines,
  `buffer.rs` is 833, and new extraction modules stay below 700 lines.
- 2026-06-13: Markdown asset dimensions use the shared sidecar under the
  new append-only `asset_sizes` migration component. Cached dimensions enter
  measure before async rendering; identical worker dimensions install pixels
  without reflow, changed dimensions trigger one normal remeasure.
- 2026-06-13: shaped geometry is the sole Markdown caret/selection/hit-test
  source. Concealed display offsets map through the editor engine back to
  source offsets; tables use measured cell columns. Ctrl-click follows links,
  while checkbox clicks dispatch the registered toggle command.
- 2026-06-13: tracker integration tests receive an explicit per-test database
  path. Production still uses the application config database; tests no
  longer race through shared global state during the binary workspace gate.
- 2026-06-13: ADR-0106 assigns fenced-code syntax tokenization to
  `v3/editor` and theme-color mapping to shell. Tokens are incremental cached
  document state and paint-only; renderer parsing and geometry changes are
  forbidden. Implementation remains a separate Phase 7.6 slice.
- 2026-06-13: shaped caret geometry resolves logical clusters independently
  of visual glyph order, follows each glyph's bidi level, and skips
  zero-width duplicate records emitted by shaping. Mixed Latin, Hebrew,
  numeric, and Arabic caret-to-hit round trips are pinned.
- 2026-06-13: Markdown measure/paint font families now agree: prose,
  headings, links, and emphasis paint with the same sans attributes used by
  shaping; code and math remain monospace. Cached inline-math dimensions now
  reserve their rendered width during shaping, so following text wraps and
  hit-tests after the asset instead of painting underneath it.
- 2026-06-13: inline-math vertical alignment fixed (user-reported: math drawn
  ~7px above the text row, occasionally overlapping a neighbour). The asset
  was centered against a `line_height`-tall box topped at
  `run.line_y - line_height`, but prose paints at `run.line_y - font_size`
  (cosmic's visual line top). The two references disagreed by
  `line_height - font_size` (8px). Inline math now centers within the same
  `font_size` box the surrounding text uses, so the rendered glyph rides the
  text row. Paint-only change — measure, shaping, wrap width reservation, and
  caret/hit geometry are untouched; golden draw plan re-pinned (the single
  changed line moved the `E = mc^2` asset 696→700, centering it on text at
  703). Block math/images still center in their own full-height lines.
- 2026-06-13: table-cell text vertical alignment fixed (same bug family as
  inline math). Cell glyphs were painted at `run.line_y + glyph.y` (the
  baseline used as the iced Top-aligned position), placing text ~font_size too
  low so it overflowed the cell box into the next row. Now centered within the
  full cell box using prose's `run.line_y - font_size` top reference
  (`y + (line_height - metrics.line_height)/2 + run.line_y - font_size`).
  Paint-only; golden re-pinned (cell text 426.2→412.2, 498.2→484.2). Pinned by
  `table_cell_text_stays_within_its_cell_box` (asserts every cell glyph's
  vertical extent stays inside its StrokeRect).
- 2026-06-13: p95 keypress→layout bench added (`shell/tests/keypress_bench.rs`,
  impl-plan Phase 7.6 / master plan §6). It drives the real shell keystroke
  cycle (`MdSession::apply`: incremental parse + restyle + shaped remeasure of
  the touched range) over a 5k-line document, 1k insert+motion pairs after a
  warm-up, and asserts p95 < 16ms (one 60fps frame). The budget is deliberately
  generous — it guards against an order-of-magnitude regression (a full
  reparse/remeasure creeping onto the keystroke path), not micro-cost; ~0.4ms
  locally in debug. Runs inside the existing `cargo test --workspace` CI step,
  no new job or dependency.
- 2026-06-13 (scoping note for the next session): fenced-code syntax
  highlighting (ADR-0106) is the last substantive 7.6 slice and is **bigger
  than a paint tweak** — it is a parser-core change. CodeContent lines must
  know their fence language: carry `lang` in `BlockState::Fence` (only 4
  non-test match sites: `parse.rs` x2, `style.rs` x1, `document.rs` x1, plus
  test updates). The hard part is **multi-line constructs** (block comments,
  multi-line strings): to keep the parser's forward-convergence correct, the
  lexer's cross-line state must live in `BlockState::Fence` too, so a line
  whose entry lexer-state changed re-tokenizes — this is exactly the
  convergence contract the ADR's "invalidation continues until lexer state
  converges" test requires. Plan: add a `SpanKind` syntax-role variant (e.g.
  `Syntax(SynRole)`), have the styler split a CodeContent line into role-
  tagged sub-spans for known langs (single `CodeContent` span fallback for
  unknown/empty), and map roles to colors in `paint.rs`/`shaped_measurer.rs`
  **using the same Monospace attrs** so shaping/measure/caret/hit geometry stay
  byte-identical (ADR-0106 geometry-invariance). Keep the lexer dependency-light
  per the ADR. Golden + a convergence test + a geometry-invariance test + a
  paint-visits-only-visible-lines budget test complete the verification
  contract.
