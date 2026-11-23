//! Tests for `buffer`.

use crate::editor::buffer::{Affinity, DocBuffer, EditorCommand, Movement};

/// A bulk rewrite (replace-all) must be undoable — it is the edit a user is
/// most likely to want back, and autosave commits it within 400ms.
#[test]
fn replace_all_text_is_a_single_undoable_step() {
    let mut buffer = DocBuffer::from_text("cat cat cat");
    buffer.replace_all_text("dog dog dog");
    assert_eq!(buffer.text(), "dog dog dog");

    assert!(buffer.execute(EditorCommand::Undo).text_changed);
    assert_eq!(buffer.text(), "cat cat cat", "replace-all is undoable");

    assert!(buffer.execute(EditorCommand::Redo).text_changed);
    assert_eq!(buffer.text(), "dog dog dog");
}

/// A no-op rewrite must not push an empty step onto the undo stack.
#[test]
fn replace_all_text_with_identical_content_is_not_an_edit() {
    let mut buffer = DocBuffer::from_text("same");
    let result = buffer.replace_all_text("same");
    assert!(!result.text_changed);
    assert!(
        !buffer.execute(EditorCommand::Undo).text_changed,
        "nothing should have been recorded to undo"
    );
}

/// Typing a run of characters is one undo step, not one per keystroke.
#[test]
fn typing_a_run_coalesces_into_a_single_undo_step() {
    let mut buffer = DocBuffer::from_text("");
    for ch in "hello".chars() {
        buffer.execute(EditorCommand::InsertText(ch.to_string()));
    }
    assert_eq!(buffer.text(), "hello");

    assert!(buffer.execute(EditorCommand::Undo).text_changed);
    assert_eq!(
        buffer.text(),
        "",
        "one undo should take back the whole typed run"
    );

    assert!(buffer.execute(EditorCommand::Redo).text_changed);
    assert_eq!(buffer.text(), "hello", "redo restores the merged run");
}

/// A newline closes the run, so undo never jumps across a line boundary.
#[test]
fn newline_breaks_the_undo_run() {
    let mut buffer = DocBuffer::from_text("");
    for ch in "ab".chars() {
        buffer.execute(EditorCommand::InsertText(ch.to_string()));
    }
    buffer.execute(EditorCommand::InsertText("\n".to_string()));
    for ch in "cd".chars() {
        buffer.execute(EditorCommand::InsertText(ch.to_string()));
    }
    assert_eq!(buffer.text(), "ab\ncd");

    buffer.execute(EditorCommand::Undo);
    assert_eq!(buffer.text(), "ab\n", "second line's run undone on its own");
    buffer.execute(EditorCommand::Undo);
    assert_eq!(buffer.text(), "ab", "the newline is its own step");
    buffer.execute(EditorCommand::Undo);
    assert_eq!(buffer.text(), "");
}

/// Backspacing a word is one undo step too.
#[test]
fn backspace_run_coalesces() {
    let mut buffer = DocBuffer::from_text("hello");
    buffer.set_cursor(0, 5);
    for _ in 0..5 {
        buffer.execute(EditorCommand::DeleteBackward);
    }
    assert_eq!(buffer.text(), "");

    buffer.execute(EditorCommand::Undo);
    assert_eq!(buffer.text(), "hello", "one undo restores the deleted run");
}

/// A non-adjacent edit must start a new step, or undo would rewrite text
/// the user never typed in that burst.
#[test]
fn cursor_jump_breaks_the_undo_run() {
    let mut buffer = DocBuffer::from_text("XY");
    buffer.set_cursor(0, 0);
    buffer.execute(EditorCommand::InsertText("a".to_string()));
    // Jump elsewhere, then type again.
    buffer.set_cursor(0, 3);
    buffer.execute(EditorCommand::InsertText("b".to_string()));
    assert_eq!(buffer.text(), "aXYb");

    buffer.execute(EditorCommand::Undo);
    assert_eq!(buffer.text(), "aXY", "only the second insert is undone");
}

