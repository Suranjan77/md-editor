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
