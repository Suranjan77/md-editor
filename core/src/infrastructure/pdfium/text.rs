use crate::application::pdf_service::PdfSearchMatch;
use crate::domain::pdf::{PdfPageText, PdfRect, PdfTextChar, PdfTextLine, merge_char_rects};
use crate::infrastructure::pdfium::document::ensure_document;
use pdfium_render::prelude::*;

pub fn get_page_text_impl<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
    index: u16,
) -> Result<PdfPageText, String> {
    ensure_document(pdfium, current_document, path)?;
    let Some((_, doc)) = current_document.as_ref() else {
        return Err("PDF document was not loaded".to_string());
    };
    let pages = doc.pages();
    if i32::from(index) >= pages.len() {
        return Err("Page index out of bounds".to_string());
    }
    let page = pages.get(index as i32).map_err(|e| e.to_string())?;
    let text_page = page.text().map_err(|e| e.to_string())?;

    let page_width = page.width().value;
    let page_height = page.height().value;

    let mut text = String::new();
    let mut chars = Vec::new();
    let mut text_index = 0usize;

    for c in text_page.chars().iter() {
        if let Some(ch) = c.unicode_char() {
            let char_index = c.index() as u32;
            text.push(ch);

            let bbox = match c.loose_bounds() {
                Ok(rect) => PdfRect {
                    x: rect.left().value,
                    y: rect.bottom().value,
                    width: rect.width().value,
                    height: rect.height().value,
                },
                Err(_) => PdfRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            };

            chars.push(PdfTextChar {
                char_index,
                text_index,
                ch,
                bbox,
            });
            text_index += 1;
        }
    }

    let mut lines = Vec::new();
    let mut current_line: Option<PdfTextLine> = None;

    for c in chars
        .iter()
        .filter(|c| c.bbox.width > 0.0 && c.bbox.height > 0.0)
    {
        match &mut current_line {
            Some(line) => {
                let line_y_min = line.bbox.y;
                let line_y_max = line.bbox.y + line.bbox.height;
                let c_y_min = c.bbox.y;
                let c_y_max = c.bbox.y + c.bbox.height;

                let overlap = line_y_max.min(c_y_max) - line_y_min.max(c_y_min);
                let min_h = line.bbox.height.min(c.bbox.height);

                if overlap > 0.0 && overlap > 0.3 * min_h {
                    let x_min = line.bbox.x.min(c.bbox.x);
                    let x_max = (line.bbox.x + line.bbox.width).max(c.bbox.x + c.bbox.width);
                    let y_min = line.bbox.y.min(c.bbox.y);
                    let y_max = (line.bbox.y + line.bbox.height).max(c.bbox.y + c.bbox.height);

                    line.bbox.x = x_min;
                    line.bbox.y = y_min;
                    line.bbox.width = x_max - x_min;
                    line.bbox.height = y_max - y_min;
                    line.end_text_index = c.text_index + 1;
                } else {
                    lines.push(current_line.take().unwrap());
                    current_line = Some(PdfTextLine {
                        start_text_index: c.text_index,
                        end_text_index: c.text_index + 1,
                        bbox: c.bbox,
                    });
                }
            }
            None => {
                current_line = Some(PdfTextLine {
                    start_text_index: c.text_index,
                    end_text_index: c.text_index + 1,
                    bbox: c.bbox,
                });
            }
        }
    }
    if let Some(line) = current_line {
        lines.push(line);
    }

    Ok(PdfPageText {
        page_index: index,
        page_width,
        page_height,
        text,
        chars,
        lines,
    })
}

pub fn scan_page_for_search<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
    index: u16,
    query: &str,
    regex: bool,
    match_case: bool,
) -> Vec<PdfSearchMatch> {
    let Ok(page_layer) = get_page_text_impl(pdfium, current_document, path, index) else {
        return Vec::new();
    };
    let page_text = page_layer.text.as_str();

    let re = {
        let pattern = if regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        match regex::RegexBuilder::new(&pattern)
            .case_insensitive(!match_case)
            .build()
        {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        }
    };

    let page_matches: Vec<(usize, usize, Vec<PdfRect>)> = re
        .find_iter(page_text)
        .filter_map(|found| {
            let match_char_idx_in_text = page_text[..found.start()].chars().count();
            let match_char_count = found.as_str().chars().count();

            if match_char_count > 0 {
                let match_end = match_char_idx_in_text + match_char_count;
                let chars = page_layer
                    .chars
                    .iter()
                    .filter(|c| c.text_index >= match_char_idx_in_text && c.text_index < match_end)
                    .cloned()
                    .collect::<Vec<_>>();
                let rects = merge_char_rects(&chars);
                Some((match_char_idx_in_text, match_char_count, rects))
            } else {
                None
            }
        })
        .collect();

    let mut matches = Vec::new();
    for (pos, match_len, rects) in page_matches {
        if rects.is_empty() {
            continue;
        }
        let start = pos.saturating_sub(48);
        let take = match_len + 96;
        let context = page_text
            .chars()
            .skip(start)
            .take(take)
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        matches.push(PdfSearchMatch {
            page_index: index,
            context,
            rects,
        });
    }
    matches
}
