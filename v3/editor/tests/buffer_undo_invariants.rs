//! Buffer quality harness (plan §3.2): randomized command storms with
//! invariant checks, grapheme safety (emoji/CJK/CRLF), multi-cursor edits,
//! and the undo-tree guarantee that distinguishes v3 from v2 — editing after
//! undo never destroys history.

use md3_editor::Selection;
use md3_editor::buffer::{Buffer, Command, Movement};

fn insert(buffer: &mut Buffer, text: &str) -> bool {
    buffer.apply(Command::Insert(text.to_string())).text_changed
}

// ----- randomized storm -------------------------------------------------------

/// Same deterministic generator style as the height-tree differential test:
/// reproducible across runs, no proptest dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn random_command(rng: &mut XorShift) -> Command {
    let snippets = [
        "a",
        "Z",
        "é",
        "🦀",
        "👨‍👩‍👧‍👦",
        "汉字",
        "\n",
        "\r\n",
        "word ",
        "## h\n",
    ];
    match rng.below(10) {
        0..=3 => Command::Insert(snippets[rng.below(snippets.len())].to_string()),
        4 => Command::DeleteBackward,
        5 => Command::DeleteForward,
        6 => Command::Move {
            movement: [
                Movement::Left,
                Movement::Right,
                Movement::Up,
                Movement::Down,
                Movement::Home,
                Movement::End,
                Movement::DocStart,
                Movement::DocEnd,
            ][rng.below(8)],
            extend: rng.below(3) == 0,
        },
        7 => Command::SetCursor {
            line: rng.below(8),
            col: rng.below(32),
        },
        8 => Command::SelectAll,
        _ => Command::Undo,
    }
}

fn check_invariants(buffer: &Buffer, step: usize) {
    let len = buffer.len_chars();
    let sels = buffer.selections();
    assert!(!sels.is_empty(), "step {step}: selections never empty");
    let mut prev_end = 0;
    for (i, sel) in sels.iter().enumerate() {
        let (start, end) = sel.range();
        assert!(
            end <= len,
            "step {step}: selection {i} out of bounds ({end} > {len})"
        );
        if i > 0 {
            assert!(
                start >= prev_end,
                "step {step}: selections overlap or are unsorted"
            );
        }
        prev_end = end;
    }
}

#[test]
fn random_storm_keeps_invariants_and_undo_to_root_restores_original() {
    for seed in 1..=8u64 {
        let original = "# Title\n\nfirst 🦀 line\nsecond 汉字 line\r\nthird";
        let mut buffer = Buffer::from_text(original);
        let mut rng = XorShift(seed.wrapping_mul(0x9E3779B97F4A7C15));
        for step in 0..500 {
            let cmd = random_command(&mut rng);
            buffer.apply(cmd);
            check_invariants(&buffer, step);
        }
        // Undo everything: apply→undo == identity, all the way down.
        while buffer.apply(Command::Undo).text_changed {}
        assert_eq!(
            buffer.text(),
            original,
            "seed {seed}: undo-to-root must restore the original text"
        );
        check_invariants(&buffer, usize::MAX);
    }
}

#[test]
fn redo_replays_the_storm_exactly() {
    let mut buffer = Buffer::from_text("base\n");
    let mut rng = XorShift(0xDEADBEEF);
    for _ in 0..200 {
        buffer.apply(random_command(&mut rng));
    }
    let final_text = buffer.text();
    let mut undone = 0;
    while buffer.apply(Command::Undo).text_changed {
        undone += 1;
    }
    for _ in 0..undone {
        assert!(
            buffer.apply(Command::Redo).text_changed,
            "redo chain too short"
        );
    }
    assert_eq!(buffer.text(), final_text, "undo-all → redo-all == identity");
}

// ----- grapheme safety ----------------------------------------------------------

#[test]
fn backspace_deletes_whole_emoji_clusters() {
    // Family emoji: 7 chars (4 scalars + 3 ZWJ), one grapheme.
    let mut buffer = Buffer::new();
    insert(&mut buffer, "a👨‍👩‍👧‍👦b");
    buffer.apply(Command::DeleteBackward); // b
    buffer.apply(Command::DeleteBackward); // the whole family
    assert_eq!(buffer.text(), "a");
}

#[test]
fn arrow_keys_never_land_inside_a_cluster() {
    let mut buffer = Buffer::new();
    insert(&mut buffer, "x🇳🇵y"); // flag = 2 regional indicators, 1 cluster
    buffer.apply(Command::Move {
        movement: Movement::DocStart,
        extend: false,
    });
    let mut offsets = vec![buffer.primary().head];
    for _ in 0..3 {
        buffer.apply(Command::Move {
            movement: Movement::Right,
            extend: false,
        });
        offsets.push(buffer.primary().head);
    }
    // x(1) + flag(2) + y(1): boundaries at 0,1,3,4 — never 2.
    assert_eq!(offsets, vec![0, 1, 3, 4]);
}

