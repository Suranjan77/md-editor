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
