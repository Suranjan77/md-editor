#![allow(clippy::unwrap_used, clippy::expect_used)]
use md3_pdf::DocLayout;
use md3_pdf::select::SelRect;
use md3_shell::gui::paint::{Tint, page_plan, tint_plan};
use md3_shell::gui::session::{PdfSelection, PdfSession};
use md3_vault::{Annotation, Quad};

fn setup_session() -> PdfSession {
    let mut session = PdfSession::new("test.pdf");
    // 3 pages, each 612x792, zoom 1.0, gap 16.0
    session.layout = Some(DocLayout::new(vec![(612.0, 792.0); 3], 1.0, 16.0));
    // Set typical viewport (e.g. 1000x800)
    session.viewport = (1000.0, 800.0);
    session
}

#[test]
fn selection_on_a_visible_page_produces_tint_ops() {
    let mut session = setup_session();
    // Page 1 selection (page 1 is 0-based index 1)
    session.selection = Some(PdfSelection {
        page: 1,
        anchor: (10.0, 10.0),
        quads: vec![SelRect {
            x0: 10.0,
            y0: 10.0,
            x1: 200.0,
            y1: 30.0,
        }],
        text: "hello".to_string(),
    });

    // Scroll so page 1 is visible
    // Page 0 top is 0.0, page 1 top is 792.0 + 16.0 = 808.0
    session.scroll = 400.0;

    let (sheets, _) = page_plan(&session, session.viewport);
    assert!(sheets.len() >= 2);
    let page1_sheet = &sheets[1];

    let ops = tint_plan(&session, session.viewport);
    let selection_ops: Vec<_> = ops.iter().filter(|op| op.tint == Tint::Selection).collect();
    assert!(!selection_ops.is_empty());
    for op in selection_ops {
        // Assert its rect lies inside the page-1 sheet rect
        assert!(op.rect.x >= page1_sheet.x);
        assert!(op.rect.y >= page1_sheet.y);
        assert!(op.rect.x + op.rect.w <= page1_sheet.x + page1_sheet.w);
        assert!(op.rect.y + op.rect.h <= page1_sheet.y + page1_sheet.h);
    }
}

#[test]
fn selection_scrolled_offscreen_produces_no_ops() {
    let mut session = setup_session();
    session.selection = Some(PdfSelection {
        page: 2, // Page 2 selection (page 2 is 0-based index 2)
        anchor: (10.0, 10.0),
        quads: vec![SelRect {
            x0: 10.0,
            y0: 10.0,
            x1: 200.0,
            y1: 30.0,
        }],
        text: "world".to_string(),
    });

    // Scroll is at 0.0, only Page 0 and maybe Page 1 is visible, Page 2 is offscreen
    session.scroll = 0.0;

    let ops = tint_plan(&session, session.viewport);
    let selection_ops: Vec<_> = ops.iter().filter(|op| op.tint == Tint::Selection).collect();
    assert!(selection_ops.is_empty());
}

#[test]
fn same_line_selection_single_quad_is_painted() {
    let mut session = setup_session();
    session.selection = Some(PdfSelection {
        page: 0,
        anchor: (10.0, 10.0),
        quads: vec![SelRect {
            x0: 10.0,
            y0: 10.0,
            x1: 200.0,
            y1: 30.0,
        }],
        text: "test".to_string(),
    });
    session.scroll = 0.0;

    let ops = tint_plan(&session, session.viewport);
    let selection_ops: Vec<_> = ops.iter().filter(|op| op.tint == Tint::Selection).collect();
    assert_eq!(selection_ops.len(), 1);
}

#[test]
fn picked_annotation_is_distinguishable() {
    let mut session = setup_session();
    session.annotations = vec![
        Annotation {
            id: 42,
            doc_hash: "hash".to_string(),
            page: 0,
            quads: vec![Quad {
                x0: 10.0,
                y0: 10.0,
                x1: 100.0,
                y1: 30.0,
            }],
            color: "#ff0000".to_string(),
            note: "".to_string(),
            linked_note: None,
            created_at: 0,
            modified_at: 0,
        },
        Annotation {
            id: 43,
            doc_hash: "hash".to_string(),
            page: 0,
            quads: vec![Quad {
                x0: 10.0,
                y0: 40.0,
                x1: 100.0,
                y1: 60.0,
            }],
            color: "#00ff00".to_string(),
            note: "".to_string(),
            linked_note: None,
            created_at: 0,
            modified_at: 0,
        },
    ];
    session.selected_annotation = Some(42);
    session.scroll = 0.0;

    let ops = tint_plan(&session, session.viewport);

    // Find the op for annotation 42 and assert it carries picked: true
    let op_42 = ops.iter().find(|op| {
        if let Tint::Annotation { color, picked } = &op.tint {
            color == "#ff0000" && *picked
        } else {
            false
        }
    });
    assert!(op_42.is_some());

    // Find the op for annotation 43 and assert it carries picked: false
    let op_43 = ops.iter().find(|op| {
        if let Tint::Annotation { color, picked } = &op.tint {
            color == "#00ff00" && !*picked
        } else {
            false
        }
    });
    assert!(op_43.is_some());
}
