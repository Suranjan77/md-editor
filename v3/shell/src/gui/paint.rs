use super::session::PdfSession;
use md3_pdf::TileKey;

#[derive(Debug, Clone, PartialEq)]
pub struct RectPx {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tint {
    Annotation { color: String, picked: bool },
    Selection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TintOp {
    pub rect: RectPx,
    pub tint: Tint,
}

pub fn tint_plan(session: &PdfSession, viewport: (f32, f32)) -> Vec<TintOp> {
    let mut ops = Vec::new();
    let Some(layout) = &session.layout else {
        return ops;
    };
    let zoom = layout.zoom();
    for page in layout.placed_pages(session.scroll, viewport) {
        let project = |x0: f32, y0: f32, x1: f32, y1: f32| RectPx {
            x: page.x + x0 * zoom,
            y: page.y + y0 * zoom,
            w: (x1 - x0) * zoom,
            h: (y1 - y0) * zoom,
        };
        for a in &session.annotations {
            if a.page != page.page {
                continue;
            }
            let picked = session.selected_annotation == Some(a.id);
            for q in &a.quads {
                ops.push(TintOp {
                    rect: project(q.x0 as f32, q.y0 as f32, q.x1 as f32, q.y1 as f32),
                    tint: Tint::Annotation {
                        color: a.color.clone(),
                        picked,
                    },
                });
            }
        }
        if let Some(sel) = &session.selection
            && sel.page == page.page
        {
            for q in &sel.quads {
                ops.push(TintOp {
                    rect: project(q.x0, q.y0, q.x1, q.y1),
                    tint: Tint::Selection,
                });
            }
        }
    }
    ops
}

pub fn page_plan(
    session: &PdfSession,
    viewport: (f32, f32),
) -> (Vec<RectPx>, Vec<(TileKey, RectPx)>) {
    let mut sheets = Vec::new();
    let mut tiles = Vec::new();
    let Some(layout) = &session.layout else {
        return (sheets, tiles);
    };

    for page in layout.placed_pages(session.scroll, viewport) {
        sheets.push(RectPx {
            x: page.x,
            y: page.y,
            w: page.width,
            h: page.height,
        });
    }

    for tile in layout.visible_tiles(session.scroll, viewport) {
        tiles.push((
            tile.key,
            RectPx {
                x: tile.x,
                y: tile.y,
                w: tile.width,
                h: tile.height,
            },
        ));
    }

    (sheets, tiles)
}

use super::session::MdSession;
use md3_editor::layout::{ConcealMode, StyledLine};
use md3_editor::parse::LineKind;
use md3_editor::style::SpanKind;

#[derive(Debug, Clone, PartialEq)]
pub enum PaintRole {
    Text,
    Marker,
    Heading,
    Code,
    Math,
    Link,
    WikiLink,
    Quote,
    Caret,
    CodeBg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FontRole {
    Sans,
    SansBold,
    SansItalic,
    Mono,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssetKind {
    Image(String),
    Math(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
    Text {
        content: String,
        x: f32,
        y: f32,
        size: f32,
        role: PaintRole,
        font: FontRole,
    },
    FillRect {
        rect: RectPx,
        role: PaintRole,
    },
    StrokeRect {
        rect: RectPx,
        role: PaintRole,
        thickness: f32,
    },
    Asset {
        kind: AssetKind,
        rect: RectPx,
    },
}

use super::editor_canvas::{PAD, block_asset_size, content_width, inline_math_size, span_text};

fn marker_is_concealed(kind: &SpanKind, styled: &StyledLine) -> bool {
    matches!(kind, SpanKind::Marker)
        && styled.conceal == ConcealMode::Concealed
        && !matches!(
            styled.kind,
            md3_editor::parse::LineKind::TableRow | md3_editor::parse::LineKind::TableSep
        )
}

fn span_style(kind: &SpanKind, styled: &StyledLine) -> (PaintRole, FontRole) {
    match kind {
        SpanKind::Marker => (PaintRole::Marker, FontRole::Mono),
        SpanKind::Bold => (PaintRole::Text, FontRole::SansBold),
        SpanKind::Italic => (PaintRole::Text, FontRole::SansItalic),
        SpanKind::Code | SpanKind::CodeContent => (PaintRole::Code, FontRole::Mono),
        SpanKind::Math | SpanKind::MathContent => (PaintRole::Math, FontRole::Mono),
        SpanKind::LinkText { .. } => (PaintRole::Link, FontRole::Sans),
        SpanKind::Image { .. } => (PaintRole::Link, FontRole::SansItalic),
        SpanKind::WikiLink => (PaintRole::WikiLink, FontRole::Sans),
        SpanKind::FrontMatter => (PaintRole::Marker, FontRole::Mono),
        SpanKind::QuoteText => (PaintRole::Quote, FontRole::SansItalic),
        SpanKind::Text => {
            if matches!(styled.kind, md3_editor::parse::LineKind::Heading { .. }) {
                (PaintRole::Heading, FontRole::SansBold)
            } else {
                (PaintRole::Text, FontRole::Sans)
            }
        }
    }
}

pub fn line_plan(
    index: usize,
    styled: &StyledLine,
    y: f32,
    line_height: f32,
    width: f32,
    session: &MdSession,
) -> Vec<PaintOp> {
    let mut ops = Vec::new();

    // Block decoration
    match styled.kind {
        LineKind::Rule if styled.conceal == ConcealMode::Concealed => {
            ops.push(PaintOp::FillRect {
                rect: RectPx {
                    x: PAD,
                    y: y + line_height / 2.0,
                    w: content_width(width),
                    h: 1.0,
                },
                role: PaintRole::Marker,
            });
        }
        LineKind::Bullet {
            checkbox: Some(checked),
        } if styled.conceal == ConcealMode::Concealed => {
            ops.push(PaintOp::StrokeRect {
                rect: RectPx {
                    x: PAD,
                    y: y + 5.0,
                    w: 12.0,
                    h: 12.0,
                },
                role: PaintRole::Marker,
                thickness: 1.0,
            });
            if checked {
                ops.push(PaintOp::Text {
                    content: "✓".to_string(),
                    x: PAD + 1.0,
                    y: y + 1.0,
                    size: 14.0,
                    role: PaintRole::Caret,
                    font: FontRole::Sans,
                });
            }
        }
        LineKind::CodeContent => {
            ops.push(PaintOp::FillRect {
                rect: RectPx {
                    x: 4.0,
                    y,
                    w: (width - 8.0).max(0.0),
                    h: line_height,
                },
                role: PaintRole::CodeBg,
            });
        }
        _ => {}
    }

    let chars: Vec<char> = styled.display.chars().collect();
    if styled.conceal == ConcealMode::Concealed {
        if let Some(tex) = session.math_block_at(index) {
            if let Some((_, asset_w, asset_h)) = session.math_cache.get(tex) {
                let available_w = content_width(width);
                let (draw_w, draw_h) =
                    block_asset_size(*asset_w, *asset_h, available_w, 320.0, 1.0);
                ops.push(PaintOp::Asset {
                    kind: AssetKind::Math(tex.to_string()),
                    rect: RectPx {
                        x: PAD + (available_w - draw_w).max(0.0) / 2.0,
                        y: y + (line_height - draw_h).max(0.0) / 2.0,
                        w: draw_w,
                        h: draw_h,
                    },
                });
            }
            return ops;
        }
        if session.is_math_block_continuation(index) {
            return ops;
        }

        let mut has_block_asset = false;
        for span in &styled.spans {
            let (asset, max_h, max_upscale) = match &span.kind {
                SpanKind::Image { url } => (session.image_cache.get(url), 420.0, 1.5),
                SpanKind::MathContent => {
                    let tex = span_text(&chars, span.range.clone());
                    (session.math_cache.get(&tex), 220.0, 1.0)
                }
                _ => continue,
            };
            if let Some((_, asset_w, asset_h)) = asset {
                let available_w = content_width(width);
                let (draw_w, draw_h) =
                    block_asset_size(*asset_w, *asset_h, available_w, max_h, max_upscale);
                let kind = match &span.kind {
                    SpanKind::Image { url } => AssetKind::Image(url.clone()),
                    SpanKind::MathContent => AssetKind::Math(span_text(&chars, span.range.clone())),
                    _ => unreachable!(),
                };
                ops.push(PaintOp::Asset {
                    kind,
                    rect: RectPx {
                        x: PAD + (available_w - draw_w).max(0.0) / 2.0,
                        y: y + (line_height - draw_h).max(0.0) / 2.0,
                        w: draw_w,
                        h: draw_h,
                    },
                });
                has_block_asset = true;
                break;
            }
        }
        if has_block_asset {
            return ops;
        }
    }

    let (metrics, pad_top, _) = session.measurer.line_metrics(styled);
    if styled.conceal == ConcealMode::Concealed
        && matches!(styled.kind, LineKind::TableRow | LineKind::TableSep)
        && let Some(widths) = session.table_widths(index)
    {
        let is_sep = matches!(styled.kind, LineKind::TableSep);
        let cells = styled.display.split('|').skip(1).collect::<Vec<_>>();
        let cell_count = if cells.last().is_some_and(|&s| s.trim().is_empty()) {
            cells.len().saturating_sub(1)
        } else {
            cells.len()
        };

        let mut current_x = PAD;

        for (i, cell) in cells.iter().take(cell_count).enumerate() {
            let w = *widths.get(i).unwrap_or(&0.0) + 16.0; // 16px padding

            if is_sep {
                ops.push(PaintOp::FillRect {
                    rect: RectPx {
                        x: current_x,
                        y: y + line_height / 2.0,
                        w: w - 8.0,
                        h: 1.0,
                    },
                    role: PaintRole::Marker,
                });
            } else {
                ops.push(PaintOp::StrokeRect {
                    rect: RectPx {
                        x: current_x,
                        y,
                        w,
                        h: line_height,
                    },
                    role: PaintRole::Marker,
                    thickness: 1.0,
                });
                let cell_text = cell.trim();
                if !cell_text.is_empty() {
                    let cell_styled =
                        md3_editor::layout::StyledLine::plain(cell_text, ConcealMode::Concealed);
                    let cell_buffer = session.measurer.create_buffer(&cell_styled, w);
                    for run in cell_buffer.layout_runs() {
                        for glyph in run.glyphs.iter() {
                            ops.push(PaintOp::Text {
                                content: cell_text[glyph.start..glyph.end].to_string(),
                                x: current_x + 8.0 + glyph.x, // padding
                                y: y + pad_top + run.line_y + glyph.y,
                                size: metrics.font_size,
                                role: PaintRole::Text,
                                font: FontRole::Sans,
                            });
                        }
                    }
                }
            }

            current_x += w;
        }
        return ops;
    }

    let buffer = session.measurer.create_buffer(styled, content_width(width));

    let char_indices: Vec<usize> = styled.display.char_indices().map(|(b, _)| b).collect();
    let mut byte_to_char = vec![0; styled.display.len() + 1];
    for (c_idx, &b_idx) in char_indices.iter().enumerate() {
        byte_to_char[b_idx] = c_idx;
    }
    byte_to_char[styled.display.len()] = char_indices.len();

    let mut painted_inline_math = std::collections::HashSet::new();
    for run in buffer.layout_runs() {
        if run.glyphs.is_empty() {
            continue;
        }

        let mut chunk_start_idx = 0;
        while chunk_start_idx < run.glyphs.len() {
            let start_glyph = &run.glyphs[chunk_start_idx];
            let start_char = byte_to_char[start_glyph.start];

            let span = styled.spans.iter().find(|s| s.range.contains(&start_char));
            if let Some(span) = span {
                if marker_is_concealed(&span.kind, styled) {
                    chunk_start_idx += 1;
                    continue;
                }

                let mut chunk_end_idx = chunk_start_idx + 1;
                while chunk_end_idx < run.glyphs.len() {
                    let g = &run.glyphs[chunk_end_idx];
                    let c = byte_to_char[g.start];
                    if !span.range.contains(&c) {
                        break;
                    }
                    chunk_end_idx += 1;
                }

                if matches!(span.kind, SpanKind::Math) && styled.conceal == ConcealMode::Concealed {
                    let tex = span_text(&chars, span.range.clone());
                    if painted_inline_math.insert(span.range.clone())
                        && let Some((_, asset_w, asset_h)) = session.math_cache.get(&tex)
                    {
                        let available_w = content_width(width);
                        let (draw_w, draw_h) = inline_math_size(*asset_w, *asset_h, available_w);
                        // Center the rendered glyph on the same text row the
                        // surrounding prose paints against. Text tops at
                        // `run.line_y - font_size` (cosmic's visual line top);
                        // centering within that font-size box keeps the math
                        // vertically aligned with the text rather than riding
                        // a `line_height`-tall box that sits ~7px too high.
                        ops.push(PaintOp::Asset {
                            kind: AssetKind::Math(tex.clone()),
                            rect: RectPx {
                                x: PAD + start_glyph.x,
                                y: y + pad_top + run.line_y - metrics.font_size
                                    + (metrics.font_size - draw_h) / 2.0,
                                w: draw_w,
                                h: draw_h,
                            },
                        });
                    }
                } else {
                    let end_glyph = &run.glyphs[chunk_end_idx - 1];
                    let chunk_text = &styled.display[start_glyph.start..end_glyph.end];
                    let (role, font) = span_style(&span.kind, styled);
                    ops.push(PaintOp::Text {
                        content: chunk_text.to_string(),
                        x: PAD + start_glyph.x,
                        y: y + pad_top + run.line_y - metrics.font_size,
                        size: metrics.font_size,
                        role,
                        font,
                    });
                }

                chunk_start_idx = chunk_end_idx;
            } else {
                chunk_start_idx += 1;
            }
        }
    }
    ops
}
