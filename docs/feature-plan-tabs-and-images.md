# Feature Plan — Tabs, and Pasting/Dropping Images

**Scope:** exactly two features. (1) Document tabs. (2) Getting images into notes by pasting
from the clipboard or dropping files. Nothing else.

**Date:** 2026-11-23 · **Status:** proposed. Visual design follows the
[design system](design-system/md-editor/MASTER.md); behaviour is verified with the
[e2e harness](e2e-bug-discovery-plan.md); code lands under the rules of the
[code health plan](code-health-plan.md).

---

## Part 1 — Tabs

### 1.1 Today

The app shows one document per kind: one note (`active_path` + `editor.buffer`), one PDF
(`pdf.active_path`), one deck (`pptx.active_path`), one image (`active_image_path`). Split
view shows the note beside the PDF. Switching notes parks the outgoing buffer in a
**retained-buffer registry** (up to 32, with undo history and scroll), so returning to a note
already restores its state. Opening anything replaces what was shown.

### 1.2 Behaviour

- A **tab strip** above the main area lists open documents of every kind: notes, PDFs, decks,
  images. (Handwriting stays in its own window; it is not a tab.)
- **Opening** a document (sidebar, palette, link, search result) focuses its tab if open,
  otherwise opens a new tab to the right of the current one. `Ctrl`+click or middle-click in
  the sidebar opens it in the background.
- **Closing:** `Ctrl+W`, middle-click, or the tab's ✕ (shown on hover and on the active tab).
  A note is flushed (autosave rules) before its tab closes; a failed save keeps the tab open
  with an error toast. Closing the last tab shows the empty state. `Ctrl+Shift+T` reopens
  the most recently closed tab (stack of 10).
- **Switching:** `Ctrl+Tab` / `Ctrl+Shift+Tab` (most-recent order), `Ctrl+PageUp/PageDown`
  (strip order), `Ctrl+1…8`, `Ctrl+9` = last. Palette: "Close tab", "Close other tabs",
  "Reopen closed tab".
- **Tab content:** file-type icon, file name (ellipsized, full path in tooltip), a dot for
  unsaved changes, ✕. Two notes with the same name show their parent folder to tell them
  apart (`Index — notes/`).
- **Reordering:** drag tabs within the strip, with keyboard alternatives
  `Ctrl+Shift+PageUp/PageDown`.
- **Overflow:** the strip scrolls horizontally, and a ▾ menu lists all tabs.
- **Split view** keeps its meaning: the left pane shows the most recently active **note**
  tab, the right pane the most recently active **PDF** tab. Activating a tab updates the
  pane of its kind; the strip marks both visible tabs.
- **Session:** open tabs, their order and the active tab persist per vault (the `settings`
  table, keyed by vault) and are restored on launch, replacing today's single `last_file`.
- **Files changing on disk:** renaming a file renames its tab (reusing `rename_open_path`).
  Deleting a file closes its tab after flushing. An external delete marks the tab
  "missing", greyed out with a tooltip.
- **Limits:** no hard cap on tabs. Only the 32 most recent notes keep undo history (the
  existing registry); older note tabs reload from disk when activated, as today.

### 1.3 Design

```rust
// native/src/tabs.rs (new)
pub struct Tabs {
    tabs: Vec<Tab>,               // strip order
    active: Option<TabId>,
    recent: Vec<TabId>,           // MRU for Ctrl+Tab and split-pane selection
    closed: Vec<Tab>,             // reopen stack, capped at 10
}
pub struct Tab { id: TabId, kind: TabKind, path: String, view: TabView, missing: bool }
pub enum TabKind { Note, Pdf, Deck, Image }
pub enum TabView {               // what is restored when the tab is activated
    Note,                        // buffer, scroll and caret live in the retained registry
    Pdf { page: u16, zoom: f32, fit_to_width: bool, scroll_y: f32 },
    Deck { zoom: f32, scroll_y: f32 },
    Image { zoom: f32 },
}
```

- `Tabs` is pure state with unit tests (open/focus/close/reorder/MRU/reopen/rename/delete).
- Activation reuses the existing open paths (`open_file_extended`, `open_pdf`, `open_pptx`,
  `open_image` in `app/documents.rs` and `app/pdf_pages.rs`). They gain a "restore view"
  step from `TabView`, and leaving a tab saves its `TabView`.
