use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use image::GenericImageView;
use std::sync::Arc;

use crate::editor::buffer::{Affinity, DocBuffer, EditorCommand};
use crate::editor::renderer::{CaretView, Reveal};
use crate::messages::{Message, Shortcut};
use crate::motion::{SCROLL_STIFFNESS, ScrollGlide, Spring};
use crate::pdf_notes::{
    append_linked_pdf_note_section, new_linked_pdf_note_content, normalize_note_path, slug_fragment,
};
use crate::theme as app_theme;
use crate::views;
use crate::views::pdf_viewer::PDF_PAGE_LIST_PADDING;

const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";
const EDITOR_SCROLLABLE_ID: &str = "editor_scrollable";
/// How far one arrow key press scrolls a PDF or a presentation.
const ARROW_SCROLL: f32 = 64.0;
/// Space around the editor widget inside its scrollable.
const EDITOR_PADDING: f32 = app_theme::SPACE_6;
/// Upper bound on PDF supersampling. The actual factor tracks the display's
/// scale factor (see `MdEditor::pdf_supersample`); this caps bitmap cost on
/// unusually high-DPI displays.
const PDF_SUPERSAMPLE_MAX: f32 = 3.0;
/// Highest zoom fit-to-width will pick, whether automatic or from the fit
/// button. Manual zoom still goes to 4.0; this only stops a wide pane from
/// blowing a small page up past a comfortable reading size.
const PDF_FIT_MAX_ZOOM: f32 = 2.0;
/// How many pages beyond the rendered window keep their cached bitmap before
/// being evicted. Bounds resident bitmap memory for long PDFs while leaving
/// enough slack that ordinary scrolling reuses cached pages.
const PDF_PAGE_BITMAP_EVICT_MARGIN: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Markdown,
    Pdf,
    Ink,
}

/// Result of attempting to persist the active document. Callers decide how
/// loudly to report each case: an explicit `Ctrl+S` confirms success, autosave
/// stays silent unless it fails, and a file switch refuses to proceed on
/// failure rather than navigating away from unsaved work.
enum SaveOutcome {
    Saved,
    /// Nothing was dirty, or no document is open.
    NothingToDo,
    Failed(String),
}

fn search_result_should_reload(active_path: Option<&str>, target_path: &str) -> bool {
    active_path != Some(target_path)
}

fn pdf_search_generation_is_current(current: u64, incoming: u64) -> bool {
    current == incoming
}

fn rename_open_path(path: &mut Option<String>, old_path: &str, new_path: &str) {
    let Some(current) = path.as_deref() else {
        return;
    };
    if current == old_path {
        *path = Some(new_path.to_string());
    } else if let Some(suffix) = current
        .strip_prefix(old_path)
        .filter(|suffix| suffix.starts_with('/'))
    {
        *path = Some(format!("{new_path}{suffix}"));
    }
}

pub(crate) fn is_supported_image_path(path: &str) -> bool {
    path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".gif")
        || path.ends_with(".bmp")
        || path.ends_with(".webp")
}

fn text_by_char_range(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    text.chars().skip(start).take(end - start).collect()
}

pub struct MdEditor {
    state: Arc<md_editor_core::state::AppState>,
    // Vault navigation: root, file tree, sidebar selection/expansion, backlinks
    vault: crate::vault_state::VaultState,
    active_path: Option<String>,

    // Editor pane: buffer, highlighting, TOC, scroll/viewport, resource caches
    editor: crate::editor_state::EditorPane,

    // PDF viewer (document pages, geometry, annotations, render bookkeeping)
    pdf: crate::pdf_pane::PdfPane,

    // Presentation viewer (prepared deck, zoom, scroll position)
    pptx: crate::pptx_pane::PptxPane,

    // Image viewer + active-viewer mode flag (shell-level routing)
    active_image_path: Option<String>,
    active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    showing_pdf: bool,

    // Study tracker
    tracker: crate::tracker_state::TrackerState,