#[test]
fn inserts_at_cursor_and_updates_offset_cursor() {
    let mut buffer = DocBuffer::from_text("Hello");
    buffer.set_cursor(0, 5);
    buffer.insert_at_cursor(" world");
    assert_eq!(buffer.text(), "Hello world");
    assert_eq!(buffer.cursor_offset(), 11);
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 11));
}

#[test]
fn auto_pair_inserts_matching_closer_with_cursor_between() {
    let mut buffer = DocBuffer::from_text("");
    buffer.execute(EditorCommand::TypePaired('('));
    assert_eq!(buffer.text(), "()");
    assert_eq!(buffer.cursor_offset(), 1);
    // Typing the closer skips over the auto-inserted one.
    buffer.execute(EditorCommand::TypePaired(')'));
    assert_eq!(buffer.text(), "()");
    assert_eq!(buffer.cursor_offset(), 2);
}

#[test]
fn auto_pair_wraps_selection() {
    let mut buffer = DocBuffer::from_text("hello");
    buffer.set_selection(0, 0, 0, 5);
    buffer.execute(EditorCommand::TypePaired('['));
    assert_eq!(buffer.text(), "[hello]");
}

#[test]
fn auto_pair_quote_word_guard() {
    // Apostrophe after a word character must not pair.
    let mut buffer = DocBuffer::from_text("don");
    buffer.set_cursor(0, 3);
    buffer.execute(EditorCommand::TypePaired('\''));
    assert_eq!(buffer.text(), "don'");
    assert_eq!(buffer.cursor_offset(), 4);

    // Quote at the start of a word pairs.
    let mut buffer = DocBuffer::from_text("");
    buffer.execute(EditorCommand::TypePaired('"'));
    assert_eq!(buffer.text(), "\"\"");
    assert_eq!(buffer.cursor_offset(), 1);
}

#[test]
fn auto_pair_closer_inserts_when_not_skipping() {
    let mut buffer = DocBuffer::from_text("");
    // A lone closing bracket with nothing ahead inserts literally.
    buffer.execute(EditorCommand::TypePaired(')'));
    assert_eq!(buffer.text(), ")");
    assert_eq!(buffer.cursor_offset(), 1);
}

#[test]
fn handles_multiline_insert_cursor_position() {
    let mut buffer = DocBuffer::from_text("Hello");
    buffer.set_cursor(0, 5);
    buffer.insert_at_cursor("\nworld\nagain");
    assert_eq!(buffer.text(), "Hello\nworld\nagain");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (2, 5));
}

#[test]
fn replaces_multiline_selection() {
    let mut buffer = DocBuffer::from_text("one\ntwo\nthree");
    buffer.set_selection(0, 2, 2, 2);
    buffer.insert_at_cursor("X");
    assert_eq!(buffer.text(), "onXree");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 3));
    assert_eq!(buffer.selection_offsets(), None);
}

#[test]
fn deletes_selection_with_backspace_and_delete() {
    let mut buffer = DocBuffer::from_text("abc\ndef");
    buffer.set_selection(0, 1, 1, 2);
    buffer.backspace();
    assert_eq!(buffer.text(), "af");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 1));

    buffer.set_selection(0, 0, 0, 2);
    buffer.delete();
    assert_eq!(buffer.text(), "");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 0));
}

#[test]
fn undo_redo_restores_replacement_transaction() {
    let mut buffer = DocBuffer::from_text("Hello brave world");
    buffer.set_selection(0, 6, 0, 11);
    buffer.insert_at_cursor("small");
    assert_eq!(buffer.text(), "Hello small world");

    assert!(buffer.undo());
    assert_eq!(buffer.text(), "Hello brave world");
    assert_eq!(buffer.selection, Some((0, 6, 0, 11)));

    assert!(buffer.redo());
    assert_eq!(buffer.text(), "Hello small world");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 11));
    assert_eq!(buffer.selection, None);
}

#[test]
fn replace_all_is_undoable_without_destroying_prior_history() {
    let mut buffer = DocBuffer::from_text("one one");
    buffer.set_cursor(0, 7);
    buffer.insert_at_cursor("!");
    assert!(buffer.replace_all_text("two two!").text_changed);
    assert_eq!(buffer.text(), "two two!");

    assert!(buffer.undo());
    assert_eq!(buffer.text(), "one one!");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "one one");
}

