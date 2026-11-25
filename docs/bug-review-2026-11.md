# Bug Review and Fix Plan — 2026-11-25

**Scope.** User-reported problems (ink page split, Wacom pen, slow PDF loading) plus a
sweep of the ink, PDF, input-routing and logging paths. The method: read the code paths
end to end, read the real app log (`~/.local/state/md-editor/md-editor.log*`), and run
the full test suite (540 pass, 7 ignored, so every bug below gets past the current tests).
The editor save path, vault and rename flows were already covered by
`e2e-bug-discovery-plan.md` (B1–B13) and were not re-audited.

**Status (2026-11-25).** Fixed: LOG-1, LOG-2, INK-1 to INK-7, PEN-1 (Wayland, in
`vendor/winit`), PEN-3 (drag motion scoped to the main window; ink flushed on close),
PERF-1 (`app::blocking`), PERF-2 (two-lane worker), PERF-3 (one shared scan, outline cached
in `pdf_outline`), PERF-4, PDF-6. Not a bug: PERF-5 (the generation is bumped only when the
zoom crosses a render bucket, and no page renders before the first fit). Still open: PEN-2
needs re-checking on real use (no `TrySendError` in short runs since PERF-1); UI-1 needs a
repro; a drop on the ink window still opens in the shell (harmless). The pen needs a check
on real hardware.

**Severity.** S1 data loss · S2 feature broken · S3 degraded / slow · S4 minor.
**Confidence.** *Confirmed* = traced in code or seen in the log; *Likely* = strong
code evidence, needs a device repro to close.

---

## 1. Findings

### Ink (handwriting)

| ID | Sev | Finding | Root cause | Conf. |
|:--|:--|:--|:--|:--|
| INK-1 | S2 | **Page "splits" / strokes smear while writing near the bottom.** | `ink/input.rs::advance_if_near_bottom` pans by `threshold - screen_y` on *every* move sample. The nib doesn't move on screen when the camera pans, so each sample in the bottom 120 px pans again. The page runs away and successive samples land 60+ page units apart. It also clears the stroke cache every sample, so the whole page re-tessellates at pen rate. The test `writing_near_the_bottom_advances_the_page` checks only one sample. | Confirmed |
| INK-2 | S1 | **Opening a `.ink` page discards unsaved strokes.** | `app/ink.rs::open_ink_page` → `InkState::load` replaces the document without checking `dirty`. | Confirmed |
| INK-3 | S1 | **A second new page for the same note overwrites the first.** | `save_ink_page` derives `notes/x.md → notes/x.ink` when `path` is `None` and writes without checking whether that file exists. | Confirmed |
| INK-4 | S1 | **Unsaved ink is lost on app exit.** There is no autosave and no prompt. | `WindowCloseRequested` saves the note and the session but not `self.ink`. The wiki documents this as intended, but it breaks the project rule "never drop a dirty buffer". | Confirmed |
| INK-5 | S2 | **Lasso move cancelled mid-drag corrupts undo.** | `cancel_stroke` resets `drag_from`/`drag_total`, but the strokes were already moved with `translate_without_history`. The move is never recorded and `dirty` stays false. | Confirmed |
| INK-6 | S4 | Pen samples use the shell window's scale factor. | `InkMessage::PenSamples` passes `self.ui.scale_factor` (main window), while the hit test uses the ink window's factor. Wrong on mixed-DPI setups. Windows-only today. | Confirmed |
| INK-7 | S4 | Mouse drag stops extending once the cursor leaves the canvas. | `canvas.rs` `CursorMoved` returns early on `position_in(bounds)?` even while `state.drawing`. | Confirmed |

### Pen / Wacom on Linux

| ID | Sev | Finding | Root cause | Conf. |
|:--|:--|:--|:--|:--|
| PEN-1 | S2 | **No pen support on Linux at all.** You get no pressure, eraser end or barrel button (so no radial menu), and no hover. | `ink/pen.rs::install` returns `Err("only implemented on Windows")`, which is the first warning in every log. winit 0.30 has no Wayland `zwp_tablet_v2` support, and iced drops pressure anyway. The pen only works through the compositor's pointer emulation, as a fixed-pressure mouse. | Confirmed |
| PEN-2 | S2 | **Input events dropped under load**, which can lose a pen-up or button release (stuck or missing strokes). | Log: bursts of `iced_futures::subscription::tracker: Error sending event to subscription: TrySendError { kind: Full }`. iced drops events when a `listen_with` stream isn't polled. Leading cause: PERF-1, which blocks the tokio workers that poll subscriptions. | Likely |
| PEN-3 | S4 | Pointer-drag and drop subscriptions ignore the window id. | `subscription.rs::pointer_drag_subscription` and `window_events` (`FileDropped`, `CloseRequested`) discard `window_id`. Moving the pen in the ink window during a split drag moves the shell's split, and a file dropped on the ink window opens in the shell. This breaks the CLAUDE.md "scope window events" rule. | Confirmed |

