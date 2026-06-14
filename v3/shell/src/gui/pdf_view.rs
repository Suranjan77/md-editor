//! PDF pane: continuous scroll over tile-rendered pages (plan §3.3). The
//! engine's `DocLayout` decides *what* is visible and the `TileCache`/
//! `RenderQueue` pair decides *what work happens* — this module only turns
//! decisions into pixmaps and paint calls. Without the `pdfium` feature the
//! pane shows an honest placeholder; the *commands* (zoom input, go-to-page,
//! paging keys) flow through the kernel keymap identically either way.

use std::path::Path;

use iced::widget::{button, canvas, column, container, row, stack, text};
use iced::{Background, Border, Element, Fill, Padding, Point, Rectangle, Size, mouse};
use md3_kernel::CommandId;
use md3_kernel::pane::TabId;

use super::Message;
use super::icons::{self, Icon};
use super::paint::{self, Tint};
#[cfg(feature = "pdfium")]
use super::session::PAGE_GAP;
use super::session::PdfSession;
use super::tokens;
use super::worker::{PdfJob, WorkerHandle};

#[cfg(feature = "pdfium")]
pub fn renderer() -> Option<&'static md3_pdf::render::PdfRenderer> {
    use md3_pdf::render::PdfRenderer;
    static RENDERER: std::sync::OnceLock<Option<PdfRenderer>> = std::sync::OnceLock::new();
    RENDERER
        .get_or_init(|| {
            crate::paths::pdfium_dirs()
                .into_iter()
                .find_map(|dir| PdfRenderer::new(Some(&dir)).ok())
                .or_else(|| {
                    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/pdfium");
                    PdfRenderer::new(Some(&local)).ok()
                })
                .or_else(|| PdfRenderer::new(None).ok())
        })
        .as_ref()
}

/// Load page geometry into the session (idempotent; cheap after the first
/// call). No-op without the feature — `layout` stays `None` and the
/// placeholder explains why.
pub fn load_geometry(session: &mut PdfSession, abs_path: &Path) {
    if session.layout.is_some() {
        return;
    }
    #[cfg(feature = "pdfium")]
    {
        let Some(renderer) = renderer() else {
            session.status = "libpdfium not found — placeholder view".to_string();
            return;
        };
        let count = match renderer.page_count(abs_path) {
            Ok(n) => n,
            Err(e) => {
                session.status = format!("cannot open: {e}");
                return;
            }
        };
        let mut sizes = Vec::with_capacity(count as usize);
        for page in 0..u32::from(count) {
            match renderer.page_size(abs_path, page) {
                Ok(size) => sizes.push(size),
                Err(e) => {
                    session.status = format!("cannot read page {}: {e}", page + 1);
                    return;
                }
            }
        }
        session.layout = Some(md3_pdf::DocLayout::new(sizes, session.zoom, PAGE_GAP));
        session.outline = renderer.outline(abs_path).unwrap_or_default();
        session.status = String::new();
    }
    #[cfg(not(feature = "pdfium"))]
    {
        let _ = abs_path;
        session.status =
            "built without the `pdfium` feature — run with `--features pdfium`".to_string();
    }
}

