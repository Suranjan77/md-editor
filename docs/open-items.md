# Open Items

What is still not done, as of 2026-11-26. It replaces the finished plans (UI overhaul, code
health, bug review, e2e bug discovery, tabs and images, split layouts) and the product
roadmap. They were checked against the code, and everything else in them is built and
covered by tests (git history has the plans themselves, up to `0cdab3a`). Remove an item
here when it lands; record how the feature works in `wiki/`.

## Roadmap features built only in part

- **Keystroke cost.** `EditorPane::refresh_highlighting` still copies the whole note into
  a `String` on each edit, and loading images, math and PDF crops walks every highlighted
  line instead of just the changed range. Image decoding, file checks and math rendering
  are off the UI thread and deduplicated, and the word count is cached per revision.
- **LaTeX rendering.** `editor_state.rs::render_latex_task` encodes each formula to PNG and
  decodes it again just to read its size, and renders it twice (inline and display), so
  it parses twice. Handing over RGBA (`Handle::from_rgba`) and parsing once would fix
  both.
- **PDF page text cache.** The worker caches open documents (an LRU of three) but not
  extracted page text, so a repeated search extracts the text from PDFium again.
- **Daily Desk brief.** It takes the first five `[!question]` / `[!claim]` callouts in path
  order. Nothing is scheduled or comes due (no spacing), PDF highlights are not included,
  and there is no footer of the notes edited and PDFs read that day.
- **`@` citations** come from `.bib` files and PDF file names (`Author - Year - Title`), not
  from the metadata inside PDFs.
- **Run Code Block** handles `sh`/`bash` and `python` (not `rust` or `sqlite`), and it is
  not sandboxed. It runs with the user's permissions, as the User Guide says.

## Roadmap features not built

- Inline `#tags` (highlighting, index, search).
- Drift badges on `pdf://` crop citations (amber when the PDF changed under the crop).
- Note history: snapshots and a slider to scrub through them (plus git history when the
  vault is a repository).
- AVIF images. The format was dropped from the supported list rather than decoded.

## Known gaps and limits

- A bare `[[Paper.pdf]]` resolves only next to the note. The name index
  (`core/src/file_index.rs`) holds Markdown files only, so link to PDFs elsewhere with
  their path, `[[papers/Paper.pdf]]`.
- The knowledge graph's fit ignores label width, so a label on a node at the edge can be
  clipped.
- UI overhaul leftovers: italic source styling on reveal (3.3), loading shimmer (3.6), a
  selection popover on PDFs (3.7; the toolbar has those tools), a checkerboard behind
  transparent images (3.8), light paper for handwriting (it needs an ink format change).
- State ownership (code health W7): views build `Message::Pdf(PdfMessage::…)` directly
  instead of returning family enums, and the sub-states (`UiState`, `TrackerState`,
  `PdfPane`, `SearchState`, `VaultState`, `EditorPane`) take the whole `Message`.
- Dropping a file on the handwriting window opens it in the main window.

## Needs real hardware or a person

- Pen: confirm pressure with a Wacom tablet on Wayland (PEN-2), and the barrel button ring.
- UI-1, a burst of layout invalidations seen once in the log: needs the view that was on
  screen when it happened.
- Pasting a screenshot and copy-pasting files from the file manager on a real Wayland
  session (the e2e harness cannot reach the OS clipboard).

## Owner actions

- Report the `iced_test` 0.14 screenshot bugs upstream (`Emulator::screenshot` keeps the UI
  cache; a new headless renderer draws no text on its first frame). The harness works
  around both in `native/src/e2e/mod.rs`.
- Tag `v1.0.0` (the `md-editor` PKGBUILD's checksum is `SKIP` until then) and publish both
  PKGBUILDs to the AUR.
