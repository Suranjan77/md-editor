# PDF Viewer Internals

This page covers the **native** half of the PDF subsystem: the viewport state in
`native/src/pdf_pane.rs`, the scheduling and cache invariants enforced in
`native/src/app.rs`, and the navigation and scroll model. The engine side — the PDFium
worker, TOC recovery, the reference resolver, and sidecar storage — is in
[PDF Engine & Sidecars](PDF-Engine-and-Sidecars.md).

The flow of one interaction:

```text
iced UI
  -> Message handler in native/src/app.rs
  -> iced::Task async call
  -> PdfRenderer API in core/src/pdf.rs
  -> single PDFium worker thread
  -> typed Message result back into app.rs
  -> generation validation, pending cleanup, cache update, optional follow-up task
```

---

## 1. Viewport State (`PdfPane`)

| Field | Purpose |
| :--- | :--- |
| `active_path` | Vault-relative path of the open PDF |
| `current_page`, `total_pages` | Navigation position and page count from `PdfLoaded` |
| `zoom`, `fit_to_width` | Display scale; fit mode defaults on, `zoom` starts at 1.5 |
| `scroll_y` | Current scroll offset |
| `pages`, `dimensions` | Rendered page handles and their **logical display** dimensions |
| `page_sizes` | Unscaled page sizes from PDFium, in PDF points |
| `placeholder_page_size` | One stable unscaled size used by every unloaded page slot |
| `page_links` | Embedded link annotations, per page |
| `references` | Resolved internal references converted to `LinkInfo`, keyed by source page |
| `link_preview`, `link_preview_size` | The right-click preview bitmap and its aspect ratio |
| `document_id` | Content hash keying annotations and cached references |
| `page_text` | Lazily loaded text geometry for visible pages |
| `selection` | Active text selection, in page text-index coordinates |
| `annotations` | Sidecar highlights and notes, grouped by page |
| `focused_annotation_id` | The highlight reached through a backlink or a `pdf://` link |
| `initial_target_page`, `initial_target_annotation` | Deferred navigation target when opening from a `pdf://` link |
| `toc_entries`, `toc_is_synthetic` | The pane's own outline, and whether it was recovered rather than embedded |
| `pending_pages`, `pending_links`, `pending_text` | In-flight request sets, one per kind of work |
| `text_lru` | Bounds `page_text` to 12 pages, evicting oldest first |
| `render_generation` | Invalidates stale async results after a document, zoom, or navigation change |
| `programmatic_scroll` | Suppresses normal viewport scheduling for a jump's own scroll event |
| `toc_target_page` | The page currently being navigated to |
| `next_highlight_color` | Advances through the palette on each quick highlight |

The pane owns its outline separately from the editor's markdown TOC, because in split view
both documents are open at once and each needs its own.

`MdEditor::active_panel` tracks whether the markdown or PDF pane last received direct user
interaction, so split-view shortcuts route to the right one.

---

## 2. Generation & Pending Invariants

`render_generation` exists to ignore **stale results**. It must never be used to skip
**cleanup** — that is the single easiest way to strand a page in a permanent loading state.

The invariant, in every async result handler:

```rust
Message::PdfRendered(generation, page, img) => {
    self.pdf.pending_pages.remove(&page);          // 1. always clean up first

    if generation != self.pdf.render_generation {  // 2. then discard if stale
        return Task::none();
    }

    // 3. only now touch the cache
}
```

The same shape applies to `PdfRenderFailed`, `PdfPageLinksLoaded`, and `PdfPageTextLoaded`.

Any code path that **increments** `render_generation` must also clear the in-flight state
belonging to the previous generation — `pending_pages.clear()` and `pending_links.clear()` —
and a document change additionally clears `page_links`, `pages`, `dimensions`, and
`page_sizes`.

One deliberate exception: `PdfReferencesLoaded` is gated on `document_id`, **never** on
`render_generation`. References are a document-level artifact, and the chunked scan
routinely outlives the fit-to-width zoom change that bumps the generation on open.
Generation-gating silently dropped them on first open — the tell was that a *reopened* PDF
worked (via the synchronous cache path) while a fresh one did not. `PdfTocLoaded` is gated
on the document path for the same reason.

