# V3 Handoff — execution state of docs/V3_GROUND_UP_PLAN.md

> **Read this first when resuming v3 work.** Updated after every completed unit of work.
> Sibling ledgers: `PLAN-NOTES.md` (v2 incremental plan), `docs/V3_GROUND_UP_PLAN.md` (the master plan).

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
| CI: v3 job in quality workflow | §6 | ✅ | `.github/workflows/quality.yml` `v3` job: fmt, clippy -D warnings, tests, demo, generated-doc freshness diff |

Statuses: ✅ done · 🔶 partial · ⬜ not started · ❌ blocked

**Verification snapshot (2026-06-11, post session-restore):** v3 — 189
tests green workspace-wide (204 with `--features pdfium`, incl. the fixture
corpus, the shell reading/annotation suites, and the new session-restore +
settings suites), clippy `-D warnings` clean in both feature configs, fmt
clean, `md3-shell --demo` all-ok, `V3_SHORTCUTS.md` freshness-checked.
v2 suite unaffected (root workspace excludes `v3/`).

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
11. Remaining M2/M3 surfaces, in rough value order: PDF search overlay
    (`pdf.find` routed but stubbed) + TOC with section tracking +
    back/forward history (plan §3.3); editing-ergonomics bundle (plan §3.2 /
    M3); link graph UI (backlinks panel — the vault service exists);
    annotation niceties from item 7's leftovers (colors, linked notes,
    orphan report, copy-selection).

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