    // Research graph: cached vault topology, layout, filters, and selection
    graph: crate::graph_state::GraphState,
    // Handwritten notes surface, shown in its own window
    ink: crate::ink::InkState,

    /// The shell window. Closing it exits the app.
    main_window: Option<iced::window::Id>,
    /// The handwriting window, while it is open.
    ink_window: Option<iced::window::Id>,

    // Shell UI chrome: modals, command palette, toast, split view, window size
    ui: crate::ui_state::UiState,

    // Search (query/replace, vault + PDF results, in-document match cache)
    search: crate::search_state::SearchState,
    active_panel: ActivePanel,

    /// Paths this app wrote very recently, so the vault watcher can tell our
    /// own saves apart from genuine external edits.
    self_writes: std::collections::HashMap<String, std::time::Instant>,
    /// Whether the current run of autosave failures has already been reported.
    /// Retries continue quietly; only the first failure raises a toast.
    autosave_failure_reported: bool,

    /// In-flight UI transitions (panels opening, toasts fading). Mirrors the
    /// visibility flags that live on the sub-states; see [`crate::motion`].
    motion: crate::motion::Motion,
}

/// How long a path stays marked as "we just wrote this". Comfortably longer
/// than the vault watcher's debounce, short enough that a real external edit
/// arriving later is never mistaken for our own.
const SELF_WRITE_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

mod documents;
mod editor;
mod graph;
mod ink;
mod links;
mod pdf;
mod pdf_pages;
mod search;
mod shell;
mod shortcuts;
mod subscription;
mod vault;
mod view;

#[cfg(test)]
use editor::editor_command_keeps_cursor_visible;
#[cfg(test)]
use ink::derive_ink_path;
use links::*;
use search::{focus_file_search_input, focus_global_search_input, focus_pdf_search_input};

impl MdEditor {
    /// `startup_file` is a path passed on the command line (typically by the
    /// OS "open with" handler). When set it overrides the remembered
    /// last-vault/last-file: a file inside the last vault opens in that vault,
    /// any other file opens with its parent directory as the vault, and a
    /// directory opens as a vault.
    /// Build the app and open its shell window.
    ///
    /// The app runs as a daemon (the handwriting canvas has its own window),
    /// and a daemon starts with no windows, so opening the main one is part of
    /// booting rather than something the runtime does for us.
    pub fn boot(
        main_window: iced::window::Settings,
        startup_file: Option<std::path::PathBuf>,
    ) -> (Self, Task<Message>) {
        let (mut app, restore) = Self::new_with_startup_file(startup_file);
        let (id, open) = iced::window::open(main_window);
        app.main_window = Some(id);
        (app, Task::batch(vec![open.discard(), restore]))
    }

    pub fn new_with_startup_file(
        startup_file: Option<std::path::PathBuf>,
    ) -> (Self, Task<Message>) {
        let state = Arc::new(md_editor_core::state::AppState::new());
        let startup_notice = state.take_startup_notice();
        let last_vault = md_editor_core::config::get_sys_config(&state, "last_vault")
            .ok()
            .flatten();
        let last_file = md_editor_core::config::get_sys_config(&state, "last_file")
            .ok()
            .flatten();
        let tracker = crate::tracker_state::TrackerState::new(&state);
        let graph = crate::graph_state::GraphState::new();

        let mut app = Self {
            state: state.clone(),
            vault: crate::vault_state::VaultState::new(),
            active_path: None,
            editor: crate::editor_state::EditorPane::new(),
            pdf: crate::pdf_pane::PdfPane::new(),
            pptx: crate::pptx_pane::PptxPane::new(),
            active_image_path: None,
            active_image: None,
            showing_pdf: false,
            tracker,
            graph,
            ink: crate::ink::InkState::new(),
            main_window: None,
            ink_window: None,
            ui: crate::ui_state::UiState::new(),
            search: crate::search_state::SearchState::new(),
            active_panel: ActivePanel::Markdown,
            self_writes: std::collections::HashMap::new(),
            autosave_failure_reported: false,
            motion: crate::motion::Motion::new(),
        };
        app.ui.toast = startup_notice;
        app.restore_layout();

        // Seed the geometry the window was actually restored at. Without this,
        // every layout computation (PDF fit-to-width, editor viewport, modal
        // sizing) would use the 1200x800 default until the first resize event
        // arrived — and on a platform that emits none, forever.
        let restored = crate::restore_window_size();
        app.ui.window_width = restored.width;
        app.ui.window_height = restored.height;

        let startup = startup_file.and_then(|p| resolve_startup_target(&p, last_vault.as_deref()));

        let mut task = Task::none();
        if let Some((vault_root, rel_file)) = startup {
            let index_task = app.open_vault(&vault_root);
            if let Some(file_path) = rel_file {
                task = app.open_startup_file(&file_path);
            }
            task = Task::batch(vec![index_task, task]);
        } else if let Some(path) = last_vault {
            let index_task = app.open_vault(&path);
            if let Some(file_path) = last_file {
                task = app.open_startup_file(&file_path);
            }
            task = Task::batch(vec![index_task, task]);
        }

        (app, task)
    }

