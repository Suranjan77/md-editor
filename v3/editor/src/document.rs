//! The editor session: [`Buffer`] + [`IncrementalParser`] + [`LayoutEngine`]
//! composed behind one `apply()` — the type a shell embeds per open
//! markdown document.
//!
//! Responsibilities (and nothing else):
//! - forward commands to the buffer,
//! - keep the parser converged via the buffer's [`ChangedSpan`],
//! - keep the layout spliced for the span *and* restyled for every line the
//!   parser invalidated beyond it (the fence-typing cascade),
//! - reveal the primary caret's line, conceal the one it left,
//! - merge all of that into a single [`Damage`] for the paint phase.

use crate::buffer::{ApplyResult, Buffer, Command};
use crate::layout::{ConcealMode, Damage, LayoutEngine, Measurer, StyledLine, Styler};
use crate::parse::{BlockState, IncrementalParser};
use crate::style::MarkdownStyler;

pub struct EditorDocument<M> {
    buffer: Buffer,
    parser: IncrementalParser,
    layout: LayoutEngine<MarkdownStyler, M>,
    /// Line currently rendered revealed (the primary caret's line).
    revealed: Option<usize>,
}

impl<M: Measurer> EditorDocument<M> {
    pub fn new(measurer: M, wrap_width: f64, text: &str) -> EditorDocument<M> {
        let buffer = Buffer::from_text(text);
        let mut parser = IncrementalParser::new();
        parser.parse_full((0..buffer.line_count()).map(|i| buffer.line_text(i)));
        let mut layout = LayoutEngine::new(MarkdownStyler, measurer, wrap_width);
        layout.set_text((0..buffer.line_count()).map(|i| {
            let block = parser.line(i).map(|l| l.entry.clone()).unwrap_or_default();
            (buffer.line_text(i), block)
        }));
        let mut doc = EditorDocument {
            buffer,
            parser,
            layout,
            revealed: None,
        };
        doc.sync_conceal();
        doc
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn layout(&self) -> &LayoutEngine<MarkdownStyler, M> {
        &self.layout
    }

    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Style a line for painting (pure; spans + conceal mode included).
    pub fn styled_line(&self, index: usize) -> Option<StyledLine> {
        let block = self.parser.line(index)?.entry.clone();
        let conceal = if self.revealed == Some(index) {
            ConcealMode::Revealed
        } else {
            ConcealMode::Concealed
        };
        Some(MarkdownStyler.style(&self.buffer.line_text(index), &block, conceal))
    }

    /// The single mutation path. Returns the merged damage plus what the
    /// buffer reported (selection motion etc.).
    pub fn apply(&mut self, command: Command) -> (ApplyResult, Damage) {
        let result = self.buffer.apply(command);
        let mut damage = Damage::none();
        if let Some(span) = result.changed {
            // 1. Parser convergence (may invalidate past the edit).
            let buffer = &self.buffer;
            let invalidated = self
                .parser
                .splice(span.first, span.old_lines, span.new_lines, |i| {
                    buffer.line_text(i)
                });
            // 2. Layout splice for the edited span…
            let span_new_end = span.first + span.new_lines;
            let items: Vec<(String, BlockState)> = (span.first..span_new_end)
                .map(|i| (self.buffer.line_text(i), self.entry_state(i)))
                .collect();
            if let Ok(d) = self.layout.splice(span.first, span.old_lines, items) {
                damage = damage.merge(d);
            }
            // 3. …and restyle the cascade beyond it.
            for i in span_new_end..invalidated.end {
                let (text, block) = (self.buffer.line_text(i), self.entry_state(i));
                if let Ok(d) = self.layout.replace_line(i, &text, block) {
                    damage = damage.merge(d);
                }
            }
        }
        if result.selection_changed || result.text_changed {
            damage = damage.merge(self.sync_conceal());
        }
        (result, damage)
    }

    fn entry_state(&self, index: usize) -> BlockState {
        self.parser
            .line(index)
            .map(|l| l.entry.clone())
            .unwrap_or_default()
    }

    /// Reveal the primary caret's line, conceal the previously revealed one.
    /// With the layout-stable styler this damages at most two lines and
    /// never shifts geometry — the M1 golden gate, now wired end to end.
    fn sync_conceal(&mut self) -> Damage {
        let (line, _) = self.buffer.offset_to_line_col(self.buffer.primary().head);
        let old = self.revealed;
        if old == Some(line) {
            return Damage::none();
        }
        self.revealed = Some(line);
        let mut damage = Damage::none();
        // The old line may have been removed by the edit; ignore stale ids.
        if let Some(old) = old
            && old < self.layout.line_count()
            && let Ok(d) = self.layout.set_conceal(old, ConcealMode::Concealed)
        {
            damage = damage.merge(d);
        }
        if let Ok(d) = self.layout.set_conceal(line, ConcealMode::Revealed) {
            damage = damage.merge(d);
        }
        damage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Movement;
    use crate::layout::LineMeasure;

    struct CharMeasurer;

    impl Measurer for CharMeasurer {
        fn measure(&self, line: &StyledLine, wrap_width: f64) -> LineMeasure {
            let cols = line.display.chars().count().max(1) as f64;
            let rows = (cols / wrap_width.max(1.0)).ceil().max(1.0);
            LineMeasure {
                height: rows * 16.0,
                rows: rows as u32,
            }
        }
    }

    fn doc(text: &str) -> EditorDocument<CharMeasurer> {
        EditorDocument::new(CharMeasurer, 80.0, text)
    }

    #[test]
    fn typing_a_fence_restyles_the_cascade() {
        let mut d = doc("a\nplain\nplain");
        // Put the caret at line 0 end and type a fence.
        d.apply(Command::SetCursor { line: 0, col: 1 });
        let (_, _) = d.apply(Command::Insert("\n```".into()));
        // Lines below the new fence are now code content.
        let styled = match d.styled_line(2) {
            Some(s) => s,
            None => panic!("line 2 missing"),
        };
        assert_eq!(styled.kind, crate::parse::LineKind::CodeContent);
        assert_eq!(d.layout().line_count(), d.line_count());
    }

    #[test]
    fn caret_motion_damages_at_most_two_lines_end_to_end() {
        let mut d = doc("# one\ntwo **bold**\nthree\nfour");
        d.apply(Command::SetCursor { line: 0, col: 0 });
        let (_, damage) = d.apply(Command::Move {
            movement: Movement::Down,
            extend: false,
        });
        assert!(
            damage.shifted_from.is_none(),
            "conceal flip must not shift geometry (reserved width)"
        );
        assert!(
            damage.repaint.len() <= 2,
            "caret motion repainted {:?}",
            damage.repaint
        );
    }

    #[test]
    fn revealed_line_follows_the_caret() {
        let mut d = doc("**a**\n**b**");
        d.apply(Command::SetCursor { line: 0, col: 0 });
        match d.styled_line(0) {
            Some(s) => assert_eq!(s.conceal, ConcealMode::Revealed),
            None => panic!("line 0 missing"),
        }
        d.apply(Command::SetCursor { line: 1, col: 0 });
        match (d.styled_line(0), d.styled_line(1)) {
            (Some(a), Some(b)) => {
                assert_eq!(a.conceal, ConcealMode::Concealed);
                assert_eq!(b.conceal, ConcealMode::Revealed);
            }
            _ => panic!("lines missing"),
        }
    }

    #[test]
    fn undo_keeps_layout_and_parser_in_sync() {
        let mut d = doc("start");
        d.apply(Command::Move {
            movement: Movement::DocEnd,
            extend: false,
        });
        d.apply(Command::Insert("\n```\ncode".into()));
        d.apply(Command::Undo);
        assert_eq!(d.buffer().text(), "start");
        assert_eq!(d.layout().line_count(), 1);
        match d.styled_line(0) {
            Some(s) => assert_eq!(s.kind, crate::parse::LineKind::Paragraph),
            None => panic!("line 0 missing"),
        }
    }
}
