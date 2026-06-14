#![allow(clippy::unwrap_used)]
//! Checkbox list items: the box is drawn in a left gutter and the item text is
//! inset clear of it, and the inset is a *measure* input so a wrapped item
//! never paints past its measured height onto the line below.

use md3_editor::buffer::Command;
use md3_shell::gui::paint::{PaintOp, line_plan};
use md3_shell::gui::session::MdSession;

fn session(text: &str) -> MdSession {
    MdSession::new(
        "t.md",
        text,
        md3_shell::gui::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
            std::sync::Mutex::new(cosmic_text::FontSystem::new()),
        )),
    )
}

#[test]
fn checkbox_box_clears_the_item_text() {
    let mut s = session("- [ ] task one\nplain");
    s.doc.set_wrap_width(600.0);
    s.doc.remeasure();
    // Caret off the line so it renders concealed (box + inset text).
    s.doc.apply(Command::SetCursor { line: 1, col: 0 });

    let styled = s.doc.styled_line(0).unwrap();
    let ops = line_plan(0, &styled, 0.0, 28.0, 632.0, &s);

    let mut box_right = 0.0f32;
    let mut first_text_x = f32::MAX;
    for op in &ops {
        match op {
            PaintOp::StrokeRect { rect, .. } => box_right = box_right.max(rect.x + rect.w),
            PaintOp::Text { x, content, .. } if !content.trim().is_empty() && content != "✓" => {
                first_text_x = first_text_x.min(*x)
            }
            _ => {}
        }
    }
    assert!(box_right > 0.0, "checkbox box must be drawn");
    assert!(
        first_text_x >= box_right,
        "item text ({first_text_x}) must start at/after the box right edge ({box_right})"
    );
}

#[test]
fn bullet_and_ordered_show_gutter_markers_with_inset_text() {
    let mut s = session("- bullet item\n1. first\n2. second\nplain");
    s.doc.set_wrap_width(600.0);
    s.doc.remeasure();
    s.doc.apply(Command::SetCursor { line: 3, col: 0 });

    let marker = |idx: usize| -> (Vec<String>, f32) {
        let styled = s.doc.styled_line(idx).unwrap();
        let ops = line_plan(idx, &styled, 0.0, 28.0, 632.0, &s);
        let mut markers = Vec::new();
        let mut first_text_x = f32::MAX;
        for op in &ops {
            if let PaintOp::Text { content, x, .. } = op {
                if content == "•" || content.ends_with('.') {
                    markers.push(content.clone());
                } else if !content.trim().is_empty() {
                    first_text_x = first_text_x.min(*x);
                }
            }
        }
        (markers, first_text_x)
    };

    let (bullet_markers, bullet_text_x) = marker(0);
    assert!(
        bullet_markers.contains(&"•".to_string()),
        "bullet dot drawn"
    );
    assert!(bullet_text_x >= 40.0, "bullet text inset past the gutter");

    let (ord1, _) = marker(1);
    let (ord2, _) = marker(2);
    assert!(
        ord1.contains(&"1.".to_string()),
        "ordinal 1. drawn: {ord1:?}"
    );
    assert!(
        ord2.contains(&"2.".to_string()),
        "ordinal 2. drawn: {ord2:?}"
    );
}

#[test]
fn wrapped_checkbox_stays_within_its_measured_height() {
    let long = "- [ ] this is a very long checkbox item that should certainly wrap \
                across more than one visual row in a narrow editor pane width";
    let mut s = session(&format!("intro\n{long}\ntail"));
    s.doc.set_wrap_width(280.0);
    s.doc.remeasure();
    // Reveal the checkbox line (prefix shown) — the harshest wrap case.
    s.doc.apply(Command::SetCursor { line: 1, col: 8 });

    let width = 280.0 + 56.0;
    for idx in 0..s.doc.line_count() {
        let styled = s.doc.styled_line(idx).unwrap();
        let top = s.doc.layout().offset_of(idx).unwrap() as f32;
        let h = s.doc.layout().height_of(idx).unwrap() as f32;
        let ops = line_plan(idx, &styled, top, h, width, &s);
        // No painted glyph top may spill past the line's measured bottom.
        for op in &ops {
            if let PaintOp::Text { y, .. } = op {
                assert!(
                    *y <= top + h,
                    "L{idx} glyph at y={y} spills past measured bottom {}",
                    top + h
                );
            }
        }
    }
}
