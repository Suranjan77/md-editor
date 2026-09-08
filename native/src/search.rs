#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMatch {
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentMatch {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
}

pub fn line_matches(text: &str, query: &str, regex: bool, match_case: bool) -> Vec<LineMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    if regex {
        let Ok(re) = regex::RegexBuilder::new(query)
            .case_insensitive(!match_case)
            .build()
        else {
            return Vec::new();
        };
        return re
            .find_iter(text)
            .filter_map(|m| {
                if m.start() == m.end() {
                    return None;
                }
                let start_col = text[..m.start()].chars().count();
                let end_col = start_col + text[m.start()..m.end()].chars().count();
                Some(LineMatch { start_col, end_col })
            })
            .collect();
    }

    // Each haystack entry carries the column of the *original* char it came
    // from: some chars lowercase to more than one char (e.g. 'İ' → "i̇"), so
    // indices into the lowercased sequence are not valid columns in the
    // original line.
    let haystack: Vec<(usize, char)> = if match_case {
        text.chars().enumerate().collect()
    } else {
        text.chars()
            .enumerate()
            .flat_map(|(col, c)| c.to_lowercase().map(move |lc| (col, lc)))
            .collect()
    };
    let needle: Vec<char> = if match_case {
        query.chars().collect()
    } else {
        query.chars().flat_map(char::to_lowercase).collect()
    };

    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        let window = &haystack[index..index + needle.len()];
        if window.iter().map(|&(_, c)| c).eq(needle.iter().copied()) {
            matches.push(LineMatch {
                start_col: window[0].0,
                end_col: window[needle.len() - 1].0 + 1,
            });
            index += needle.len().max(1);
        } else {
            index += 1;
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_columns_stay_aligned_after_expanding_lowercase() {
        // 'İ' lowercases to two chars ("i" + combining dot); columns reported
        // must index the original line, not the lowercased sequence.
        let matches = line_matches("İİ abc", "abc", false, false);
        assert_eq!(
            matches,
            vec![LineMatch {
                start_col: 3,
                end_col: 6
            }]
        );

        // Plain ASCII behaviour unchanged.
        let matches = line_matches("Hello hello", "hello", false, false);
        assert_eq!(
            matches,
            vec![
                LineMatch {
                    start_col: 0,
                    end_col: 5
                },
                LineMatch {
                    start_col: 6,
                    end_col: 11
                }
            ]
        );
    }
}
