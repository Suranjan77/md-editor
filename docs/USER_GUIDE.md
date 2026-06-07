# User Guide

## Vault Setup

1. Start MD Editor.
2. Select **Open Vault** or press `Ctrl+O`.
3. Choose folder containing markdown, PDFs, and images.
4. Wait for markdown and PDF indexing status to finish.
5. Open files from sidebar.

Use **Create New Vault** to choose empty folder. Recent vaults appear on welcome
screen.

Supported files:

- Markdown: `.md`, `.markdown`
- PDF: `.pdf`
- Images: `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.webp`

Opening invalid or unavailable vault keeps current vault open and reports error.

## Write And Search

- Press `Ctrl+N` to create markdown file.
- Press `Ctrl+S` to save.
- Press `Ctrl+F` to search active markdown file.
- Use toolbar search while no local document search applies to search vault,
  indexed PDF text, annotations, and notes.
- Press `Ctrl+T` to open Outline / TOC.
- Press `Ctrl+Alt+B` to open backlinks.

Search results group filenames, headings, markdown content, PDF content,
annotations, and quick notes. Result selection opens source location.

## Split Research

1. Open markdown note.
2. Open PDF.
3. Select **Split View** from toolbar or command palette.
4. Click or scroll pane before pane-specific shortcut.
5. Press `Alt+P` to switch active pane from keyboard.
6. Drag divider to resize panes.

Below 720px window width, only active split pane renders. `Alt+P` switches
visible pane.

`Ctrl+F` follows active pane:

- Markdown active: search current note.
- PDF active: search current PDF.

Mapped companion note appears in PDF toolbar. Press `Alt+N` to reopen it without
resetting PDF page, scroll, or zoom.

## Citations And Annotations

### Create Annotation

1. Select embedded PDF text.
2. Press `Ctrl+H` for highlight, `Ctrl+Shift+H` for underline, or
   `Ctrl+Alt+H` for strikeout.
3. Use annotations panel to add tags, quick note, status, or linked note.

Annotations use sidecar storage. Source PDF remains unchanged.

### Insert PDF Quote

1. Keep markdown note and PDF open.
2. Select PDF text.
3. Choose **Quote** from selection toolbar.

Editor inserts blockquote and stable `pdf://` page link in one undoable
transaction.

### Insert Annotation Citation

1. Focus PDF annotation.
2. Choose **Cite**.

Editor inserts annotation link into active markdown note.

### Citation Palette And Excerpts

- `Alt+C`: search current selection, annotations, and indexed PDF text.
- `Alt+E`: toggle excerpt queue mode.
- `Alt+I`: insert queued excerpts into active markdown note.

## Recovery And Diagnostics

- Vault-open failure preserves current vault.
- Search and indexing failures appear in status bar or toast.
- `Ctrl+Shift+D` opens diagnostics panel with bounded cache/index counters.
- Command palette action **Toggle Reduced Motion** stops PDF spinner animation.
- `Esc` closes most specific active surface first: selection, preview, modal,
  tracker/search/palette, then side panel.

Settings location:

- Release archive: package directory beside executable or `MD Editor.app`.
- Unmarked build/install: platform user config directory.
- Custom portable mode: place `portable.flag` beside executable or macOS app.
- Existing `md_editor_settings.sqlite` in package directory also keeps local
  mode.

Vault documents remain normal files and are never stored in settings database.

## Screenshots

Current repository examples:

- Markdown editor: [`markdown_window.png`](../images/markdown_window.png)
- Markdown/PDF split research: [`split_view.png`](../images/split_view.png)
- Study tracker: [`study_tracker.png`](../images/study_tracker.png)
- Global search: [`search_window.png`](../images/search_window.png)
- Recoverable vault error: [`recovery_window.png`](../images/recovery_window.png)

See [Keyboard Shortcuts](SHORTCUTS.md) for complete key reference.
Inside app, open command palette with `Ctrl+P` and choose **Help & Shortcuts**
for quick reference.