### PDF loading and performance

| ID | Sev | Finding | Root cause | Conf. |
|:--|:--|:--|:--|:--|
| PERF-1 | S3 | **Blocking calls on the async executor.** | Every `PdfRenderer` method (`core/src/pdf/renderer.rs`) blocks on `std::sync::mpsc::Receiver::recv`, and the app calls them inside `Task::perform(async { … })` on iced's tokio runtime. Opening a PDF queues about 10 renders, 10 text loads, the TOC, sizes and a reference scan, and each one parks a worker thread. `load_pdf_references` holds a worker for an *N*-page loop. Once all workers block, subscriptions, timers and the vault watcher stall (see PEN-2). | Confirmed (code) |
| PERF-2 | S3 | **The first page waits behind full-document text extraction.** | The worker is a single FIFO. `open_pdf` batches `page_count`, `page_sizes` and `get_toc`. For a PDF without bookmarks, `get_toc` runs `build_all_page_texts` over every page. Renders are only requested after `Loaded` + `PageSizesLoaded` → `FitToWidth`, so they queue behind it. A large textbook shows a blank pane for seconds. | Confirmed (code) |
| PERF-3 | S3 | **Each page's text is extracted twice or three times per open, and nothing is cached across opens.** | `get_toc` extracts all pages, `load_pdf_references` extracts them all again page by page, and the visible pages get a third pass for selection. Only the references are cached (`put_pdf_references`); the synthesized TOC is recomputed on every open. | Confirmed |
| PERF-4 | S3 | `page_sizes` loads every page object. | `worker.rs::page_sizes` calls `doc.pages().get(i)` (`FPDF_LoadPage`) per page. pdfium-render 0.9.6 has `pages().page_sizes()`, which reads sizes without loading pages. | Confirmed |
| PERF-5 | S3 | The fit-to-width pass throws away in-flight renders. | `FitToWidth` bumps `render_generation`, so any page rendered at the provisional zoom is dropped and rendered again. | Likely |
| PDF-6 | S4 | **Spurious "Could not render PDF page N" toast** when navigating quickly. | `worker.rs::drain_priority` keeps only the newest priority render and drops the older `resp` senders. Those callers get `WorkerGone`, which becomes `RenderFailed` and shows a toast. A superseded request should map to `Skipped`. | Confirmed |
| UI-1 | S3 | Bursts of layout invalidation every frame. | Log: `iced_winit: More than 3 consecutive RedrawRequested events produced layout invalidation`, 29 times in about 150 ms. The widget that invalidates layout during redraw hasn't been found yet. | Needs repro |

### Logging and diagnosability

| ID | Sev | Finding | Root cause | Conf. |
|:--|:--|:--|:--|:--|
| LOG-1 | S3 | **Each launch writes about 85 MB of log** and evicts the previous session's log. That makes field bugs undiagnosable and wastes disk. | iced_winit logs `Window attributes` at INFO with the window icon's RGBA bytes as a `Debug` array (two windows, two dumps). `logging.rs` has no per-target filter and no per-line cap. A line bigger than `MAX_FILE_BYTES` triggers a rotation and is then written in full, so the 1 MB × 3 budget doesn't hold. | Confirmed |
| LOG-2 | S4 | Repeated warnings flood the log (the `TrySendError` and layout-invalidation lines). | No rate limiting or deduplication. | Confirmed |

### PDF UI (second pass, 2026-11-25)

Found with a new e2e scenario (`s8b_pdf_select_highlight_and_search`, which selects,
highlights, restarts, searches and switches documents) and a rendered-ink geometry test.
All are fixed.

