//! Tests for completion triggers and suggestions.

use super::*;

#[test]
fn detects_an_open_wikilink_and_a_slash_at_line_start() {
    assert_eq!(
        detect("see [[Atten"),
        Some((CompletionKind::Wikilink, 4, "Atten".to_string()))
    );
    assert_eq!(
        detect("[["),
        Some((CompletionKind::Wikilink, 0, String::new()))
    );
    assert_eq!(detect("[[Note]] and"), None, "closed link");
    assert_eq!(detect("[[Note|ali"), None, "alias");
    assert_eq!(detect("[[Note#sec"), None, "anchor");
    assert_eq!(
        detect("  /tab"),
        Some((CompletionKind::Slash, 2, "tab".to_string()))
    );
    assert_eq!(detect("a /tab"), None, "slash mid-line");
    assert_eq!(detect("/tab le"), None, "space ends the command");
    assert_eq!(detect("https://x"), None);
}

#[test]
fn wikilink_items_rank_and_disambiguate() {
    let paths = [
        "notes/Attention.md",
        "archive/Attention.md",
        "papers/attention-paper.pdf",
        "Optimizers.md",
    ];
    let items = wikilink_items(&paths, "atten");
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|i| !i.label.starts_with("Optim")));
    let insert_for = |path: &str| {
        items
            .iter()
            .find(|i| i.detail.as_deref() == Some(path))
            .map(|i| i.insert.clone())
    };
    // Two notes share a name: link by path.
    assert_eq!(
        insert_for("notes/Attention.md").as_deref(),
        Some("[[notes/Attention]]")
    );
    // A PDF keeps its extension and path.
    assert_eq!(
        insert_for("papers/attention-paper.pdf").as_deref(),
        Some("[[papers/attention-paper.pdf]]")
    );
    let unique = wikilink_items(&paths, "opt");
    assert_eq!(unique[0].insert, "[[Optimizers]]");
}

#[test]
fn slash_items_match_names_and_keywords() {
    assert_eq!(slash_items("tab")[0].label, "Table");
    assert_eq!(slash_items("todo")[0].label, "Task");
    let code = slash_items("code");
    assert_eq!(code[0].insert, "```\n\n```");
    assert_eq!(code[0].caret_back, 4);
    assert_eq!(slash_items("").len(), MAX_ITEMS);
    assert!(slash_items("zzzz").is_empty());
}

#[test]
fn selection_wraps() {
    let mut c = Completion {
        kind: CompletionKind::Slash,
        line: 0,
        trigger_col: 0,
        query: String::new(),
        items: slash_items("h"),
        selected: 0,
    };
    let n = c.items.len();
    c.move_selection(-1);
    assert_eq!(c.selected, n - 1);
    c.move_selection(1);
    assert_eq!(c.selected, 0);
}