- Messages (`TabsMessage` family, per the code-health W5 layout): `Activate`, `Close`,
  `CloseOthers`, `Reopen`, `Move`, `Next`, `Previous`, `Select(n)`.
- **PDF switching cost:** the PDF worker keeps one open `PdfDocument`. Extend its cache to the
  3 most recent documents (LRU), so switching between two PDF tabs does not re-parse.
  Page bitmaps are not kept for background tabs (memory), and they re-render for the
  visible range only.
- View: `ui::Tabs` component from the design system (the UI overhaul component list gains
  `TabStrip`). If tabs ship before the overhaul, build it with today's tokens and migrate.

### 1.4 Phases

| Phase | Work | Done when |
| :--- | :--- | :--- |
| T1 | `Tabs` state + unit tests; strip with open/focus/close; notes only | Notes open in tabs; undo history survives switching; close flushes |
| T2 | PDFs, decks, images as tabs; `TabView` restore; split-view pane rules | Switching keeps page/zoom/scroll per tab |
| T3 | Shortcuts, palette commands, MRU switching, reopen-closed, overflow menu | Every action keyboard-reachable |
| T4 | Session persistence per vault; rename/delete/external-change sync | Relaunch restores tabs; rename and delete keep tabs correct |
| T5 | Drag reorder, duplicate-name disambiguation, PDF document LRU | Polished |

### 1.5 Tests

- Unit: `Tabs` operations (including MRU, reopen stack, rename and delete propagation).
- E2E scenarios (new): open three notes and a PDF → switch → edit → switch back and undo;
  close a dirty note (flushes to disk); split view with two PDFs (right pane follows the
  last-active PDF tab); rename an open note (tab renamed, buffer intact); relaunch restores
  tabs; `Ctrl+Shift+T` after closing.
- Telemetry budget: activating a note tab < 16 ms update in release (it is a buffer swap);
  a PDF tab < 50 ms before first page is requested.

---

## Part 2 — Pasting and dropping images

### 2.1 Today

Notes render `![alt](path)` images (path relative to the note) through
`EditorPane::load_images`. There is no way to bring an image in except writing the file and
the Markdown by hand.

### 2.2 Platform facts that shape the design

| Fact | Consequence |
| :--- | :--- |
| iced 0.14's clipboard API is **text only** (`clipboard::read() -> Option<String>`) | Reading image data needs a new dependency: **`arboard`** (MIT/Apache) with `wayland-data-control`, which reads `image/png` and other bitmap formats on X11, Wayland (wlr/ext data-control, which Hyprland supports), Windows and macOS |
| iced delivers `window::Event::FileHovered` / `FileDropped` from **winit 0.30.13**, which emits dropped files **only on Windows, macOS and X11 — not on native Wayland** | On Wayland (your Hyprland session), drag-and-drop will not reach the app. Paths: (a) copy-and-paste of files (file managers offer `text/uri-list`; read it through `arboard`'s file-list support or as text — **verify per platform in I3**, as `arboard`'s file-list coverage differs by OS and version); (b) an "Insert image…" file picker (rfd, already a dependency); (c) track winit's Wayland DnD support and enable drop there when it lands. Documented as a known limitation, not hidden |

### 2.3 Behaviour

- **Paste (`Ctrl+V`) in a note:**
  1. Clipboard holds bitmap data (screenshot, copied image) → save it into the vault
     (§2.4) and insert `![](relative/path.png)` at the caret.
  2. Clipboard holds file URIs or paths (`text/uri-list`, or text that is an existing
     image path) → for each image file: copy into the vault if it is outside, or link it in
     place if it is already inside; insert one image line per file.
  3. Otherwise → today's text paste, unchanged.
- **Drop on the editor** (Windows, macOS, X11): same as case 2. A drop indicator (caret
  preview at the drop point, from `FileHovered`) shows where the image will go; the image
  is inserted at the drop point. Dropping on the sidebar copies the files into the hovered
  folder (images and any supported file type) without editing a note.
- **Insert image…** (palette command and editor context menu): file picker, then case 2.
- Non-image files dropped or pasted on the editor: PDFs/notes in the vault insert a
  wikilink (`[[name]]`); other types show an info toast "Only images can be inserted".
- **One undo step** for the whole paste/drop insertion (`DocBuffer` transaction); the copied
  file stays on disk (an orphan image is harmless, losing a pasted screenshot is not).