#[test]
fn vertical_movement_preserves_desired_column() {
    let mut buffer = DocBuffer::from_text("abcdef\nxy\n123456");
    buffer.set_cursor(0, 5);
    buffer.move_cursor_down();
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 2));
    buffer.move_cursor_down();
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (2, 5));
}

#[test]
fn movement_can_extend_selection() {
    let mut buffer = DocBuffer::from_text("abc");
    buffer.set_cursor(0, 1);
    buffer.execute(EditorCommand::MoveCursor {
        movement: Movement::Right,
        extend: true,
    });
    assert_eq!(buffer.selection, Some((0, 1, 0, 2)));
    assert_eq!(buffer.selected_text().as_deref(), Some("b"));
}

#[test]
fn select_all_and_replace() {
    let mut buffer = DocBuffer::from_text("a\nb\nc");
    buffer.execute(EditorCommand::SelectAll);
    assert_eq!(buffer.selected_text().as_deref(), Some("a\nb\nc"));
    buffer.execute(EditorCommand::InsertText("z".to_string()));
    assert_eq!(buffer.text(), "z");
}

#[test]
fn toggles_task_checkbox_as_transaction() {
    let mut buffer = DocBuffer::from_text("- [ ] item\n  - [X] nested");
    buffer.execute(EditorCommand::ToggleCheckbox { line: 0 });
    buffer.execute(EditorCommand::ToggleCheckbox { line: 1 });
    assert_eq!(buffer.text(), "- [x] item\n  - [ ] nested");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "- [x] item\n  - [X] nested");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "- [ ] item\n  - [X] nested");
}

#[test]
fn formatting_wraps_selection_and_preserves_inner_selection() {
    let mut buffer = DocBuffer::from_text("make this bold");
    buffer.set_selection(0, 5, 0, 9);
    buffer.execute(EditorCommand::FormatBold);

    assert_eq!(buffer.text(), "make **this** bold");
    assert_eq!(buffer.selected_text().as_deref(), Some("this"));
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "make this bold");
    assert_eq!(buffer.selection, Some((0, 5, 0, 9)));
    assert!(buffer.redo());
    assert_eq!(buffer.text(), "make **this** bold");
}

#[test]
fn formatting_without_selection_inserts_editable_placeholder() {
    let mut buffer = DocBuffer::from_text("prefix ");
    buffer.set_cursor(0, 7);

    buffer.execute(EditorCommand::FormatItalic);
    assert_eq!(buffer.text(), "prefix *italic*");
    assert_eq!(buffer.selected_text().as_deref(), Some("italic"));

    buffer.execute(EditorCommand::InsertText("emphasis".to_string()));
    assert_eq!(buffer.text(), "prefix *emphasis*");
}

#[test]
fn inline_code_and_link_shortcuts_are_transactions() {
    let mut buffer = DocBuffer::from_text("x and y");
    buffer.set_selection(0, 0, 0, 1);
    buffer.execute(EditorCommand::FormatInlineCode);
    assert_eq!(buffer.text(), "`x` and y");

    buffer.set_selection(0, 8, 0, 9);
    buffer.execute(EditorCommand::InsertLink);
    assert_eq!(buffer.text(), "`x` and [y](url)");

    assert!(buffer.undo());
    assert_eq!(buffer.text(), "`x` and y");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "x and y");
}

#[test]
fn selection_drag_direction_does_not_affect_replacement() {
    let mut buffer = DocBuffer::from_text("alpha beta gamma");
    buffer.set_selection(0, 10, 0, 6);
    assert_eq!(buffer.selected_text().as_deref(), Some("beta"));
    buffer.execute(EditorCommand::InsertText("B".to_string()));
    assert_eq!(buffer.text(), "alpha B gamma");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 7));
}