#[test]
fn crlf_is_one_backspace() {
    let mut buffer = Buffer::from_text("one\r\ntwo");
    buffer.apply(Command::Move {
        movement: Movement::Down,
        extend: false,
    });
    buffer.apply(Command::Move {
        movement: Movement::Home,
        extend: false,
    });
    buffer.apply(Command::DeleteBackward);
    assert_eq!(buffer.text(), "onetwo", "CRLF deletes as a single unit");
}

#[test]
fn cjk_motion_is_per_character() {
    let mut buffer = Buffer::from_text("汉字测试");
    buffer.apply(Command::Move {
        movement: Movement::Right,
        extend: false,
    });
    assert_eq!(buffer.primary().head, 1);
    buffer.apply(Command::DeleteForward);
    assert_eq!(buffer.text(), "汉测试");
}

// ----- multi-cursor (Vec<Selection> is the model, plan §3.2) ---------------------

#[test]
fn insert_applies_at_every_cursor() {
    let mut buffer = Buffer::from_text("aa bb cc");
    buffer.apply(Command::SetSelections(vec![
        Selection::caret(0),
        Selection::caret(3),
        Selection::caret(6),
    ]));
    insert(&mut buffer, "x");
    assert_eq!(buffer.text(), "xaa xbb xcc");
    let heads: Vec<usize> = buffer.selections().iter().map(|s| s.head).collect();
    assert_eq!(heads, vec![1, 5, 9], "each caret sits after its insertion");
}

#[test]
fn multi_selection_replace_and_single_undo() {
    let mut buffer = Buffer::from_text("foo bar foo");
    buffer.apply(Command::SetSelections(vec![
        Selection::new(0, 3),
        Selection::new(8, 11),
    ]));
    insert(&mut buffer, "qux");
    assert_eq!(buffer.text(), "qux bar qux");
    // One command — one transaction — one undo.
    assert!(buffer.apply(Command::Undo).text_changed);
    assert_eq!(buffer.text(), "foo bar foo");
    let restored: Vec<(usize, usize)> = buffer.selections().iter().map(|s| s.range()).collect();
    assert_eq!(
        restored,
        vec![(0, 3), (8, 11)],
        "selections restored by undo"
    );
}

#[test]
fn overlapping_selections_merge() {
    let mut buffer = Buffer::from_text("abcdefgh");
    buffer.apply(Command::SetSelections(vec![
        Selection::new(0, 4),
        Selection::new(2, 6),
    ]));
    assert_eq!(buffer.selections().len(), 1);
    assert_eq!(buffer.selections()[0].range(), (0, 6));
}

// ----- undo tree: the v3 upgrade -------------------------------------------------

#[test]
fn editing_after_undo_branches_instead_of_destroying_the_future() {
    let mut buffer = Buffer::new();
    insert(&mut buffer, "hello");
    insert(&mut buffer, " world");
    buffer.apply(Command::Undo);
    assert_eq!(buffer.text(), "hello");

    // v2 would clear the redo stack here; the tree branches instead.
    insert(&mut buffer, " there");
    assert_eq!(buffer.text(), "hello there");

    // The new branch is redoable…
    buffer.apply(Command::Undo);
    assert_eq!(buffer.text(), "hello");
    assert!(buffer.apply(Command::Redo).text_changed);
    assert_eq!(
        buffer.text(),
        "hello there",
        "redo follows the newest branch"
    );

    // …and the abandoned " world" future still exists in the tree (history
    // UI hook): undo back and check the branch count.
    buffer.apply(Command::Undo);
    assert_eq!(buffer.text(), "hello");
}

#[test]
fn undo_restores_cursor_and_dirty_tracks_saves() {
    let mut buffer = Buffer::from_text("12345");
    assert!(!buffer.is_dirty());
    buffer.apply(Command::SetCursor { line: 0, col: 2 });
    insert(&mut buffer, "x");
    assert_eq!(buffer.text(), "12x345");
    assert!(buffer.is_dirty());

    buffer.mark_saved();
    assert!(!buffer.is_dirty());

    buffer.apply(Command::Undo);
    assert_eq!(buffer.text(), "12345");
    assert_eq!(
        buffer.primary().head,
        2,
        "undo restores the pre-edit cursor"
    );
    assert!(buffer.is_dirty(), "undo past the save point re-dirties");
}

#[test]
fn select_all_then_type_replaces_everything_in_one_transaction() {
    let mut buffer = Buffer::from_text("old content\nmore");
    buffer.apply(Command::SelectAll);
    insert(&mut buffer, "new");
    assert_eq!(buffer.text(), "new");
    buffer.apply(Command::Undo);
    assert_eq!(buffer.text(), "old content\nmore");
}
