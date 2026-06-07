use crate::domain::pdf::PdfTextLine;
use image::DynamicImage;
use pdfium_render::prelude::*;

pub fn render_page_from_cache<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
    index: u16,
    scale: f32,
) -> Result<DynamicImage, String> {
    if current_document
        .as_ref()
        .map(|(p, _)| p != path)
        .unwrap_or(true)
    {
        let doc = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
        *current_document = Some((path.to_string(), doc));
    }
    let Some((_, doc)) = current_document.as_ref() else {
        return Err("PDF document was not loaded".to_string());
    };
    let pages = doc.pages();
    if i32::from(index) >= pages.len() {
        return Err("Page index out of bounds".to_string());
    }
    let page = pages
        .get(index as i32)
        .map_err(|e| format!("Failed to get page: {:?}", e))?;

    let render_config = PdfRenderConfig::new()
        .set_target_width((page.width().value * scale) as i32)
        .set_target_height((page.height().value * scale) as i32);

    let bitmap = page
        .render_with_config(&render_config)
        .map_err(|e| format!("Failed to render page: {:?}", e))?;

    bitmap
        .as_image()
        .map_err(|e| format!("Failed to convert to image: {:?}", e))
}

pub fn link_preview_crop(
    full_width: u32,
    full_height: u32,
    target_y: Option<f32>,
    scale: f32,
) -> (u32, u32, u32, u32, f32) {
    if full_height == 0 {
        return (0, 0, full_width.max(1), 1, 0.5);
    }

    let desired_w = (full_width as f32 * 0.82).round().max(1.0) as u32;
    let crop_w = desired_w.min(full_width.max(1));
    let crop_x = full_width.saturating_sub(crop_w) / 2;

    let desired_h = (720.0 * scale).round().max(1.0) as u32;
    let crop_h = desired_h.min(full_height);
    let target = target_y.unwrap_or(full_height as f32 / (2.0 * scale)) * scale;
    let max_y = full_height.saturating_sub(crop_h);
    let crop_y = (target - crop_h as f32 * 0.42)
        .round()
        .clamp(0.0, max_y as f32) as u32;
    let center_ratio = ((target - crop_y as f32) / crop_h as f32).clamp(0.0, 1.0);

    (crop_x, crop_y, crop_w.max(1), crop_h.max(1), center_ratio)
}

pub fn link_preview_content_crop(
    full_width: u32,
    full_height: u32,
    page_width: f32,
    page_height: f32,
    target_y: Option<f32>,
    scale: f32,
    lines: &[PdfTextLine],
) -> (u32, u32, u32, u32, f32) {
    let target = target_y.unwrap_or(page_height / 2.0);
    let mut sorted = lines
        .iter()
        .filter(|line| line.bbox.width > 0.0 && line.bbox.height > 0.0)
        .collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        let a_top = page_height - a.bbox.y - a.bbox.height;
        let b_top = page_height - b.bbox.y - b.bbox.height;
        a_top
            .partial_cmp(&b_top)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if sorted.is_empty() {
        return link_preview_crop(full_width, full_height, target_y, scale);
    }

    let start_idx = sorted
        .iter()
        .position(|line| {
            let line_bottom = page_height - line.bbox.y;
            line_bottom >= target - 8.0
        })
        .unwrap_or_else(|| sorted.len().saturating_sub(1));

    let mut selected = Vec::new();
    let mut block_bottom = page_height - sorted[start_idx].bbox.y;
    let block_top = page_height - sorted[start_idx].bbox.y - sorted[start_idx].bbox.height;

    for line in sorted.iter().skip(start_idx) {
        let line_top = page_height - line.bbox.y - line.bbox.height;
        let line_bottom = page_height - line.bbox.y;
        if !selected.is_empty() {
            let gap = line_top - block_bottom;
            if gap > 34.0 || line_bottom - block_top > 220.0 || selected.len() >= 10 {
                break;
            }
        }
        block_bottom = block_bottom.max(line_bottom);
        selected.push(*line);
    }

    let x_min = selected
        .iter()
        .map(|line| line.bbox.x)
        .fold(page_width, f32::min);
    let x_max = selected
        .iter()
        .map(|line| line.bbox.x + line.bbox.width)
        .fold(0.0, f32::max);
    let y_top = selected
        .iter()
        .map(|line| page_height - line.bbox.y - line.bbox.height)
        .fold(page_height, f32::min);
    let y_bottom = selected
        .iter()
        .map(|line| page_height - line.bbox.y)
        .fold(0.0, f32::max);

    let x_pad = 36.0;
    let y_pad = 42.0;
    let crop_x = ((x_min - x_pad).max(0.0) * scale).round() as u32;
    let right = ((x_max + x_pad).min(page_width) * scale).round() as u32;
    let min_w = (360.0 * scale).round() as u32;
    let crop_w = right
        .saturating_sub(crop_x)
        .max(min_w)
        .min(full_width.saturating_sub(crop_x).max(1));

    let content_top = ((y_top - y_pad).max(0.0) * scale).round();
    let content_bottom = ((y_bottom + y_pad).min(page_height) * scale).round();
    let content_h = (content_bottom - content_top).max(1.0) as u32;
    let min_h = (180.0 * scale).round() as u32;
    let max_h = (330.0 * scale).round() as u32;
    let crop_h = content_h.max(min_h).min(max_h).min(full_height.max(1));
    let content_center = (content_top + content_bottom) / 2.0;
    let max_y = full_height.saturating_sub(crop_h);
    let crop_y = (content_center - crop_h as f32 / 2.0)
        .round()
        .clamp(0.0, max_y as f32) as u32;
    let target_scaled = target * scale;
    let center_ratio = ((target_scaled - crop_y as f32) / crop_h as f32).clamp(0.0, 1.0);

    (crop_x, crop_y, crop_w, crop_h, center_ratio)
}
