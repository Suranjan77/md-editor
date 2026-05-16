use iced::widget::{column, container, row, scrollable, stack, Space};
use iced::{Alignment, Element, Length, Theme, Subscription, Task};

use std::collections::BTreeSet;
use std::sync::Arc;
use image::GenericImageView;

use crate::editor::buffer::DocBuffer;
use crate::messages::EditorAction;
use crate::editor::highlight;
use crate::messages::{Message, Shortcut};
use crate::theme as app_theme;
use crate::views;

pub struct MdEditor {
    state: Arc<md_editor_core::state::AppState>,
    vault_root: Option<String>,
    vault_entries: Vec<md_editor_core::types::FileEntry>,
    selected_path: Option<String>,
    active_path: Option<String>,
    expanded_folders: BTreeSet<String>,
    sidebar_visible: bool,
    backlinks_visible: bool,
    backlinks: Vec<String>,

    // Editor state
    buffer: DocBuffer,
    highlighted_lines: Vec<highlight::StyledLine>,

    // PDF state
    pdf_current_page: u16,
    pdf_total_pages: u16,
    pdf_zoom: f32,
    pdf_pages: Vec<Option<iced::widget::image::Handle>>,
    pdf_dimensions: Vec<Option<(u32, u32)>>,
    active_pdf_path: Option<String>,

    // Modal state
    active_modal: Option<views::modals::ModalType>,
    modal_input: String,

    // Command palette
    command_palette_visible: bool,
    command_palette_query: String,
    commands: Vec<views::command_palette::Command>,

    // Toast
    toast: Option<String>,

    // Search
    search_visible: bool,
    search_query: String,
    search_results: Vec<md_editor_core::types::SearchResult>,
    
    // TOC
    toc_visible: bool,
    toc_entries: Vec<views::toc::TocEntry>,
    image_cache: std::collections::HashMap<String, iced::widget::image::Handle>,
    math_cache: std::collections::HashMap<String, iced::widget::image::Handle>,
}

impl MdEditor {
    pub fn new() -> (Self, Task<Message>) {
        let state = Arc::new(md_editor_core::state::AppState::new());
        let last_vault = md_editor_core::config::get_sys_config(&state, "last_vault").ok().flatten();

        let mut app = Self {
            state,
            vault_root: None,
            vault_entries: Vec::new(),
            selected_path: None,
            active_path: None,
            expanded_folders: BTreeSet::new(),
            sidebar_visible: true,
            backlinks_visible: false,
            backlinks: Vec::new(),
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            pdf_current_page: 0,
            pdf_total_pages: 0,
            pdf_zoom: 1.5,
            pdf_pages: Vec::new(),
            pdf_dimensions: Vec::new(),
            active_pdf_path: None,
            active_modal: None,
            modal_input: String::new(),
            command_palette_visible: false,
            command_palette_query: String::new(),
            commands: views::command_palette::get_commands(),
            toast: None,
            search_visible: false,
            search_query: String::new(),
            search_results: Vec::new(),
            toc_visible: false,
            toc_entries: Vec::new(),
            image_cache: std::collections::HashMap::new(),
            math_cache: std::collections::HashMap::new(),
        };

        if let Some(path) = last_vault {
            app.open_vault(&path);
        }

        (app, Task::none())
    }

    pub fn title(&self) -> String {
        format!(
            "{}Antigravity — {}",
            if self.buffer.dirty { "● " } else { "" },
            self.active_path.as_deref().unwrap_or("New File")
        )
    }

