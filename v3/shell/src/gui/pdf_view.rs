//! PDF pane: continuous scroll over tile-rendered pages (plan §3.3). The
//! engine's `DocLayout` decides *what* is visible and the `TileCache`/
//! `RenderQueue` pair decides *what work happens* — this module only turns
//! decisions into pixmaps and paint calls. Without the `pdfium` feature the
//! pane shows an honest placeholder; the *commands* (zoom input, go-to-page,
//! paging keys) flow through the kernel keymap identically either way.

use std::path::Path;

use iced::widget::{canvas, column, container, text};
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

pub fn view(session: &PdfSession, tab: TabId) -> Element<'_, Message> {
    if session.layout.is_none() {
        return placeholder(session);
    }
    canvas(PdfCanvas { tab, session })
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

impl canvas::Program<Message> for PdfCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
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
                    viewport: (bounds.width, bounds.height),
                }))
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                cursor.position_in(bounds)?;
                // Focus the pane; also syncs the real viewport for tiles.
                Some(canvas::Action::publish(Message::PdfScrolled {
                    tab: self.tab,
                    dy: 0.0,
                    viewport: (bounds.width, bounds.height),
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
        // …then every visible tile we have a pixmap for.
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

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}
