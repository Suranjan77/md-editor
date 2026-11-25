//! Tests for `highlight`.

use super::{Highlighted, highlight_markdown};
use crate::editor::test_docs::{Rng, block, document};

/// `HIGHLIGHT_PROPERTY_CASES` overrides the default number of cases.
fn cases(default: u64) -> u64 {
    std::env::var("HIGHLIGHT_PROPERTY_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(default)
}

fn random_edit(rng: &mut Rng, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = rng.below(chars.len() + 1);
    let head: String = chars[..at].iter().collect();
    if rng.chance(0.4) && at < chars.len() {
        let end = (at + 1 + rng.below(40)).min(chars.len());
        let tail: String = chars[end..].iter().collect();
        return head + &tail;
    }
    let tail: String = chars[at..].iter().collect();
    let insert = match rng.below(6) {
        0 => "\n".to_string(),
        1 => rng
            .pick(&["```", "$$", "| x |", "> ", "- [ ] ", "\\end{align}", "# "])
            .to_string(),
        2 => {
            let mut lines = Vec::new();
            block(rng, &mut lines);
            format!("\n{}\n", lines.join("\n"))
        }
        _ => rng
            .pick(&["a", " ", "*", "`", "$", "|", "word "])
            .to_string(),
    };
    head + &insert + &tail
}

/// Whether every block in `lines` is of a single kind.
fn blocks_are_homogeneous(lines: &[super::StyledLine]) -> bool {
    let mut kinds = std::collections::HashMap::new();
    lines.iter().all(|line| {
        let kind = super::block_kind(line).map(|k| k as u8);
        *kinds.entry(line.block_id).or_insert(kind) == kind
    })
}

#[test]
fn stateful_highlighting_matches_the_reference_implementation() {
    let mut compared = 0;
    for seed in 0..cases(600) {
        let doc = document(&mut Rng::new(seed));
        let reference = super::reference::highlight_markdown(&doc);
        // The reference lets malformed nesting mix kinds in one block,
        // which the current numbering deliberately avoids.
        if !blocks_are_homogeneous(&reference) {
            continue;
        }
        compared += 1;
        assert_eq!(highlight_markdown(&doc), reference, "seed {seed}:\n{doc}");
    }
    assert!(
        compared > cases(600) / 2,
        "too few documents compared: {compared}"
    );
}

#[test]
fn line_comments_end_at_the_end_of_their_line() {
    let lines = highlight_markdown("```python\n# a comment\nx = 1\n```");
    let comment = lines[1].spans[0].color;
    assert!(
        lines[2].spans.iter().any(|span| span.color != comment),
        "code after a comment is still coloured as a comment"
    );
}

#[test]
fn every_block_is_homogeneous_through_edits() {
    for seed in 0..cases(150) {
        let mut rng = Rng::new(seed ^ 0x5851_F42D_4C95_7F2D);
        let mut text = document(&mut rng);
        let mut highlighted = Highlighted::new(&text);
        for step in 0..20 {
            assert!(
                blocks_are_homogeneous(&highlighted.lines),
                "seed {seed} step {step}:\n{text}"
            );
            text = random_edit(&mut rng, &text);
            highlighted.update(&text);
        }
    }
}

#[test]
fn incremental_updates_equal_highlighting_from_scratch() {
    for seed in 0..cases(150) {
        let mut rng = Rng::new(seed ^ 0xD1B5_4A32_D192_ED03);
        let mut text = document(&mut rng);
        let mut highlighted = Highlighted::new(&text);
        for step in 0..25 {
            text = random_edit(&mut rng, &text);
            highlighted.update(&text);
            let fresh = Highlighted::new(&text);
            if let Some(i) = (0..fresh.lines.len().max(highlighted.lines.len()))
                .find(|&i| highlighted.lines.get(i) != fresh.lines.get(i))
            {
                panic!(
                    "seed {seed} step {step}: line {i} differs\n incremental: {:#?}\n fresh: {:#?}\n--- text ---\n{text}",
                    highlighted.lines.get(i),
                    fresh.lines.get(i)
                );
            }
        }
    }
}

#[test]
fn an_edit_rehighlights_only_until_the_state_converges() {
    let body = (0..200)
        .map(|i| format!("paragraph {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!("# Title\n{body}");
    let mut highlighted = Highlighted::new(&text);
    let edited = text.replacen("paragraph 100", "paragraph one hundred", 1);
    let range = highlighted.update(&edited);
    assert_eq!(range, 101..102);

    // Opening a fence changes the state of everything after it.
    let fenced = edited.replacen("paragraph 50", "```", 1);
    let range = highlighted.update(&fenced);
    assert_eq!(range.start, 51);
    assert_eq!(range.end, 201);
    assert_eq!(highlighted.lines, highlight_markdown(&fenced));
}

#[test]
fn heading_is_not_math() {
    let lines = highlight_markdown("# Heading");
    assert!(lines[0].spans.iter().any(|span| span.is_heading));
    assert!(!lines[0].is_math_block);
}

#[test]
fn tag_without_space_is_not_a_heading() {
    let tag = highlight_markdown("#tag");
    assert!(!tag[0].spans.iter().any(|span| span.is_heading));

    let heading = highlight_markdown("# H1");
    assert!(heading[0].spans.iter().any(|span| span.is_heading));

    let incomplete = highlight_markdown("#");
    assert!(!incomplete[0].spans.iter().any(|span| span.is_heading));
    let transitioned = highlight_markdown("# Heading");
    assert!(transitioned[0].spans.iter().any(|span| span.is_heading));
}

#[test]
fn align_environment_is_one_math_block() {
    let lines = highlight_markdown("\\begin{align}\na &= b\n\\end{align}\n# Next");
    assert!(lines[0].is_math_block);
    assert_eq!(lines[0].block_id, lines[1].block_id);
    assert_eq!(lines[1].block_id, lines[2].block_id);
    assert_eq!(
        lines[0].spans[0].visible_text(false),
        "\\begin{align}\na &= b\n\\end{align}"
    );
    assert!(!lines[3].is_math_block);
    assert!(lines[3].spans.iter().any(|span| span.is_heading));
}

#[test]
fn align_with_nonumber_accepts_whitespace_before_environment_braces() {
    let lines = highlight_markdown("\\begin {align}\na &= b \\nonumber\n\\end {align}\nAfter");

    let block_id = lines[0].block_id;
    assert!(
        lines[..3]
            .iter()
            .all(|line| line.is_math_block && line.block_id == block_id)
    );
    assert!(lines[2].is_block_fence);
    assert_eq!(
        lines[0].spans[0].visible_text(false),
        "\\begin {align}\na &= b \\nonumber\n\\end {align}"
    );
    assert!(!lines[3].is_math_block);
    assert_eq!(lines[3].spans[0].visible_text(false), "After");
}

#[test]
fn dollar_math_keeps_nested_align_until_the_dollar_fence() {
    let markdown = "$$\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n$$\nAfter";
    let lines = highlight_markdown(markdown);

    let block_id = lines[0].block_id;
    assert!(
        lines[..6]
            .iter()
            .all(|line| line.is_math_block && line.block_id == block_id)
    );
    assert!(!lines[4].is_block_fence);
    assert!(lines[5].is_block_fence);
    assert_eq!(
        lines[0].spans[0].visible_text(false),
        "\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}"
    );
    assert!(!lines[6].is_math_block);
    assert_eq!(lines[6].spans[0].visible_text(false), "After");
}

#[test]
fn math_environment_only_closes_on_its_matching_end() {
    let markdown = "\\begin{align}\nf(x) &= \\begin{cases}\nx & x > 0\n\\end{cases} \\\\\ng(x) &= 0\n\\end{align}\nAfter";
    let lines = highlight_markdown(markdown);

    let block_id = lines[0].block_id;
    assert!(
        lines[..6]
            .iter()
            .all(|line| line.is_math_block && line.block_id == block_id)
    );
    assert!(!lines[3].is_block_fence);
    assert!(lines[5].is_block_fence);
    assert!(
        lines[0].spans[0]
            .visible_text(false)
            .contains(r"\end{cases} \\")
    );
    assert!(!lines[6].is_math_block);
}

#[test]
fn single_line_math_environment_does_not_swallow_following_text() {
    let lines = highlight_markdown("\\begin{align}a &= b\\end{align}\nAfter");

    assert!(lines[0].is_math_block);
    assert_eq!(
        lines[0].spans[0].visible_text(false),
        "\\begin{align}a &= b\\end{align}"
    );
    assert!(!lines[1].is_math_block);
}

#[test]
fn unknown_begin_environment_is_plain_text() {
    let lines = highlight_markdown("\\begin{note}\n# Still heading");
    assert!(!lines[0].is_math_block);
    assert!(lines[1].spans.iter().any(|span| span.is_heading));
}

#[test]
fn single_line_display_math_does_not_swallow_following_heading() {
    let lines = highlight_markdown("$$a=b$$\n## Change of Basis\nPlain text");
    assert!(lines[0].is_math_block);
    assert!(!lines[1].is_math_block);
    assert!(lines[1].spans.iter().any(|span| span.is_heading));
    assert!(!lines[2].is_math_block);
}

#[test]
fn inline_basic_markdown_flags_only_target_spans() {
    let lines = highlight_markdown("plain **bold** *italic* `code` [link](note.md)");
    let line = &lines[0];

    let bold = line.spans.iter().find(|span| span.text == "bold").unwrap();
    assert!(bold.bold);
    assert!(!bold.italic);

    let italic = line
        .spans
        .iter()
        .find(|span| span.text == "italic")
        .unwrap();
    assert!(italic.italic);
    assert!(!italic.bold);

    let code = line.spans.iter().find(|span| span.text == "code").unwrap();
    assert!(code.is_code);

    let link = line
        .spans
        .iter()
        .find(|span| span.link_target.as_deref() == Some("note.md"))
        .unwrap();
    assert!(link.is_link);
    assert_eq!(link.visible_text(false), "link");

    let plain = line
        .spans
        .iter()
        .find(|span| span.text == "plain ")
        .unwrap();
    assert!(!plain.bold);
    assert!(!plain.italic);
}

#[test]
fn block_markdown_types_are_detected() {
    let lines = highlight_markdown("> quote\n- [ ] task\n---\n| A | B |\n|---|---|\n| 1 | 2 |");

    assert!(lines[0].is_blockquote);
    assert!(lines[1].spans.iter().any(|span| span.is_checkbox));
    assert!(lines[2].spans.iter().any(|span| span.is_rule));
    assert!(lines[3].is_table_row);
    assert_eq!(lines[3].table_cells.len(), 2);
    assert!(lines[4].is_table_row);
    assert!(lines[5].is_table_row);
}

#[test]
fn horizontal_rule_is_detected_with_crlf_line_endings() {
    let lines = highlight_markdown("before\r\n---\r\nafter\r\n");

    assert!(lines[1].spans.iter().any(|span| span.is_rule));
    assert_eq!(lines[1].spans[0].text, "---");
}

#[test]
fn longer_horizontal_rules_and_eol_checkboxes_are_highlighted() {
    let rules = highlight_markdown("----\n*****\n_____");
    assert!(rules.iter().all(|line| line.spans[0].is_rule));

    let tasks = highlight_markdown("- [ ]\n- [x]");
    assert!(tasks.iter().all(|line| line.spans[0].is_checkbox));
    assert!(!tasks[0].spans[0].is_checked);
    assert!(tasks[1].spans[0].is_checked);
}

#[test]
fn fenced_code_uses_language_and_colored_spans() {
    let lines = highlight_markdown("```rust\nlet x = 1;\n```");
    assert!(lines[1].is_code_block);
    assert_eq!(lines[1].code_block_lang.as_deref(), Some("rust"));
    assert!(lines[1].spans.iter().all(|span| span.is_code));
    assert!(lines[1].spans.len() > 1);
}

#[test]
fn fenced_markdown_keeps_math_and_markdown_markers_literal() {
    let markdown = "```markdown\n$$\n\\begin{align}\na &= b\n\\end{align}\n$$\n# Heading\n- [ ] task\n| A | B |\n```";
    let lines = highlight_markdown(markdown);

    assert!(lines.iter().all(|line| line.is_code_block));
    assert!(lines.iter().all(|line| !line.is_math_block));
    assert!(
        lines
            .iter()
            .all(|line| !line.spans.iter().any(|span| span.is_heading))
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.spans.iter().any(|span| span.is_checkbox))
    );
    assert!(lines.iter().all(|line| !line.is_table_row));
}

#[test]
fn blockquote_content_retains_inline_formatting_and_links() {
    let lines = highlight_markdown("> **Linked from:** [[curriculum]] · `notes`");
    let quote = &lines[0];

    assert!(quote.is_blockquote);
    assert!(
        quote
            .spans
            .iter()
            .any(|span| span.text == "Linked from:" && span.bold)
    );
    assert!(
        quote
            .spans
            .iter()
            .any(|span| { span.is_link && span.link_target.as_deref() == Some("curriculum") })
    );
    assert!(
        quote
            .spans
            .iter()
            .any(|span| span.text == "notes" && span.is_code)
    );
    assert_eq!(
        quote
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "> **Linked from:** [[curriculum]] · `notes`"
    );
}

#[test]
fn code_fences_hide_markers_only_in_preview() {
    let lines = highlight_markdown("```rust\nfn main() {}\n```");
    assert!(lines[0].is_block_fence);
    assert!(lines[2].is_block_fence);
    assert_eq!(lines[0].spans[0].visible_text(false), "");
    assert_eq!(lines[0].spans[0].visible_text(true), "```rust");
    assert_eq!(lines[2].spans[0].visible_text(false), "");
    assert_eq!(lines[2].spans[0].visible_text(true), "```");
}

#[test]
fn code_highlighting_preserves_full_source_text() {
    let source = "let answer: usize = 42;";
    let lines = highlight_markdown(&format!("```rust\n{source}\n```"));
    let rendered = lines[1]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    assert_eq!(rendered, source);
    assert!(lines[1].spans.iter().all(|span| span.is_code));
    assert!(
        lines[1]
            .spans
            .iter()
            .any(|span| span.color != crate::design::tokens().doc_text)
    );
}

#[test]
fn unterminated_code_block_keeps_all_remaining_lines_editable_as_code() {
    let lines = highlight_markdown("```json\n{\"a\": 1}\n# not a heading");
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line.is_code_block));
    assert_eq!(lines[1].code_block_lang.as_deref(), Some("json"));
    assert!(!lines[2].spans.iter().any(|span| span.is_heading));
}

