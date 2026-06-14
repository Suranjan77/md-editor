#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListKind {
    Unordered,
    Checkbox,
    Ordered(u64, char),
}

pub(super) struct ListPrefix {
    pub kind: ListKind,
    pub raw: String,
    pub indent: String,
}

pub(super) fn matching_pair(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '*' => Some('*'),
        '_' => Some('_'),
        '`' => Some('`'),
        _ => None,
    }
}

pub(super) fn is_closing_char(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'' | '*' | '_' | '`')
}

pub(super) fn parse_list_prefix(line: &str) -> Option<ListPrefix> {
    let indent_len = line
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
        .count();
    let indent: String = line.chars().take(indent_len).collect();
    let rest = &line[indent.len()..];

    if ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "* [X] "]
        .iter()
        .any(|prefix| rest.starts_with(prefix))
    {
        return Some(ListPrefix {
            kind: ListKind::Checkbox,
            raw: rest[..6].to_string(),
            indent,
        });
    }
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        return Some(ListPrefix {
            kind: ListKind::Unordered,
            raw: rest[..2].to_string(),
            indent,
        });
    }

    let digit_count = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    let num_str = rest.get(..digit_count)?;
    let post_digits = rest.get(digit_count..)?;
    if digit_count > 0
        && (post_digits.starts_with(". ") || post_digits.starts_with(") "))
        && let Ok(num) = num_str.parse::<u64>()
        && let Some(delimiter) = post_digits.chars().next()
    {
        return Some(ListPrefix {
            kind: ListKind::Ordered(num, delimiter),
            raw: rest[..digit_count + 2].to_string(),
            indent,
        });
    }
    None
}