#[test]
fn multi_line_select_all_cut_undo_redo_round_trip() {
    let original = "one\n\nthree\n四";
    let mut buffer = DocBuffer::from_text(original);
    buffer.execute(EditorCommand::SelectAll);
    assert!(buffer.execute(EditorCommand::DeleteSelection).text_changed);
    assert_eq!(buffer.text(), "");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), original);
    assert_eq!(buffer.selection, Some((0, 0, 3, 1)));
    assert!(buffer.redo());
    assert_eq!(buffer.text(), "");
}

#[test]
fn command_result_reports_no_change_for_boundaries() {
    let mut buffer = DocBuffer::from_text("");
    assert!(!buffer.execute(EditorCommand::DeleteBackward).text_changed);
    assert!(!buffer.execute(EditorCommand::DeleteForward).text_changed);
    assert!(!buffer.execute(EditorCommand::Undo).text_changed);
    assert!(!buffer.execute(EditorCommand::Redo).text_changed);
}

#[test]
fn deterministic_editing_stress_keeps_cursor_and_selection_valid() {
    let mut buffer = DocBuffer::new();
    let mut seed = 0xC0FFEE_u64;

    for _ in 0..1500 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        match (seed >> 32) % 18 {
            0 => {
                buffer.execute(EditorCommand::InsertText("a".to_string()));
            }
            1 => {
                buffer.execute(EditorCommand::InsertText("λ".to_string()));
            }
            2 => {
                buffer.execute(EditorCommand::InsertText("\n".to_string()));
            }
            3 => {
                buffer.execute(EditorCommand::DeleteBackward);
            }
            4 => {
                buffer.execute(EditorCommand::DeleteForward);
            }
            5 => buffer.move_cursor_left(),
            6 => buffer.move_cursor_right(),
            7 => buffer.move_cursor_up(),
            8 => buffer.move_cursor_down(),
            9 => buffer.move_cursor_home(),
            10 => buffer.move_cursor_end(),
            11 => {
                buffer.execute(EditorCommand::MoveCursor {
                    movement: Movement::Right,
                    extend: true,
                });
            }
            12 => {
                buffer.execute(EditorCommand::FormatBold);
            }
            13 => {
                buffer.execute(EditorCommand::FormatInlineCode);
            }
            14 => {
                buffer.execute(EditorCommand::SelectAll);
            }
            15 => {
                buffer.execute(EditorCommand::Undo);
            }
            16 => {
                buffer.execute(EditorCommand::Redo);
            }
            _ => {
                let line = buffer.cursor_line;
                buffer.execute(EditorCommand::ToggleCheckbox { line });
            }
        }

        assert!(buffer.cursor_offset() <= buffer.text().chars().count());
        assert!(buffer.cursor_line < buffer.line_count());
        assert!(buffer.cursor_col <= buffer.line_text(buffer.cursor_line).chars().count());
        if let Some(selection) = buffer.selection_offsets() {
            let (start, end) = selection.range();
            assert!(start < end);
            assert!(end <= buffer.text().chars().count());
        }
    }
}

#[test]
fn editing_steps_over_whole_grapheme_clusters() {
    // "👩‍💻" is three chars (woman, ZWJ, laptop) but one cluster.
    let mut buffer = DocBuffer::from_text("a👩‍💻b");
    buffer.set_cursor(0, 4);
    buffer.backspace();
    assert_eq!(buffer.text(), "ab");
    assert_eq!(buffer.cursor_col, 1);
    buffer.undo();
    assert_eq!(buffer.text(), "a👩‍💻b");

    buffer.set_cursor(0, 1);
    buffer.move_cursor_right();
    assert_eq!(buffer.cursor_col, 4);
    buffer.move_cursor_left();
    assert_eq!(buffer.cursor_col, 1);
    buffer.delete();
    assert_eq!(buffer.text(), "ab");

    // A column inside the cluster snaps to its start.
    let mut buffer = DocBuffer::from_text("a👩‍💻b");
    buffer.set_cursor(0, 2);
    assert_eq!(buffer.cursor_col, 1);
    let mut buffer = DocBuffer::from_text("e\u{301}x");
    buffer.set_cursor(0, 1);
    assert_eq!(buffer.cursor_col, 0);
    buffer.backspace();
    assert_eq!(buffer.text(), "e\u{301}x");
    buffer.set_cursor(0, 2);
    buffer.move_cursor_right();
    buffer.backspace();
    assert_eq!(buffer.text(), "e\u{301}");
}