/// Make every tile the viewport needs displayable: schedule missing ones,
/// cancel offscreen requests (plan §3.3: dropped, not rendered), and drop
/// pixmaps the byte budget evicts. With a worker, queued tiles are submitted
/// and arrive as [`Message::PdfWorker`] results; without one (windowless
/// tests, or before the subscription's handshake) they render inline.
pub fn ensure_tiles(session: &mut PdfSession, abs_path: &Path, worker: Option<&WorkerHandle>) {
    #[cfg(feature = "pdfium")]
    {
        let Some(layout) = &session.layout else {
            return;
        };
        let placed = layout.visible_tiles(session.scroll, session.viewport);
        let visible: std::collections::HashSet<md3_pdf::TileKey> =
            placed.iter().map(|t| t.key).collect();
        // Glyph geometry for the visible pages, so hovering shows the text
        // cursor before any click and a drag never starts against an
        // unloaded page (idempotent per page).
        let pages: Vec<u32> = layout
            .visible_pages(session.scroll, session.viewport.1)
            .map(|p| p as u32)
            .collect();
        for page in pages {
            request_page_chars(session, abs_path, page, worker);
            request_page_links(session, abs_path, page, worker);
        }
        session.queue.retain_visible(&visible);
        for key in &visible {
            if session.cache.touch(*key) {
                continue; // displayable already; recency bumped
            }
            if session.tiles_in_flight.contains(key) {
                continue; // already at the worker
            }
            session.queue.schedule(*key);
        }
        if let Some(worker) = worker {
            while let Some(key) = session.queue.pop() {
                session.tiles_in_flight.insert(key);
                worker.submit(PdfJob::Tile {
                    path: abs_path.to_path_buf(),
                    key,
                });
            }
            return;
        }
        let Some(renderer) = renderer() else {
            return;
        };
        while let Some(key) = session.queue.pop() {
            match renderer.render_tile(abs_path, key) {
                Ok(tile) => {
                    let bytes = tile.byte_size();
                    let handle =
                        iced::widget::image::Handle::from_rgba(tile.width, tile.height, tile.rgba);
                    for evicted in session.cache.insert(key, bytes) {
                        session.tiles.remove(&evicted);
                    }
                    session.tiles.insert(key, handle);
                }
                Err(e) => {
                    session.status = format!("render failed: {e}");
                }
            }
        }
    }
    #[cfg(not(feature = "pdfium"))]
    {
        let _ = (abs_path, worker);
        let _ = &session.tiles;
    }
}

/// Glyph geometry for one page: through the worker when present (tracked in
/// `chars_pending`), inline otherwise. Idempotent either way.
pub fn request_page_chars(
    session: &mut PdfSession,
    abs_path: &Path,
    page: u32,
    worker: Option<&WorkerHandle>,
) {
    if session.chars.contains_key(&page) || session.chars_pending.contains(&page) {
        return;
    }
    match worker {
        Some(worker) => {
            session.chars_pending.insert(page);
            worker.submit(PdfJob::PageGlyphs {
                path: abs_path.to_path_buf(),
                page,
            });
        }
        None => load_page_chars(session, abs_path, page),
    }
}

/// Link rectangles for one page; same shape as [`request_page_chars`].
#[cfg(feature = "pdfium")]
pub fn request_page_links(
    session: &mut PdfSession,
    abs_path: &Path,
    page: u32,
    worker: Option<&WorkerHandle>,
) {
    if session.links.contains_key(&page) || session.links_pending.contains(&page) {
        return;
    }
    match worker {
        Some(worker) => {
            session.links_pending.insert(page);
            worker.submit(PdfJob::PageLinks {
                path: abs_path.to_path_buf(),
                page,
            });
        }
        None => load_page_links(session, abs_path, page),
    }
}

/// Make a page's glyph geometry available for selection (idempotent; one
/// pdfium pass per page per session). Without pdfium the cache stays empty
/// and selection is simply inert.
pub fn load_page_chars(session: &mut PdfSession, abs_path: &Path, page: u32) {
    if session.chars.contains_key(&page) {
        return;
    }
    #[cfg(feature = "pdfium")]
    {
        let Some(renderer) = renderer() else {
            return;
        };
        let chars = renderer.page_chars(abs_path, page).unwrap_or_default();
        session.chars.insert(page, chars);
    }
    #[cfg(not(feature = "pdfium"))]
    {
        let _ = abs_path;
    }
}

/// Load a page's link annotations (idempotent).
#[allow(dead_code)]
pub fn load_page_links(session: &mut PdfSession, abs_path: &Path, page: u32) {
    if session.links.contains_key(&page) {
        return;
    }
    #[cfg(feature = "pdfium")]
    {
        let Some(renderer) = renderer() else {
            return;
        };
        let links = renderer.page_links(abs_path, page).unwrap_or_default();
        session.links.insert(page, links);
    }
    #[cfg(not(feature = "pdfium"))]
    {
        let _ = abs_path;
    }
}

