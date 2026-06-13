#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use md3_editor::buffer::Command;
use md3_shell::gui::paint::{FontRole, PaintOp, line_plan};
use md3_shell::gui::session::MdSession;
use std::fs;

fn session(text: &str) -> MdSession {
    MdSession::new(
        "test.md",
        text,
        md3_shell::gui::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
            std::sync::Mutex::new(cosmic_text::FontSystem::new()),
        )),
    )
}

#[test]
fn golden_draw_plan_matches_snapshot() {
    let golden_md = fs::read_to_string("tests/fixtures/golden.md").unwrap();
    let measurer = md3_shell::gui::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
        std::sync::Mutex::new(cosmic_text::FontSystem::new()),
    ));
    let mut session = MdSession::new("golden.md", &golden_md, measurer);

    // Park the caret on the line with "revealed"
    let buffer = session.doc.buffer();
    let line_idx = buffer
        .text()
        .lines()
        .position(|l| l.contains("revealed"))
        .unwrap();
    session.doc.apply(Command::SetCursor {
        line: line_idx,
        col: 0,
    });

    // Add dummy math/image sizes to measurer so they show up in the plan
    // (the layout and paint code checks the session cache and measurer)
    session.math_cache.insert(
        "E = mc^2".to_string(),
        (
            iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
            100.0,
            20.0,
        ),
    );
    session.math_cache.insert(
        "\\int_0^\\infty e^{-x^2} dx = \\frac{\\sqrt{\\pi}}{2}".to_string(),
        (
            iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
            200.0,
            60.0,
        ),
    );
    session.image_cache.insert(
        "foo.png".to_string(),
        (
            iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
            400.0,
            300.0,
        ),
    );
    session
        .measurer
        .set_math_size("E = mc^2".to_string(), 100.0, 20.0);
    session.measurer.set_math_block_size(
        "\\int_0^\\infty e^{-x^2} dx = \\frac{\\sqrt{\\pi}}{2}".to_string(),
        200.0,
        60.0,
    );
    session
        .measurer
        .set_image_size("foo.png".to_string(), 400.0, 300.0);
    session.doc.remeasure();

    let mut plan_out = String::new();
    let width = 800.0;

    for index in 0..session.doc.line_count() {
        let Some(styled) = session.doc.styled_line(index) else {
            continue;
        };
        let top = session.doc.layout().offset_of(index).unwrap_or(0.0) as f32;
        let height = session.doc.layout().height_of(index).unwrap_or(28.0) as f32;

        plan_out.push_str(&format!(
            "L{:02} [{:?}] '{}'\n",
            index, styled.conceal, styled.display
        ));

        let ops = line_plan(index, &styled, top, height, width, &session);
        for op in ops {
            plan_out.push_str(&format!("  {:?}\n", op));
        }
    }

    let expected_path = "tests/fixtures/golden.plan.txt";
    let expected = fs::read_to_string(expected_path).unwrap_or_default();

    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::write(expected_path, &plan_out).unwrap();
    } else {
        assert_eq!(
            plan_out, expected,
            "Run UPDATE_EXPECT=1 cargo test to update golden snapshot"
        );
    }
}

#[test]
fn prose_paints_with_same_sans_family_used_for_measurement() {
    let session = session("plain **bold** and *italic*\nnext");
    let styled = session.doc.styled_line(0).unwrap();
    let ops = line_plan(0, &styled, 0.0, 36.0, 800.0, &session);
    let fonts = ops
        .iter()
        .filter_map(|op| match op {
            PaintOp::Text { font, .. } => Some(font),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(fonts.contains(&&FontRole::Sans));
    assert!(fonts.contains(&&FontRole::SansBold));
    assert!(fonts.contains(&&FontRole::SansItalic));
}

#[test]
fn inline_math_reserves_its_rendered_width_before_following_text() {
    let mut session = session("before $x$ after\nnext");
    session.doc.apply(Command::SetCursor { line: 1, col: 0 });
    session.math_cache.insert(
        "x".to_string(),
        (
            iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
            40.0,
            20.0,
        ),
    );
    session.measurer.set_math_size("x".to_string(), 40.0, 20.0);
    session.doc.remeasure();

    let styled = session.doc.styled_line(0).unwrap();
    let height = session.doc.layout().height_of(0).unwrap() as f32;
    let ops = line_plan(0, &styled, 0.0, height, 800.0, &session);
    let asset_right = ops
        .iter()
        .find_map(|op| match op {
            PaintOp::Asset { rect, .. } => Some(rect.x + rect.w),
            _ => None,
        })
        .unwrap();
    let after_x = ops
        .iter()
        .find_map(|op| match op {
            PaintOp::Text { content, x, .. } if content.contains("after") => Some(*x),
            _ => None,
        })
        .unwrap();

    assert!(
        after_x >= asset_right,
        "following text starts at {after_x}, inside asset ending at {asset_right}"
    );
}