| ID | Sev | Finding | Root cause |
|:--|:--|:--|:--|
| PDF-7 | S1 | **Highlights randomly impossible, and saved ones not shown.** | `on_pdf_document_id` was gated on `render_generation`, which the first fit-to-width bumps. When the hash arrived after the fit, the document never got an id. |
| PDF-8 | S2 | **Highlights, search hits and the selection drawn beside the text.** | Fit-to-width estimated the pane width without the Folio chrome, so pages were fitted wider than the pane. iced squeezed the bitmap while the overlay and hit testing used the full zoom. |
| PDF-9 | S2 | **Misplaced rects on rotated or cropped pages.** | Char, search and link rects were raw user space, not the displayed page box. |
| PDF-10 | S2 | **Text on the first pages sometimes unselectable.** | `PageTextLoaded` and `PageLinksLoaded` were gated on `render_generation`, so a fit landing mid-load dropped them, and they were never re-requested. |
| PDF-11 | S2 | **Search found nothing past about the 250th match.** | A silent cap of 250 results in `search_text`. |
| PDF-12 | S2 | **Search results dropped or stale.** | Results were gated on `render_generation` (dropped after a zoom change), with no guard against an older query finishing last; switching PDFs with find open showed no results for the new document. |
| PDF-13 | S3 | The first Next jumped to page 1, not the page being read. | `navigate_pdf_search` started at index 0. |
| PDF-14 | S4 | One Ctrl+C copied once per visible page. | Every page widget handled Ctrl+C on top of the global shortcut. |
| PDF-15 | S4 | No double-click word or triple-click line selection. | Not implemented. |
| PDF-16 | S4 | A highlight made while the document id was pending was silently dropped. | No feedback; it now shows a toast. |

Known limits, not fixed: a selection stays within one page, and highlights saved on
rotated or cropped pages *before* this fix keep their old, misplaced rects.

---

## 2. Fix Plan

Ordered by data safety, then what you hit every day, then root causes that unblock other
fixes. Every fix lands with a regression test that fails before the change. Headless where
possible: unit tests in `foo/tests.rs`, scenarios in `native/src/e2e/`.

### Phase 1 — Stop losing data (S, ~1 day)

1. **INK-2.** In `open_ink_page`, if `ink.dirty`, save to the current path first. If the page has no path yet, save it to a derived path. Test: dirty page, open another `.ink`, and the first page is on disk.
2. **INK-3.** On first save, pick a free path: `x.ink`, then `x-2.ink`, and so on (`derive_ink_path` plus an existence check under the vault root). Test: two new pages for one note make two files.
3. **INK-4.** Autosave ink the same way notes do: debounce about 1 s after `finish()` and also on window close, plus a flush in `WindowCloseRequested`. Update `wiki/Handwritten-Notes.md` §1 ("not autosaved"). e2e: draw, close the app, and the page is on disk.
4. **INK-5.** In `cancel_stroke`, if `drag_from` is set and `drag_total ≠ 0`, either record the move or translate the strokes back by `-drag_total`. Translating back is the better choice because cancel means cancel. Test: drag, cancel, and stroke positions and undo depth are unchanged.

### Phase 2 — Ink behaves (S, ~½ day)

5. **INK-1.** Make the advance edge-triggered. Pan once when a stroke *enters* the margin, by a fixed step (for example a third of the viewport), and don't pan again until the nib leaves the margin or a new stroke starts. Better still, advance on pen-up so a stroke never moves under the nib. Only clear the cache when the camera actually moved. Test: 50 move samples at the same `y` inside the margin pan at most once, and consecutive live points stay within the sample spacing.
6. **INK-7.** While `state.drawing`, use `cursor.position()` relative to `bounds` (unclamped) instead of `position_in`.
7. **INK-6.** Keep an `ink_scale_factor` from `on_window_rescaled` and pass it to `apply_samples`.

### Phase 3 — Unblock the executor and make PDFs open fast (M, ~2–3 days)