    /// Open a vault-relative file by extension (markdown, PDF, or image);
    /// unsupported extensions are ignored.
    fn open_startup_file(&mut self, file_path: &str) -> Task<Message> {
        // Select the reopened file so its row offers delete without a click.
        self.vault.selected_path = Some(file_path.to_string());
        let lower = file_path.to_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            self.open_file(file_path)
        } else if lower.ends_with(".pdf") {
            self.pdf.active_path = Some(file_path.to_string());
            self.showing_pdf = true;
            self.open_pdf(file_path)
        } else if is_supported_image_path(&lower) {
            self.open_image(file_path)
        } else if lower.ends_with(".pptx") {
            self.open_pptx(file_path)
        } else if lower.ends_with(".ink") {
            self.open_ink_page(file_path)
        } else {
            Task::none()
        }
    }

    pub fn title(&self, window: iced::window::Id) -> String {
        if Some(window) == self.ink_window {
            return format!(
                "{}Handwriting — {}",
                if self.ink.dirty { "● " } else { "" },
                self.ink.path.as_deref().unwrap_or("Unsaved page")
            );
        }

        if self.graph.visible {
            return format!(
                "{}Md-editor — Knowledge Graph",
                if self.editor.buffer.dirty { "● " } else { "" }
            );
        }
        format!(
            "{}Md-editor — {}",
            if self.editor.buffer.dirty { "● " } else { "" },
            self.active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref())
                .or(self.pptx.active_path.as_deref())
                .unwrap_or("New File")
        )
    }

    pub fn theme(&self) -> Theme {
        app_theme::md_editor_theme()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_inner(message);

        // Point every transition at the flags that own the real state. Doing
        // it here, once, means a flag flipped anywhere animates without the
        // code that flipped it having to know motion exists.
        self.motion.sync(
            self.vault.sidebar_visible,
            self.editor.toc_visible,
            self.vault.backlinks_visible,
            self.ui.toast.as_deref(),
            self.ui.command_palette_visible,
        );
        // Refresh the memoized search-match cache once per message so the
        // subsequent view() can read it without rescanning the buffer.
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        task
    }

    /// Route a message to the handler for its family. Each `update_*` method
    /// lives in the file of the same name under `app/`.
    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            m @ (Message::OpenVaultDialog
            | Message::VaultOpened(..)
            | Message::VaultIndexed(..)
            | Message::SidebarToggle
            | Message::SidebarFolderToggled(..)
            | Message::SidebarSelectionCleared
            | Message::SidebarFileClicked(..)
            | Message::CreateFileDialog
            | Message::CreateFolderDialog
            | Message::DeleteFileDialog(..)
            | Message::RenameEntryDialog(..)
            | Message::NameModalInputChanged(..)
            | Message::PdfLinkNoteFolderSelected(..)
            | Message::PdfLinkNoteFileSelected(..)
            | Message::PdfLinkNotePickerSearchChanged(..)
            | Message::NameModalCancel
            | Message::CommandPaletteQueryChanged(..)
            | Message::ShowToast(..)
            | Message::ToastHide
            | Message::SplitViewDragStart
            | Message::NameModalSubmit(..)
            | Message::DeleteFile(..)
            | Message::NameModalSubmitCurrent
            | Message::VaultFilesChanged(..)) => self.update_vault(m),
            m @ (Message::GraphToggle
            | Message::GraphRefresh
            | Message::GraphSnapshotLoaded(..)
            | Message::GraphScopeGlobal
            | Message::GraphScopeLocal
            | Message::GraphLocalDepthChanged(..)
            | Message::GraphQueryChanged(..)
            | Message::GraphShowPdfsToggled(..)
            | Message::GraphShowMissingToggled(..)
            | Message::GraphShowOrphansToggled(..)
            | Message::GraphNodeSelected(..)
            | Message::GraphNodeMoved { .. }
            | Message::GraphNodeOpen(..)
            | Message::GraphNodeFocused(..)
            | Message::GraphPinToggled(..)
            | Message::GraphUnpinAll
            | Message::GraphZoomBy(..)
            | Message::GraphPhysicsToggled
            | Message::GraphFiltersReset
            | Message::GraphInspectorToggled
            | Message::GraphFitView
            | Message::GraphResetLayout
            | Message::GraphPhysicsTick
            | Message::GraphSetView { .. }
            | Message::GraphHovered(..)) => self.update_graph(m),
            m @ (Message::CommandPaletteOpen
            | Message::TrackerToggle
            | Message::CommandPaletteFileClicked(..)
            | Message::CommandPaletteCommandClicked(..)
            | Message::OverlaySubmitCurrent
            | Message::TrackerStart
            | Message::TrackerStop
            | Message::TrackerTabSelected(..)
            | Message::TrackerProjectStatusChanged(..)
            | Message::TrackerGateToggled(..)
            | Message::TrackerReadingToggled(..)
            | Message::TrackerConfigEdited(..)
            | Message::TrackerConfigSave
            | Message::TrackerManualDateChanged(..)
            | Message::TrackerManualHoursChanged(..)
            | Message::TrackerManualNotesChanged(..)
            | Message::TrackerManualAdd
            | Message::TrackerSessionDelete(..)
            | Message::SplitViewToggle
            | Message::SplitViewDragging(..)
            | Message::SplitViewDragEnd
            | Message::WindowResized(..)
            | Message::WindowOpened(..)
            | Message::WindowCloseRequested
            | Message::WindowClosed(..)
            | Message::WindowRescaled(..)
            | Message::PptxLoaded(..)
            | Message::PptxScrolled { .. }
            | Message::PptxZoomIn
            | Message::PptxZoomOut
            | Message::PptxZoomFit
            | Message::PptxStep(..)) => self.update_shell(m),
            m @ (Message::EditorCommand(..)
            | Message::EditorCommandNoScroll(..)
            | Message::MathRendered(..)
            | Message::EditorSave
            | Message::AnimationTick(..)
            | Message::AutosaveElapsed
            | Message::EditorCheckboxToggle(..)
            | Message::EditorScrolled { .. }
            | Message::EditorCaretView(..)
            | Message::EditorScrollInterrupted
            | Message::TocClicked(..)
            | Message::ToggleTOC) => self.update_editor(m),
            m @ (Message::PdfLoaded(..)
            | Message::PdfZoomChanged(..)
            | Message::PdfFitToWidth
            | Message::PdfPageSizesLoaded(..)
            | Message::PdfRenderSkipped(..)
            | Message::ClosePdfLinkPreview
            | Message::PdfPageTextLoaded(..)
            | Message::PdfSelectionCleared
            | Message::PdfRendered(..)
            | Message::PdfRenderFailed(..)
            | Message::PdfScrolled { .. }
            | Message::PdfLeftClicked(..)
            | Message::PdfRightClicked(..)
            | Message::PdfLinkPreviewResult(..)
            | Message::PdfTocLoaded(..)
            | Message::PdfPageLinksLoaded(..)
            | Message::PdfSearchLooseToggled(..)
            | Message::PdfSearchResult(..)
            | Message::PdfSearchResultClicked(..)
            | Message::PdfScrollBy(..)
            | Message::PdfDocumentIdComputed(..)
            | Message::PdfReferencesLoaded(..)
            | Message::PdfSelectionChanged(..)
            | Message::PdfSelectionFinished(..)
            | Message::PdfCopySelection
            | Message::PdfCreateHighlight(..)
            | Message::PdfQuickHighlight
            | Message::PdfCopyAnnotationText(..)
            | Message::PdfOrphanReport
            | Message::PdfDeleteHighlight(..)
            | Message::PdfAddQuickNote(..)
            | Message::PdfLinkNote(..)
            | Message::PdfOpenLinkedNote(..)
            | Message::PdfAnnotationFocused { .. }) => self.update_pdf(m),
            m @ (Message::GlobalSearchOpen
            | Message::GlobalSearchSubmit
            | Message::SearchClose
            | Message::SearchQueryChanged(..)
            | Message::SearchDebounceElapsed
            | Message::SearchReplaceChanged(..)
            | Message::SearchRegexToggled(..)
            | Message::SearchMatchCaseToggled(..)
            | Message::SearchPrevious
            | Message::SearchNext
            | Message::SearchReplaceAll
            | Message::SearchResultClicked(..)) => self.update_search(m),
            m @ Message::KeyboardShortcut(..) => self.update_shortcuts(m),
            m @ (Message::InkPenHookReady(..)
            | Message::InkToggle
            | Message::InkPenSamples(..)
            | Message::InkPointerDown(..)
            | Message::InkPointerMove(..)
            | Message::InkPointerUp
            | Message::InkHoverMoved(..)
            | Message::InkToolSelected(..)
            | Message::InkEraserModeSelected(..)
            | Message::InkPaperSelected(..)
            | Message::InkColorSelected(..)
            | Message::InkSizeSelected(..)
            | Message::InkPan { .. }
            | Message::InkZoom { .. }
            | Message::InkZoomStep(..)
            | Message::InkResetView
            | Message::InkDeleteSelection
            | Message::InkUndo
            | Message::InkRedo
            | Message::InkClear
            | Message::InkSave
            | Message::InkSaved(..)) => self.update_ink(m),
        }
    }
}

