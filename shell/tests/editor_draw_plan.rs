#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use md_editor::buffer::Command;
use md_shell::gui::paint::{FontRole, PaintOp, line_plan};
use md_shell::gui::session::MdSession;
use std::fs;

fn session(text: &str) -> MdSession {
    MdSession::new(
        "test.md",
        text,
        md_shell::gui::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
            std::sync::Mutex::new(cosmic_text::FontSystem::new()),
        )),
    )
}

#[test]
fn golden_draw_plan_matches_snapshot() {
    let golden_md = fs::read_to_string("tests/fixtures/golden.md").unwrap();
    let measurer = md_shell::gui::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
        std::sync::Mutex::new(cosmic_text::FontSystem::new()),
    ));
    let mut session = MdSession::new("golden.md", &golden_md, measurer);

    // Park the caret *inside* the italic word on the "revealed" line so the
    // snapshot exercises element-level reveal (only that element shows its
    // `*` markers; the rest of the line stays concealed).
    let buffer = session.doc.buffer();
    let line_idx = buffer
        .text()
        .lines()
        .position(|l| l.contains("revealed"))
        .unwrap();
    let col = buffer
        .text()
        .lines()
        .nth(line_idx)
        .unwrap()
        .find("revealed")
        .unwrap()
        + 1;
    session.doc.apply(Command::SetCursor {
        line: line_idx,
        col,
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
fn blockquote_bar_and_text_share_the_reading_column() {
    let mut session = session("intro\n> quoted text");
    session.doc.apply(Command::SetCursor { line: 0, col: 0 });
    let styled = session.doc.styled_line(1).unwrap();
    let ops = line_plan(1, &styled, 0.0, 40.0, 1200.0, &session);

    let bar_x = ops.iter().find_map(|op| match op {
        PaintOp::FillRect {
            rect,
            role: md_shell::gui::paint::PaintRole::Quote,
        } => Some(rect.x),
        _ => None,
    });
    let text_x = ops.iter().find_map(|op| match op {
        PaintOp::Text { x, content, .. } if content.contains("quoted") => Some(*x),
        _ => None,
    });

    // content_left(1200) = (1200 - 752 reading width) / 2 = 224; bar sits 4px in.
    assert_eq!(bar_x, Some(228.0));
    assert!(text_x.is_some_and(|x| x >= 202.0));
}

#[test]
fn table_cell_text_stays_within_its_cell_box() {
    // Caret on line 0 so the table rows (1..) render concealed as a grid.
    let session = session("intro\n| Heading | Another |\n| --- | --- |\n| Cell 1 | Cell 2 |\n");

    let mut checked = 0;
    for index in 1..session.doc.line_count() {
        let Some(styled) = session.doc.styled_line(index) else {
            continue;
        };
        let top = session.doc.layout().offset_of(index).unwrap_or(0.0) as f32;
        let height = session.doc.layout().height_of(index).unwrap_or(36.0) as f32;
        let ops = line_plan(index, &styled, top, height, 800.0, &session);

        // The cell box is the StrokeRect; every text glyph on this row must
        // sit inside it vertically (no spilling into the neighbouring row).
        let cell = ops.iter().find_map(|op| match op {
            PaintOp::StrokeRect { rect, .. } => Some(rect.clone()),
            _ => None,
        });
        let Some(cell) = cell else { continue };
        for op in &ops {
            if let PaintOp::Text { y, size, .. } = op {
                assert!(
                    *y >= cell.y - 0.5 && *y + *size <= cell.y + cell.h + 0.5,
                    "cell text at y={y} size={size} escapes box [{}, {}]",
                    cell.y,
                    cell.y + cell.h
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "expected at least one rendered table cell glyph"
    );
}

/// ADR-0106: token boundaries change paint only. The measured geometry of a
/// highlighted code line must equal the same code measured as plain content.
#[test]
fn syntax_highlighting_is_geometry_invariant() {
    use md_editor::layout::{ConcealMode, Measurer, Styler};
    use md_editor::parse::BlockState;
    use md_editor::style::{MarkdownStyler, SpanKind};
    use md_editor::syntax::{Lang, LexState};

    let measurer = md_shell::gui::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
        std::sync::Mutex::new(cosmic_text::FontSystem::new()),
    ));
    let code = "fn main() { let x = 42; /* note */ }";

    // Highlighted (rust) vs plain (no language) styling of the identical line.
    let highlighted = MarkdownStyler.style(
        code,
        &BlockState::Fence {
            marker: '`',
            len: 3,
            lang: Lang::Rust,
            lex: LexState::Normal,
        },
        ConcealMode::Concealed,
    );
    let plain = MarkdownStyler.style(
        code,
        &BlockState::Fence {
            marker: '`',
            len: 3,
            lang: Lang::None,
            lex: LexState::Normal,
        },
        ConcealMode::Concealed,
    );

    // Sanity: the highlighted line really did split into multiple role spans,
    // while plain stayed a single code span — otherwise the test is vacuous.
    assert!(highlighted.spans.len() > 1);
    assert_eq!(plain.spans.len(), 1);
    assert!(
        highlighted
            .spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::CodeToken(_)))
    );

    // Same display text, same measured height/rows — geometry is untouched.
    assert_eq!(highlighted.display, plain.display);
    assert_eq!(
        measurer.measure(&highlighted, 800.0),
        measurer.measure(&plain, 800.0)
    );
}

/// Known-language code paints with semantic syntax roles; an unknown language
/// falls back to the single plain `Code` role (no `Syntax` ops).
#[test]
fn known_language_paints_syntax_roles_unknown_falls_back() {
    use md_shell::gui::paint::PaintRole;

    let rust = session("intro\n```rust\nlet x = 1;\n```\n");
    let plain = session("intro\n```text\nlet x = 1;\n```\n");

    // Line 2 is the code content in both.
    let rust_ops = {
        let styled = rust.doc.styled_line(2).unwrap();
        line_plan(2, &styled, 0.0, 24.0, 800.0, &rust)
    };
    let plain_ops = {
        let styled = plain.doc.styled_line(2).unwrap();
        line_plan(2, &styled, 0.0, 24.0, 800.0, &plain)
    };

    let has_syntax = |ops: &[PaintOp]| {
        ops.iter().any(|op| {
            matches!(
                op,
                PaintOp::Text {
                    role: PaintRole::Syntax(_),
                    ..
                }
            )
        })
    };
    assert!(
        has_syntax(&rust_ops),
        "rust code should produce syntax roles"
    );
    assert!(
        !has_syntax(&plain_ops),
        "unknown language must fall back to the plain code role"
    );
    // The fallback still paints the code text with the base Code role.
    assert!(plain_ops.iter().any(|op| matches!(
        op,
        PaintOp::Text {
            role: PaintRole::Code,
            ..
        }
    )));
}

/// Paint must not tokenize: the styled line handed to `line_plan` already
/// carries the role-tagged spans (tokenization happens in the editor during
/// styling/measure, off the viewport paint path).
#[test]
fn tokens_are_precomputed_before_paint() {
    use md_editor::style::SpanKind;

    let rust = session("intro\n```rust\nlet x = 1;\n```\n");
    let styled = rust.doc.styled_line(2).unwrap();
    assert!(
        styled
            .spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::CodeToken(_))),
        "styled_line already carries syntax tokens before paint runs"
    );
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
