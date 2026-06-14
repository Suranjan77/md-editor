//! Buffer quality harness (plan §3.2): randomized property tests with a
//! deterministic seed, in the same differential style as the height-tree
//! suite. Three properties the plan names explicitly:
//!
//! 1. **apply → undo == identity**: after any command burst, undoing to the
//!    root restores the exact original text, and redoing all the way back
//!    restores the exact final text (robust to insert-run coalescing, which
//!    merges several commands into one undo node).
//! 2. **Cursor in bounds**: every selection stays sorted, non-overlapping,
//!    and inside the document after every command.
//! 3. **Grapheme safety**: caret motion and deletion never land inside an
//!    extended grapheme cluster — emoji ZWJ families, flags, CJK, CRLF.
//!
//! Plus the v3-specific bridge property: feeding every [`ChangedSpan`] to a
//! [`LayoutEngine`] keeps layout and buffer in perfect line-level agreement.

use md_editor::{
    BlockState, Buffer, Command, ConcealMode, LayoutEngine, LineMeasure, Measurer, Movement,
    Selection, StyledLine, Styler,
};
use unicode_segmentation::UnicodeSegmentation;

/// Deterministic xorshift stream (same recipe as the height-tree suite).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

/// Tricky alphabet: ASCII, combining accents, CJK, ZWJ emoji, flag, CRLF.
const ATOMS: &[&str] = &[
    "a",
    "b",
    " ",
    "\n",
    "é",
    "한",
    "字",
    "👨‍👩‍👧‍👦",
    "🇳🇵",
    "\r\n",
    "x",
    "#",
];

fn random_text(rng: &mut Rng, max_atoms: usize) -> String {
    let n = rng.below(max_atoms) + 1;
    (0..n).map(|_| ATOMS[rng.below(ATOMS.len())]).collect()
}

fn random_command(rng: &mut Rng, buffer: &Buffer) -> Command {
    match rng.below(12) {
        0..=3 => Command::Insert(random_text(rng, 4)),
        4 => Command::DeleteBackward,
        5 => Command::DeleteForward,
        6..=8 => {
            let movement = match rng.below(8) {
                0 => Movement::Left,
                1 => Movement::Right,
                2 => Movement::Up,
                3 => Movement::Down,
                4 => Movement::Home,
                5 => Movement::End,
                6 => Movement::DocStart,
                _ => Movement::DocEnd,
            };
            Command::Move {
                movement,
                extend: rng.below(3) == 0,
            }
        }
        9 => {
            let line = rng.below(buffer.line_count());
            Command::SetCursor {
                line,
                col: rng.below(40),
            }
        }
        10 => {
            let line = rng.below(buffer.line_count());
            Command::AddCaret {
                line,
                col: rng.below(40),
            }
        }
        _ => {
            let a = rng.below(buffer.len_chars() + 1);
            let b = rng.below(buffer.len_chars() + 1);
            Command::SetSelections(vec![Selection::new(a, b)])
        }
    }
}

fn assert_selection_invariants(buffer: &Buffer) {
    let sels = buffer.selections();
    assert!(!sels.is_empty(), "selection set must never be empty");
    let len = buffer.len_chars();
    let mut prev_end = 0usize;
    for (i, sel) in sels.iter().enumerate() {
        let (start, end) = sel.range();
        assert!(end <= len, "selection {i} out of bounds: {end} > {len}");
        if i > 0 {
            assert!(
                start >= prev_end,
                "selections {i} overlaps its predecessor ({start} < {prev_end})"
            );
        }
        prev_end = end;
    }
}

fn assert_grapheme_aligned(buffer: &Buffer) {
    let text = buffer.text();
    // Char offsets that are cluster boundaries.
    let mut boundaries = vec![0usize];
    let mut chars = 0usize;
    for g in text.graphemes(true) {
        chars += g.chars().count();
        boundaries.push(chars);
    }
    for sel in buffer.selections() {
        for offset in [sel.anchor, sel.head] {
            assert!(
                boundaries.binary_search(&offset).is_ok(),
                "offset {offset} is inside a grapheme cluster (text {text:?})"
            );
        }
    }
}