- Feedback: a toast "Added `attachments/…png`"; errors (read-only vault, unreadable image,
  clipboard empty) as persistent error toasts with the cause.

### 2.4 Where images go

- Default: `attachments/` at the vault root. Setting: *Attachments folder* = vault-root
  folder / beside the note (`<note folder>/attachments/`) / custom path. Stored in `settings`.
- Name: pasted bitmaps → `Pasted image 2026-11-23 14-05-31.png` (encoded as PNG via the
  `image` crate, already a dependency); copied files keep their name. Collisions get
  ` (2)`, ` (3)`…; identical content (same SHA-256, `sha2` already in core) reuses the
  existing file instead of duplicating.
- Writes go through `vault::write_file`'s atomic path (bytes variant), and the vault index
  is updated so the file appears in the tree immediately.
- Inserted Markdown uses a path relative to the note with `/` separators; spaces encoded as
  `%20` or the path wrapped in `<…>`, matching what the highlighter's image parser accepts
  (verify and add a test).
- Large images: no resizing, but a warning toast above 20 MB.

### 2.5 Design

| Piece | Where |
| :--- | :--- |
| `attachments` service: `store_bitmap(rgba) -> RelPath`, `import_file(path) -> RelPath`, naming, dedupe | `core/src/attachments.rs` (headless, unit-tested) |
| `write_bytes_atomic` alongside `write_file` | `core/src/vault/files.rs` |
| Clipboard image/URI read via `arboard`, run in a `Task` (off the UI thread) | `native/src/platform.rs` (clipboard shim) |
| Editor paste hook: `Ctrl+V` emits `EditorMessage::Paste` to the app, which tries image paths before falling back to the existing text paste | `editor/renderer/events.rs`, `app/editor.rs` |
| Drop: `FileHovered`/`FileDropped`/`FilesHoveredLeft` in `app/subscription.rs` window events, hit-tested against the editor or sidebar bounds; drop caret drawn by the editor widget | `app/subscription.rs`, `app/editor.rs`, `editor/renderer/draw/overlays.rs` |
| Insert command: `EditorCommand::InsertBlock(String)` (one transaction, own line) | `editor/buffer.rs` |
| Settings: attachments folder | Settings view (UI overhaul 2.10); until then a `settings` key with the default |

### 2.6 Phases

| Phase | Work | Done when |
| :--- | :--- | :--- |
| I1 | `core::attachments` (naming, dedupe, atomic bytes write, index update) + tests | Unit tests for collisions, dedupe, relative paths with spaces and subfolders |
| I2 | Paste bitmap from clipboard (arboard) → stored + inserted, one undo | Screenshot paste works on Wayland, X11, Windows, macOS |
| I3 | Paste file URIs/paths; "Insert image…" picker | Copy file in a file manager → paste in note works on Wayland |
| I4 | Drop on editor and sidebar (Windows/macOS/X11), drop caret | Drop inserts at the drop point; sidebar drop copies into the folder |
| I5 | Attachments-folder setting; wikilinks for dropped notes/PDFs | Setting honoured; documented in the wiki user guide |

### 2.7 Tests

- Core unit: name generation, collision suffixes, SHA-256 dedupe, relative path from nested
  notes, spaces and non-ASCII names, read-only vault error.
- Editor unit: `InsertBlock` is one undo step and lands on its own line mid-paragraph.
- E2E (harness can't touch the OS clipboard): drive `EditorMessage::Paste` with an injected
  clipboard payload (the clipboard shim takes a test double), assert the file on disk, the
  inserted Markdown and the rendered image in a screenshot. Drop: send `FileDropped` window
  events with a point inside the editor.
- Manual check per platform before release: real screenshot paste, file-manager copy/paste,
  and drag-and-drop where supported (listed in the wiki *Release Checklist*).

---

## Order relative to the other plans

1. Code health W1–W2 (CI gate) first, so both features land under it.
2. Image paste/drop I1–I3: small and self-contained, high daily value, no UI-overhaul
   dependency.
3. Tabs T1–T2 after code health W5 (message families) and before UI overhaul Phase 2, so the
   shell redesign lays out the strip from the start; T3–T5 alongside Phase 2.
4. Image drop I4–I5 with the Settings view (UI overhaul 2.10).
