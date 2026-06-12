use md3_editor::{Buffer, Command, Selection};

#[test]
fn test_formatting_bold_italic_code_wikilink() {
    let mut buf = Buffer::from_text("hello world");

    // Test Bold wrapping empty selection (caret)
    buf.apply(Command::SetSelections(vec![Selection::caret(5)]));
    buf.apply(Command::ToggleBold);
    assert_eq!(buf.text(), "hello**** world");
    assert_eq!(buf.selections(), &[Selection::caret(7)]);

    // Bold unwrapping empty selection
    buf.apply(Command::ToggleBold);
    assert_eq!(buf.text(), "hello world");
    assert_eq!(buf.selections(), &[Selection::caret(5)]);

    // Bold wrapping non-empty selection
    buf.apply(Command::SetSelections(vec![Selection::new(6, 11)]));
    buf.apply(Command::ToggleBold);
    assert_eq!(buf.text(), "hello **world**");
    assert_eq!(buf.selections(), &[Selection::new(8, 13)]);

    // Bold unwrapping non-empty selection (internal wrapper)
    buf.apply(Command::ToggleBold);
    assert_eq!(buf.text(), "hello world");
    assert_eq!(buf.selections(), &[Selection::new(6, 11)]);

    // Bold unwrapping non-empty selection (external wrapper)
    buf.apply(Command::SetSelections(vec![Selection::new(6, 11)]));
    buf.apply(Command::ToggleBold); // Wrap it -> "hello **world**"
    // Now select only the "world" text, excluding the "**"
    buf.apply(Command::SetSelections(vec![Selection::new(8, 13)]));
    buf.apply(Command::ToggleBold); // Should detect external wrap and unwrap it
    assert_eq!(buf.text(), "hello world");
    assert_eq!(buf.selections(), &[Selection::new(6, 11)]);

    // Test Wikilink wrapping empty selection
    buf.apply(Command::SetSelections(vec![Selection::caret(5)]));
    buf.apply(Command::ToggleWikilink);
    assert_eq!(buf.text(), "hello[[]] world");
    assert_eq!(buf.selections(), &[Selection::caret(7)]);

    // Wikilink unwrapping empty selection
    buf.apply(Command::ToggleWikilink);
    assert_eq!(buf.text(), "hello world");
    assert_eq!(buf.selections(), &[Selection::caret(5)]);

    // Test Wikilink wrapping non-empty selection
    buf.apply(Command::SetSelections(vec![Selection::new(6, 11)]));
    buf.apply(Command::ToggleWikilink);
    assert_eq!(buf.text(), "hello [[world]]");
    assert_eq!(buf.selections(), &[Selection::new(8, 13)]);

    // Undo restores to hello world
    buf.apply(Command::Undo);
    assert_eq!(buf.text(), "hello world");
}

#[test]
fn test_heading_cycle() {
    let mut buf = Buffer::from_text("hello\nworld");

    // Cycle heading on first line
    buf.apply(Command::SetSelections(vec![Selection::caret(2)]));
    buf.apply(Command::HeadingCycle);
    assert_eq!(buf.text(), "# hello\nworld");

    buf.apply(Command::HeadingCycle);
    assert_eq!(buf.text(), "## hello\nworld");

    // Let's cycle all the way to 6 hashes and then off
    for _ in 0..4 {
        buf.apply(Command::HeadingCycle);
    }
    assert_eq!(buf.text(), "###### hello\nworld");

    buf.apply(Command::HeadingCycle);
    assert_eq!(buf.text(), "hello\nworld");
}

#[test]
fn test_bullet_list() {
    let mut buf = Buffer::from_text("hello\nworld");

    // Prepend list bullet
    buf.apply(Command::SetSelections(vec![Selection::caret(2)]));
    buf.apply(Command::ToggleBullet);
    assert_eq!(buf.text(), "- hello\nworld");

    // Remove list bullet
    buf.apply(Command::ToggleBullet);
    assert_eq!(buf.text(), "hello\nworld");
}

#[test]
fn test_checkbox_toggle() {
    let mut buf = Buffer::from_text("hello\n- world");

    // Toggle checkbox on non-list line: prepends "- [ ] "
    buf.apply(Command::SetSelections(vec![Selection::caret(2)]));
    buf.apply(Command::ToggleCheckbox);
    assert_eq!(buf.text(), "- [ ] hello\n- world");

    // Toggle checkbox on bullet list item: inserts "[ ] "
    buf.apply(Command::SetSelections(vec![Selection::caret(15)]));
    buf.apply(Command::ToggleCheckbox);
    assert_eq!(buf.text(), "- [ ] hello\n- [ ] world");

    // Toggle checkboxes: [ ] -> [x]
    buf.apply(Command::ToggleCheckbox);
    assert_eq!(buf.text(), "- [ ] hello\n- [x] world");

    // Toggle checkbox [x] -> [ ]
    buf.apply(Command::ToggleCheckbox);
    assert_eq!(buf.text(), "- [ ] hello\n- [ ] world");
}

