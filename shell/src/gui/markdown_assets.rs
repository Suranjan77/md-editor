use md_editor::style::SpanKind;

use super::editor_canvas::span_text;
use super::session::MdSession;
use super::worker::PdfJob;

pub(super) enum MarkdownActivation {
    Uri(String),
    WikiLink(String),
}

pub(super) fn activation_at(
    session: &MdSession,
    line: usize,
    source_col: usize,
) -> Option<MarkdownActivation> {
    let styled = session.doc.styled_line(line)?;
    let chars = styled.display.chars().collect::<Vec<_>>();
    styled.spans.into_iter().find_map(|span| {
        if !span.range.contains(&source_col) {
            return None;
        }
        match span.kind {
            SpanKind::LinkText { url } => Some(MarkdownActivation::Uri(url)),
            SpanKind::WikiLink => Some(MarkdownActivation::WikiLink(
                chars[span.range].iter().collect(),
            )),
            _ => None,
        }
    })
}

pub(super) fn image_key(url: &str) -> String {
    format!("image:{url}")
}

pub(super) fn math_key(tex: &str) -> String {
    format!("math:{tex}")
}

pub(super) fn jobs(session: &MdSession, document: &std::path::Path) -> Vec<PdfJob> {
    let Some(base_path) = document.parent() else {
        return Vec::new();
    };
    let mut jobs = Vec::new();
    for line in 0..session.doc.line_count() {
        if let Some(tex) = session.math_block_at(line) {
            jobs.push(PdfJob::MarkdownMath {
                document: document.to_path_buf(),
                key: math_key(tex),
                tex: tex.to_string(),
            });
            continue;
        }
        if session.is_math_block_continuation(line) {
            continue;
        }
        let Some(styled) = session.doc.styled_line(line) else {
            continue;
        };
        let chars = styled.display.chars().collect::<Vec<_>>();
        for span in styled.spans {
            match span.kind {
                SpanKind::Image { url } => jobs.push(PdfJob::MarkdownImage {
                    document: document.to_path_buf(),
                    key: image_key(&url),
                    abs_path: base_path.join(url),
                }),
                SpanKind::Math | SpanKind::MathContent => {
                    let tex = span_text(&chars, span.range);
                    if !tex.is_empty() {
                        jobs.push(PdfJob::MarkdownMath {
                            document: document.to_path_buf(),
                            key: math_key(&tex),
                            tex,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    jobs.sort_by(|a, b| job_key(a).cmp(job_key(b)));
    jobs.dedup_by(|a, b| job_key(a) == job_key(b));
    jobs
}

pub(super) fn apply_dimensions(session: &mut MdSession, key: &str, width: f32, height: f32) {
    if let Some(url) = key.strip_prefix("image:") {
        session
            .measurer
            .set_image_size(url.to_string(), width, height);
    } else if let Some(tex) = key.strip_prefix("math:") {
        session
            .measurer
            .set_math_size(tex.to_string(), width, height);
        for line in 0..session.doc.line_count() {
            if session.math_block_at(line) == Some(tex) {
                session.measurer.set_math_block_size(
                    session.doc.buffer().line_text(line),
                    width,
                    height,
                );
            }
        }
    }
}

pub(super) fn install_handle(
    session: &mut MdSession,
    key: &str,
    handle: iced::widget::image::Handle,
    width: f32,
    height: f32,
) {
    if let Some(url) = key.strip_prefix("image:") {
        session
            .image_cache
            .insert(url.to_string(), (handle, width, height));
    } else if let Some(tex) = key.strip_prefix("math:") {
        session
            .math_cache
            .insert(tex.to_string(), (handle, width, height));
    }
    apply_dimensions(session, key, width, height);
}

fn job_key(job: &PdfJob) -> &str {
    match job {
        PdfJob::MarkdownImage { key, .. } | PdfJob::MarkdownMath { key, .. } => key,
        _ => "",
    }
}

pub(super) struct TableCell<'a> {
    pub start: usize,
    pub text: &'a str,
}

pub(super) fn table_cells(source: &str) -> Vec<TableCell<'_>> {
    let mut cells = Vec::new();
    let mut char_start = 0;
    let source_len = source.chars().count();
    for part in source.split('|') {
        let len = part.chars().count();
        if char_start > 0 && !(part.is_empty() && char_start + len == source_len) {
            cells.push(TableCell {
                start: char_start,
                text: part,
            });
        }
        char_start += len + 1;
    }
    cells
}

pub(super) fn render_math(tex: &str) -> Result<(iced::widget::image::Handle, f32, f32), String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list};
    use ratex_parser::parser::parse;
    use ratex_render::{RenderOptions, render_to_png};
    use ratex_types::color::Color as RatexColor;
    use ratex_types::math_style::MathStyle;

    let render_options = RenderOptions {
        font_size: 24.0,
        padding: 4.0,
        background_color: RatexColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.0,
        },
        font_dir: String::new(),
        device_pixel_ratio: 2.0,
    };
    let layout_options = LayoutOptions::default()
        .with_style(MathStyle::Display)
        .with_color(RatexColor {
            r: 0.89,
            g: 0.90,
            b: 0.93,
            a: 1.0,
        });
    let ast = parse(tex).map_err(|error| format!("math parse: {error}"))?;
    let layout_box = layout(&ast, &layout_options);
    let bytes = render_to_png(&to_display_list(&layout_box), &render_options)
        .map_err(|error| format!("math render: {error:?}"))?;
    let image = image::load_from_memory(&bytes).map_err(|error| error.to_string())?;
    Ok((
        iced::widget::image::Handle::from_bytes(bytes),
        image.width() as f32 / 2.0,
        image.height() as f32 / 2.0,
    ))
}

pub(super) fn load_image(
    abs_path: &std::path::Path,
) -> Result<(iced::widget::image::Handle, f32, f32), String> {
    let image = image::open(abs_path).map_err(|error| error.to_string())?;
    let width = image.width();
    let height = image.height();
    Ok((
        iced::widget::image::Handle::from_rgba(width, height, image.into_rgba8().into_raw()),
        width as f32,
        height as f32,
    ))
}

impl MdSession {
    /// Synchronous fallback used by focused tests. Production queues `jobs`.
    pub fn load_visual_assets(&mut self, abs_path: &std::path::Path) {
        let Some(base_path) = abs_path.parent() else {
            return;
        };
        let block_lines = self.refresh_block_metadata();
        for (leader, tex) in self.math_blocks.clone() {
            if !self.math_cache.contains_key(&tex)
                && let Ok((handle, width, height)) = render_math(&tex)
            {
                self.measurer.set_math_block_size(
                    self.doc.buffer().line_text(leader),
                    width,
                    height,
                );
                self.math_cache.insert(tex, (handle, width, height));
            }
        }
        for line in 0..self.doc.line_count() {
            if block_lines.contains(&line) {
                continue;
            }
            let Some(styled) = self.doc.styled_line(line) else {
                continue;
            };
            let chars = styled.display.chars().collect::<Vec<_>>();
            for span in styled.spans {
                match span.kind {
                    SpanKind::Image { url } if !self.image_cache.contains_key(&url) => {
                        if let Ok((handle, width, height)) = load_image(&base_path.join(&url)) {
                            install_handle(self, &image_key(&url), handle, width, height);
                        }
                    }
                    SpanKind::Math | SpanKind::MathContent => {
                        let tex = span_text(&chars, span.range);
                        if !tex.is_empty()
                            && !self.math_cache.contains_key(&tex)
                            && let Ok((handle, width, height)) = render_math(&tex)
                        {
                            install_handle(self, &math_key(&tex), handle, width, height);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.doc.remeasure();
        self.scroll_caret_into_view();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use cosmic_text::FontSystem;
    use md_editor::buffer::Command;

    use super::*;
    use crate::gui::shaped_measurer::ShapedMeasurer;

    fn session(text: &str) -> MdSession {
        MdSession::new(
            "note.md",
            text,
            ShapedMeasurer::new(Arc::new(Mutex::new(FontSystem::new()))),
        )
    }

    #[test]
    fn discovery_does_not_render_assets() {
        let session = session("![plot](plot.png)\n\n$$\nx^2\n$$");
        let jobs = jobs(&session, std::path::Path::new("/vault/note.md"));
        assert_eq!(jobs.len(), 2);
        assert!(session.image_cache.is_empty());
        assert!(session.math_cache.is_empty());
    }

    #[test]
    fn cached_size_stabilizes_layout_until_real_size_changes() {
        let mut session = session("![plot](plot.png)\nafter");
        session.doc.apply(Command::SetCursor { line: 1, col: 0 });
        let initial = session.doc.layout().offset_of(1).unwrap_or(0.0);

        apply_dimensions(&mut session, "image:plot.png", 400.0, 300.0);
        session.doc.remeasure();
        let cached = session.doc.layout().offset_of(1).unwrap_or(0.0);
        assert!(cached > initial);

        apply_dimensions(&mut session, "image:plot.png", 400.0, 300.0);
        session.doc.remeasure();
        assert_eq!(session.doc.layout().offset_of(1).unwrap_or(0.0), cached);

        apply_dimensions(&mut session, "image:plot.png", 800.0, 100.0);
        session.doc.remeasure();
        assert_ne!(session.doc.layout().offset_of(1).unwrap_or(0.0), cached);
    }
}