#[test]
fn caret_affinity_lasts_only_until_the_next_change() {
    let upstream_at = |col| EditorCommand::SetCursor {
        line: 0,
        col,
        affinity: Affinity::Upstream,
    };
    let mut buffer = DocBuffer::from_text("hello world");
    buffer.execute(upstream_at(6));
    assert_eq!(buffer.cursor_affinity, Affinity::Upstream);

    buffer.move_cursor_right();
    buffer.move_cursor_left();
    assert_eq!(
        (buffer.cursor_col, buffer.cursor_affinity),
        (6, Affinity::Downstream)
    );

    buffer.execute(upstream_at(6));
    buffer.insert_at_cursor("x");
    assert_eq!(buffer.cursor_affinity, Affinity::Downstream);

    buffer.execute(upstream_at(6));
    buffer.undo();
    assert_eq!(buffer.cursor_affinity, Affinity::Downstream);
}

#[test]
fn a_crlf_line_break_is_one_caret_stop() {
    let mut buffer = DocBuffer::from_text("ab\r\ncd");
    buffer.set_cursor(0, 2);
    buffer.move_cursor_right();
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));
    buffer.move_cursor_left();
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 2));
    buffer.delete();
    assert_eq!(buffer.text(), "abcd");
    buffer.undo();
    buffer.set_cursor(1, 0);
    buffer.backspace();
    assert_eq!(buffer.text(), "abcd");
}

#[test]
fn arrow_steps_over_regional_indicator_grapheme() {
    let mut buffer = DocBuffer::from_text("🇩🇪x");
    buffer.set_cursor(0, 0);
    buffer.move_cursor_right();
    assert_eq!(buffer.cursor_offset(), 2);
    buffer.move_cursor_left();
    assert_eq!(buffer.cursor_offset(), 0);
}

#[test]
fn undo_and_redo_track_the_saved_revision() {
    let mut buffer = DocBuffer::from_text("saved");
    buffer.set_cursor(0, 5);
    buffer.insert_at_cursor(" edit");
    assert!(buffer.dirty);
    assert!(buffer.undo());
    assert!(!buffer.dirty, "undo returned to the loaded saved state");
    assert!(buffer.redo());
    buffer.mark_saved();
    assert!(!buffer.dirty);
    assert!(buffer.undo());
    assert!(
        buffer.dirty,
        "undo moved away from the explicitly saved revision"
    );
    assert!(buffer.redo());
    assert!(
        !buffer.dirty,
        "redo returned to the explicitly saved revision"
    );
}

#[test]
fn backspace_between_auto_pairs_deletes_both_delimiters() {
    for delimiter in ['(', '"', '`'] {
        let mut buffer = DocBuffer::from_text("");
        buffer.execute(EditorCommand::TypePaired(delimiter));
        buffer.backspace();
        assert_eq!(buffer.text(), "", "delimiter: {delimiter}");
        assert!(buffer.undo());
    }

    let mut normal = DocBuffer::from_text(")x");
    normal.set_cursor(0, 1);
    normal.backspace();
    assert_eq!(normal.text(), "x");
}

#[test]
fn enter_preserves_dominant_line_endings() {
    let mut crlf = DocBuffer::from_text("first\r\nsecond");
    crlf.set_cursor(1, 6);
    crlf.insert_at_cursor("\n");
    assert_eq!(crlf.text(), "first\r\nsecond\r\n");

    let mut crlf_list = DocBuffer::from_text("- one\r\n- two");
    crlf_list.set_cursor(1, 5);
    crlf_list.insert_at_cursor("\n");
    assert_eq!(crlf_list.text(), "- one\r\n- two\r\n- ");

    let mut lf = DocBuffer::from_text("first\nsecond");
    lf.set_cursor(1, 6);
    lf.insert_at_cursor("\n");
    assert_eq!(lf.text(), "first\nsecond\n");
}

