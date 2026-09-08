# Release Checklist

Everything to verify before packaging an MD Editor build.

---

## 1. Build & Test

```bash
cargo fmt --check               # must be clean
cargo check --workspace
cargo test --workspace          # expect 158 passing: 56 core + 102 native
cargo clippy --workspace        # baseline is 34 warnings; confirm it has not grown
cargo build --release
```

Expected artifacts:

- Windows — `target\release\md-editor.exe`
- Linux / macOS — `target/release/md-editor`
- the PDFium shared library, copied next to the executable by the build script

For a reproducible release, pin and verify the PDFium download:

```bash
PDFIUM_RELEASE=chromium/6996 PDFIUM_SHA256=<hex digest> cargo build --release
```

---

## 2. Durability Checks — Run These First

Pass/fail, not judgement calls. **If any one fails, the build does not ship**, regardless of
what else works. See [Data Flows & Durability Invariants](Data-Flows-and-Durability-Invariants.md)
for what each is protecting.

- [ ] **Kill it mid-edit.** Type into a note, then `kill -9` the process without saving.
      Reopen the vault: no characters lost, no truncated or empty file, no stray `.tmp` files
      in the folder.
- [ ] **Switch with unsaved edits.** Edit a note, click straight to another file, then come
      back. The edits are on disk and `Ctrl+Z` still walks back through the history from
      before the switch.
- [ ] **Undo is word-sized.** Type a sentence, press `Ctrl+Z` once. A run of typing
      disappears, not a single character.
- [ ] **Leave a note by every route.** With unsaved edits, in turn: click another note, click
      an image, and close the window. In each case the edits reach disk, and the undo history
      survives wherever the note is reopened.
- [ ] **Session restored.** Quit and relaunch: same window size, same file, same scroll offset
      and cursor position.
- [ ] **Zero idle CPU.** Leave the settled window alone and check `top` or `htop`: 0.0%.

---

## 3. Motion & Palette Checks

- [ ] Toggle the sidebar, table of contents, and backlinks. Each **slides** rather than
      snapping, and the window goes idle once the transition finishes.
- [ ] Trigger a toast by saving a file. It fades in and out rather than blinking.
- [ ] Press `Ctrl+P` and type immediately without clicking — the query field already has focus.
- [ ] Type a few letters of a note's name; the note appears among the results. Move the
      highlight with `↑`/`↓` and open it with `Enter`.
- [ ] Type a command's initials (for example `sv`) and confirm it ranks first. Press `Escape`
      and confirm the palette closes.

---

## 4. Smoke Test

Use a **fresh temporary vault**:

### Vault and markdown
- [ ] Open a vault; the sidebar indexes folders, markdown files, PDFs, and images.
- [ ] Create, edit, save, reopen, and delete a markdown file.
- [ ] Confirm `.git` and `node_modules` are not listed.
- [ ] Edit a file outside the app and confirm the watcher picks the change up.

### Search
- [ ] `Ctrl+F` in markdown: highlighted matches plus next/previous navigation.
- [ ] Toggle regex and match case; run a replace-all and confirm `Ctrl+Z` undoes it.
- [ ] Global search from the toolbar: both markdown and PDF results appear.

### PDF
- [ ] Open a PDF: continuous rendering, fit-to-width, zoom, TOC, internal links, scrolling.
- [ ] Open a PDF **without** embedded bookmarks and confirm a recovered outline appears,
      marked as synthetic — or is correctly empty for a scanned document.
- [ ] Right-click a numbered cross-reference (an equation, figure, or table) and confirm the
      preview shows the target.
- [ ] Select PDF text, copy it, paste it elsewhere.
- [ ] Search inside the PDF: matches highlight and next/previous scrolls the active match into
      view. Toggle **Loose WS** and confirm a phrase that wraps across a line break matches.
- [ ] Create several highlights on different pages and confirm the colour cycles.
- [ ] Add a quick note to a highlight.
- [ ] Run the orphan report and confirm it states how many annotations were checkable.

### Linked notes and split view
- [ ] Link one highlight to a **new** markdown note through the picker.
- [ ] Link another highlight to the **same** note; a new section is appended, not overwritten.
- [ ] Re-link the first highlight and confirm nothing is duplicated.
- [ ] `Ctrl` + click the generated `pdf://` link and confirm the PDF opens at the target page
      with the highlight focused.
- [ ] Confirm opening a linked note in split view does **not** reset the PDF to page 1.
- [ ] In split view, scroll or click the PDF pane and press `Ctrl+F` — PDF search opens. Then
      interact with the markdown pane and press `Ctrl+F` — markdown search opens.
- [ ] Check the Backlinks panel of the linked note lists the highlight.

### Other
- [ ] Open image files and confirm the preview renders.
- [ ] Open the study tracker, start and stop a session, edit the configuration, restart the
      app, and confirm the state persisted.
- [ ] On Linux, run `--install` and `--uninstall` and confirm the launcher appears and
      disappears.

---

## 5. Packaging

### PDFium

Ship the platform library beside the executable:

| Platform | Library |
| :--- | :--- |
| Windows | `pdfium.dll` |
| Linux | `libpdfium.so` |
| macOS | `libpdfium.dylib` |

The app looks for it in, in order:

1. `resources/` beside the executable — what CI produces;
2. the executable's own directory.

For portable distribution ship the executable, the PDFium library, the app icon, and the
licence together.

### Linux desktop integration

Portable by default; integration is explicit:

```bash
./md-editor --install
./md-editor --uninstall
```

`--install` writes `~/.local/share/applications/md-editor.desktop`, installs multi-size icons
under `~/.local/share/icons/hicolor/`, and refreshes the desktop and icon caches where those
tools exist.

### First-run portability check

- [ ] Run the packaged build from a fresh directory and confirm
      `md_editor_settings.sqlite` is created **beside the executable**, not in a system
      configuration directory.
- [ ] Move the whole folder somewhere else and confirm settings, highlights, and study
      history come with it.

---

## 6. Known Constraints

State these in release notes; they are design decisions, not defects.

- **PDF annotations are sidecar records.** The original PDF files are never modified — which
  also means annotations do not travel with the PDF to another application.
- **PDF text selection, search, and reference recognition require an embedded text layer.**
  Scanned or image-only PDFs need OCR outside the app.
- **PDFium operations run on one worker thread.** Navigation prioritizes the newest target
  page, but an operation already in flight cannot be interrupted.
- **One theme.** The app ships a single dark palette.
- **Window position is not restored**, only window size.
