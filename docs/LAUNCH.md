# Launch Checklist

This checklist captures the steps needed to prepare an MD Editor build for a
local release.

## Build And Test

Run the full verification set before packaging:

```bash
cargo fmt --check
cargo check
cargo test
cargo build --release
```

Expected release artifacts:

- Windows: `target/release/md-editor.exe`
- Linux/macOS: `target/release/md-editor`
- PDFium shared library copied next to the executable by the build script

## Smoke Test

Use a fresh temporary vault and verify:

- Open a vault and confirm the sidebar indexes folders, markdown files, PDFs,
  and images.
- Create, edit, save, reopen, and delete a markdown file.
- Use `Ctrl+F` in markdown and confirm highlighted matches plus next/previous
  navigation.
- Use global search from the toolbar and confirm markdown/PDF results appear.
- Open a PDF and confirm continuous rendering, fit-to-width, zoom, TOC, internal
  links, and page scrolling.
- Select PDF text, copy it, and paste it into a text editor.
- Search inside a PDF and confirm matches highlight and next/previous scrolls
  the active match into view.
- Create several PDF highlights on different pages.
- Add a quick note to a PDF highlight.
- Select PDF text in split view, use the context menu to insert a quote link,
  and confirm the markdown note receives a blockquote plus `pdf://` page link.
- Link one highlight to a new markdown note through the picker.
- Link another highlight to the same markdown note and confirm a new section is
  appended instead of replacing the file.
- Ctrl+click a generated `pdf://` link in the markdown note and confirm the PDF
  opens to the target page/highlight.
- In split view, scroll/click the PDF pane and press `Ctrl+F`; confirm PDF
  search opens. Then interact with markdown and confirm `Ctrl+F` opens markdown
  search.
- Confirm opening a linked note in split view does not reset the PDF to page 1.
- Open image files and confirm the image preview renders.
- Open the study tracker, start/stop a session, edit tracker configuration, and
  confirm persisted state after restart.

## UI/UX Regression Pass

Run `docs/UI_UX_RELEASE_CHECKLIST.md` before release signoff. At minimum,
verify:

- layout overlap on desktop and narrow windows;
- keyboard traps and Escape priority for overlays and modals;
- missing labels or missing disabled reasons on icon-only and workflow-critical
  controls;
- stale loading states after search, PDF render, indexing, and recovery flows;
- unreadable colors in light/dark themes and known high-contrast gaps;
- reduced-motion limitations for programmatic scroll, progress, and status
  transitions.

## PDFium Packaging

PDF support requires the platform PDFium shared library:

- Windows: `pdfium.dll`
- Linux: `libpdfium.so`
- macOS: `libpdfium.dylib`

The app searches for the library in:

1. a `resources` directory next to the executable;
2. the executable directory itself.

For portable distribution, ship the executable, PDFium library, app icon, and
license files together.

## Linux Desktop Integration

Linux builds are portable by default. Optional desktop integration is explicit:

```bash
./md-editor --install
./md-editor --uninstall
```

`--install` creates a desktop entry in `~/.local/share/applications/`, installs
icons under `~/.local/share/icons/hicolor/`, and refreshes desktop/icon caches
when those tools are available.

## Release Notes

The current launch candidate includes:

- local vault markdown editing;
- global search and per-file search;
- active-pane-aware split-view search;
- integrated PDF viewer with search highlighting and match navigation;
- PDF text selection, copy, highlights, quick notes, and linked markdown notes;
- searchable note picker for linking PDF highlights;
- markdown backlinks for notes, PDFs, and PDF highlights;
- study tracker;
- portable settings in `md_editor_settings.sqlite` next to the executable.

Known constraints:

- PDF annotations are sidecar records; original PDF files are not modified.
- PDF text selection and search depend on embedded PDF text. Scanned/image-only
  PDFs need OCR outside the app.
- PDFium operations are serialized behind a process-wide lock across render and
  query workers. Navigation prioritizes the newest target page, but an
  already-running PDFium operation cannot be interrupted.
