//! Byte ranges of Markdown code (fenced blocks and inline code spans), where `[[...]]` is code, not a wikilink.

use std::ops::Range;

#[cfg(test)]
mod tests;

/// Sorted, non-overlapping byte ranges of `content` that are code: fenced
/// blocks (```` ``` ```` / `~~~`, fence lines included, an unclosed fence runs
/// to the end) and inline code spans (a backtick run closed by a run of the
/// same length within the same paragraph).
pub(crate) fn code_ranges(content: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(u8, usize, usize)> = None; // (char, length, block start)
    let mut prose_start = 0;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        match fence {
            Some((ch, len, block_start)) => {
                if closes_fence(line, ch, len) {
                    ranges.push(block_start..offset);
                    fence = None;
                    prose_start = offset;
                }
            }
            None => {
                if let Some((ch, len)) = opens_fence(line) {
                    inline_spans(content, prose_start..start, &mut ranges);
                    fence = Some((ch, len, start));
                }
            }
        }
    }
    match fence {
        Some((_, _, block_start)) => ranges.push(block_start..content.len()),
        None => inline_spans(content, prose_start..content.len(), &mut ranges),
    }
    ranges
}

/// Whether byte `pos` lies inside one of `ranges` (as from [`code_ranges`]).
pub(crate) fn in_code(ranges: &[Range<usize>], pos: usize) -> bool {
    let i = ranges.partition_point(|r| r.end <= pos);
    ranges.get(i).is_some_and(|r| r.start <= pos)
}

/// An opening fence: up to three spaces, then three or more backticks or
/// tildes. A backtick fence's info string may not contain a backtick.
fn opens_fence(line: &str) -> Option<(u8, usize)> {
    let rest = strip_indent(line)?;
    let ch = *rest.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == ch).count();
    if len < 3 || (ch == b'`' && rest[len..].contains('`')) {
        return None;
    }
    Some((ch, len))
}

/// A closing fence: the same character, at least as long, nothing after it
/// but whitespace.
fn closes_fence(line: &str, ch: u8, len: usize) -> bool {
    let Some(rest) = strip_indent(line) else {
        return false;
    };
    let run = rest.bytes().take_while(|&b| b == ch).count();
    run >= len && rest[run..].trim().is_empty()
}

fn strip_indent(line: &str) -> Option<&str> {
    let spaces = line.bytes().take_while(|&b| b == b' ').count();
    (spaces <= 3).then(|| &line[spaces..])
}

/// Inline code spans in `content[region]`. A span may wrap lines but not
/// cross a blank line; an unmatched backtick run is literal text.
fn inline_spans(content: &str, region: Range<usize>, out: &mut Vec<Range<usize>>) {
    let bytes = content.as_bytes();
    let end = region.end;
    let mut i = region.start;
    while i < end {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = i;
        let n = run_len(bytes, i, end);
        i += n;
        let mut j = i;
        let mut closed = None;
        while j < end {
            match bytes[j] {
                b'`' => {
                    let m = run_len(bytes, j, end);
                    if m == n {
                        closed = Some(j + m);
                        break;
                    }
                    j += m;
                }
                b'\n' if blank_line_follows(bytes, j + 1, end) => break,
                _ => j += 1,
            }
        }
        if let Some(close) = closed {
            out.push(open..close);
            i = close;
        }
    }
}

fn run_len(bytes: &[u8], from: usize, end: usize) -> usize {
    bytes[from..end].iter().take_while(|&&b| b == b'`').count()
}

/// Whether the line starting at `from` holds only whitespace.
fn blank_line_follows(bytes: &[u8], from: usize, end: usize) -> bool {
    let line_end = bytes[from..end]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(end, |p| from + p);
    from < end
        && bytes[from..line_end]
            .iter()
            .all(|b| matches!(b, b' ' | b'\t' | b'\r'))
}
