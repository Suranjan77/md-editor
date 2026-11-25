# MD Editor — Project Tasks & Issues

## 1. Bug Fixes & Correctness

- [x] **Fix `Ctrl+Shift+Z` triggering Undo instead of Redo**
  * **Done**: `Ctrl+Shift+Z` redoes (`editor/renderer/events.rs::on_shortcut`; test `ctrl_shift_z_redoes_and_ctrl_z_undoes`).
  * **File**: [`native/src/editor/renderer.rs:2676-2685`](file:///home/sur/repo/md-editor/native/src/editor/renderer.rs#L2676-L2685)
  * **Problem**: The keyboard handler matches `keyboard::Key::Character(c) if c == "z"` under Ctrl/Cmd modifiers without verifying `!modifiers.shift()`. Pressing `Ctrl+Shift+Z` (the standard redo shortcut across Linux, macOS, and Windows) accidentally dispatches [`EditorCommand::Undo`](file:///home/sur/repo/md-editor/native/src/editor/buffer.rs#L154) instead of [`EditorCommand::Redo`](file:///home/sur/repo/md-editor/native/src/editor/buffer.rs#L155).
  * **Solution**: Check `modifiers.shift()` on `"z"` to dispatch `Redo`, or fall back to `Undo` when Shift is not held.

- [x] **Resolve `Ctrl+B` shortcut collision (Format Bold vs. Toggle Sidebar)**
  * **Done**: chords the editor binds are captured, so the app shortcut does not also fire (test `editor_shortcuts_are_captured_so_app_shortcuts_do_not_also_fire`).
  * **Files**: [`native/src/app.rs:236`](file:///home/sur/repo/md-editor/native/src/app.rs#L236), [`native/src/editor/renderer.rs:2691-2695`](file:///home/sur/repo/md-editor/native/src/editor/renderer.rs#L2691-L2695), and [`wiki/Keyboard-Shortcuts.md:74-76`](file:///home/sur/repo/md-editor/wiki/Keyboard-Shortcuts.md#L74-L76)
  * **Problem**: When editing text in the markdown editor, pressing `Ctrl+B` formats the selected text as bold and simultaneously dispatches [`Shortcut::ToggleSidebar`](file:///home/sur/repo/md-editor/native/src/messages.rs#L169), toggling the sidebar unexpectedly while typing.
  * **Solution**: Either suppress the application-layer `ToggleSidebar` shortcut when the editor is focused, or rebind sidebar toggle to `Ctrl+\` or `Ctrl+Shift+B`.

- [x] **Fix hardcoded `• Saved` indicator in Toolbar**
  * **Done**: the status bar shows Saved / Unsaved / save failed (`ui/status_bar.rs::SaveStatus`).
  * **Files**: [`native/src/views/toolbar.rs:39-42`](file:///home/sur/repo/md-editor/native/src/views/toolbar.rs#L39-L42) and [`native/src/app.rs:2044`](file:///home/sur/repo/md-editor/native/src/app.rs#L2044)
  * **Problem**: `toolbar.rs` renders `text(" • Saved")` statically regardless of the document state. While [`buffer.dirty`](file:///home/sur/repo/md-editor/native/src/editor/buffer.rs#L184) is true (e.g. before the 400ms autosave debounce fires or if an autosave fails), it still displays `" • Saved"`.
  * **Solution**: Pass the actual save status (e.g. `Saved`, `Unsaved`, `Saving...`) from [`app.rs`](file:///home/sur/repo/md-editor/native/src/app.rs) to [`toolbar::view`](file:///home/sur/repo/md-editor/native/src/views/toolbar.rs#L8-L19) and reflect it dynamically.

- [x] **Support hyphenated line breaks in PDF Loose-Whitespace Search**
  * **Done**: `app/search.rs::loose_pdf_pattern` (BUG-13, 2026-11-25).
  * **File**: [`core/src/pdf.rs`](file:///home/sur/repo/md-editor/core/src/pdf.rs)
  * **Problem**: PDF loose-whitespace mode matches across whitespace and newlines, but fails when words are hyphenated across line breaks (e.g. `algo-` on one line and `rithm` on the next).
  * **Solution**: Strip trailing hyphens followed immediately by line-break whitespace in the loose-search scanner.

- [ ] **Default fit of PDF files should not be more than 200%, However, manual fit should use available space**
---

## 2. Feature Additions

- [x] **File & Folder Renaming in UI**
  * **Done**: `F2`, the row button and the context menu rename files and folders (`app/vault_entries.rs::submit_rename`); renames now also fix Markdown links and folder paths (BUG-03).
  * **Files**: [`core/src/vault.rs:269-366`](file:///home/sur/repo/md-editor/core/src/vault.rs#L269-L366), [`native/src/views/modals.rs:9-15`](file:///home/sur/repo/md-editor/native/src/views/modals.rs#L9-L15), and [`native/src/views/sidebar.rs:144-176`](file:///home/sur/repo/md-editor/native/src/views/sidebar.rs#L144-L176)
  * **Context**: [`vault::rename_entry`](file:///home/sur/repo/md-editor/core/src/vault.rs#L269) already exists in `core`, with full support for physical rename, SQLite FTS5 re-indexing, and vault-wide automated wikilink rewriting (`[[wikilink]]`). However, there is no UI affordance to trigger it.
  * **Task**:
    1. Add `ModalType::Rename(String)` to [`views::modals::ModalType`](file:///home/sur/repo/md-editor/native/src/views/modals.rs#L9-L15).
    2. Add a rename button / context action to file and folder rows in [`sidebar.rs`](file:///home/sur/repo/md-editor/native/src/views/sidebar.rs).
    3. Add "Rename File" to the command palette.

---

## 3. UI / UX Polish

- [x] **Double-Click & Triple-Click Selection in Markdown Editor**
  * **Done**: double-click selects a word, triple-click a line (BUG-04, 2026-11-25).
  * **File**: [`native/src/editor/renderer.rs:2364-2435`](file:///home/sur/repo/md-editor/native/src/editor/renderer.rs#L2364-L2435)
  * **Task**: Track click timestamp and coordinates in widget state. Double-click selects the word under cursor; triple-click selects the whole line.

- [ ] **Line Numbers in Editor Gutter**
  * **Files**: [`native/src/editor/renderer.rs:17-24`](file:///home/sur/repo/md-editor/native/src/editor/renderer.rs#L17-L24) and [`native/src/theme.rs:24`](file:///home/sur/repo/md-editor/native/src/theme.rs#L24)
  * **Task**: Use the reserved `MARGIN_LEFT = 64.0px` to render line numbers in the gutter for lines in the viewport range, matching `TEXT_XS` and muted styling.