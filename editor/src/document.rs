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
use crate::parse::{BlockState, IncrementalParser, LineKind};
use crate::style::{MarkdownStyler, SpanKind, line_spans};

pub struct EditorDocument<M> {
    buffer: Buffer,
    parser: IncrementalParser,
    layout: LayoutEngine<MarkdownStyler, M>,
    /// Range of lines currently rendered revealed (the primary caret's block).
    revealed: Option<std::ops::Range<usize>>,
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

    /// Record that the buffer's current state was persisted (dirty tracking).
    /// Not a text mutation, so it bypasses `apply` legitimately.
    pub fn mark_saved(&mut self) {
        self.buffer.mark_saved();
    }

    pub fn layout(&self) -> &LayoutEngine<MarkdownStyler, M> {
        &self.layout
    }

    pub fn set_wrap_width(&mut self, wrap_width: f64) {
        self.layout.set_wrap_width(wrap_width);
    }

    pub fn remeasure(&mut self) {
        self.layout.remeasure();
    }

    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Style a line for painting (pure; spans + conceal mode included).
    /// The conceal mode is derived from the live caret position so paint and
    /// the layout engine's measured form (set in [`Self::sync_conceal`]) agree.
    pub fn styled_line(&self, index: usize) -> Option<StyledLine> {
        let block = self.parser.line(index)?.entry.clone();
        let (caret_line, caret_col) = self.buffer.offset_to_line_col(self.buffer.primary().head);
        let reveal_lines = self.reveal_range(caret_line);
        let conceal = self.conceal_at(index, caret_line, caret_col, &reveal_lines);
        Some(MarkdownStyler.style(&self.buffer.line_text(index), &block, conceal))
    }

    /// Map a character offset in styled display text back to source text.
    pub fn display_col_to_source(&self, line: usize, display_col: usize) -> usize {
        let source = self.buffer.line_text(line);
        let Some(styled) = self.styled_line(line) else {
            return display_col.min(source.chars().count());
        };
        // Clicking at/after the visual end of a line lands the caret at the
        // true source end. Trailing concealed markers (a closing `**`, `` ` ``,
        // `$`) belong to the line end, so without this the caret would settle
        // *before* them — the "cursor a few letters short of the end" bug.
        if display_col >= styled.display.chars().count() {
            return source.chars().count();
        }
        subsequence_offset(&source, &styled.display, display_col)
    }

