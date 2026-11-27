//! Widget-level tests for the editor's keymap and multi-click selection.

use iced::keyboard::{self, Key, key::Named};
use iced::{Point, Rectangle, Size};

use super::super::testing::View;

const CTRL: keyboard::Modifiers = keyboard::Modifiers::CTRL;
const SHIFT: keyboard::Modifiers = keyboard::Modifiers::SHIFT;
const NONE: keyboard::Modifiers = keyboard::Modifiers::empty();

fn named(view: &mut View, key: Named, modifiers: keyboard::Modifiers) -> Vec<String> {
    view.focus();
    view.key(Key::Named(key), modifiers, None).messages
}

#[test]
fn ctrl_arrows_home_end_and_backspace_act_on_words_and_the_document() {
    let mut view = View::new("one two three", 600.0);
    let cases = [
        (
            Named::ArrowLeft,
            CTRL,
            "cmd MoveCursor { movement: WordLeft, extend: false }",
        ),
        (
            Named::ArrowRight,
            CTRL | SHIFT,
            "cmd MoveCursor { movement: WordRight, extend: true }",
        ),
        (
            Named::Home,
            CTRL,
            "cmd MoveCursor { movement: DocStart, extend: false }",
        ),
        (
            Named::End,
            CTRL | SHIFT,
            "cmd MoveCursor { movement: DocEnd, extend: true }",
        ),
        (Named::Backspace, CTRL, "cmd DeleteWordBackward"),
        (Named::Delete, CTRL, "cmd DeleteWordForward"),
        (
            Named::ArrowLeft,
            NONE,
            "cmd MoveCursor { movement: Left, extend: false }",
        ),
        (Named::Backspace, NONE, "cmd DeleteBackward"),
    ];
    for (key, modifiers, expected) in cases {
        assert_eq!(
            named(&mut view, key, modifiers),
            [expected],
            "{key:?} {modifiers:?}"
        );
    }
}

#[test]
fn ctrl_tab_and_ctrl_page_keys_are_left_to_the_tab_strip() {
    let mut view = View::new("text", 600.0);
    for key in [Named::Tab, Named::PageUp, Named::PageDown] {
        let outcome = {
            view.focus();
            view.key(Key::Named(key), CTRL, None)
        };
        assert!(
            outcome.messages.is_empty(),
            "Ctrl+{key:?}: {:?}",
            outcome.messages
        );
        assert!(!outcome.captured, "Ctrl+{key:?} must reach the app");
    }
}

#[test]
fn tab_indents_a_multi_line_selection_and_shift_tab_outdents() {
    let mut view = View::new("a\nb\nc", 600.0);
    assert_eq!(
        named(&mut view, Named::Tab, NONE),
        [r#"cmd InsertText("    ")"#]
    );
    assert_eq!(named(&mut view, Named::Tab, SHIFT), ["cmd OutdentLines"]);

    view.buffer.set_selection(0, 0, 2, 1);
    view.key(Key::Named(Named::Tab), NONE, None);
    let messages = view.key(Key::Named(Named::Tab), NONE, None).messages;
    assert_eq!(messages, ["cmd IndentLines"]);
}

fn page_key(view: &mut View, key: Named, viewport_height: f32) -> Vec<String> {
    let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(key),
        modified_key: Key::Named(key),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: NONE,
        text: None,
        repeat: false,
    });
    let viewport = Rectangle::new(Point::ORIGIN, Size::new(600.0, viewport_height));
    view.event_in(event, viewport).messages
}

#[test]
fn page_down_and_up_move_about_a_screen_and_stop_at_the_ends() {
    let doc: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut view = View::new(&doc, 600.0);
    view.focus();
    view.draw();
    view.set_cursor(10, 2);

    let down = page_key(&mut view, Named::PageDown, 300.0);
    let [message] = down.as_slice() else {
        panic!("{down:?}")
    };
    let line: usize = message
        .split("line: ")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{message}"));
    assert!(
        message.starts_with("cmd SetCursor") && message.contains("col: 2"),
        "{message}"
    );
    assert!((15..40).contains(&line), "moved to line {line}");

    // Within the first screen, PageUp goes to the very start.
    view.set_cursor(3, 2);
    assert_eq!(
        page_key(&mut view, Named::PageUp, 300.0),
        ["cmd MoveCursor { movement: DocStart, extend: false }"]
    );
}

#[test]
fn double_click_selects_a_word_and_triple_click_a_line() {
    let mut view = View::new("alpha beta gamma", 600.0);
    let at = Point::new(60.0, 12.0);
    view.click(at);
    let second = view.click(at).messages;
    assert!(
        second
            .iter()
            .any(|m| m.starts_with("ptr SelectWord { line: 0")),
        "{second:?}"
    );
    let third = view.click(at).messages;
    assert!(
        third.iter().any(|m| m == "ptr SelectLine { line: 0 }"),
        "{third:?}"
    );
    // A fourth click starts over.
    let fourth = view.click(at).messages;
    assert!(fourth.iter().all(|m| !m.contains("Select")), "{fourth:?}");

    // Clicks far apart are separate single clicks.
    view.click(Point::new(10.0, 12.0));
    let apart = view.click(Point::new(120.0, 12.0)).messages;
    assert!(apart.iter().all(|m| !m.contains("Select")), "{apart:?}");
}

#[test]
fn an_open_completion_takes_the_navigation_keys_from_the_editor() {
    let mut view = View::new("[[No", 600.0);
    view.completion_open = true;
    for key in [
        Named::Enter,
        Named::Tab,
        Named::ArrowUp,
        Named::ArrowDown,
        Named::Escape,
    ] {
        let outcome = named(&mut view, key, NONE);
        assert!(outcome.is_empty(), "{key:?} still edited: {outcome:?}");
    }
    // Typing and other keys still edit.
    assert_eq!(
        named(&mut view, Named::Backspace, NONE),
        ["cmd DeleteBackward"]
    );
    view.completion_open = false;
    assert_eq!(
        named(&mut view, Named::Enter, NONE),
        [r#"cmd InsertText("\n")"#]
    );
}
