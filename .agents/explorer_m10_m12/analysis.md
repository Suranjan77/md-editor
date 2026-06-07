# Analysis Report: Milestones 10 & 12 Implementation

This report details findings and design proposals for implementing **Milestone 10 (Performance/Speed)** and **Milestone 12 (Release Hardening)** in the `md-editor` workspace.

---

## 1. Milestone 10: Performance/Speed

### 1.1 Indexing Progress Placeholders

#### Current Mechanism
- **Markdown file indexing** is initiated when a vault is opened via `open_vault()` in `native/src/app.rs`.
- It calls `md_editor_core::vault::set_vault_root()` and `reindex_vault_with_parser_targets()` synchronously on the GUI thread.
- This process scans all files (`list_all_md_files`), reads their contents, extracts wikilinks, updates the in-memory graph (`FileIndex`), and inserts content into the SQLite FTS table (`file_search`).
- Because it runs synchronously, the UI freezes during indexing, and no indexing progress is shown in any view.

#### Views Showing Progress / Placeholders
- **Status Bar** (`native/src/views/status_bar.rs`): Shows search and background status (`background_status` from `pdf_index_status`).
- **Global Search Overlay** (`native/src/views/search.rs`): Displays search status and matching count. Shows "Searching..." when active.
- **Backlinks Panel** (`native/src/views/backlinks.rs`): Displays "No backlinks found" if empty.

#### Proposed Progress Placeholders
To prevent UI lockups, markdown indexing must run in a background task (using `Task::perform` and `tokio::task::spawn_blocking`), similar to PDF text indexing. While running:
1. **Status Bar**: Display `"Indexing markdown files..."` in `background_status`.
2. **Backlinks Sidebar**: Check an `is_indexing` state. If true, display a loading placeholder (e.g. `"Indexing backlinks..."` with an animated spinner) instead of `"No backlinks found"`.
3. **File Explorer / Sidebar**: Show a loading indicator/shimmer overlaying the file list.
4. **Search Overlays**: Display `"Indexing vault, search results may be partial..."` and disable action triggers until indexing finishes.

---

### 1.2 PDF Loading Spinner

#### Current Mechanism
- When a PDF is opened, `open_pdf()` in `app.rs` resets PDF dimensions and sets `self.pdf_total_pages = 0` (and `self.pdf_pages` to empty).
- It dispatches multiple asynchronous tasks via `Task::batch` to compute the file hash, fetch page count, load page sizes, and extract the Table of Contents.
- Once page count is retrieved, `Message::PdfLoaded` updates `self.pdf_total_pages` and allocates `self.pdf_pages = vec![None; pages]`.

#### Render Location
- In `native/src/views/pdf_viewer.rs`, the `view_continuous()` function checks if `pages.is_empty()`:
  ```rust
  if pages.is_empty() {
      return container(text("Loading PDF...").color(theme::text_muted()).size(14))
          .width(Length::Fill)
          .height(Length::Fill)
          .center_x(Length::Fill)
          .center_y(Length::Fill)
          // ...
  }
  ```
- This static container displays `"Loading PDF..."` while loading.

#### Proposed Spinner Implementation
1. **State Tracking**: Add a `pdf_loading: bool` flag to `app::MdEditor`. Set it to `true` inside `open_pdf()`, and set it to `false` in `Message::PdfLoaded`.
2. **Subscription Tick**: Add a periodic timer to the app's `subscription()` when `pdf_loading` is true:
   ```rust
   let spinner_tick = if self.pdf_loading {
       iced::time::every(std::time::Duration::from_millis(50)).map(|_| Message::SpinnerTick)
   } else {
       Subscription::none()
   };
   ```
3. **Spinner Tick Message**: Increment a rotation angle or frame index in `MdEditor` (e.g. `self.spinner_frame = (self.spinner_frame + 1) % 12`).
4. **Spinner Rendering**: Replace the static `"Loading PDF..."` text with a custom canvas program (using `iced::widget::canvas::Program`) that draws a rotating sequence of fading dots or a rotating arc based on the current `self.spinner_frame`.

---

### 1.3 Annotation Debounce Logic

#### Current Mechanism
- Annotation edits/notes are updated when the user submits modal forms (e.g. `ModalType::QuickNote(id)` or `ModalType::AnnotationTags(id)`).
- Submitting dispatches `Message::PdfAddQuickNote` or `Message::PdfUpdateAnnotationTags`.
- These messages write directly and synchronously to the SQLite database and sync notes to the linked markdown file on disk (`sync_annotation_note_in_markdown`).
- Typing in the modals is done character-by-character via `Message::NameModalInputChanged`, updating `self.modal_input` without intermediate saves.

#### Proposed Debounce Implementation
If we want to autosave notes/tags as the user types (or debounce disk writes):
1. **Pending State**: Add `pending_annotation_save: Option<PendingAnnotationSave>` to `MdEditor` state:
   ```rust
   struct PendingAnnotationSave {
       annotation_id: String,
       note: String,
       tags: Option<Vec<String>>,
       requested_at: std::time::Instant,
   }
   ```
2. **Tick Subscription**: Add a debounce timer subscription:
   ```rust
   let annotation_debounce = if self.pending_annotation_save.is_some() {
       iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::AnnotationDebounceElapsed)
   } else {
       Subscription::none()
   };
   ```