    pub fn theme(&self) -> Theme {
        app_theme::md_editor_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keyboard = iced::keyboard::listen().map(|event| {
            match event {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    if modifiers.command() || modifiers.control() {
                        match key {
                            iced::keyboard::Key::Character(c) if c == "s" => return Message::KeyboardShortcut(Shortcut::Save),
                            iced::keyboard::Key::Character(c) if c == "o" => return Message::KeyboardShortcut(Shortcut::OpenVault),
                            iced::keyboard::Key::Character(c) if c == "n" => return Message::KeyboardShortcut(Shortcut::NewFile),
                            iced::keyboard::Key::Character(c) if c == "f" => return Message::KeyboardShortcut(Shortcut::Search),
                            iced::keyboard::Key::Character(c) if c == "p" => return Message::KeyboardShortcut(Shortcut::CommandPalette),
                            iced::keyboard::Key::Character(c) if c == "b" => return Message::KeyboardShortcut(Shortcut::ToggleSidebar),
                            iced::keyboard::Key::Character(c) if c == "t" => return Message::KeyboardShortcut(Shortcut::TableOfContents),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            Message::Tick
        });

        let toast = if self.toast.is_some() {
            iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastHide)
        } else {
            Subscription::none()
        };

        Subscription::batch(vec![keyboard, toast])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenVaultDialog => {
                return Task::perform(
                    async {
                        let folder = rfd::AsyncFileDialog::new()
                            .set_title("Open Vault Folder")
                            .pick_folder()
                            .await;
                        folder.map(|f| f.path().to_string_lossy().to_string())
                    },
                    Message::VaultOpened,
                );
            }
            Message::VaultOpened(Some(path)) => {
                self.open_vault(&path);
            }
            Message::SidebarToggle => {
                self.sidebar_visible = !self.sidebar_visible;
            }
            Message::SidebarFileClicked(path) => {
                self.selected_path = Some(path.clone());
                if path.ends_with(".md") || path.ends_with(".markdown") {
                    self.active_pdf_path = None;
                    self.open_file(&path);
                } else if path.ends_with(".pdf") {
                    self.active_path = None;
                    self.active_pdf_path = Some(path.clone());
                    return self.open_pdf(&path);
                }
            }
            Message::SidebarFolderToggled(path) => {
                if self.expanded_folders.contains(&path) {
                    self.expanded_folders.remove(&path);
                } else {
                    self.expanded_folders.insert(path);
                }
            }

            Message::FileLoaded(path, content) => {
                self.buffer = DocBuffer::from_text(&content);
                self.active_path = Some(path);
                self.highlight_all();
                self.load_images();
                self.load_math();
                self.toc_entries = views::toc::get_toc(&self.buffer.text());
            }
            Message::EditorContentChanged(c) => {
                self.buffer.insert_at_cursor(&c);
                self.highlight_all();
                self.load_images();
                self.load_math();
                self.toc_entries = views::toc::get_toc(&self.buffer.text());
            }
            Message::EditorAction(action) => {
                match action {
                    EditorAction::Backspace => self.buffer.backspace(),
                    EditorAction::MoveLeft => self.buffer.move_cursor_left(),
                    EditorAction::MoveRight => self.buffer.move_cursor_right(),
                    EditorAction::MoveUp => self.buffer.move_cursor_up(),
                    EditorAction::MoveDown => self.buffer.move_cursor_down(),
                    EditorAction::MoveHome => self.buffer.move_cursor_home(),
                    EditorAction::MoveEnd => self.buffer.move_cursor_end(),
                    EditorAction::Delete => self.buffer.delete(),
                    EditorAction::Undo => self.buffer.undo(),
                    EditorAction::Redo => self.buffer.redo(),
                }
                self.highlight_all();
            }
            Message::EditorSave => {
                if let Some(path) = &self.active_path {
                    let content = self.buffer.text();
                    let _ = md_editor_core::vault::save_file(&self.state, path, &content);
                    self.buffer.dirty = false;
                    self.toast = Some("File saved".to_string());
                }
            }
            Message::EditorCheckboxToggle(line_idx) => {
                let text = self.buffer.text();
                let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
                if let Some(line) = lines.get_mut(line_idx) {
                    if line.trim_start().starts_with("- [ ]") {
                        *line = line.replace("- [ ]", "- [x]");
                    } else if line.trim_start().starts_with("- [x]") {
                        *line = line.replace("- [x]", "- [ ]");
                    }
                }
                let new_content = lines.join("\n");
                self.buffer.set_text(&new_content);
                self.highlight_all();
            }
            Message::EditorCursorMove(line, col) => {
                self.buffer.set_cursor(line, col);
            }

            Message::PdfLoaded(pages) => {
                self.pdf_total_pages = pages;
                self.pdf_pages = vec![None; pages as usize];
                self.pdf_dimensions = vec![None; pages as usize];
                return self.render_all_pdf_pages();
            }
            Message::PdfPageChanged(page) => {
                self.pdf_current_page = page;
                return self.render_current_pdf_page();
            }
            Message::PdfZoomChanged(zoom) => {
                self.pdf_zoom = zoom;
                self.pdf_pages = vec![None; self.pdf_total_pages as usize];
                self.pdf_dimensions = vec![None; self.pdf_total_pages as usize];
                return self.render_all_pdf_pages();
            }
            Message::PdfRendered(page, img) => {
                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(width, height, img.into_rgba8().into_raw());
                if (page as usize) < self.pdf_pages.len() {
                    self.pdf_pages[page as usize] = Some(handle);
                    self.pdf_dimensions[page as usize] = Some((width, height));
                }
            }
            Message::PdfRightClicked(x, y) => {
                self.toast = Some(format!("Ref Preview at {:.2}, {:.2} coming soon...", x, y));
            }

            Message::SearchOpen => { self.search_visible = true; }
            Message::SearchClose => { self.search_visible = false; }
            Message::SearchQueryChanged(q) => {
                self.search_query = q.clone();
                if q.len() > 2 {
                    if let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q) {
                        self.search_results = res;
                    }
                }
            }
            Message::SearchResultClicked(path) => {
                self.search_visible = false;
                self.open_file(&path);
            }

            Message::ToastHide => { self.toast = None; }
            Message::KeyboardShortcut(s) => {
                match s {
                    Shortcut::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
                    Shortcut::Save => return Task::done(Message::EditorSave),
                    Shortcut::OpenVault => return Task::done(Message::OpenVaultDialog),
                    Shortcut::Search => { self.search_visible = true; }
                    Shortcut::CommandPalette => { self.command_palette_visible = true; }
                    Shortcut::TableOfContents => { self.toc_visible = !self.toc_visible; }
                    _ => {}
                }
            }
            Message::ToggleTOC => { self.toc_visible = !self.toc_visible; }
            _ => {}
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.vault_root.is_none() {
            return views::welcome::view();
        }

        let toolbar = views::toolbar::view(
            self.active_path.as_deref(),
            None,
            self.sidebar_visible,
            self.backlinks_visible,
        );

        let sidebar = views::sidebar::view(
            &self.vault_entries,
            self.selected_path.as_deref(),
            self.active_path.as_deref(),
            &self.expanded_folders,
            !self.sidebar_visible,
        );

        let editor_view = scrollable(
            container(
                crate::editor::renderer::Editor::new(
                    &self.buffer,
                    &self.highlighted_lines,
                    &self.image_cache,
                    &self.math_cache,
                    Message::EditorContentChanged,
                    Message::EditorCursorMove,
                    Message::EditorAction,
                    Message::SidebarFileClicked,
                    Message::EditorCheckboxToggle,
                )
            )
            .padding(20)
            .width(Length::Fill)
        )
        .height(Length::Fill);

        let pdf_view: Element<Message, Theme, iced::Renderer> = if let Some(_) = &self.active_pdf_path {
            scrollable(
                views::pdf_viewer::view_continuous(
                    &self.pdf_pages,
                    self.pdf_zoom,
                    &self.pdf_dimensions,
                )
            )
            .height(Length::Fill)
            .into()
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let toc_view: Element<Message, Theme, iced::Renderer> = if self.toc_visible {
            views::toc::view(&self.toc_entries)
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let main_content: Element<Message, Theme, iced::Renderer> = if self.active_pdf_path.is_some() && self.active_path.is_some() {
            row![
                editor_view,
                container(pdf_view)
                    .width(Length::FillPortion(1))
                    .style(|_| container::Style {
                        border: iced::Border {
                            color: app_theme::BORDER,
                            width: 1.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
            ].into()
        } else if self.active_pdf_path.is_some() {
            pdf_view
        } else {
            editor_view.into()
        };

        let content = column![toolbar, main_content].height(Length::Fill);

        let layout = row![sidebar, content, toc_view].height(Length::Fill);

        let mut layers = vec![
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                })
                .into()
        ];

        if self.search_visible {
             layers.push(
                container(views::search::view(&self.search_query, &self.search_results, true))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
                        ..Default::default()
                    })
                    .into()
             );
        }

        if let Some(msg) = &self.toast {
            layers.push(
                container(views::toast::view(msg))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(40)
                    .into()
            );
        }

        stack(layers).into()
    }

    fn open_vault(&mut self, path: &str) {
        self.vault_root = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_vault", path);
        self.state.vault_root.lock().unwrap().replace(std::path::PathBuf::from(path));
        self.vault_entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
    }

    fn open_file(&mut self, path: &str) {
        if let Ok(bytes) = md_editor_core::vault::open_file(&self.state, path) {
            if let Ok(content) = String::from_utf8(bytes) {
                self.buffer = DocBuffer::from_text(&content);
                self.active_path = Some(path.to_string());
                self.toc_entries = views::toc::get_toc(&content);
                self.highlighted_lines = highlight::highlight_markdown(&content);
                self.backlinks = md_editor_core::vault::get_backlinks(&self.state, path).unwrap_or_default();
                self.active_pdf_path = None;
            }
        }
    }

    fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let abs_path = md_editor_core::vault::resolve_vault_path(
            self.state.vault_root.lock().unwrap().as_ref().unwrap(),
            path
        );
        let path_str = abs_path.to_string_lossy().to_string();
        self.active_pdf_path = Some(path.to_string());
        self.pdf_current_page = 0;
        self.pdf_pages = Vec::new();
        self.pdf_dimensions = Vec::new();

        let _state = self.state.clone();
        let path_clone = path_str.clone();

        Task::batch(vec![
            Task::perform(async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                renderer.page_count(&path_clone).ok()
            }, |res| Message::PdfLoaded(res.unwrap_or(0))),
            self.render_current_pdf_page(),
        ])
    }