    /// Map a source character offset to nearest styled display offset.
    pub fn source_col_to_display(&self, line: usize, source_col: usize) -> usize {
        let source = self.buffer.line_text(line);
        let Some(styled) = self.styled_line(line) else {
            return source_col.min(source.chars().count());
        };
        source_to_subsequence_offset(&source, &styled.display, source_col)
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

    /// Reveal the primary caret's block, conceal the previously revealed one.
    /// The caret's own line always re-syncs (its *element*-level reveal can
    /// change while the line range stays the same — caret moving from a bold
    /// word into an inline `$math$` on the same line).
    fn sync_conceal(&mut self) -> Damage {
        let (caret_line, caret_col) = self.buffer.offset_to_line_col(self.buffer.primary().head);
        let new_range = self.reveal_range(caret_line);
        let old_range = self.revealed.clone();
        self.revealed = Some(new_range.clone());
        let mut damage = Damage::none();

        // Conceal lines that left the reveal set.
        if let Some(old) = &old_range {
            for i in old.clone() {
                if i < self.layout.line_count()
                    && !new_range.contains(&i)
                    && let Ok(d) = self.layout.set_conceal(i, ConcealMode::Concealed)
                {
                    damage = damage.merge(d);
                }
            }
        }

        // (Re)style every line in the reveal set; `set_conceal` no-ops the
        // unchanged ones, so damage stays confined to what actually moved.
        for i in new_range.clone() {
            if i < self.layout.line_count() {
                let mode = self.conceal_at(i, caret_line, caret_col, &new_range);
                if let Ok(d) = self.layout.set_conceal(i, mode) {
                    damage = damage.merge(d);
                }
            }
        }

        damage
    }

    /// Conceal mode for line `index` given where the caret is. Lines outside
    /// the reveal set are concealed; a multi-line (block) reveal shows every
    /// line's source; a single paragraph under the caret gets **element-level**
    /// reveal — only the inline element the caret sits in shows its markers.
    fn conceal_at(
        &self,
        index: usize,
        caret_line: usize,
        caret_col: usize,
        reveal_lines: &std::ops::Range<usize>,
    ) -> ConcealMode {
        if !reveal_lines.contains(&index) {
            return ConcealMode::Concealed;
        }
        let is_paragraph = self
            .parser
            .line(index)
            .is_some_and(|l| matches!(l.kind, LineKind::Paragraph));
        // Element-level reveal applies only to a lone paragraph under the
        // caret. Block constructs (tables, fences, math, front-matter) and
        // multi-line reveals show their whole source as before.
        if reveal_lines.len() != 1 || index != caret_line || !is_paragraph {
            return ConcealMode::Revealed;
        }
        let text = self.buffer.line_text(index);
        let kind = self
            .parser
            .line(index)
            .map(|l| l.kind.clone())
            .unwrap_or(LineKind::Paragraph);
        let spans = line_spans(&text, &kind);
        // A paragraph carrying a block asset (image) reveals whole-line: the
        // asset is centered block geometry, not an inline element.
        if spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::Image { .. }))
        {
            return ConcealMode::Revealed;
        }
        match element_reveal_range(&spans, caret_col) {
            Some(range) => ConcealMode::Partial(range),
            // Caret in plain text: nothing to reveal, the line stays concealed.
            None => ConcealMode::Concealed,
        }
    }

    pub fn reveal_range(&self, line: usize) -> std::ops::Range<usize> {
        if line >= self.line_count() {
            return line..line + 1;
        }
        let p = match self.parser.line(line) {
            Some(p) => p,
            None => return line..line + 1,
        };

        let is_block_state = |state: &BlockState| {
            matches!(
                state,
                BlockState::Fence { .. } | BlockState::Math | BlockState::FrontMatter
            )
        };

        if is_block_state(&p.entry) || is_block_state(&p.exit) {
            let mut start = line;
            while start > 0 {
                if let Some(prev) = self.parser.line(start - 1) {
                    if is_block_state(&prev.entry) || is_block_state(&prev.exit) {
                        start -= 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let mut end = line + 1;
            while end < self.line_count() {
                if let Some(next) = self.parser.line(end) {
                    if is_block_state(&next.entry) || is_block_state(&next.exit) {
                        end += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            return start..end;
        }

        if matches!(p.kind, LineKind::TableRow | LineKind::TableSep) {
            let mut start = line;
            while start > 0 {
                if let Some(prev) = self.parser.line(start - 1) {
                    if matches!(prev.kind, LineKind::TableRow | LineKind::TableSep) {
                        start -= 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let mut end = line + 1;
            while end < self.line_count() {
                if let Some(next) = self.parser.line(end) {
                    if matches!(next.kind, LineKind::TableRow | LineKind::TableSep) {
                        end += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            return start..end;
        }

        line..line + 1
    }

    pub fn headings(&self) -> Vec<(u8, String, usize)> {
        let mut out = Vec::new();
        for i in 0..self.line_count() {
            if let Some(parse) = self.parser.line(i)
                && let LineKind::Heading { level } = parse.kind
            {
                let raw = self.buffer.line_text(i);
                let clean = raw.trim_start_matches('#').trim().to_string();
                out.push((level, clean, i));
            }
        }
        out
    }
}

/// The source char range of the inline element the caret sits in, or `None`
/// if the caret is in plain text. An "element" is a maximal run of consecutive
/// non-[`SpanKind::Text`] spans (a marker/content/marker trio like `**bold**`,
/// `$math$`, `` `code` ``, `[link](url)`), so the returned range spans the
/// element's content *and* its surrounding markers. Boundaries are inclusive —
/// a caret resting just before/after an element reveals it (Typora's feel).
fn element_reveal_range(
    spans: &[crate::style::Span],
    caret_col: usize,
) -> Option<std::ops::Range<usize>> {
    let mut i = 0;
    while i < spans.len() {
        if matches!(spans[i].kind, SpanKind::Text) {
            i += 1;
            continue;
        }
        let start = spans[i].range.start;
        let mut j = i;
        while j < spans.len() && !matches!(spans[j].kind, SpanKind::Text) {
            j += 1;
        }
        let end = spans[j - 1].range.end;
        if (start..=end).contains(&caret_col) {
            return Some(start..end);
        }
        i = j;
    }
    None
}

fn subsequence_offset(source: &str, display: &str, display_col: usize) -> usize {
    let source = source.chars().collect::<Vec<_>>();
    let display = display.chars().collect::<Vec<_>>();
    let mut source_col = 0;
    for displayed in display.iter().take(display_col.min(display.len())) {
        while source_col < source.len() && source[source_col] != *displayed {
            source_col += 1;
        }
        source_col = (source_col + 1).min(source.len());
    }
    source_col
}

fn source_to_subsequence_offset(source: &str, display: &str, source_col: usize) -> usize {
    let source = source.chars().collect::<Vec<_>>();
    let display = display.chars().collect::<Vec<_>>();
    let target = source_col.min(source.len());
    let mut source_index = 0;
    let mut display_col = 0;
    while source_index < target && display_col < display.len() {
        if source[source_index] == display[display_col] {
            display_col += 1;
        }
        source_index += 1;
    }
    display_col
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

        fn hit_test(&self, line: &StyledLine, wrap_width: f64, x: f64, y: f64) -> usize {
            let cols = wrap_width.floor().max(1.0) as usize;
            let row = (y / 16.0).floor().max(0.0) as usize;
            let col = (x / 10.0).round().max(0.0) as usize;
            let char_idx = row * cols + col;
            char_idx.min(line.display.chars().count())
        }
    }

    fn doc(text: &str) -> EditorDocument<CharMeasurer> {
        EditorDocument::new(CharMeasurer, 80.0, text)
    }

    #[test]
    fn concealed_display_offsets_map_to_source_markers() {
        let mut doc = doc("before **bold** after\nnext");
        doc.apply(Command::SetCursor { line: 1, col: 0 });
        assert_eq!(doc.display_col_to_source(0, 7), 7);
        assert_eq!(doc.display_col_to_source(0, 8), 10);
        assert_eq!(doc.source_col_to_display(0, 9), 7);
        assert_eq!(doc.source_col_to_display(0, 13), 11);
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
            "this unwrapped fixture keeps equal line heights"
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
        // Element-level reveal: the caret sits in the bold element (which is
        // the whole line here), so its markers show and the line is revealed.
        match d.styled_line(0) {
            Some(s) => {
                assert!(matches!(s.conceal, ConcealMode::Partial(_)));
                assert_eq!(s.display, "**a**");
            }
            None => panic!("line 0 missing"),
        }
        d.apply(Command::SetCursor { line: 1, col: 0 });
        match (d.styled_line(0), d.styled_line(1)) {
            (Some(a), Some(b)) => {
                assert_eq!(a.conceal, ConcealMode::Concealed);
                assert_eq!(a.display, "a");
                assert!(matches!(b.conceal, ConcealMode::Partial(_)));
                assert_eq!(b.display, "**b**");
            }
            _ => panic!("lines missing"),
        }
    }

    #[test]
    fn only_the_element_under_the_caret_reveals() {
        // The reported bug: inline math and bold on one line. Clicking into
        // the math must reveal *only* the math, leaving the bold rendered.
        let mut d = doc("see $x^2$ and **bold** here");
        // Caret inside the inline math ("$x^2$" is source 4..9).
        d.apply(Command::SetCursor { line: 0, col: 6 });
        let display = match d.styled_line(0) {
            Some(s) => s.display,
            None => panic!("line 0 missing"),
        };
        // Math markers revealed (source shown), bold markers still concealed.
        assert!(display.contains("$x^2$"), "math source shown: {display}");
        assert!(
            !display.contains("**bold**"),
            "bold stays concealed: {display}"
        );
        assert!(display.contains("bold"));

        // Move the caret into the bold word: now bold reveals, math conceals.
        let bold_col = match d.buffer().text().find("bold") {
            Some(c) => c,
            None => panic!("no bold"),
        };
        d.apply(Command::SetCursor {
            line: 0,
            col: bold_col + 1,
        });
        let display = match d.styled_line(0) {
            Some(s) => s.display,
            None => panic!("line 0 missing"),
        };
        assert!(display.contains("**bold**"), "bold source shown: {display}");
        assert!(
            !display.contains("$x^2$"),
            "math stays concealed: {display}"
        );
    }

    #[test]
    fn click_at_line_end_maps_past_trailing_markers() {
        // "text **bold**" conceals to "text bold" (9 display chars). A click at
        // the visual end must land at the true source end (13), not before the
        // closing `**` (11) — the "caret a few letters short" bug.
        let d = doc("text **bold**");
        let display_len = match d.styled_line(0) {
            Some(s) => s.display.chars().count(),
            None => panic!("line 0 missing"),
        };
        assert_eq!(display_len, 9);
        assert_eq!(d.display_col_to_source(0, 9), 13);
        // A click well past the end clamps to the same true end.
        assert_eq!(d.display_col_to_source(0, 99), 13);
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

    #[test]
    fn test_headings_outline() {
        let d = doc("# Title\n## Sub\nParagraph");
        let heads = d.headings();
        assert_eq!(heads.len(), 2);
        assert_eq!(heads[0], (1, "Title".to_string(), 0));
        assert_eq!(heads[1], (2, "Sub".to_string(), 1));
    }
}