8. **PERF-1 (root cause).** Give `PdfRenderer` an async API: replies on `tokio::sync::oneshot`, awaited in the tasks, so no worker thread is ever parked. Core may depend on `tokio` with the `sync` feature only, which is not a GUI dependency. Alternatively, wrap each call in `tokio::task::spawn_blocking`. Oneshot is preferred because it also makes cancellation (dropping the receiver) observable to the worker. Turn `load_pdf_references` into a sequence of awaited per-page calls. Then re-check the log: the `TrySendError Full` bursts should be gone (verifies PEN-2).
9. **PERF-2.** Two-lane worker. Interactive commands (renders, page count, sizes, links, and the visible pages' text) go ahead of background work (TOC synthesis, the reference scan, search). Split background jobs into per-page steps so a render never waits more than one page of extraction. Issue the first visible pages' renders at `Loaded` time at a provisional fit zoom computed from `page_sizes`, instead of waiting for `FitToWidth`.
10. **PERF-3.** One page-text pass per document, shared: background extraction fills a per-document page-text cache that the TOC synthesizer, the reference resolver and selection all read. Persist the synthesized TOC in SQLite next to `pdf_references`, keyed by `document_id` and a synth version. That takes a new `user_version` migration in `state.rs`, per `core/CLAUDE.md`.
11. **PERF-4.** Use `doc.pages().page_sizes()`.
12. **PERF-5.** Don't bump the generation when only the fit zoom settles within the same render bucket. Otherwise, keep the provisional bitmaps on screen until replacements arrive.
13. **PDF-6.** Reply `Err(PdfError::Skipped)` to superseded priority requests, and route that to `RenderSkipped`.
14. **Budget test.** An e2e scenario opens a 300-page bookmark-less fixture PDF, asserts that page 0's bitmap arrives within a time budget (for example under 500 ms in CI), and asserts that a render enqueued after the TOC job finishes before it.

### Phase 4 — A real pen on Linux (M–L, needs a decision, see §3)

15. **PEN-3 first** (S): scope `pointer_drag_subscription`, `FileDropped` and `CloseRequested` to `main_window` / `ink_window`.
16. Implement Linux pen capture behind the existing platform-neutral `pen::emit` / `PenSample` interface, so nothing above `pen.rs` changes. The options are in §3.
17. Update `wiki/Handwritten-Notes.md` (platform support, input pipeline diagram) and `wiki/Repository-Structure.md` for the new module.

### Phase 5 — Diagnosability (S, ~½ day; can land first, since it helps everything else)

18. **LOG-1.** Filter by target in `logging.rs`: `iced*`, `wgpu*`, `naga`, `winit`, `cosmic_text` and `sctk` default to WARN, and `MD_EDITOR_LOG` accepts `target=level` pairs. Truncate any line over 8 KB with a `…[truncated N bytes]` marker. Rotate on startup only when the file is over budget. Test: a 100 MB line leaves the file under `MAX_FILE_BYTES`.
19. **LOG-2.** Collapse repeats: log the first occurrence of an identical warning, then `(repeated N×)` once per second.
20. **UI-1.** With LOG-1 in place, reproduce with `MD_EDITOR_LOG=debug`, find the widget that invalidates layout during redraw, and fix or cache it.

### Suggested order

Phase 5 (log hygiene) → Phase 1 → Phase 2 → Phase 3 → Phase 4. Phases 1–2 and 5 are
independent and small. Phase 3 is the biggest win for perceived speed and also clears the
dropped input events. Phase 4 waits on the decision below.

---

## 3. Decision Needed — Linux Pen Capture

Your session is Wayland (Hyprland). winit 0.30 exposes no tablet events, so pressure has
to come from somewhere else:

| Option | How | Pros | Cons |
|:--|:--|:--|:--|
| **A. Tablet protocol in winit** | Carry a patched winit (or upstream a patch) that binds `zwp_tablet_manager_v2` and surfaces tool, pressure, tilt and buttons per surface. A small iced hook forwards them to `pen::emit`. | This is the standard approach (GTK, Qt, Krita, Xournal++ all use tablet-v2). Coordinates are surface-local, and it needs no special permissions. | It's a fork to maintain until upstream lands; the most work. |
| **B. evdev side channel** | A thread reads the Wacom `/dev/input/event*` for `ABS_PRESSURE`, `BTN_TOUCH`, `BTN_STYLUS` and `BTN_TOOL_RUBBER`. Position still comes from the compositor's pointer emulation, and the two are paired by timestamp. | Small, self-contained, works on any compositor. | Needs the user in the `input` group (a packaging and doc step). Pairing can jitter. Hover only works where pointer emulation reports it. |
| **C. Force XWayland** | `WINIT_UNIX_BACKEND=x11` plus XInput2 valuators. | Well-trodden. | The whole app goes to XWayland: blurry on fractional scaling, worse IME. Not recommended. |

**Recommendation:** A for the real fix. Optionally ship B first as a stopgap if pressure
is needed within days.

**Decided: A, vendored.** winit v0.30.13 is vendored at `vendor/winit`, so clones, CI and
release tarballs need nothing extra. The patch (`vendor/winit/md-editor-patches/`) adds
`seat/tablet.rs` and the public hook `winit::platform::wayland::set_tablet_tool_handler`.
Unclaimed tool frames are replayed as pointer events.

---

## 4. Repro Still Needed

- **PEN-2 / Wacom without pressure:** confirm that the pen draws at all, as a mouse, in
  the ink window once Phase 3 lands. Capture `libinput debug-events` and
  `WAYLAND_DEBUG=1` output if it doesn't.
- **UI-1:** which view was on screen when the layout-invalidation burst fired.
- Anything else you've seen ("and many more"): send the symptom and roughly when it
  happened. With LOG-1 fixed, the log will still hold the session.

## 5. Not Covered by This Review

Editor widget and highlighter, graph, PPTX, study tracker, settings, and the UI overhaul
in progress on `redesign/folio`. These were covered by the e2e plan or are out of scope
here. Next step: run a seeded `e2e/monkey.rs` session over them once Phase 3 lands, since
executor starvation could hide timing bugs everywhere.
