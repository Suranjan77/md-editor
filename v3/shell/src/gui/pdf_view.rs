//! PDF pane. With the `pdfium` shell feature the current page renders as a
//! real image through `md3-pdf`'s wired renderer; without it the pane shows
//! an honest placeholder. Either way the *commands* (zoom input, go-to-page)
//! flow through the kernel keymap identically — the BUG-A pair is live.

use std::path::Path;

use iced::widget::{column, container, text};
use iced::{Element, Fill};

use super::Message;
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

/// (Re)render the session's current page. No-op without the feature.
pub fn refresh(session: &mut PdfSession, abs_path: &Path) {
    #[cfg(feature = "pdfium")]
    {
        let Some(renderer) = renderer() else {
            session.status = "libpdfium not found — placeholder view".to_string();
            return;
        };
        match renderer.page_count(abs_path) {
            Ok(count) => session.page_count = u32::from(count),
            Err(e) => {
                session.status = format!("cannot open: {e}");
                session.frame = None;
                return;
            }
        }
        session.page = session.page.min(session.page_count.saturating_sub(1));
        // 1.5 ≈ comfortable reading scale at 96 dpi; zoom multiplies it.
        match renderer.render_page(abs_path, session.page, 1.5 * session.zoom) {
            Ok(page) => {
                let handle = iced::widget::image::Handle::from_rgba(
                    page.width,
                    page.height,
                    page.rgba,
                );
                session.frame = Some((page.width, page.height, handle));
                session.status = format!(
                    "p. {}/{} · {:.0}%",
                    session.page + 1,
                    session.page_count,
                    session.zoom * 100.0
                );
            }
            Err(e) => {
                session.frame = None;
                session.status = format!("render failed: {e}");
            }
        }
    }
    #[cfg(not(feature = "pdfium"))]
    {
        let _ = abs_path;
        session.status =
            "built without the `pdfium` feature — run with `--features pdfium`".to_string();
    }
}

pub fn view(session: &PdfSession) -> Element<'_, Message> {
    let body: Element<'_, Message> = match &session.frame {
        Some((_, _, handle)) => iced::widget::scrollable(
            container(iced::widget::image(handle.clone()))
                .center_x(Fill)
                .padding(8),
        )
        .width(Fill)
        .height(Fill)
        .into(),
        None => container(
            column![
                text(session.rel_path.clone()).size(16),
                text(session.status.clone()).size(13),
                text("ctrl+g go to page · ctrl+z zoom · pgup/pgdn pages").size(12),
            ]
            .spacing(8),
        )
        .center(Fill)
        .into(),
    };
    column![
        body,
        container(text(session.status.clone()).size(12)).padding([2, 8]),
    ]
    .into()
}
