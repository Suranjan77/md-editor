//! Tests for word motion and deletion, word/line selection and block indent on `DocBuffer`.

use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};

fn at(text: &str, offset: usize) -> DocBuffer {
    let mut buffer = DocBuffer::from_text(text);
    buffer.set_cursor_offset(offset);
    buffer
}

fn mv(buffer: &mut DocBuffer, movement: Movement, extend: bool) {
    buffer.execute(EditorCommand::MoveCursor { movement, extend });
}

#[test]
fn word_right_and_left_jump_whole_words_and_punctuation_runs() {
    let mut b = at("let foo_bar = baz();", 0);
    let mut stops = Vec::new();
    for _ in 0..5 {
        mv(&mut b, Movement::WordRight, false);
        stops.push(b.cursor_offset());
    }
    assert_eq!(stops, [3, 11, 13, 17, 20]);
    mv(&mut b, Movement::WordLeft, false);
    assert_eq!(b.cursor_offset(), 17);
    mv(&mut b, Movement::WordLeft, false);
    assert_eq!(b.cursor_offset(), 14);
}

#[test]
fn word_motion_crosses_lines_and_extends_selection() {
    let mut b = at("one\ntwo", 4);
    mv(&mut b, Movement::WordLeft, true);
    assert_eq!(b.cursor_offset(), 0);
    assert_eq!(b.selected_text().as_deref(), Some("one\n"));
}

#[test]
fn doc_start_and_end() {
    let mut b = at("a\nb\nc", 2);
    mv(&mut b, Movement::DocEnd, false);
    assert_eq!(b.cursor_offset(), 5);
    mv(&mut b, Movement::DocStart, true);
    assert_eq!(b.selected_text().as_deref(), Some("a\nb\nc"));
}

#[test]
fn ctrl_backspace_and_delete_remove_a_word_as_one_undo_step() {
    let mut b = at("hello brave world", 11);
    assert!(b.execute(EditorCommand::DeleteWordBackward).text_changed);
    assert_eq!(b.text(), "hello  world");
    assert_eq!(b.cursor_offset(), 6);
    b.execute(EditorCommand::DeleteWordForward);
    assert_eq!(b.text(), "hello ");
    b.execute(EditorCommand::Undo);
    assert_eq!(b.text(), "hello  world");
    b.execute(EditorCommand::Undo);
    assert_eq!(b.text(), "hello brave world");
}

#[test]
fn word_delete_with_a_selection_deletes_the_selection() {
    let mut b = at("abc def", 0);
    mv(&mut b, Movement::Right, true);
    b.execute(EditorCommand::DeleteWordForward);
    assert_eq!(b.text(), "bc def");
}

#[test]
fn double_click_selects_the_word_and_triple_click_the_line() {
    let mut b = DocBuffer::from_text("first line\nsome words_here!\nlast");
    b.execute(EditorCommand::SelectWord { line: 1, col: 7 });
    assert_eq!(b.selected_text().as_deref(), Some("words_here"));
    b.execute(EditorCommand::SelectWord { line: 1, col: 15 });
    assert_eq!(b.selected_text().as_deref(), Some("!"));
    b.execute(EditorCommand::SelectLine { line: 1 });
    assert_eq!(b.selected_text().as_deref(), Some("some words_here!\n"));
    b.execute(EditorCommand::SelectLine { line: 2 });
    assert_eq!(b.selected_text().as_deref(), Some("last"));
}

#[test]
fn tab_indents_and_shift_tab_outdents_the_selected_lines() {
    let mut b = DocBuffer::from_text("a\n\n  b\nc\nd");
    // Select from inside "a" to the start of "c": "c" is not touched.
    b.execute(EditorCommand::SetSelection {
        anchor_line: 0,
        anchor_col: 1,
        focus_line: 3,
        focus_col: 0,
        affinity: Default::default(),
    });
    b.execute(EditorCommand::IndentLines);
    assert_eq!(b.text(), "    a\n\n      b\nc\nd");
    assert_eq!(b.selected_text().as_deref(), Some("\n\n      b\n"));

    b.execute(EditorCommand::OutdentLines);
    assert_eq!(b.text(), "a\n\n  b\nc\nd");
    b.execute(EditorCommand::OutdentLines);
    assert_eq!(b.text(), "a\n\nb\nc\nd");

    b.execute(EditorCommand::Undo);
    assert_eq!(b.text(), "a\n\n  b\nc\nd");
}

#[test]
fn shift_tab_without_selection_outdents_the_caret_line() {
    let mut b = at("x\n\tlist item", 6);
    b.execute(EditorCommand::OutdentLines);
    assert_eq!(b.text(), "x\nlist item");
    assert_eq!(b.cursor_offset(), 5);
}

#[test]
fn word_count_does_not_split_words_at_rope_chunk_boundaries() {
    // Long enough to span many rope chunks, with no whitespace to realign on.
    let word = "a".repeat(5000);
    let text = format!("{word} {word} {word}");
    let mut b = DocBuffer::from_text(&text);
    assert_eq!(b.word_count(), 3);
    b.set_cursor_offset(0);
    b.execute(EditorCommand::InsertText("x ".into()));
    assert_eq!(b.word_count(), 4, "cache follows edits");
    b.execute(EditorCommand::Undo);
    assert_eq!(b.word_count(), 3);
}