/// `#rrggbb` → iced color with the given alpha; highlights fall back to the
/// default highlight yellow on malformed input.
pub(crate) fn quad_color(hex: &str, alpha: f32, tokens: &tokens::Tokens) -> iced::Color {
    let parsed = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .and_then(|h| u32::from_str_radix(h, 16).ok());
    match parsed {
        Some(rgb) => iced::Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha),
        None => {
            let c = tokens.highlight_default;
            iced::Color {
                r: c.r,
                g: c.g,
                b: c.b,
                a: alpha,
            }
        }
    }
}

pub fn view<'a>(
    session: &'a PdfSession,
    tab: TabId,
    tokens: &'static tokens::Tokens,
) -> Element<'a, Message> {
    if session.layout.is_none() {
        return placeholder(session);
    }
    // Tints live on a second stacked canvas, not in the page frame: within
    // one layer iced_wgpu renders every image after every mesh, so quads
    // filled in the same frame as `draw_image` land *under* the opaque page
    // tiles. The stack gives the tint canvas its own layer, which renders
    // after the page layer. It captures nothing, so all mouse events fall
    // through to the page canvas below.
    let pages = format!("{}/{}", session.current_page() + 1, session.page_count());
    let zoom = format!("{:.0}%", session.zoom * 100.0);
    let control = |icon, command| {
        button(icons::view(icon, tokens.text_primary, 16.0))
            .padding(7)
            .style(button::text)
            .on_press(Message::PdfCommand { tab, command })
    };
    let bar = container(
        row![
            control(Icon::Back, CommandId("pdf.previous-page")),
            button(text(pages).size(12))
                .padding([7, 10])
                .style(button::text)
                .on_press(Message::PdfCommand {
                    tab,
                    command: CommandId("pdf.go-to-page"),
                }),
            control(Icon::Forward, CommandId("pdf.next-page")),
            control(Icon::ZoomOut, CommandId("pdf.zoom-out")),
            button(text(zoom).size(12))
                .padding([7, 10])
                .style(button::text)
                .on_press(Message::PdfCommand {
                    tab,
                    command: CommandId("pdf.zoom-input"),
                }),
            control(Icon::ZoomIn, CommandId("pdf.zoom-in")),
            control(Icon::FitWidth, CommandId("pdf.fit-width")),
            control(Icon::FitPage, CommandId("pdf.fit-page")),
            control(Icon::Find, CommandId("pdf.find")),
            control(Icon::ListTree, CommandId("pdf.toc")),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center),
    )
    .padding(5)
    .style(move |_| container::Style {
        background: Some(Background::Color(tokens.bg_secondary)),
        border: Border {
            color: tokens.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    });
    let positioned = container(bar)
        .width(Fill)
        .height(Fill)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 18.0,
            left: 0.0,
        })
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Bottom);
    stack![
        canvas(PdfCanvas {
            tab,
            session,
            tokens
        })
        .width(Fill)
        .height(Fill),
        canvas(TintCanvas { session, tokens })
            .width(Fill)
            .height(Fill),
        positioned,
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

fn placeholder(session: &PdfSession) -> Element<'_, Message> {
    container(
        column![
            text(session.rel_path.clone()).size(16),
            text(session.status.clone()).size(13),
            text("ctrl+g go to page · ctrl+z zoom · pgup/pgdn scroll").size(12),
        ]
        .spacing(8),
    )
    .center(Fill)
    .into()
}

/// The scrolling page strip. Painting reads geometry straight from the
/// layout at the *real* bounds, so it is always correct; `viewport` on the
/// session only steers which tiles get rendered.
struct PdfCanvas<'a> {
    tab: TabId,
    session: &'a PdfSession,
    tokens: &'static tokens::Tokens,
}

/// Per-widget drag tracking: cursor moves only become messages between a
/// press and its release.
#[derive(Default)]
pub struct DragState {
    dragging: bool,
    viewport: Option<Size>,
}