#[test]
fn empty_boundary_actions_do_not_panic() {
    let mut buffer = DocBuffer::new();
    buffer.backspace();
    buffer.delete();
    buffer.move_cursor_left();
    buffer.move_cursor_right();
    buffer.move_cursor_up();
    buffer.move_cursor_down();
    buffer.move_cursor_home();
    buffer.move_cursor_end();
    assert!(!buffer.undo());
    assert!(!buffer.redo());
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 0));
}

#[test]
fn format_toggle_bold_inner_and_outer() {
    let mut buffer = DocBuffer::from_text("hello");
    buffer.set_selection(0, 0, 0, 5);
    buffer.execute(EditorCommand::FormatBold);
    assert_eq!(buffer.text(), "**hello**");

    // Inner toggle (selection is the formatted text itself)
    buffer.set_selection(0, 0, 0, 9);
    buffer.execute(EditorCommand::FormatBold);
    assert_eq!(buffer.text(), "hello");

    // Re-bold
    buffer.set_selection(0, 0, 0, 5);
    buffer.execute(EditorCommand::FormatBold);
    assert_eq!(buffer.text(), "**hello**");

    // Outer toggle (selection is the inner unformatted text)
    buffer.set_selection(0, 2, 0, 7);
    buffer.execute(EditorCommand::FormatBold);
    assert_eq!(buffer.text(), "hello");
}

#[test]
fn list_auto_continuation_unordered_and_checklist() {
    // Unordered list continuation
    let mut buffer = DocBuffer::from_text("- Buy milk");
    buffer.set_cursor(0, 10);
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "- Buy milk\n- ");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 2));

    // Unordered list empty line clearing
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "- Buy milk\n");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));

    // Checklist continuation
    let mut buffer2 = DocBuffer::from_text("* [ ] Code task");
    buffer2.set_cursor(0, 15);
    buffer2.insert_at_cursor("\n");
    assert_eq!(buffer2.text(), "* [ ] Code task\n* [ ] ");
    assert_eq!((buffer2.cursor_line, buffer2.cursor_col), (1, 6));

    // Checklist empty line clearing
    buffer2.insert_at_cursor("\n");
    assert_eq!(buffer2.text(), "* [ ] Code task\n");
    assert_eq!((buffer2.cursor_line, buffer2.cursor_col), (1, 0));
}

#[test]
fn list_auto_continuation_ordered() {
    let mut buffer = DocBuffer::from_text("1. Step one");
    buffer.set_cursor(0, 11);
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "1. Step one\n2. ");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 3));

    // Ordered list empty line clearing
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "1. Step one\n");
    assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));
}

#[test]
fn list_parsing_survives_multibyte_chars_near_marker() {
    // Regression: parse_list_item used to byte-slice at fixed offsets and
    // panic when a multibyte char sat right after the marker.
    let mut buffer = DocBuffer::from_text("- [xλ");
    buffer.set_cursor(0, 5);
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "- [xλ\n- ");

    let mut buffer = DocBuffer::from_text("- [ ]λ");
    buffer.set_cursor(0, 6);
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "- [ ]λ\n- [ ] ");

    let mut buffer = DocBuffer::from_text("1.λ");
    buffer.set_cursor(0, 3);
    buffer.insert_at_cursor("\n");
    assert_eq!(buffer.text(), "1.λ\n");
}

#[test]
fn list_continuation_handles_multibyte_content_after_markers() {
    for (input, expected) in [
        ("- [ ]é", "- [ ]é\n- [ ] "),
        ("- [x]世界", "- [x]世界\n- [ ] "),
        ("* [ ]🙂", "* [ ]🙂\n* [ ] "),
        // No space after an ordered marker is not a list item, but Enter
        // must still insert a newline without slicing through UTF-8.
        ("1.任务", "1.任务\n"),
    ] {
        let mut buffer = DocBuffer::from_text(input);
        buffer.set_cursor(0, input.chars().count());
        buffer.insert_at_cursor("\n");
        assert_eq!(buffer.text(), expected, "input: {input}");
    }
}