/// Resolve a path passed on the command line to `(vault_root, vault_relative_file)`.
/// A directory becomes the vault itself; a file inside `last_vault` reuses that
/// vault; any other file uses its parent directory as the vault.
fn resolve_startup_target(
    path: &std::path::Path,
    last_vault: Option<&str>,
) -> Option<(String, Option<String>)> {
    // `std::path::absolute` (not `canonicalize`) to avoid `\\?\` verbatim
    // prefixes on Windows, which would leak into stored vault paths.
    let abs = std::path::absolute(path).ok()?;
    if abs.is_dir() {
        return Some((abs.to_string_lossy().to_string(), None));
    }
    if !abs.is_file() {
        return None;
    }
    let (root, rel) = match last_vault
        .map(std::path::Path::new)
        .and_then(|v| std::path::absolute(v).ok())
        .and_then(|v| {
            abs.strip_prefix(&v)
                .ok()
                .map(|rel| (v.clone(), rel.to_path_buf()))
        }) {
        Some((vault, rel)) => (vault, rel),
        None => {
            let parent = abs.parent()?.to_path_buf();
            let rel = abs.strip_prefix(&parent).ok()?.to_path_buf();
            (parent, rel)
        }
    };
    Some((
        root.to_string_lossy().to_string(),
        Some(rel.to_string_lossy().replace('\\', "/")),
    ))
}

#[cfg(test)]
mod tests;