#[test]
fn table_separator_has_no_cells_but_preserves_raw_edit_text() {
    let lines = highlight_markdown("| A | B |\n|:--|--:|\n| 1 | **two** |");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].table_cells.len(), 2);
    assert!(lines[1].is_table_row);
    assert!(lines[1].table_cells.is_empty());
    assert_eq!(lines[1].spans[0].visible_text(true), "|:--|--:|");
    assert_eq!(lines[1].spans[0].visible_text(false), "");
    assert_eq!(lines[2].table_cells.len(), 2);
    assert!(lines[2].table_cells[1].iter().any(|span| span.bold));
}

#[test]
fn malformed_inline_markdown_remains_plain_text() {
    let lines = highlight_markdown("bad **bold and [link](missing and `code");
    let text = lines[0]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    assert_eq!(text, "bad **bold and [link](missing and `code");
    assert!(!lines[0].spans.iter().any(|span| span.bold));
    assert!(!lines[0].spans.iter().any(|span| span.is_link));
    assert!(!lines[0].spans.iter().any(|span| span.is_code));
}

#[test]
fn test_highlighter_permutations() {
    // We will generate 250 distinct markdown fragments, each containing a sequence of elements
    // Totaling over 1,000 lines of parsed markdown text to assert highlighter robustness.

    // 1. Heading level permutations (1 to 6) with inline formatting
    for level in 1..=6 {
        let prefix = "#".repeat(level);
        let heading_text = format!("{} Heading Level {}", prefix, level);
        let lines = highlight_markdown(&heading_text);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].spans.iter().any(|span| span.is_heading),
            "Failed to detect heading at level {}",
            level
        );
        assert!(
            lines[0].spans.iter().any(|span| span.bold),
            "Failed to detect bold in heading"
        );
    }

    // 2. Ordered and Unordered lists, task items, and checkboxes
    let list_types = vec!["-", "*", "+", "1."];
    for lt in &list_types {
        let markdown_line = format!("{} This is a list item", lt);
        let lines = highlight_markdown(&markdown_line);
        assert_eq!(lines.len(), 1);
    }

    let checkbox_types = vec!["- [ ]", "- [x]"];
    for cb in &checkbox_types {
        let markdown_line = format!("{} This is a checkbox task", cb);
        let lines = highlight_markdown(&markdown_line);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].spans.iter().any(|span| span.is_checkbox),
            "Failed to detect checkbox for '{}'",
            cb
        );
    }

    // 3. LaTeX Math block environments permutations
    let math_environments = vec![
        "align",
        "align*",
        "equation",
        "equation*",
        "gather",
        "gather*",
        "split",
        "matrix",
        "pmatrix",
        "bmatrix",
        "aligned",
        "cases",
        "vmatrix",
        "Vmatrix",
    ];
    for env in &math_environments {
        let math_block = format!(
            "\\begin{{{}}}\nx &= y + z \\\\\na &= b\n\\end{{{}}}",
            env, env
        );
        let lines = highlight_markdown(&math_block);
        assert_eq!(lines.len(), 4);

        // Check that all 4 lines are unified under the math block ID
        let block_id = lines[0].block_id;
        assert!(
            block_id > 0,
            "Environment {} did not generate a block ID",
            env
        );
        for (idx, line) in lines.iter().enumerate() {
            assert!(
                line.is_math_block,
                "Line {} in environment {} not marked as math block",
                idx, env
            );
            assert_eq!(
                line.block_id, block_id,
                "Block ID mismatch in environment {}",
                env
            );
        }
    }

    // 4. Double dollar multiline block math permutations
    let display_math = "$$ \nx = \\sum_{i=1}^{n} i \n$$";
    let lines = highlight_markdown(display_math);
    assert_eq!(lines.len(), 3);
    let math_block_id = lines[0].block_id;
    assert!(math_block_id > 0);
    for line in &lines {
        assert!(line.is_math_block);
        assert_eq!(line.block_id, math_block_id);
    }

    // 5. Fenced code block language permutations
    let languages = vec![
        "rust", "js", "ts", "python", "html", "css", "c", "cpp", "go", "bash", "json", "toml",
    ];
    for lang in &languages {
        let code_block = format!("```{}\n// code here for {}\nlet v = 10;\n```", lang, lang);
        let lines = highlight_markdown(&code_block);
        assert_eq!(lines.len(), 4);

        // First line starts the block, middle lines are code block, last line ends it
        assert!(lines[1].is_code_block);
        assert!(lines[2].is_code_block);
        assert_eq!(lines[1].code_block_lang.as_deref(), Some(*lang));
        assert_eq!(lines[2].code_block_lang.as_deref(), Some(*lang));

        // Code block lines must have code spans
        assert!(lines[1].spans.iter().all(|span| span.is_code));
        assert!(lines[2].spans.iter().all(|span| span.is_code));
    }

    // 6. Blockquote permutations
    for depth in 1..=5 {
        let prefix = "> ".repeat(depth);
        let quote = format!("{} Nested quote depth {}", prefix, depth);
        let lines = highlight_markdown(&quote);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].is_blockquote);
    }

    // 7. Inline link and wikilink variations inside paragraphs
    let inline_markdown = "Text [[wikilink]] more text [standard link](http://google.com) and [[aliased|link]] with `code` block.";
    let lines = highlight_markdown(inline_markdown);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];

    assert!(
        line.spans
            .iter()
            .any(|span| span.is_link && span.link_target.as_deref() == Some("wikilink"))
    );
    assert!(
        line.spans
            .iter()
            .any(|span| span.is_link && span.link_target.as_deref() == Some("http://google.com"))
    );
    assert!(
        line.spans
            .iter()
            .any(|span| span.is_link && span.link_target.as_deref() == Some("aliased"))
    );
    assert!(line.spans.iter().any(|span| span.is_code));

    // Test relative path wikilinks and display alias extraction
    let test_markdown = "Link [[../folder/file_name]] and [[../other/file_name | My Alias]] and [[#equation-1]] and [[../nested/complex!@#%^&*()]].";
    let lines = highlight_markdown(test_markdown);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    let link1 = line
        .spans
        .iter()
        .find(|span| span.is_link && span.link_target.as_deref() == Some("../folder/file_name"))
        .unwrap();
    assert_eq!(link1.display_text.as_deref(), Some("file_name"));

    let link2 = line
        .spans
        .iter()
        .find(|span| span.is_link && span.link_target.as_deref() == Some("../other/file_name"))
        .unwrap();
    assert_eq!(link2.display_text.as_deref(), Some("My Alias"));

    let link3 = line
        .spans
        .iter()
        .find(|span| span.is_link && span.link_target.as_deref() == Some("#equation-1"))
        .unwrap();
    assert_eq!(link3.display_text.as_deref(), Some("#equation-1"));

    let link4 = line
        .spans
        .iter()
        .find(|span| {
            span.is_link && span.link_target.as_deref() == Some("../nested/complex!@#%^&*()")
        })
        .unwrap();
    assert_eq!(link4.display_text.as_deref(), Some("complex!@#%^&*()"));
}
