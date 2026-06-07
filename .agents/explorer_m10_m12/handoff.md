# Handoff Report: Milestones 10 & 12 Investigation

## 1. Observation
The following source code and layout details were examined:

- **Markdown Indexing**: `core/src/vault.rs` lines 28-50 shows synchronous indexing loop using `read_file()` and `FileIndex::update_file()` during vault opening:
  ```rust
  for file_path in md_files {
      if let Ok(content) = read_file(&file_path) {
          index.update_file(&file_path, &content);
          ...
  ```
- **PDF Loading state**: `native/src/app.rs` lines 5059-5064 inside `open_pdf()` initializes loading variables:
  ```rust
  self.pdf_current_page = 0;
  self.pdf_total_pages = 0;
  self.pdf_pages = Vec::new();
  ```
  And `native/src/views/pdf_viewer.rs` line 682 handles rendering:
  ```rust
  if pages.is_empty() {
      return container(text("Loading PDF...").color(theme::text_muted()).size(14))
  ```
- **Annotation Updates**: `native/src/app.rs` line 3319 (`save_pdf_annotation`), line 3395 (`sync_annotation_note_in_markdown`), line 4080 (`save_pdf_annotation`) execute database write and disk sync synchronously:
  ```rust
  if let Err(e) = self.state.save_pdf_annotation(&ann) {
  ```
- **Debug diagnostics panel**: `native/src/messages.rs` lines 225-272 contains `Shortcut` enum; `native/src/command_registry.rs` contains `CommandMetadata`; `native/src/pdf_page_cache.rs` lines 76-86 defines `len()` and `total_bytes()`.
- **Portable settings**: `core/src/state.rs` lines 633-651 defines `settings_db_path()` and `config_dir()`:
  ```rust
  fn config_dir() -> PathBuf {
      if let Ok(exe) = std::env::current_exe()
          && let Some(dir) = exe.parent()
      {
          return dir.to_path_buf();
      }
      PathBuf::from(".")
  }
  ```
- **DPI Scaling**: `native/src/app.rs` line 5192 multiplies zoom by a hardcoded 2x supersampling factor:
  ```rust
  let zoom = self.pdf_state.zoom * PDF_RENDER_SUPERSAMPLE;
  ```
- **Release Checklist**: `docs/UI_UX_RELEASE_CHECKLIST.md` specifies key layout, keyboard, contrast, loading, and platform items.

---

## 2. Logic Chain
- **Markdown Indexing**: Because markdown files are scanned, read, parsed, and indexed synchronously inside the `set_vault_root()` call on vault load, the UI thread blocks. Moving this to a tokio background task requires an `is_indexing` state. While `is_indexing` is true, progress placeholders should be shown in the Status Bar (replacing background status) and Backlinks panel (replacing the empty state list).
- **PDF Spinner**: Because `pages` is empty (`pdf_total_pages == 0`) when a PDF is first opened, `view_continuous` shows a static "Loading PDF..." text. Adding a timer tick to `subscription` when a PDF is loading allows updating a state frame counter, which can be rendered as a rotating canvas spinner in `view_continuous`.
- **Annotation Debounce**: Because annotation updates (`PdfAddQuickNote`, `PdfUpdateAnnotationTags`) write to SQLite and sync to markdown files on disk immediately, keystroke-by-keystroke editing would trigger excessive I/O. Introducing a `pending_annotation_save` state and debouncing it via a timer subscription before committing to the DB/file solves this.
- **Diagnostics Panel**: Because we need memory and cache stats, we can query `PdfPageCache::len()` and `total_bytes()` alongside index size, and display it in a new sidebar tab (e.g. `WorkflowSidebarTab::Diagnostics`) or command.
- **Portable settings**: Because `config_dir()` uses `current_exe()` to write settings to the binary directory, it causes permission failures when installed to protected system directories. Standardizing this requires testing if a portable flag exists locally, and if not, fallback to using standard directories resolved by the `directories` crate.
- **DPI Scaling**: Because Iced handles widget scaling automatically but PDF pages are rasterized using a hardcoded `PDF_RENDER_SUPERSAMPLE = 2.0`, displays with non-2.0 DPI factors (e.g., 1.5x) experience blurriness. Subscribing to `ScaleFactorChanged` and dynamically multiplying the zoom by the actual DPI factor resolves this.

---

## 3. Caveats
- Did not test changes inside the GUI running target since this is a read-only investigation.
- Assumed standard winit/iced API behavior for scale factors.

---

## 4. Conclusion
- **Milestone 10**: Can be solved by moving markdown indexing to a background tokio task, adding an animated Canvas spinner during PDF load, debouncing quick-note modal input writes, and adding a Diagnostics tab in the sidebar.
- **Milestone 12**: Can be solved by importing the `directories` library to dynamically resolve user configuration path (with a portable fallback check), and subscribing to winit's DPI change event to render PDF page raster images.

---

## 5. Verification Method
1. Inspect config resolution: Ensure `portable.flag` allows local DB creation, whereas its absence defaults to the path returned by the `directories` crate.
2. Verify scale factors: Print the detected `scale_factor` upon `ScaleFactorChanged` event to ensure correct multiplier is applied to page render zoom.
3. Test suite: Run `cargo test --workspace` to ensure no regressions in current unit tests.