#[test]
fn test_multicursor_formatting() {
    let mut buf = Buffer::from_text("first\nsecond");

    // Put caret on both lines
    buf.apply(Command::SetSelections(vec![
        Selection::caret(2),
        Selection::caret(9),
    ]));

    buf.apply(Command::ToggleBold);
    assert_eq!(buf.text(), "fi****rst\nsec****ond");
}

#[test]
fn test_auto_pairs() {
    let mut buf = Buffer::from_text("hello");

    // Test inserting opening character without selection (auto-pairs)
    buf.apply(Command::SetSelections(vec![Selection::caret(5)]));
    buf.apply(Command::Insert("(".to_string()));
    assert_eq!(buf.text(), "hello()");
    assert_eq!(buf.selections(), &[Selection::caret(6)]);

    // Test overtyping closing character
    buf.apply(Command::Insert(")".to_string()));
    assert_eq!(buf.text(), "hello()");
    assert_eq!(buf.selections(), &[Selection::caret(7)]);

    // Test backspace deleting both characters
    buf.apply(Command::SetSelections(vec![Selection::caret(6)])); // caret inside ()
    buf.apply(Command::DeleteBackward);
    assert_eq!(buf.text(), "hello");

    // Test wrapping selection with pair
    buf.apply(Command::SetSelections(vec![Selection::new(0, 5)]));
    buf.apply(Command::Insert("[".to_string()));
    assert_eq!(buf.text(), "[hello]");
    assert_eq!(buf.selections(), &[Selection::new(1, 6)]);
}

#[test]
fn test_smart_paste_url() {
    let mut buf = Buffer::from_text("google");
    buf.apply(Command::SetSelections(vec![Selection::new(0, 6)]));
    buf.apply(Command::Insert("https://google.com".to_string()));
    assert_eq!(buf.text(), "[google](https://google.com)");
}

#[test]
fn test_list_continuation_and_renumbering() {
    // Unordered list continuation
    let mut buf = Buffer::from_text("- hello");
    buf.apply(Command::SetSelections(vec![Selection::caret(7)]));
    buf.apply(Command::Insert("\n".to_string()));
    assert_eq!(buf.text(), "- hello\n- ");

    // Empty list item clears prefix
    buf.apply(Command::Insert("\n".to_string()));
    assert_eq!(buf.text(), "- hello\n");

    // Checkbox list continuation
    let mut buf = Buffer::from_text("- [x] done");
    buf.apply(Command::SetSelections(vec![Selection::caret(10)]));
    buf.apply(Command::Insert("\n".to_string()));
    assert_eq!(buf.text(), "- [x] done\n- [ ] ");

    // Ordered list continuation + renumbering
    let mut buf = Buffer::from_text("1. first\n2. second");
    buf.apply(Command::SetSelections(vec![Selection::caret(8)])); // after "first"
    buf.apply(Command::Insert("\n".to_string()));
    assert_eq!(buf.text(), "1. first\n2. \n3. second");
}

#[test]
fn test_set_heading() {
    let mut buf = Buffer::from_text("hello");
    buf.apply(Command::SetSelections(vec![Selection::caret(2)]));
    buf.apply(Command::SetHeading(3));
    assert_eq!(buf.text(), "### hello");
    buf.apply(Command::SetHeading(3)); // Toggle off
    assert_eq!(buf.text(), "hello");
}

#[test]
fn test_table_cell_nav_and_reflow() {
    let mut buf = Buffer::from_text("| col 1 | col 2 |\n|---|---|\n| a | b |");

    // Tab from "a" to "b"
    buf.apply(Command::SetSelections(vec![Selection::caret(25)])); // caret in cell "a"
    buf.apply(Command::TableTab { backward: false });
    assert_eq!(
        buf.text(),
        "| col 1 | col 2 |\n| :---- | :---- |\n| a     | b     |\n"
    );
    assert_eq!(buf.selections(), &[Selection::caret(38)]); // caret in cell "a"

    // Shift+Tab from "b" back to "a"
    buf.apply(Command::TableTab { backward: true });
    assert_eq!(buf.selections(), &[Selection::caret(10)]); // caret in cell "col 2"

    // Tab from last cell appends a new empty row
    buf.apply(Command::SetSelections(vec![Selection::caret(46)])); // in cell "b"
    buf.apply(Command::TableTab { backward: false });
    assert_eq!(
        buf.text(),
        "| col 1 | col 2 |\n| :---- | :---- |\n| a     | b     |\n|       |       |\n"
    );
    assert_eq!(buf.selections(), &[Selection::caret(56)]); // caret in cell 0 of new row
}
