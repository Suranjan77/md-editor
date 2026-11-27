//! Non-destructive PDF margin ink handlers: pen/eraser toggle, color selection, commit, erase and undo.

use md_editor_core::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfInkStroke};

use super::*;
use crate::pdf_pane::PdfInkTool;
use crate::views::pdf_ink::{
    DEFAULT_INK_WIDTH_FRAC, ERASE_HIT_RADIUS_FRAC, clamp_fraction_points, hit_test_ink_stroke,
};

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl MdEditor {
    /// `PdfMessage::InkToolToggled`: switch between pen, eraser, or normal PDF reading.
    pub(super) fn on_pdf_ink_tool_toggled(&mut self, tool: Option<PdfInkTool>) -> Task<Message> {
        self.active_panel = ActivePanel::Pdf;
        self.pdf.ink_tool = tool;
        self.pdf.live_ink = None;
        if tool.is_some() {
            self.pdf.selection = None;
        }
        Task::none()
    }

    /// `PdfMessage::InkColorSelected`: choose stroke color (and switch from None/Eraser to Pen).
    pub(super) fn on_pdf_ink_color_selected(&mut self, color: PdfAnnotationColor) -> Task<Message> {
        self.active_panel = ActivePanel::Pdf;
        self.pdf.ink_color = color;
        if self.pdf.ink_tool != Some(PdfInkTool::Pen) {
            self.pdf.ink_tool = Some(PdfInkTool::Pen);
            self.pdf.selection = None;
        }
        Task::none()
    }

    /// `PdfMessage::InkLiveUpdated`: track in-progress stroke points for smooth preview.
    pub(super) fn on_pdf_ink_live_updated(
        &mut self,
        live: Option<(u16, Vec<[f32; 3]>)>,
    ) -> Task<Message> {
        self.pdf.live_ink = live.map(|(page, pts)| (page, clamp_fraction_points(&pts)));
        Task::none()
    }

    /// `PdfMessage::InkStrokeCommitted`: persist a finished non-destructive margin ink stroke.
    pub(super) fn on_pdf_ink_stroke_committed(
        &mut self,
        page: u16,
        points: Vec<[f32; 3]>,
    ) -> Task<Message> {
        self.pdf.live_ink = None;
        let points = clamp_fraction_points(&points);
        if points.len() < 2 {
            return Task::none();
        }
        let Some(doc_id) = self.pdf.document_id.clone() else {
            self.ui.toast = Some("The PDF is still opening; draw again in a moment".into());
            return Task::none();
        };
        let page_size = self
            .pdf
            .page_sizes
            .get(page as usize)
            .copied()
            .flatten()
            .unwrap_or((612.0, 792.0));
        let now = unix_now();
        let ann = PdfAnnotation::new_ink(
            uuid::Uuid::new_v4().to_string(),
            doc_id,
            page,
            self.pdf.ink_color,
            PdfInkStroke {
                size: DEFAULT_INK_WIDTH_FRAC,
                points,
            },
            page_size,
            now,
        );
        if let Err(e) = self.state.save_pdf_annotation(&ann) {
            self.ui.toast = Some(format!("Failed to save margin ink: {e}"));
        } else {
            self.pdf.annotations.entry(page).or_default().push(ann);
        }
        Task::none()
    }

    /// `PdfMessage::InkEraseAt`: erase any margin ink stroke on `page` touching `(x_frac, y_frac)`.
    pub(super) fn on_pdf_ink_erase_at(
        &mut self,
        page: u16,
        x_frac: f32,
        y_frac: f32,
    ) -> Task<Message> {
        let Some(page_anns) = self.pdf.annotations.get(&page) else {
            return Task::none();
        };
        let doomed: Vec<String> = page_anns
            .iter()
            .filter(|a| a.kind == PdfAnnotationKind::Ink)
            .filter(|a| {
                a.ink_stroke
                    .as_ref()
                    .is_some_and(|s| hit_test_ink_stroke(s, x_frac, y_frac, ERASE_HIT_RADIUS_FRAC))
            })
            .map(|a| a.id.clone())
            .collect();
        if doomed.is_empty() {
            return Task::none();
        }
        let mut deleted_ids = Vec::new();
        for id in doomed {
            match self.state.delete_pdf_annotation(&id) {
                Ok(()) => deleted_ids.push(id),
                Err(e) => {
                    self.ui.toast = Some(format!("Failed to erase margin ink: {e}"));
                    break;
                }
            }
        }
        if let Some(anns) = self.pdf.annotations.get_mut(&page) {
            anns.retain(|a| !deleted_ids.iter().any(|d| d == &a.id));
        }
        Task::none()
    }

    /// `PdfMessage::UndoInkStroke`: delete the most recently created margin ink stroke.
    pub(super) fn on_pdf_undo_ink_stroke(&mut self) -> Task<Message> {
        let latest = self
            .pdf
            .annotations
            .values()
            .flatten()
            .enumerate()
            .filter(|(_, a)| a.kind == PdfAnnotationKind::Ink)
            .max_by_key(|(order, a)| (a.created_at, *order))
            .map(|(_, a)| (a.page_index, a.id.clone()));
        let Some((page, id)) = latest else {
            return Task::none();
        };
        if let Err(e) = self.state.delete_pdf_annotation(&id) {
            self.ui.toast = Some(format!("Failed to undo margin ink: {e}"));
            return Task::none();
        }
        if let Some(anns) = self.pdf.annotations.get_mut(&page) {
            anns.retain(|a| a.id != id);
        }
        Task::none()
    }
}