impl canvas::Program<Message> for PdfCanvas<'_> {
    type State = DragState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let viewport = (bounds.width, bounds.height);
        if state.viewport != Some(bounds.size()) {
            state.viewport = Some(bounds.size());
            return Some(canvas::Action::publish(Message::PdfViewportChanged {
                tab: self.tab,
                width: bounds.width,
                height: bounds.height,
            }));
        }
        match event {
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * 60.0,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                Some(canvas::Action::publish(Message::PdfScrolled {
                    tab: self.tab,
                    dy,
                    viewport,
                }))
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                state.dragging = true;
                Some(canvas::Action::publish(Message::PdfMouseDown {
                    tab: self.tab,
                    pos: (pos.x, pos.y),
                    viewport,
                }))
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let pos = cursor.position_in(bounds)?;
                let abs = cursor.position().unwrap_or(pos);
                Some(canvas::Action::publish(Message::PdfRightClick {
                    tab: self.tab,
                    pos: (pos.x, pos.y),
                    abs_pos: (abs.x, abs.y),
                    viewport,
                }))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let pos = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(Message::PdfMouseDragged {
                    tab: self.tab,
                    pos: (pos.x, pos.y),
                    viewport,
                }))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging =>
            {
                state.dragging = false;
                Some(canvas::Action::publish(Message::PdfMouseUp {
                    tab: self.tab,
                }))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.tokens.bg_secondary);
        let viewport = (bounds.width, bounds.height);

        let (sheets, tiles) = paint::page_plan(self.session, viewport);
        for sheet in sheets {
            frame.fill_rectangle(
                Point::new(sheet.x, sheet.y),
                Size::new(sheet.w, sheet.h),
                iced::Color::WHITE,
            );
        }
        for (key, rect) in tiles {
            if let Some(handle) = self.session.tiles.get(&key) {
                frame.draw_image(
                    Rectangle::new(Point::new(rect.x, rect.y), Size::new(rect.w, rect.h)),
                    canvas::Image::new(handle.clone()),
                );
            }
        }
        vec![frame.into_geometry()]
    }

    /// I-beam over selectable text, pointer over a stored highlight — the
    /// affordances for the two things a press can do here. Glyph geometry is
    /// loaded per visible page by `ensure_tiles`, so hover works pre-click.
    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(pos) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let Some(layout) = &self.session.layout else {
            return mouse::Interaction::default();
        };
        let hit = layout.page_at_point(
            self.session.scroll,
            (bounds.width, bounds.height),
            (pos.x, pos.y),
        );
        let Some((page, pt)) = hit else {
            return mouse::Interaction::default();
        };
        let page = page as u32;
        if self.session.link_at(page, pt).is_some() {
            return mouse::Interaction::Pointer;
        }
        if self.session.annotation_at(page, pt).is_some() {
            return mouse::Interaction::Pointer;
        }
        let over_text = self.session.chars.get(&page).is_some_and(|chars| {
            chars
                .iter()
                .any(|c| pt.0 >= c.x0 && pt.0 <= c.x1 && pt.1 >= c.y0 && pt.1 <= c.y1)
        });
        if over_text {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Annotation tints and the live selection, projected from page points onto
/// the visible sheets. A separate canvas (own render layer) so the
/// translucent quads composite *over* the tile images; it handles no events.
struct TintCanvas<'a> {
    session: &'a PdfSession,
    tokens: &'static tokens::Tokens,
}

impl canvas::Program<Message> for TintCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let viewport = (bounds.width, bounds.height);
        let ops = paint::tint_plan(self.session, viewport);
        for op in ops {
            let color = match &op.tint {
                Tint::Annotation { color, picked } => {
                    quad_color(color, if *picked { 0.55 } else { 0.35 }, self.tokens)
                }
                Tint::Selection => self.tokens.sel_tint,
            };
            frame.fill_rectangle(
                Point::new(op.rect.x, op.rect.y),
                Size::new(op.rect.w, op.rect.h),
                color,
            );
        }
        vec![frame.into_geometry()]
    }
}
