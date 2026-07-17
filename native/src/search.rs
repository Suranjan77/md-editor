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

    let mut original_indices = Vec::new();
    let haystack: Vec<char> = if match_case {
        text.chars()
            .enumerate()
            .map(|(index, ch)| {
                original_indices.push(index);
                ch
            })
            .collect()
    } else {
        let mut folded = Vec::new();
        for (index, ch) in text.chars().enumerate() {
            for lower in ch.to_lowercase() {
                folded.push(lower);
                original_indices.push(index);
            }
        }
        folded
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
            let start_col = original_indices[index];
            let end_col = original_indices[index + needle.len() - 1] + 1;
            matches.push(LineMatch { start_col, end_col });
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
    fn expanding_lowercase_keeps_original_columns() {
        let matches = line_matches("İx TARGET", "target", false, false);
        assert_eq!(matches, vec![LineMatch { start_col: 3, end_col: 9 }]);
    }
}
