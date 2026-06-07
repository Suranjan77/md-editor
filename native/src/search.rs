#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineMatch {
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentMatch {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
}

pub(crate) fn line_matches(
    text: &str,
    query: &str,
    regex: bool,
    match_case: bool,
) -> Vec<LineMatch> {
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

    let haystack: Vec<char> = if match_case {
        text.chars().collect()
    } else {
        text.to_lowercase().chars().collect()
    };
    let needle: Vec<char> = if match_case {
        query.chars().collect()
    } else {
        query.to_lowercase().chars().collect()
    };

    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if haystack[index..index + needle.len()] == needle[..] {
            matches.push(LineMatch {
                start_col: index,
                end_col: index + needle.len(),
            });
            index += needle.len().max(1);
        } else {
            index += 1;
        }
    }
    matches
}
