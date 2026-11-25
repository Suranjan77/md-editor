//! Inline completion in notes: `[[` suggests vault files to link, `/` at the start of a line suggests blocks to insert.

#[cfg(test)]
mod tests;

/// Most suggestions shown at once.
pub const MAX_ITEMS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// `[[query` — a vault file to link.
    Wikilink,
    /// `/query` at the start of a line — a block to insert.
    Slash,
}

/// One suggestion.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    /// Text that replaces the trigger and query.
    pub insert: String,
    /// Characters to step the caret back after inserting, to land inside the
    /// inserted block (a code fence's body, say).
    pub caret_back: usize,
}

/// An open completion popup.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub kind: CompletionKind,
    /// Line and column (chars) where the trigger (`[[` or `/`) starts.
    pub line: usize,
    pub trigger_col: usize,
    pub query: String,
    pub items: Vec<CompletionItem>,
    pub selected: usize,
}

impl Completion {
    pub fn move_selection(&mut self, delta: isize) {
        let len = self.items.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        }
    }
}

/// What the text before the caret on its line asks to complete, if anything:
/// the kind, the column the trigger starts at, and the query typed after it.
pub fn detect(before_caret: &str) -> Option<(CompletionKind, usize, String)> {
    if let Some(at) = before_caret.rfind("[[") {
        let query = &before_caret[at + 2..];
        // Closed, aliased (`|`) or anchored (`#`) links are past completing.
        if !query.contains([']', '|', '#']) && query.chars().count() <= 80 {
            let col = before_caret[..at].chars().count();
            return Some((CompletionKind::Wikilink, col, query.to_string()));
        }
    }
    let indent = before_caret.len() - before_caret.trim_start().len();
    let rest = before_caret[indent..].strip_prefix('/')?;
    if rest.chars().all(|c| c.is_alphanumeric() || c == '-') && rest.chars().count() <= 20 {
        let col = before_caret[..indent].chars().count();
        return Some((CompletionKind::Slash, col, rest.to_string()));
    }
    None
}

/// Vault files to link, best match first. A note is linked by its name
/// alone when no other note shares it, else by its vault path; other files
/// keep their extension.
pub fn wikilink_items(paths: &[&str], query: &str) -> Vec<CompletionItem> {
    let stem = |path: &str| {
        let name = path.rsplit('/').next().unwrap_or(path);
        if is_note(path) {
            name.rsplit_once('.').map_or(name, |(s, _)| s).to_string()
        } else {
            name.to_string()
        }
    };
    let mut scored: Vec<(i32, &str)> = paths
        .iter()
        .filter_map(|path| crate::fuzzy::score_path(path, query).map(|s| (s, *path)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
    scored
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, path)| {
            let name = stem(path);
            let unique = paths
                .iter()
                .filter(|other| stem(other).eq_ignore_ascii_case(&name))
                .count()
                == 1;
            let target = if !is_note(path) || unique {
                if is_note(path) {
                    name.clone()
                } else {
                    path.to_string()
                }
            } else {
                path.rsplit_once('.').map_or(path, |(s, _)| s).to_string()
            };
            CompletionItem {
                label: name,
                detail: Some(path.to_string()),
                insert: format!("[[{target}]]"),
                caret_back: 0,
            }
        })
        .collect()
}

fn is_note(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// Blocks `/` can insert: (name, keywords, text, caret steps back).
const SLASH_BLOCKS: &[(&str, &str, &str, usize)] = &[
    ("Heading 1", "h1 title", "# ", 0),
    ("Heading 2", "h2", "## ", 0),
    ("Heading 3", "h3", "### ", 0),
    ("Task", "todo checkbox", "- [ ] ", 0),
    ("Bullet list", "ul list", "- ", 0),
    ("Numbered list", "ol list", "1. ", 0),
    ("Quote", "blockquote", "> ", 0),
    ("Note callout", "callout info", "> [!note] ", 0),
    ("Warning callout", "callout caution", "> [!warning] ", 0),
    ("Code block", "fence code", "```\n\n```", 4),
    ("Math block", "equation latex", "$$\n\n$$", 3),
    (
        "Table",
        "grid",
        "| Column 1 | Column 2 |\n| :--- | :--- |\n| Cell | Cell |",
        0,
    ),
    ("Divider", "hr rule line", "---\n", 0),
];

/// Block templates matching `query`, best first.
pub fn slash_items(query: &str) -> Vec<CompletionItem> {
    let mut scored: Vec<(i32, usize)> = SLASH_BLOCKS
        .iter()
        .enumerate()
        .filter_map(|(i, (name, keywords, _, _))| {
            let by_name = crate::fuzzy::score(name, query);
            let by_keyword = crate::fuzzy::score(keywords, query).map(|s| s - 1);
            by_name.max(by_keyword).map(|s| (s, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, i)| {
            let (name, _, text, caret_back) = SLASH_BLOCKS[i];
            CompletionItem {
                label: name.to_string(),
                detail: None,
                insert: text.to_string(),
                caret_back,
            }
        })
        .collect()
}