#[test]
fn apply_then_undo_to_root_is_identity() {
    let mut rng = Rng(7);
    for round in 0..50 {
        let initial = random_text(&mut rng, 20);
        let mut buffer = Buffer::from_text(&initial);
        for _ in 0..40 {
            let cmd = random_command(&mut rng, &buffer);
            buffer.apply(cmd);
        }
        let final_text = buffer.text();
        let mut guard = 0;
        while buffer.can_undo() {
            buffer.apply(Command::Undo);
            guard += 1;
            assert!(guard < 1000, "undo did not terminate (round {round})");
        }
        assert_eq!(buffer.text(), initial, "undo-to-root (round {round})");
        while buffer.can_redo() {
            buffer.apply(Command::Redo);
        }
        assert_eq!(buffer.text(), final_text, "redo-to-tip (round {round})");
    }
}

#[test]
fn selections_stay_sorted_in_bounds_and_grapheme_aligned() {
    let mut rng = Rng(99);
    for _ in 0..30 {
        let mut buffer = Buffer::from_text(&random_text(&mut rng, 15));
        // Even raw offsets (hit testing, char cols) are snapped onto
        // boundaries by the buffer, so the invariants are unconditional.
        for _ in 0..60 {
            let cmd = random_command(&mut rng, &buffer);
            buffer.apply(cmd);
            assert_selection_invariants(&buffer);
            assert_grapheme_aligned(&buffer);
        }
    }
}

#[test]
fn arrow_keys_step_whole_clusters() {
    let text = "a👨‍👩‍👧‍👦🇳🇵é한\r\nb";
    let mut buffer = Buffer::from_text(text);
    let cluster_count = text.graphemes(true).count();
    let mut steps = 0;
    loop {
        let before = buffer.primary().head;
        buffer.apply(Command::Move {
            movement: Movement::Right,
            extend: false,
        });
        if buffer.primary().head == before {
            break;
        }
        steps += 1;
        assert!(steps <= cluster_count, "more steps than clusters");
    }
    assert_eq!(steps, cluster_count, "one Right per cluster");
    // And back.
    let mut back = 0;
    loop {
        let before = buffer.primary().head;
        buffer.apply(Command::Move {
            movement: Movement::Left,
            extend: false,
        });
        if buffer.primary().head == before {
            break;
        }
        back += 1;
    }
    assert_eq!(back, cluster_count);
}

#[test]
fn forward_delete_consumes_one_cluster() {
    let mut buffer = Buffer::from_text("👨‍👩‍👧‍👦x");
    buffer.apply(Command::DeleteForward);
    assert_eq!(buffer.text(), "x");
    buffer.apply(Command::Undo);
    assert_eq!(buffer.text(), "👨‍👩‍👧‍👦x");
}

// --- the buffer→layout bridge property ------------------------------------

struct WidthStyler;

impl Styler for WidthStyler {
    fn style(&self, text: &str, _block: &BlockState, conceal: ConcealMode) -> StyledLine {
        StyledLine::plain(text, conceal)
    }
}

struct CountMeasurer;

impl Measurer for CountMeasurer {
    fn measure(&self, line: &StyledLine, wrap_width: f64) -> LineMeasure {
        let cols = line.display.chars().count().max(1) as f64;
        let rows = (cols / wrap_width.max(1.0)).ceil().max(1.0);
        LineMeasure {
            height: rows * 16.0,
            rows: rows as u32,
        }
    }
    fn hit_test(&self, _line: &StyledLine, _wrap_width: f64, _x: f64, _y: f64) -> usize {
        0
    }
}

/// Drive a buffer and a layout engine through random edits, syncing layout
/// only via [`md_editor::ChangedSpan`]. They must never disagree on line
/// count — the exact failure mode behind v2's BUG-B family.
#[test]
fn changed_spans_keep_layout_in_lockstep_with_buffer() {
    let mut rng = Rng(2026);
    for round in 0..20 {
        let initial = random_text(&mut rng, 12);
        let mut buffer = Buffer::from_text(&initial);
        let mut layout = LayoutEngine::new(WidthStyler, CountMeasurer, 10.0);
        layout
            .set_text((0..buffer.line_count()).map(|i| (buffer.line_text(i), BlockState::Normal)));
        for step in 0..80 {
            let cmd = random_command(&mut rng, &buffer);
            let result = buffer.apply(cmd.clone());
            if let Some(span) = result.changed {
                let new_texts: Vec<(String, BlockState)> = (span.first
                    ..span.first + span.new_lines)
                    .map(|i| (buffer.line_text(i), BlockState::Normal))
                    .collect();
                if let Err(e) = layout.splice(span.first, span.old_lines, new_texts) {
                    panic!("round {round} step {step}: splice {span:?} failed: {e} (cmd {cmd:?})");
                }
            }
            assert_eq!(
                layout.line_count(),
                buffer.line_count(),
                "round {round} step {step}: layout diverged after {cmd:?}"
            );
        }
    }
}
