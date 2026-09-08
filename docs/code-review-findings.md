# Code Review Findings

**Date:** 2026-07-05
**Scope:** Full workspace — `core` (~5.9k lines) and `native` (~17k lines), all source files read; `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and the `dump_refs` example run against real PDFs from the `study-tracker` vault (`Y1/papers`, `Y1/books`).

**How to read this:** Each finding lists the file/line, what happens, and a suggested fix. Severity: 🔴 crash/data loss, 🟠 wrong behavior, 🟡 quality/perf, ⚪ cosmetic.

**Summary of verification performed:**

| Check | Result |
|---|---|
| `cargo test --workspace` | **2 failures** (Windows path-separator bug, see B2) — 70 native + 51 core tests otherwise pass |
| `cargo clippy --workspace --all-targets` | 132 warnings (≈128 in `native`) |
| Byte-slicing panic repro (B1) | **Confirmed** — panics with "byte index 5 is not a char boundary" |
| `dump_refs` on `1412.6980.pdf` (Adam paper) and `entropy.pdf` (Shannon) | Works; false positives observed (see H1) |

---

## 1. Bugs

### B1. 🔴 Panic: pressing Enter on a list line with multibyte characters crashes the app — **confirmed by repro**

[buffer.rs:848](../native/src/editor/buffer.rs:848) and [buffer.rs:905](../native/src/editor/buffer.rs:905)

`parse_list_item` slices the line by **byte** offsets that are only valid for ASCII:

```rust
let has_space_after = rest.len() >= 6 && &rest[5..6] == " ";     // checklist branch
let has_space_after = ... && &rest[dot + 1..dot + 2] == " ";     // ordered-list branch
```

If the byte at index 5 (or `dot+1`) falls inside a multibyte character, string slicing panics. Since `insert_text("\n")` calls `parse_list_item` on the current line, **typing Enter at the end of a line such as `- [xλ` or `1.λ` crashes the whole editor** with unsaved work lost.

Verified with a standalone repro of the exact expression:

```
thread 'main' panicked: start byte index 5 is not a char boundary; it is inside 'λ' (bytes 4..6)
```

**Fix:** operate on `char_indices()`/`chars()` throughout `parse_list_item`, or guard every slice with `rest.is_char_boundary(i)`. The same file mixes char-based (rope) and byte-based (`&rest[..]`) indexing — worth a one-pass audit of that function.

### B2. 🟠 Windows: relative wikilinks resolve to backslash paths — **confirmed by 2 failing tests**

[app.rs:3325](../native/src/app.rs:3325) `normalize_path`

`cargo test` fails on Windows:

```
app::tests::test_resolve_relative_link_path
  left: "science\\chemistry"   right: "science/chemistry"
app::tests::test_resolve_relative_link_path_with_vault
  left: "subdir\\another_file" right: "subdir/another_file"
```

`normalize_path` rebuilds the path via `PathBuf` and returns `to_string_lossy()`, which uses `\` on Windows. Everywhere else the app uses forward-slash vault-relative strings (`path_to_relative_string` in core explicitly does `.replace('\\', "/")`). Consequences on Windows when following a `./` or `../` link:

- `self.pdf.active_path == Some(&resolved_pdf_path)` comparisons miss, so an already-open PDF is re-opened from scratch.
- Sidebar `selected_path` never matches the tree entries (which use `/`), so the selection highlight is lost.
- The FTS index and backlinks map are keyed by `/`-paths; saving under a `\`-path leaves a stale index row and a duplicate entry.

**Fix:** end `normalize_path` with `.replace('\\', "/")` (same as core). The two failing tests then pass.

### B3. 🟠 Race: opening a PDF via an annotation/page link can drop the document load

[app.rs:1221](../native/src/app.rs:1221) (`PdfDocumentIdComputed`) vs [app.rs:2838](../native/src/app.rs:2838) (`navigate_pdf_page`)

`open_pdf` spawns four concurrent tasks tagged with `render_generation` G: hash, `page_count` → `PdfLoaded(G, …)`, `page_sizes`, `get_toc`. When the PDF was opened with a target page/annotation (`pdf://…?page=N` link, backlink click), the hash completing triggers `navigate_pdf_page`, which **unconditionally bumps `render_generation` to G+1**. Any of the other three results still in flight then arrive with a stale G and are discarded:

- `PdfLoaded` dropped → `total_pages` stays 0 → no pages are ever rendered and no error is shown.
- `PdfTocLoaded` dropped → the TOC panel stays empty for this document.

Hashing (1 MiB read + SHA-256) frequently beats pdfium's document load, so this is not theoretical — it depends only on which async task wins.

**Fix:** `navigate_pdf_page` should not bump the generation (it doesn't change what document is being rendered), or the initial-target navigation should be deferred until `PdfLoaded` has been applied.

### B4. 🟠 PDF annotations are silently orphaned when the file's mtime changes

[pdf.rs:121](../core/src/pdf.rs:121) `compute_provisional_id`

The document id — the key all annotations are stored under — hashes the first 1 MiB **plus file size plus mtime**. Copying the vault to a new machine, restoring from backup, `touch`ing the file, or re-downloading the identical PDF changes the mtime, producing a new document id. All existing highlights/notes remain in SQLite under the old id but never load again. Nothing migrates or warns.

**Fix options:** drop mtime from the hash (content prefix + length is already a strong key), or on miss look up `pdf_documents` by `vault_relative_path` and re-key the annotations to the new id.

### B5. 🟠 Renaming a `.markdown` file does not rewrite its backlinks

[vault.rs:279](../core/src/vault.rs:279)

```rust
let is_md_file = abs_old.is_file() && abs_old.extension().map_or(false, |e| e == "md");
```

Everywhere else (indexing, `sync_path_from_disk`, open) both `md` and `markdown` are treated as markdown, so `.markdown` notes *do* get backlinks — but renaming one skips the wikilink-rewrite pass and its links break silently.

### B6. 🟠 Linux desktop entry claims to open files, but the app ignores file arguments

[main.rs:78](../native/src/main.rs:78) and [main.rs:189](../native/src/main.rs:189)

The installed `.desktop` file declares `Exec={} %F` and `MimeType=text/markdown;application/pdf;`, so the OS will offer MD Editor as a handler and pass a file path. `parse_cli_args` classifies any non-flag argument as `CliAction::RunApp` and the path is never used — double-clicking a markdown/PDF file just opens the last vault. Additionally, the whole CLI parser is inside a `#[cfg(target_os = "linux")]` call site, so on Windows/macOS `parse_cli_args` and `CliAction` are dead code (clippy confirms: *"function `parse_cli_args` is never used"*).

### B7. 🟡 TOC and anchor lookup treat `#` comments inside code blocks as headings

[toc.rs:13](../native/src/views/toc.rs:13) `get_toc` and [app.rs:3438](../native/src/app.rs:3438) `find_heading_line`

Both scan raw lines for a leading `#` without tracking fenced code blocks. A markdown note containing a bash/python snippet (e.g. `# install deps` inside ```` ``` ````) shows those comments in the Table of Contents, and `[[note#anchor]]` links can jump to a code comment instead of the real heading. The highlighter already knows which lines are code (`StyledLine::is_code_block`) — `get_toc` could consume `highlighted_lines` instead of re-parsing text.

### B8. 🟡 Case-insensitive search can report wrong match columns for some Unicode

[search.rs:39](../native/src/search.rs:39) `line_matches`

The non-regex path lowercases the haystack and computes `start_col`/`end_col` against the **lowercased** char sequence. For characters whose lowercase form has a different char count (e.g. `İ` → `i̇`, 1 char → 2 chars), the reported columns drift from the original line, so the selection/highlight lands on the wrong characters. Low frequency, but the fix is cheap: match per-char with `char::to_lowercase().eq(...)` comparison or record indices against the original string.

### B9. 🟡 Failed file opens are silent

[app.rs:2248](../native/src/app.rs:2248) `open_file_extended`

Both the read failure and the invalid-UTF-8 case fall through `if let Ok(...)` and return `Task::none()` — clicking a file that can't be opened (permissions, non-UTF-8 encoding such as UTF-16 or Latin-1 notes) does nothing, with no toast. Same pattern in `EditorSave` ([app.rs:588](../native/src/app.rs:588)): `let _ = save_file(...)` then unconditionally shows "File saved" and clears `dirty` **even when the save failed** — that one can lose the dirty flag on a file that was never written.

### B10. 🟡 Row errors silently swallowed in DB reads

[tracker.rs:58](../core/src/tracker.rs:58) and [vault.rs:421](../core/src/vault.rs:421) use `if let Ok(r) = row { … }` inside the result loop, so a malformed row is skipped without any log. Clippy also flags these (*"unnecessary `if let` since only the `Ok` variant … is used"*). At minimum `eprintln!` the error; the tracker one can hide genuine data corruption.

### B13. 🟠 Rendered math is blurry with per-axis asymmetry (user-reported: `+` has one bright and one dim stroke)

[editor_state.rs:240](../native/src/editor_state.rs:240) `render_latex_task` and [renderer.rs:1940–2055](../native/src/editor/renderer.rs:1940)

Three compounding resampling problems:

1. **Non-integer display ratio.** LaTeX is rasterized at a hard-coded `device_pixel_ratio: 2.0`, cached at logical size `w/2, h/2`, then block math is drawn at `draw_w = w * 1.2` — i.e. the bitmap is displayed at **0.6× its pixel size** (0.75× at 125% Windows scaling, 0.9× at 150%). Bilinear sampling at such ratios renders 1-px glyph stems alternately crisp (on a pixel center) or split across two dim pixels.
2. **Fractional draw coordinates with different sub-pixel phase per axis.** X comes from `(block_max_w - draw_w) / 2.0` centering; Y from `line_draw_y + (lh - draw_h) / 2.0` plus an accumulated f32 sum of line heights. X and Y phases differ, so the horizontal and vertical strokes of the same `+` sample differently — exactly the reported bright/dim asymmetry.
3. **DPR ignores the real display scale factor** — unlike the PDF pane, which supersamples by `ui.scale_factor` and re-renders on `WindowRescaled`.

**Fix:** (a) bake the 1.2 block scale into the rasterization font size instead of scaling the bitmap; (b) rasterize at `device_pixel_ratio = ui.scale_factor` and draw at `w_px / scale_factor` logical for a 1:1 device-pixel mapping; (c) snap the draw rect to the device-pixel grid — `(pos * scale_factor).round() / scale_factor` on both axes. (c) alone removes the asymmetry; (a)+(b) remove the overall softness. Note the math cache is keyed by TeX string only, so a dynamic DPR needs the scale factor in the cache key or a flush on `WindowRescaled` (the PDF pane already does the latter).

### B11. ⚪ Inconsistent tracker date formats in one column

[tracker_state.rs:98](../native/src/tracker_state.rs:98) stores timer sessions as `"%Y-%m-%d %H:%M"` while manual sessions ([tracker_state.rs:230](../native/src/tracker_state.rs:230)) store `"%Y-%m-%d"`. `ORDER BY date DESC` string-sorts these consistently by day, but same-day manual entries sort before timed ones and any future date parsing has to handle both shapes.

### B12. ⚪ Mixed backlinks can list the same note twice

[vault.rs:445](../core/src/vault.rs:445) `get_mixed_backlinks` — for a PDF, a note that both wikilinks the PDF *and* is linked from one of its annotations appears twice (once from the file index, once from the annotations query). Cosmetic duplication in the backlinks panel.

---

## 2. Performance

### P1. 🟡 PDF view builds a widget for every page on every frame

[pdf_viewer.rs:405](../native/src/views/pdf_viewer.rs:460) `view_continuous` iterates `pages` (the *full* page list) and pushes a container per page into the column each time `view()` runs — which in iced is after every message, including every scroll tick. For the 500–1000-page books in the study-tracker vault (`LADR4e.pdf`, `mml-book.pdf`, `convex_opti.pdf`) that is hundreds of container/layout nodes rebuilt per scroll event. The bitmap cache and eviction are already windowed; the widget tree isn't. Consider emitting one fixed-height spacer for each contiguous run of non-visible pages instead of a placeholder per page.

### P2. 🟡 Quadratic text slicing in PDF TOC/reference scanning

Several helpers extract line text with `text.chars().skip(start).take(n)`, which is O(page length) per call:

- [pdf.rs:1334](../core/src/pdf.rs:1334) `line_text` — called for **every line** in `synthesize_toc` (twice: once for the header-repeat census, once for candidates) and in `detect_toc_pages`/`extract_toc_entries`.
- [references.rs:554](../core/src/references.rs:554) `line_slice`, and `char_index_at_byte` (O(prefix) per token).
- [references.rs:485](../core/src/references.rs:485) `scan_keyword` lowercases the entire page text once per keyword — 6 keywords × 2 passes across every page.

For a dense 55-page paper this is milliseconds (measured: 67 ms scan / 2.9 ms resolve on `entropy.pdf`), but for the 800+ page textbooks the O(lines × page-chars) products multiply into seconds of background CPU on first open. Precomputing a `Vec<char>` (or byte-offset line table) per page makes all of these O(n).

### P3. 🟡 Synchronous image decoding on the UI thread

- [editor_state.rs:184](../native/src/editor_state.rs:184) `load_images` calls `image::open` inline for every image referenced by a freshly highlighted document — a note with several large PNGs freezes the UI while they decode.
- [app.rs:2381](../native/src/app.rs:2381) `open_image` likewise decodes fully before returning.

Move decoding into `Task::perform` like math rendering already does (`MathRendered` is the model to copy).

### P4. 🟡 Vault-wide FTS + PDF search run on every keystroke, synchronously

[app.rs:1068](../native/src/app.rs:1068) `SearchQueryChanged` calls `search_vault` (SQLite FTS, main thread) on each keystroke > 2 chars, and additionally kicks a full-document pdfium `search_text` per keystroke > 1 char when a PDF is open. The PDF search is at least async, but each keystroke queues another full scan on the single pdfium worker with no debounce or cancellation — typing a 10-char query scans the whole book ~9 times. A 150–250 ms debounce (the highlight pipeline already has one to copy) would remove nearly all of that work.

### P5. ⚪ Linear page-geometry walks per scroll event

[pdf_pane.rs:216–249](../native/src/pdf_pane.rs:216) `page_offset`, `total_height`, and `page_at_scroll` are O(total_pages) loops, called several times per scroll message. The editor already solves the identical problem with a Fenwick tree ([layout_tree.rs](../native/src/editor/layout_tree.rs)); reusing it for page heights would make these O(log n). Only noticeable on 1000+ page documents, so low priority.

### P6. ⚪ `rename_entry` re-acquires the index and DB locks per backlinking file

[vault.rs:306–335](../core/src/vault.rs:306) — for a heavily linked note the rewrite loop locks/unlocks `file_index` and `db` once per file and runs each FTS update in autocommit. Batch the DB updates in one transaction (the vault indexer already does this).

---

## 3. Non-conventional behavior and design choices

These are not bugs, but they will surprise a maintainer or user; each deserves either a doc note or a deliberate decision.

### H1. Reference-link heuristics produce visible false positives (verified on study-tracker PDFs)

`dump_refs` against `Y1/papers` shows the equation-reference resolver linking things that aren't references:

- `1412.6980.pdf`: the bibliography text "IEEE, 29**(6)**:82–97" is linked as a call-site of Equation 6; "30**(4)**:838–855" as Equation 4.
- `entropy.pdf`: the enumerated prose "**(1)** A dot, consisting of…" is linked as Equation 1; "(4), (5) and (6)" in prose likewise.

The design intent ("no target ⇒ no link", right-margin test) is sound, but any short `(n)` in prose links whenever equation *n* exists anywhere. Possible tightening: require the call-site to be preceded by cue words (`Eq`, `equation`, `in`, `by`, `from`, `see`) or reject call-sites inside reference-section pages. Also note `RESOLVER_VERSION` must be bumped when tuning ([references.rs:23](../core/src/references.rs:23)) — the cache design handles this correctly.

### H2. Settings database lives beside the executable

[state.rs:512](../core/src/state.rs:512) — portability is an explicit product decision (documented in the code), but it means: the DB (with all annotations and tracker history) sits in whatever folder the exe runs from, a writability probe file is created on every launch, and two installs of the app have two divergent databases. The study-tracker folder itself contains a stray `md_editor_settings.sqlite` + `md-editor.exe` copy, which is this behavior in action — annotations made with that copy are invisible to the copy in the repo's `target/`.

### H3. Global keyboard subscription claims keys app-wide

[app.rs:174–212](../native/src/app.rs:174) — every **Enter** press anywhere emits `NameModalSubmitCurrent`, every arrow/PageUp/PageDown press emits `PdfScrollBy`, and **Ctrl+C** emits `PdfCopySelection`. All are gated no-ops in the wrong context, but each keystroke in the editor still triggers an extra `update()` cycle, and the intent is invisible at the subscription site. A comment exists, but routing these through focus-aware widget events would be the conventional iced approach.

### H4. `Err("Skipped")` as a control-flow protocol

[pdf.rs:480](../core/src/pdf.rs:480) sends `Err("Skipped".to_string())` for out-of-viewport renders, and [app.rs:2431](../native/src/app.rs:2431) string-matches `err == "Skipped"` to distinguish it from real failure. A dedicated `enum RenderOutcome { Image(..), Skipped, Failed(String) }` would remove the stringly-typed contract. Similarly, the priority-render drain ([pdf.rs:374](../core/src/pdf.rs:374)) drops superseded requests by *dropping their response channel*, which the caller sees as a `RecvError` and logs as `PDF PRIORITY RENDER ERROR` — a normal event reported through an error path (with a leftover `println!` at [app.rs:2467](../native/src/app.rs:2467)).

### H5. Hand-rolled markdown highlighter with deliberate gaps

[highlight.rs](../native/src/editor/highlight.rs) implements its own parser rather than pulling `pulldown-cmark`. Known deviations from CommonMark: no `_italic_`/`__bold__` underscore emphasis, no setext headings (`===`), headings without a space (`#Heading`) are accepted, no nested emphasis, no reference-style links, single-line table detection only. These are fine for a personal tool but should be listed in FEATURES.md so they read as scope, not bugs.

### H6. Anchor-vs-filename `#` heuristic duplicated in three places

The rule "an anchor containing `% ^ & * ! @ ( )` is actually part of the filename" appears in [file_index.rs:206](../core/src/file_index.rs:206), [app.rs:400](../native/src/app.rs:400), and [highlight.rs:935](../native/src/editor/highlight.rs:935). Any tweak must be made three times; extract one `split_link_anchor(target) -> (path, Option<anchor>)` helper in core.

### H7. Schema migration scaffold is a no-op

[state.rs:424](../core/src/state.rs:424) `apply_migrations` only writes `PRAGMA user_version = 1`. Fine as scaffolding, but note that the base DDL runs `CREATE TABLE IF NOT EXISTS`, so *changing* an existing table's columns silently won't apply to old databases until a real migration arm is added — easy to forget given the empty example.

### H8. FTS search is phrase-only by construction

[vault.rs:402](../core/src/vault.rs:402) wraps the whole query in double quotes, so FTS5 operators (`AND`, `OR`, `*` prefix) are intentionally disabled and multi-word queries only match as exact phrases. Users coming from Obsidian will expect implicit-AND term search. Also `SearchResult.line` is hard-coded to 1.

---

## 4. Unused / dead code

| Item | Location | Note |
|---|---|---|
| `dummy.rs` | [native/src/dummy.rs](../native/src/dummy.rs) | Not declared in `main.rs` module tree; contents wouldn't even compile (incomplete `Widget` impl). Delete. |
| `dummy_test.rs` | [native/src/dummy_test.rs](../native/src/dummy_test.rs) | Contains only `// just checking something`. Delete. |
| `dummy.pdf` | repo root | Used by core tests (`../dummy.pdf`) — keep, but consider moving under `tests-fixtures/` with the other fixtures. |
| `parse_cli_args`, `CliAction` | [main.rs:182](../native/src/main.rs:182) | Dead on Windows/macOS (clippy: "never used"); only the Linux `main` calls it. Gate or use on all platforms (see B6). |
| `pdf_slot_offset` / `pdf_slot_total_height` / `pdf_slot_page_at_scroll` | [app.rs:43–66](../native/src/app.rs:43) | `#[allow(dead_code)]`, referenced only by their own tests. The live logic moved to `PdfPane::page_offset` etc. — delete both the functions and the tests that test nothing shipped. |
| `PdfState` | [core/src/pdf.rs:253](../core/src/pdf.rs:253), [state.rs:18](../core/src/state.rs:18) | Constructed into `AppState.pdf_state` and never read or written anywhere (`grep pdf_state` finds only the constructors). The native `PdfPane` superseded it. Delete the field and struct. |
| `find_heading_line` fallback | [app.rs:3438](../native/src/app.rs:3438) | Still used (fallback inside `find_heading_or_widget_line`) but duplicates `views::toc::get_toc` logic — consolidate. |
| `slugify` | [app.rs:3421](../native/src/app.rs:3421) + [pdf_notes.rs:62](../native/src/pdf_notes.rs:62) | Two identical private copies. |
| `normalize_path` | [app.rs:3325](../native/src/app.rs:3325) | Third copy of the component-normalization loop (also in `core::file_index::resolve_wikilink_target` and `core::vault::normalize_within_root`). Move one version to core. |
| `_search_match_indices_by_page` | [pdf_viewer.rs:412](../native/src/views/pdf_viewer.rs:412) | Parameter threaded through and never used. |
| PDF worker "load current document" block | [core/src/pdf.rs](../core/src/pdf.rs) | The identical 10-line "load doc if path differs" block is copy-pasted **10 times** across the command match arms (a helper `ensure_document` already effectively exists inside `render_page_from_cache`). Not dead code, but the largest single cleanup win in core. |
| Clippy total | workspace | 132 warnings, dominated by 73× collapsible-`if`, 13× useless conversions, 12× simplifiable `map_or`, 10× redundant guards, 7× too-many-arguments (up to 18 params in `views::tracker::view` / `view_continuous`). `cargo clippy --fix` auto-applies 92 of them. |

---

## 5. Other observations

1. **Duplicated layout magic numbers disagree.** `SplitViewDragging` ([app.rs:1674](../native/src/app.rs:1674)) uses sidebar = 250, tracker = 300, TOC = 250; `pdf_available_width`/`estimated_editor_viewport_width` ([app.rs:2672](../native/src/app.rs:2672)) use 260 for sidebar/TOC/backlinks and ignore the tracker; the actual sidebar view renders at 250 and the TOC panel at 250. The drift makes fit-to-width slightly wrong whenever panels are open. Define shared `const` panel widths.
2. **`PdfPageSizesLoaded` staleness guard is `&&` where every other handler uses generation-only** ([pdf_pane.rs:116](../native/src/pdf_pane.rs:116)): a stale-generation result is accepted if the path still matches. Probably intentional (page sizes survive zoom-generation bumps) but it contradicts B3's strict checks — a comment or a consistent policy would prevent the next reader from "fixing" it either way.
3. **`open_file` (core) reindexes on every read** ([vault.rs:158](../core/src/vault.rs:158)) — opening a file mutates the link index under the read path; harmless today but surprising for a function named `open_file`, and it's why opening a PDF through this API returns a UTF-8 error rather than a type error.
4. **`GetToc`/`GetReferences` both do a full text scan for bookmark-less PDFs** — opening such a PDF scans every page twice on first open (once in `GetToc` on the worker, once in the chunked reference task). The chunked task already recovers the TOC from its own scan; the `GetToc` command could reuse the cached reference-scan text or vice versa.
5. **Build script downloads pdfium at compile time** ([core/build_pdfium.rs](../core/build_pdfium.rs)) — network access during `cargo build` is unconventional; offline builds fail until the artifact is cached in `target/`. Worth documenting in the README (CI already caches it).
6. **Test-only guards in production files** — core's pdfium tests serialize via a `TEST_LOCK` static in the shipping module. Fine, but the windowless-fixture test helpers in `pdf.rs` push the file to 2.8k lines; splitting tests into `pdf/tests.rs` would help navigability.

---

## 6. Prioritized fix list

| # | Finding | Effort |
|---|---|---|
| 1 | B1 — char-boundary panic in `parse_list_item` (crash, confirmed) | Small |
| 2 | B2 — `normalize_path` backslash paths (breaks Windows links, 2 failing tests) | Trivial |
| 3 | B9 — `EditorSave` reports success on failure | Trivial |
| 4 | B13 — blurry math rendering (pixel-grid snapping + real DPR) | Small–Medium |
| 5 | B3 — generation race dropping `PdfLoaded`/TOC on targeted PDF opens | Small |
| 6 | B4 — annotation orphaning on mtime change (silent data loss over time) | Medium |
| 7 | B5/B6/B7 — `.markdown` rename, CLI file args, code-block headings in TOC | Small each |
| 8 | P4 — debounce vault/PDF search per keystroke | Small |
| 9 | P1/P2 — windowed PDF widget tree; linear-time page text slicing | Medium |
| 10 | §4 — delete dead files (`dummy.rs`, `dummy_test.rs`, `PdfState`, slot helpers), dedupe `slugify`/`normalize_path`/anchor heuristic, factor `ensure_document` in the PDF worker | Medium |
| 11 | `cargo clippy --fix` + address the remaining ~40 by hand | Small |