---

## 3. Page Rendering

Scheduling entry points, in `app.rs`:

- `render_visible_pdf_pages()`
- `render_pdf_pages_for_viewport(scroll_y, viewport_height)`
- `render_pdf_page_range(start, end)`
- `render_pdf_page(page)` — normal priority
- `render_pdf_page_direct(page)` — priority channel, used for navigation targets

`render_pdf_page_range()` deduplicates work against both the cache and the pending set:

```rust
if page image is missing && page is not pending {
    pdf.pending_pages.insert(page);
    render_pdf_page(page)
}
```

`render_pdf_page()` renders **only** the page image; it never extracts links. The PDFium
render scale is *supersampled* relative to the displayed zoom — factoring in
`ui.scale_factor`, the OS display scale — so a narrow split-view page stays sharp on HiDPI
and fractional-scale displays without over-rendering on a 1× screen. Crucially, cached
`dimensions` remain **logical display** dimensions rather than raw bitmap dimensions, so
link hit-testing and placeholder math keep working in PDF-space/display-space geometry.

A failure returns `PdfRenderFailed` rather than being swallowed by a generic tick.

On a successful `PdfRendered`:

1. remove the page from `pending_pages`;
2. discard the result if the generation is stale;
3. store the handle in `pages` and `(width, height)` in `dimensions`;
4. schedule low-priority link and text extraction for that page;
5. if the page is the active navigation target, compute the scroll offset and issue the
   programmatic scroll.

---

## 4. Stable Page Slots

Every page's unscaled size is loaded once with `page_sizes(path)` when the PDF opens — far
cheaper than rendering, and available long before most pages enter the render queue. One of
them (normally page 1) becomes `placeholder_page_size`.

**Every page occupies the same outer slot while scrolling, including pages whose bitmaps
have already rendered:**

```text
slot = (placeholder_width * zoom, placeholder_height * zoom)
```

Rendered bitmap dimensions are cached for image data and link math, but they never change
scroll geometry. This is what keeps the scroll model and the actual blank-page layout in
agreement — without it, a jump deep into a long PDF accumulates offset error, because the
model and the layout drift apart page by page.

The stable-slot behaviour is covered by unit tests in `native/src/app.rs`: target offsets
map back to the same page, blank pages reserve space in the total document height, and
placeholder slots scale with zoom.

Page-size metadata drives loading placeholder dimensions, `pdf_page_offset(page)`,
`pdf_page_at_scroll(scroll_y)`, viewport range detection, and the fit-to-width calculation.

---

## 5. Link & Text Extraction

Links load separately from page images, through `load_pdf_page_links(page)`, with their own
pending set. Keeping them decoupled is what stops link extraction from holding a page in a
visual loading state:

- `pending_pages` — rendered images;
- `pending_links` — link metadata;
- `pending_text` — text geometry, bounded by `text_lru` to 12 pages.

Links are generally loaded *after* the page image renders, so a TOC jump does not schedule
image rendering and link extraction for many nearby pages at once.

Page text is loaded lazily via `get_page_text(path, page)`, stored in **PDF-space**
coordinates so it survives zoom changes, and bounded to the visible range.

---

## 6. Overlay Composition

`views/interactive_pdf.rs` is a custom `Widget` wrapping each page bitmap. It draws the
image, performs hit testing, and emits `PdfLeftClicked`, `PdfRightClicked`,
`PdfSelectionChanged`, and `PdfSelectionFinished`. Click coordinates are normalized against
the displayed page and mapped back to PDF space using the cached dimensions and zoom.

`views/pdf_viewer.rs` composes each page with a dedicated canvas carrying five overlay
layers:

1. persistent sidecar annotation highlights;
2. focused-annotation outlines;
3. PDF search highlights, with the active match distinguished;
4. the live text-selection highlight;
5. internal-reference underlines, drawn as hairlines via the `thin` flag.

Drawing these as a small number of canvas layers — rather than one iced widget per
rectangle — is what keeps a heavily annotated document responsive.

---

## 7. Navigation

TOC clicks, internal link clicks, and PDF search-result clicks all funnel through
`navigate_pdf_page(page)`:

1. clamp and store `current_page`;
2. clear `pending_pages` and `pending_links`;
3. increment `render_generation`;
4. store `toc_target_page`;
5. schedule link loading if the target page is already cached, otherwise request it through
   `render_pdf_page_direct()`, which uses the **priority** channel;
6. compute the target offset from the same stable blank-page slot geometry the view uses;
7. set `programmatic_scroll = true` and issue `scroll_to`.

When the target render completes, `PdfRendered` stores the bitmap and dimensions but does
**not** change the page's outer slot.

Search next/previous navigation uses `navigate_pdf_search_to_index()` instead of the generic
page-top path: it computes the offset from the *active match rectangle*, priority-renders the
match page, schedules a small nearby range, and scrolls the match into view. It deliberately
leaves `toc_target_page` unset, because the render-completion page-top scroll would otherwise
override the match-precise offset.

---

## 8. Scroll Handling

`Message::PdfScrolled { y, viewport_height }` is emitted whenever the scrollable moves.

**Normal user scrolling:**

1. update `scroll_y`;
2. estimate the current page from `y + viewport_height * 0.33`;
3. if the page jump is large, clear pending image and link work;
4. schedule viewport pages via `render_pdf_pages_for_viewport()`.

**Programmatic scrolling (TOC, link, or annotation jump):**

1. `programmatic_scroll` is set before `scroll_to`;
2. the first matching `PdfScrolled` acknowledges it;
3. normal viewport scheduling is suppressed for that event;
4. only a small range around the target page is scheduled;
5. `toc_target_page` is cleared.

Without step 3, a single jump would schedule the target page, its neighbours, the whole
viewport, and link extraction all at once.

### Viewport window

```text
first = page_at(scroll_y - estimated_page_height)
last  = page_at(scroll_y + viewport_height + estimated_page_height)
range = (first - 2) ..= (last + 2)
```

Bounded and deduplicated, so ordinary scrolling preloads its neighbours smoothly without
rendering the whole document. The page lookup uses per-page metadata where available rather
than assuming every page is letter-sized.

Keyboard scrolling is handled at the app level: `ArrowUp`/`ArrowDown` scroll by 64px and
`PageUp`/`PageDown` by 520px, gated on the PDF pane being the active target.

---

## 9. Zoom & Document Changes

Zoom and fit-to-width changes invalidate rendered bitmaps because the render scale changes.
On a zoom change: set the new zoom, reset `pages` and `dimensions`, clear `pending_pages`
and `pending_links`, clear programmatic navigation state, increment `render_generation`, and
render the visible pages.

Opening a new PDF additionally clears the page and dimension vectors, `page_links`,
`current_page`, and all TOC/navigation state.

Opening a linked markdown note from a PDF highlight switches to split view but **preserves
the current PDF position**. When fit-to-width is active, fit is recomputed for the narrower
split pane while keeping the current page and relative scroll offset.

---

## 10. Reliability Rules

When changing this code, preserve these:

1. Never create more than one `Pdfium::new()` binding in the process.
2. Navigation-target renders must use `render_page_priority()`.
3. Priority rendering may use `Wake`, but `Wake` must not do real work.
4. Remove pending page/link/text state **before** the generation check.
5. Stale results must never update UI caches.
6. Link extraction must stay decoupled from page-image pending state.
7. Navigation schedules the target first, then scrolls, then schedules neighbours.
8. Programmatic scroll events must not trigger the full viewport scheduling path.
9. Rapid navigation should behave as latest-request-wins wherever practical.
10. Never "fix" a problem by rendering every page, sleeping, clearing all cached pages on
    every scroll, or skipping generation checks.
11. Search and annotation overlays stay in canvas layers; never expand them into one widget
    per rectangle.
12. Linked-note writes append to an existing note unless the exact highlight link is already
    present.
13. Reference resolution must not issue a monolithic full-document worker command from the
    UI path; extract text per page so priority renders interleave.
14. Reference results are gated on `document_id`, never on `render_generation`.
15. Bump `references::RESOLVER_VERSION` whenever the resolver's output for the same input
    changes, so cached results are invalidated.
