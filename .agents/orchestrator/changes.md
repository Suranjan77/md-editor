# Planned Code Changes

## 1. core/Cargo.toml
- Add dependency `directories = "5"`

## 2. core/src/state.rs
- Refactor `config_dir()` to look like:
```rust
fn config_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let flag = exe_dir.join("portable.flag");
            let db = exe_dir.join("md_editor_settings.sqlite");
            if flag.exists() || db.exists() {
                return exe_dir.to_path_buf();
            }
        }
    }

    if let Some(proj_dirs) = directories::ProjectDirs::from("com", "Suranjan77", "md-editor") {
        proj_dirs.config_dir().to_path_buf()
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
```

## 3. native/src/messages.rs
- Add new variants to `Message` enum:
  - `ScaleFactorChanged(f64)`
  - `SpinnerTick`
  - `MarkdownIndexFinished(Result<(), String>)`
  - `AnnotationDebounceElapsed`
- Add new variant to `Shortcut` enum:
  - `ToggleDiagnostics`

## 4. native/src/command_registry.rs
- Register `Shortcut::ToggleDiagnostics` command metadata:
```rust
        CommandMetadata {
            id: Shortcut::ToggleDiagnostics,
            name: "Toggle Diagnostics Panel",
            icon: "D",
            group: CommandGroup::View,
            default_shortcut: Some("Ctrl+Shift+D"),
        },
```
- In the `command_actions` or dynamic checks, make sure `Shortcut::ToggleDiagnostics` is allowed when vault is open.

## 5. native/src/views/backlinks.rs
- Update `pub fn view<'a>(...)` signature to accept `is_indexing: bool` as the fourth parameter.
- If `is_indexing` is true, render a loading placeholder `"Indexing backlinks..."` with `theme::text_muted()` color.

## 6. native/src/views/pdf_viewer.rs
- Update `pub fn view_continuous<'a>(...)` signature to accept `spinner_frame: u32` as the last parameter.
- If `pages.is_empty()`, instead of rendering a static `Loading PDF...` text container, render a column centering a canvas utilizing a custom `iced::widget::canvas::Program` spinner and a text `"Loading PDF..."` underneath.
- Example Canvas spinner program:
```rust
struct SpinnerProgram {
    frame: u32,
}

impl<Message> iced::widget::canvas::Program<Message> for SpinnerProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        let center = bounds.center();
        let radius = 16.0;
        let stroke = iced::widget::canvas::Stroke::default()
            .with_color(crate::theme::accent())
            .with_width(3.0)
            .with_line_cap(iced::widget::canvas::LineCap::Round);

        let start_angle = (self.frame as f32 * 30.0).to_radians();
        let end_angle = start_angle + 270.0f32.to_radians();

        let path = iced::widget::canvas::Path::new(|path| {
            path.arc(center, radius, start_angle, end_angle);
        });

        frame.stroke(&path, stroke);
        vec![frame.into_geometry()]
    }
}
```

## 7. native/src/views/diagnostics.rs
- Create a new file implementing the diagnostics sidebar panel view.
- Collect:
  - Cached PDF pages count
  - PDF Page Cache total bytes
  - Cached PDF text pages count
  - Outgoing wiki-links count
  - Incoming backlinks count
  - Database FTS document count (FTS `file_search`)
  - Active editor file characters count and lines count

## 8. native/src/app.rs
- Integrate all changes:
  - Add fields to `MdEditor`:
    - `markdown_indexing: bool`
    - `pdf_loading: bool`
    - `spinner_frame: u32`
    - `scale_factor: f32` (default to `2.0` in `MdEditor::new`)
    - `diagnostics_visible: bool`
    - `pending_annotation_save: Option<PendingAnnotationSave>` where:
      ```rust
      struct PendingAnnotationSave {
          id: String,
          input: String,
          requested_at: std::time::Instant,
      }
      ```
  - In `app_shell_status`, set `background_status` to `"Indexing markdown files..."` if `self.markdown_indexing` is true.
  - In `open_vault`, set `self.markdown_indexing = true` and return `index_markdown_vault_task()`. Update `OpenRecentVault` and `VaultOpened` update matching arms to batch PDF text indexing and markdown indexing tasks.
  - Add `index_markdown_vault_task`:
    ```rust
    fn index_markdown_vault_task(&self) -> Task<Message> {
        let state = self.state.clone();
        let path = self.vault_root.clone().unwrap_or_default();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    reindex_vault_with_parser_targets(&state, std::path::Path::new(&path))
                })
                .await
                .unwrap_or_else(|err| Err(err.to_string()))
            },
            Message::MarkdownIndexFinished,
        )
    }
    ```
  - In `subscription()`, batch:
    - `spinner_tick` when `self.pdf_loading` is true
    - `annotation_debounce` when `self.pending_annotation_save.is_some()`
    - `scale_factor_sub` listening to `ScaleFactorChanged` window events
  - Handle new messages in `update()`:
    - `Message::MarkdownIndexFinished`
    - `Message::SpinnerTick`
    - `Message::ScaleFactorChanged`
    - `Message::AnnotationDebounceElapsed`
  - In `NameModalInputChanged`, if active modal is QuickNote or AnnotationTags, update `self.pending_annotation_save`.
  - In `NameModalSubmit` / `NameModalSubmitCurrent`, clear `pending_annotation_save` and commit immediately.
  - Handle `Shortcut::ToggleDiagnostics`.
  - Update `view()` to render the diagnostics sidebar panel and pass correct arguments to backlinks and pdf view.
