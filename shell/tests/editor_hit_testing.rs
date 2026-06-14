#![allow(clippy::unwrap_used)]

use md3_editor::buffer::Command;
use md3_editor::layout::Measurer;
use md3_shell::gui::session::MdSession;
use md3_shell::gui::shaped_measurer::ShapedMeasurer;

fn session(text: &str) -> MdSession {
    MdSession::new(
        "golden.md",
        text,
        ShapedMeasurer::new(std::sync::Arc::new(std::sync::Mutex::new(
            cosmic_text::FontSystem::new(),
        ))),
    )
}

#[test]
fn shaped_position_hit_test_round_trips_every_golden_character() {
    let text = std::fs::read_to_string("tests/fixtures/golden.md").unwrap();
    let mut session = session(&text);
    let width = 768.0;

    for line in 0..session.doc.line_count() {
        for revealed in [false, true] {
            let caret_line = if revealed {
                line
            } else if line + 1 < session.doc.line_count() {
                line + 1
            } else {
                line.saturating_sub(1)
            };
            session.doc.apply(Command::SetCursor {
                line: caret_line,
                col: 0,
            });
            let Some(styled) = session.doc.styled_line(line) else {
                continue;
            };
            for char_index in 0..=styled.display.chars().count() {
                let (x, y, height) = session.measurer.caret_rect(&styled, width, char_index);
                let hit = session.measurer.hit_test(
                    &styled,
                    f64::from(width),
                    f64::from(x),
                    f64::from(y + height / 2.0),
                );
                assert_eq!(
                    hit, char_index,
                    "line {line}, revealed={revealed}, char={char_index}, text={:?}",
                    styled.display
                );
            }
        }
    }
}

#[test]
fn shaped_position_hit_test_round_trips_cjk_and_single_scalar_emoji() {
    let mut session = session("Latin 中文 🙂 emoji\nnext");
    session.doc.apply(Command::SetCursor { line: 1, col: 0 });
    let styled = session.doc.styled_line(0).unwrap();
    let width = 768.0;
    for char_index in 0..=styled.display.chars().count() {
        let (x, y, height) = session.measurer.caret_rect(&styled, width, char_index);
        let hit = session.measurer.hit_test(
            &styled,
            f64::from(width),
            f64::from(x),
            f64::from(y + height / 2.0),
        );
        assert_eq!(hit, char_index, "char={char_index}");
    }
}

#[test]
fn shaped_position_hit_test_round_trips_mixed_direction_text() {
    let mut session = session("Latin עברית 123 عربي end\nnext");
    session.doc.apply(Command::SetCursor { line: 1, col: 0 });
    let styled = session.doc.styled_line(0).unwrap();
    let width = 768.0;
    for char_index in 0..=styled.display.chars().count() {
        let (x, y, height) = session.measurer.caret_rect(&styled, width, char_index);
        let hit = session.measurer.hit_test(
            &styled,
            f64::from(width),
            f64::from(x),
            f64::from(y + height / 2.0),
        );
        assert_eq!(
            hit, char_index,
            "char={char_index}, x={x}, text={:?}",
            styled.display
        );
    }
}