    fn render_current_pdf_page(&self) -> Task<Message> {
        self.render_pdf_page(self.pdf_current_page)
    }

    fn render_pdf_page(&self, page: u16) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else { return Task::none(); };
        let abs_path = md_editor_core::vault::resolve_vault_path(
            self.state.vault_root.lock().unwrap().as_ref().unwrap(),
            path
        );
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = self.pdf_zoom;
        let _state = self.state.clone();

        Task::perform(async move {
            let renderer = _state.pdf_renderer.as_ref()?;
            renderer.render_page(&path_str, page, zoom).ok()
        }, move |res| {
            if let Some(img) = res {
                Message::PdfRendered(page, img)
            } else {
                Message::Tick
            }
        })
    }

    fn render_all_pdf_pages(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        for i in 0..self.pdf_total_pages {
            tasks.push(self.render_pdf_page(i));
        }
        Task::batch(tasks)
    }

    fn highlight_all(&mut self) {
        self.highlighted_lines = highlight::highlight_markdown(&self.buffer.text());
    }

    fn load_images(&mut self) {
        let Some(active_path) = &self.active_path else { return; };
        let Some(vault_root) = &self.vault_root else { return; };
        let base_path = std::path::Path::new(vault_root).join(active_path).parent().unwrap().to_path_buf();

        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if !self.image_cache.contains_key(path) {
                            let img_path = base_path.join(path);
                            if let Ok(img) = image::open(&img_path) {
                                let (width, height) = img.dimensions();
                                let handle = iced::widget::image::Handle::from_rgba(width, height, img.into_rgba8().into_raw());
                                self.image_cache.insert(path.clone(), handle);
                            }
                        }
                    }
                }
            }
        }
    }

    fn load_math(&mut self) {
        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_math {
                    let tex = span.text.trim_matches('$').trim();
                    if !self.math_cache.contains_key(tex) {
                        // For now, render synchronously. 
                        // In a real app, this should be a Task.
                        if let Some(handle) = self.render_latex_to_handle(tex) {
                            self.math_cache.insert(tex.to_string(), handle);
                        }
                    }
                }
            }
        }
    }

    fn render_latex_to_handle(&self, tex: &str) -> Option<iced::widget::image::Handle> {
        use ratex_layout::{layout, LayoutOptions, to_display_list};
        use ratex_parser::parser::parse;
        use ratex_render::{render_to_png, RenderOptions};
        use ratex_types::color::Color as RatexColor;
        use ratex_types::math_style::MathStyle;

        let options = RenderOptions {
            font_size: 20.0,
            padding: 5.0,
            background_color: RatexColor { r: 1.0, g: 1.0, b: 1.0, a: 0.0 },
            font_dir: String::new(),
            device_pixel_ratio: 1.0,
        };

        let layout_opts = LayoutOptions::default()
            .with_style(MathStyle::Display)
            .with_color(RatexColor::BLACK);

        let res = (|| {
            let ast = parse(tex).map_err(|e| format!("Parse error: {}", e))?;
            let lbox = layout(&ast, &layout_opts);
            let display_list = to_display_list(&lbox);
            render_to_png(&display_list, &options).map_err(|e| format!("Render error: {:?}", e))
        })();

        match res {
            Ok(bytes) => Some(iced::widget::image::Handle::from_bytes(bytes)),
            Err(e) => {
                if !e.contains("Parse error") {
                    eprintln!("LaTeX error: {}", e);
                }
                None
            }
        }
    }
}
