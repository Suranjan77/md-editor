//! Fuzzy matching for the command palette.
//!
//! The palette has to rank two different kinds of thing — commands and vault
//! files — against the same few keystrokes, so the score has to mean something
//! comparable across both. The rules are the ones people already expect from
//! this kind of input, without a dependency:
//!
//! - The query must appear in order, but not contiguously (`tocfg` finds
//!   `Tracker Configuration`).
//! - Matching the start of a word beats matching the middle of one, so typing
//!   initials works.
//! - Runs of adjacent characters beat scattered ones.
//! - Matching the file's own name beats matching a folder along its path.

/// Characters after which the next character counts as starting a word.
fn is_boundary(ch: char) -> bool {
    matches!(ch, ' ' | '/' | '\\' | '-' | '_' | '.' | '(' | '[')
}

/// Score `haystack` against `query`, or `None` when the query does not appear
/// in order. Higher is a better match; scores are only meaningful relative to
/// other scores for the same query.
pub fn score(haystack: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let hay: Vec<char> = haystack.chars().collect();
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();

    let mut total = 0i32;
    let mut hay_index = 0usize;
    let mut previous_match: Option<usize> = None;

    for &wanted in &needle {
        // Advance to the next character in the haystack that matches.
        let found = hay[hay_index..].iter().position(|candidate| {
            candidate
                .to_lowercase()
                .next()
                .is_some_and(|lowered| lowered == wanted)
        })?;
        let at = hay_index + found;

        let mut points = 10;

        // Start of the string, or the first character of a word.
        if at == 0
            || hay
                .get(at.wrapping_sub(1))
                .copied()
                .is_some_and(is_boundary)
        {
            points += 18;
        }
        // An uppercase letter inside a word also reads as a word start
        // (`CamelCase`), which is how initials work in identifiers.
        else if hay[at].is_uppercase() {
            points += 10;
        }

        // Adjacent to the previous match: reward runs.
        if previous_match == Some(at.wrapping_sub(1)) {
            points += 12;
        } else if let Some(previous) = previous_match {
            // Penalise the distance skipped, but never below zero for this
            // character, so a long path cannot score worse than no match.
            points -= ((at - previous - 1) as i32).min(8);
        }

        total += points;
        previous_match = Some(at);
        hay_index = at + 1;
    }

    // Prefer the tighter of two otherwise-equal matches.
    total -= (hay.len() as i32) / 12;

    Some(total)
}

/// Score a vault path, favouring matches in the file's own name over matches
/// in the folders leading to it. Typing `note` should surface `note.md`
/// before `notes/archive/something-else.md`.
pub fn score_path(path: &str, query: &str) -> Option<i32> {
    let filename = path.rsplit('/').next().unwrap_or(path);

    let whole = score(path, query);
    let name = score(filename, query).map(|s| s + 24);

    match (whole, name) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_the_query_in_order() {
        assert!(score("Toggle Sidebar", "tsb").is_some());
        assert!(
            score("Toggle Sidebar", "bst").is_none(),
            "out-of-order characters are not a match"
        );
        assert!(score("Toggle Sidebar", "xyz").is_none());
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("anything", ""), Some(0));
    }

    #[test]
    fn word_starts_beat_mid_word_matches() {
        // "sv" as the initials of "Split View" should beat the incidental
        // s-then-v inside a single word.
        let initials = score("Split View", "sv").unwrap();
        let incidental = score("subversive", "sv").unwrap();
        assert!(
            initials > incidental,
            "initials {initials} should outrank mid-word {incidental}"
        );
    }

    #[test]
    fn runs_beat_scattered_matches() {
        let run = score("search vault", "sea").unwrap();
        let scattered = score("some early answer", "sea").unwrap();
        assert!(
            run > scattered,
            "contiguous {run} should outrank scattered {scattered}"
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(score("Study Tracker", "study tracker").is_some());
        assert!(score("study tracker", "STUDY").is_some());
    }

    #[test]
    fn filename_matches_outrank_folder_matches() {
        let in_name = score_path("archive/2026/attention.md", "attention").unwrap();
        let in_folder = score_path("attention/2026/something-unrelated.md", "attention").unwrap();
        assert!(
            in_name > in_folder,
            "filename hit {in_name} should outrank folder hit {in_folder}"
        );
    }

    #[test]
    fn scores_are_never_absurdly_negative_for_long_paths() {
        // A real match deep in a long path must still beat no match at all.
        let deep = score_path(
            "a/very/deeply/nested/set/of/folders/leading/to/the/file/target.md",
            "target",
        );
        assert!(deep.is_some_and(|s| s > 0), "got {deep:?}");
    }
}
