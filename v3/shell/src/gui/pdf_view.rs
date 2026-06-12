//! PDF pane: continuous scroll over tile-rendered pages (plan §3.3). The
//! engine's `DocLayout` decides *what* is visible and the `TileCache`/
//! `RenderQueue` pair decides *what work happens* — this module only turns
//! decisions into pixmaps and paint calls. Without the `pdfium` feature the
//! pane shows an honest placeholder; the *commands* (zoom input, go-to-page,
//! paging keys) flow through the kernel keymap identically either way.

use std::path::Path;

use iced::widget::{canvas, column, container, stack, text};
use iced::{Element, Fill, Point, Rectangle, Size, mouse};
use md3_kernel::pane::TabId;

use super::Message;
use super::editor_canvas::palette as colors;
#[cfg(feature = "pdfium")]
use super::session::PAGE_GAP;
use super::session::PdfSession;

#[cfg(feature = "pdfium")]
pub fn renderer() -> Option<&'static md3_pdf::render::PdfRenderer> {
    use md3_pdf::render::PdfRenderer;
    static RENDERER: std::sync::OnceLock<Option<PdfRenderer>> = std::sync::OnceLock::new();
    RENDERER
        .get_or_init(|| {
            let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/pdfium");
            PdfRenderer::new(Some(&local))
                .or_else(|_| PdfRenderer::new(None))
                .ok()
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
/// cancel offscreen requests (plan §3.3: dropped, not rendered), render the
/// rest, and drop pixmaps the byte budget evicts. Synchronous for now — a
/// worker thread is a refinement once tiles dominate a profile.
pub fn ensure_tiles(session: &mut PdfSession, abs_path: &Path) {
    #[cfg(feature = "pdfium")]
    {
        let Some(layout) = &session.layout else {
            return;
        };
        let Some(renderer) = renderer() else {
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
            load_page_chars(session, abs_path, page);
        }
        session.queue.retain_visible(&visible);
        for key in &visible {
            if session.cache.touch(*key) {
                continue; // displayable already; recency bumped
            }
            session.queue.schedule(*key);
        }
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
        let _ = abs_path;
        let _ = &session.tiles;
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

/// `#rrggbb` → iced color with the given alpha; highlights fall back to the
/// default highlight yellow on malformed input.
fn quad_color(hex: &str, alpha: f32) -> iced::Color {
    let parsed = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .and_then(|h| u32::from_str_radix(h, 16).ok());
    let rgb = parsed.unwrap_or(0xffd866);
    iced::Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha)
}

pub fn view(session: &PdfSession, tab: TabId) -> Element<'_, Message> {
    if session.layout.is_none() {
        return placeholder(session);
    }
    // Tints live on a second stacked canvas, not in the page frame: within
    // one layer iced_wgpu renders every image after every mesh, so quads
    // filled in the same frame as `draw_image` land *under* the opaque page
    // tiles. The stack gives the tint canvas its own layer, which renders
    // after the page layer. It captures nothing, so all mouse events fall
    // through to the page canvas below.
    stack![
        canvas(PdfCanvas { tab, session }).width(Fill).height(Fill),
        canvas(TintCanvas { session }).width(Fill).height(Fill),
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
}

/// Per-widget drag tracking: cursor moves only become messages between a
/// press and its release.
#[derive(Default)]
pub struct DragState {
    dragging: bool,
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
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), colors::BG);
        let Some(layout) = &self.session.layout else {
            return vec![frame.into_geometry()];
        };
        let viewport = (bounds.width, bounds.height);

        // Page sheets first (white, so unrendered regions read as paper)…
        for page in layout.placed_pages(self.session.scroll, viewport) {
            frame.fill_rectangle(
                Point::new(page.x, page.y),
                Size::new(page.width, page.height),
                iced::Color::WHITE,
            );
        }
        // …then every visible tile we have a pixmap for. Annotation and
        // selection tints are painted by `TintCanvas` on the stacked layer
        // above (see `view`).
        for tile in layout.visible_tiles(self.session.scroll, viewport) {
            if let Some(handle) = self.session.tiles.get(&tile.key) {
                frame.draw_image(
                    Rectangle::new(
                        Point::new(tile.x, tile.y),
                        Size::new(tile.width, tile.height),
                    ),
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
        let Some(layout) = &self.session.layout else {
            return vec![frame.into_geometry()];
        };
        let viewport = (bounds.width, bounds.height);
        let zoom = layout.zoom();
        for page in layout.placed_pages(self.session.scroll, viewport) {
            let project = |x0: f32, y0: f32, x1: f32, y1: f32| {
                (
                    Point::new(page.x + x0 * zoom, page.y + y0 * zoom),
                    Size::new((x1 - x0) * zoom, (y1 - y0) * zoom),
                )
            };
            for a in &self.session.annotations {
                if a.page != page.page {
                    continue;
                }
                let picked = self.session.selected_annotation == Some(a.id);
                let tint = quad_color(&a.color, if picked { 0.55 } else { 0.35 });
                for q in &a.quads {
                    let (origin, size) =
                        project(q.x0 as f32, q.y0 as f32, q.x1 as f32, q.y1 as f32);
                    frame.fill_rectangle(origin, size, tint);
                }
            }
            if let Some(sel) = &self.session.selection
                && sel.page == page.page
            {
                let tint = iced::Color::from_rgba8(90, 130, 255, 0.30);
                for q in &sel.quads {
                    let (origin, size) = project(q.x0, q.y0, q.x1, q.y1);
                    frame.fill_rectangle(origin, size, tint);
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