3. **Capture Input**: When `NameModalInputChanged` is fired and the active modal is `QuickNote(id)` or `AnnotationTags(id)`, update the `pending_annotation_save` with the current time and text.
4. **Handle Debounce Tick**: In `Message::AnnotationDebounceElapsed`, if `requested_at.elapsed() >= ANNOTATION_DEBOUNCE` (e.g., 500ms), write the changes to the database and trigger the markdown note sync in the background.

---

### 1.4 Debug Diagnostics Panel

#### View and Access
- **Keybinding**: Register a new shortcut `Shortcut::ToggleDiagnostics` (mapped to `Ctrl+Shift+D` or `F12` in the subscription).
- **Command Palette**: Add a "Toggle Diagnostics Panel" command in `native/src/command_registry.rs`.
- **Display Location**: Add a new tab `WorkflowSidebarTab::Diagnostics` in the workflow sidebar, or render a dedicated overlay modal.

#### Diagnostics & Stats to Collect
- **PDF Page Cache Stats**:
  - `self.pdf_state.page_cache.len()` (number of cached rendered pages)
  - `self.pdf_state.page_cache.total_bytes()` (current memory consumption of image handles)
  - Limits: `max_pages` and `max_bytes`
- **PDF Text Cache**:
  - `self.pdf_page_text.len()` (cached pages with extracted text)
- **Wikilink Index Stats**:
  - Outgoing links count (`outgoing.len()`)
  - Incoming backlinks count (`incoming.len()`)
- **Database/FTS Stats**:
  - Count of documents indexed in `file_search` (`SELECT count(*) FROM file_search`)
  - DB size or write times
- **Markdown Editor Stats**:
  - Number of characters, lines, and active cursor positions in the open file.

---

## 2. Milestone 12: Release Hardening

### 2.1 Portable Settings

#### Current Mechanism
- `core/src/state.rs` resolves the database directory via `config_dir()`:
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
- This writes the settings database `md_editor_settings.sqlite` next to the binary.
- If the app is installed in a system directory (like `/usr/bin` on Linux), writing to this directory fails due to write permissions.

#### Portable vs System-wide Design
1. **Dependencies**: Add `directories = "5"` to `core/Cargo.toml`.
2. **Path Resolution**: Refactor `config_dir()` to check for portable mode before defaulting to system paths:
   ```rust
   fn config_dir() -> PathBuf {
       // 1. Check if a portable flag or database file exists in the executable's directory
       if let Ok(exe) = std::env::current_exe()
           && let Some(exe_dir) = exe.parent()
       {
           let flag = exe_dir.join("portable.flag");
           let db = exe_dir.join("md_editor_settings.sqlite");
           if flag.exists() || db.exists() {
               return exe_dir.to_path_buf();
           }
       }
       // 2. Otherwise, use system-wide standard user directories
       if let Some(proj_dirs) = directories::ProjectDirs::from("com", "Suranjan77", "md-editor") {
           proj_dirs.config_dir().to_path_buf()
       } else {
           // Fallback to executable dir
           std::env::current_exe()
               .ok()
               .and_then(|e| e.parent().map(|p| p.to_path_buf()))
               .unwrap_or_else(|| PathBuf::from("."))
       }
   }
   ```
- This preserves portable mode (if `portable.flag` is placed in the folder) and respects system-wide standards for normal installations.

---

### 2.2 DPI Scaling

#### Current Mechanism
- Winit and Iced automatically scale native widgets and layout boundaries using the monitor's DPI/Scale Factor.
- However, for rasterized page rendering, `app.rs` uses a hardcoded supersampling factor:
  ```rust
  let zoom = self.pdf_state.zoom * PDF_RENDER_SUPERSAMPLE; // PDF_RENDER_SUPERSAMPLE = 2.0
  ```
- This forces page images to render at exactly 2.0x logical size, causing scaling mismatches or blurriness on displays with other factors (e.g. 1.25x, 1.5x, or 3.0x Retina).

#### Verification & Dynamic Scaling
1. **Event Capture**: Register a scale factor listener in the event subscription:
   ```rust
   let scale_factor_sub = iced::event::listen_with(|event, _status, _window_id| match event {
       iced::Event::Window(_, iced::window::Event::ScaleFactorChanged { scale_factor }) => {
           Some(Message::ScaleFactorChanged(scale_factor))
       }
       _ => None,
   });
   ```
2. **State Storage**: Add `self.scale_factor: f32` to `MdEditor` state, defaulting to `1.0`.
3. **Dynamic Zoom**: Multiply logical zoom by the actual scale factor during page rendering:
   ```rust
   let zoom = self.pdf_state.zoom * self.scale_factor;
   ```
   This ensures rendering matches screen pixels 1:1, maximizing crispness and reducing resource waste on standard-DPI screens.

---

### 2.3 Visual Authenticity Pass & Release Checklist

Checking `docs/UI_UX_RELEASE_CHECKLIST.md` against the codebase:
- **Layout**: Checked. `clamp_for_window` in `app_shell.rs` successfully enforces bounds and collapses panels when window width is `< 720.0`.
- **Keyboard**: Checked. Shortcuts map key events to `Shortcut` commands. `Shortcut::Escape` is handled in priority order to close overlays and modals.
- **Contrast**: Checked. `theme.rs` defines complete colors for `Dark`, `Light`, and `HighContrast` themes, with resolvers dynamically applying them.
- **Loading & Recovery**: Mostly addressed. PDF rendering and text indexing run on background threads. *Gap:* Markdown file indexing currently blocks the UI synchronously.
- **Motion**: Checked. `reduce_motion` exists in the persistence state, although there are currently no scrolling transitions implemented.
