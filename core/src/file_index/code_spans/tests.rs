//! Unit tests for `code_ranges`: fences, inline spans, and what stays prose.

use super::*;

fn code(content: &str) -> Vec<&str> {
    code_ranges(content)
        .into_iter()
        .map(|r| &content[r])
        .collect()
}

#[test]
fn fenced_blocks_are_code_including_fence_lines() {
    let text = "a [[x]]\n```cpp\n[[nodiscard]] int f();\n```\nb [[y]]\n";
    assert_eq!(code(text), ["```cpp\n[[nodiscard]] int f();\n```\n"]);
}

#[test]
fn tilde_fences_and_longer_closers() {
    let text = "~~~~\n[[a]]\n~~~\nstill code\n~~~~~\nprose";
    assert_eq!(code(text), ["~~~~\n[[a]]\n~~~\nstill code\n~~~~~\n"]);
}

#[test]
fn unclosed_fence_runs_to_the_end() {
    let text = "p\n```\n[[ -f foo ]]";
    assert_eq!(code(text), ["```\n[[ -f foo ]]"]);
}

#[test]
fn backtick_info_string_with_a_backtick_is_not_a_fence() {
    let text = "``` a`b\n[[x]]\n";
    assert!(code(text).iter().all(|c| !c.contains("[[x]]")));
}

#[test]
fn four_space_indent_is_not_a_fence() {
    assert!(code("    ```\n[[x]]\n").is_empty());
}

#[test]
fn inline_spans_match_equal_runs() {
    let text = "see `[[a]]` and ``x ` [[b]]`` but [[c]]";
    assert_eq!(code(text), ["`[[a]]`", "``x ` [[b]]``"]);
}

#[test]
fn unmatched_backticks_are_literal() {
    assert!(code("a ` [[b]] and ``c").is_empty());
}

#[test]
fn inline_span_wraps_a_line_but_not_a_blank_line() {
    assert_eq!(code("`a\n[[b]]` x"), ["`a\n[[b]]`"]);
    assert!(code("`a\n\n[[b]]` x").is_empty());
}

#[test]
fn in_code_checks_membership() {
    let text = "x `[[a]]` [[b]]";
    let ranges = code_ranges(text);
    assert!(in_code(&ranges, text.find("[[a]]").unwrap()));
    assert!(!in_code(&ranges, text.find("[[b]]").unwrap()));
    assert!(!in_code(&ranges, 0));
}
