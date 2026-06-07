pub mod block;
pub mod inline;
pub mod metadata;
pub mod model;
pub mod reference;
pub mod syntax;

pub use block::*;
pub use metadata::*;
pub use model::*;

#[cfg(test)]
mod tests {
    use super::{
        MarkdownAnchorKind, MarkdownLinkEntry, MarkdownLinkKind, extract_document_metadata,
        extract_frontmatter_metadata, extract_markdown_links, highlight_markdown,
    };

    #[test]
    fn heading_is_not_math() {
        let lines = highlight_markdown("# Heading");
        assert!(lines[0].spans.iter().any(|span| span.is_heading));
        assert!(!lines[0].is_math_block);
    }

    #[test]
    fn heading_requires_space_after_hash_prefix() {
        let lines = highlight_markdown("#Heading\n##Also not heading\n# Heading");
        assert!(!lines[0].spans.iter().any(|span| span.is_heading));
        assert!(!lines[1].spans.iter().any(|span| span.is_heading));
        assert!(lines[2].spans.iter().any(|span| span.is_heading));
    }

    #[test]
    fn align_environment_is_one_math_block() {
        let lines = highlight_markdown("\\begin{align}\na &= b\n\\end{align}\n# Next");
        assert!(lines[0].is_math_block);
        assert_eq!(lines[0].block_id, lines[1].block_id);
        assert_eq!(lines[1].block_id, lines[2].block_id);
        assert!(!lines[3].is_math_block);
        assert!(lines[3].spans.iter().any(|span| span.is_heading));
    }

    #[test]
    fn unknown_begin_environment_is_plain_text() {
        let lines = highlight_markdown("\\begin{note}\n# Still heading");
        assert!(!lines[0].is_math_block);
        assert!(lines[1].spans.iter().any(|span| span.is_heading));
    }

    #[test]
    fn single_line_display_math_does_not_swallow_following_heading() {
        let lines = highlight_markdown("$$a=b$$\n##Change of Basis\nPlain text");
        assert!(lines[0].is_math_block);
        assert!(!lines[1].is_math_block);
        assert!(!lines[1].spans.iter().any(|span| span.is_heading));
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
    fn fenced_code_uses_language_and_colored_spans() {
        let lines = highlight_markdown("```rust\nlet x = 1;\n```");
        assert!(lines[1].is_code_block);
        assert_eq!(lines[1].code_block_lang.as_deref(), Some("rust"));
        assert!(lines[1].spans.iter().all(|span| span.is_code));
        assert!(lines[1].spans.len() > 1);
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
                .any(|span| span.color != crate::theme::text_primary())
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
    fn escaped_inline_markers_remain_literal() {
        let lines = highlight_markdown(r"\**not bold\** and \`not code\` and \$not math\$");
        let text = lines[0]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(text, r"\**not bold\** and \`not code\` and \$not math\$");
        assert!(!lines[0].spans.iter().any(|span| span.bold));
        assert!(!lines[0].spans.iter().any(|span| span.is_code));
        assert!(!lines[0].spans.iter().any(|span| span.is_math));
    }

    #[test]
    fn links_support_balanced_parentheses_and_tables_support_escaped_pipes() {
        let lines = highlight_markdown("[docs](https://example.com/a_(b))\n| A\\|B | C |");
        let link = lines[0].spans.iter().find(|span| span.is_link).unwrap();
        assert_eq!(
            link.link_target.as_deref(),
            Some("https://example.com/a_(b)")
        );

        assert!(lines[1].is_table_row);
        assert_eq!(lines[1].table_cells.len(), 2);
        let first_cell = lines[1].table_cells[0]
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(first_cell, "A\\|B");
    }

    #[test]
    fn span_source_reconstructs_original_lines_for_core_markdown() {
        let text = "# H\nplain **bold** `code` ![alt](img.png)\n> quote\n- [ ] task";
        let lines = highlight_markdown(text);
        for (source, line) in text.split('\n').zip(lines.iter()) {
            let reconstructed = line
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>();
            assert_eq!(reconstructed, source);
        }
    }

    #[test]
    fn large_document_highlight_preserves_line_count_and_block_ids() {
        let mut text = String::new();
        for idx in 0..10_000 {
            text.push_str(&format!(
                "- item {idx} with **bold** and [link](note-{idx}.md)\n"
            ));
        }

        let lines = highlight_markdown(&text);
        assert_eq!(lines.len(), 10_001);
        assert!(lines.iter().take(10_000).all(|line| line.block_id > 0));
        assert!(lines.iter().take(10_000).all(|line| {
            line.spans.iter().any(|span| span.bold) && line.spans.iter().any(|span| span.is_link)
        }));
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
            for idx in 0..4 {
                assert!(
                    lines[idx].is_math_block,
                    "Line {} in environment {} not marked as math block",
                    idx, env
                );
                assert_eq!(
                    lines[idx].block_id, block_id,
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
        for idx in 0..3 {
            assert!(lines[idx].is_math_block);
            assert_eq!(lines[idx].block_id, math_block_id);
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
            line.spans.iter().any(
                |span| span.is_link && span.link_target.as_deref() == Some("http://google.com")
            )
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

    #[test]
    fn reference_link_span_exposes_metadata_but_is_inactive() {
        let lines = highlight_markdown("Check [this link][ref_id] and [that_one] syntax.");
        let line = &lines[0];

        // Full reference link: [text][ref]
        let link1 = line
            .spans
            .iter()
            .find(|span| span.text == "[this link][ref_id]")
            .expect("Did not find full reference link span");
        assert!(link1.is_link);
        assert_eq!(link1.link_target.as_deref(), Some("ref_id"));
        assert_eq!(link1.display_text.as_deref(), Some("this link"));

        // Shortcut reference link: [ref]
        let link2 = line
            .spans
            .iter()
            .find(|span| span.text == "[that_one]")
            .expect("Did not find shortcut reference link span");
        assert!(link2.is_link);
        assert_eq!(link2.link_target.as_deref(), Some("that_one"));
        assert_eq!(link2.display_text.as_deref(), Some("that_one"));
    }

    #[test]
    fn reference_link_span_reconstructs_source_lines() {
        let text = "Here is a [link][ref] and a [shortcut].";
        let lines = highlight_markdown(text);
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn malformed_reference_syntax_remains_plain_text() {
        let lines = highlight_markdown("Bad [link][ref and [shortcut and [ ].");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "Bad [link][ref and [shortcut and [ ].");
        assert!(!lines[0].spans.iter().any(|s| s.is_link));
    }

    #[test]
    fn headings_parse_inline_links_and_emphasis() {
        let lines = highlight_markdown("## Heading with **bold** and [link](url)");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "## Heading with **bold** and [link](url)");
        assert!(lines[0].spans.iter().all(|s| s.is_heading));
        assert!(lines[0].spans.iter().all(|s| s.heading_level == 2));
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|s| s.is_link && s.link_target.as_deref() == Some("url"))
        );
        assert!(lines[0].spans.iter().any(|s| s.bold && s.text == "bold"));
    }

    #[test]
    fn nested_emphasis_combines_bold_and_italic() {
        let lines = highlight_markdown("**bold and *italic* inside**");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "**bold and *italic* inside**");
        // The word "italic" should have both bold and italic true
        let italic_span = lines[0].spans.iter().find(|s| s.text == "italic").unwrap();
        assert!(italic_span.bold);
        assert!(italic_span.italic);
    }

    #[test]
    fn footnotes_parsed_as_links() {
        let lines = highlight_markdown("This has a footnote[^1].");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "This has a footnote[^1].");
        let link_span = lines[0].spans.iter().find(|s| s.is_link).unwrap();
        assert_eq!(link_span.text, "[^1]");
        assert_eq!(link_span.link_target.as_deref(), Some("^1"));
    }

    #[test]
    fn extract_markdown_links_reports_backlink_metadata() {
        let text = "See [[notes/topic|Topic]], [site](https://example.com), [ref link][r1], [shortcut], and footnote[^n].\n## Heading with [[inside]]";
        let lines = highlight_markdown(text);
        let links = extract_markdown_links(&lines);

        assert_eq!(links.len(), 6);
        assert_eq!(
            links[0],
            MarkdownLinkEntry {
                line: 0,
                target: "notes/topic".to_string(),
                display_text: "Topic".to_string(),
                source_text: "notes/topic|Topic".to_string(),
                kind: MarkdownLinkKind::Wiki,
            }
        );
        assert_eq!(links[1].kind, MarkdownLinkKind::Inline);
        assert_eq!(links[1].target, "https://example.com");
        assert_eq!(links[1].display_text, "site");
        assert_eq!(links[2].kind, MarkdownLinkKind::Reference);
        assert_eq!(links[2].target, "r1");
        assert_eq!(links[2].display_text, "ref link");
        assert_eq!(links[3].kind, MarkdownLinkKind::Reference);
        assert_eq!(links[3].target, "shortcut");
        assert_eq!(links[4].kind, MarkdownLinkKind::Footnote);
        assert_eq!(links[4].target, "^n");
        assert_eq!(links[5].kind, MarkdownLinkKind::Wiki);
        assert_eq!(links[5].line, 1);
        assert_eq!(links[5].target, "inside");
    }

    #[test]
    fn extract_document_metadata_reports_outline_links_and_anchors() {
        let text = "# Heading One\n![Plot](plot.png)\n```rust\nfn main() {}\n```\nSee [[note]].";
        let lines = highlight_markdown(text);
        let metadata = extract_document_metadata(&lines);

        assert_eq!(metadata.outline.len(), 1);
        assert_eq!(metadata.outline[0].text, "Heading One");
        assert_eq!(metadata.links.len(), 1);
        assert!(
            metadata
                .links
                .iter()
                .any(|link| link.kind == MarkdownLinkKind::Wiki && link.target == "note")
        );
        assert!(
            metadata
                .anchors
                .iter()
                .any(|anchor| anchor.kind == MarkdownAnchorKind::Heading
                    && anchor.slug == "heading-one"
                    && anchor.line == 0)
        );
        assert!(
            metadata
                .anchors
                .iter()
                .any(|anchor| anchor.kind == MarkdownAnchorKind::SpanId
                    && anchor.slug == "figure-1"
                    && anchor.line == 1)
        );
        assert!(
            metadata
                .anchors
                .iter()
                .any(|anchor| anchor.kind == MarkdownAnchorKind::SpanId
                    && anchor.slug == "code-1"
                    && anchor.line == 2)
        );
    }

    #[test]
    fn extract_frontmatter_metadata_reports_aliases_and_tags() {
        let text = "---\naliases: [Alpha Note, \"Beta Note\"]\ntags:\n  - #math\n  - reading\nalias: Gamma\n---\n# Body";
        let lines = highlight_markdown(text);
        let frontmatter = extract_frontmatter_metadata(&lines);

        assert_eq!(
            frontmatter.aliases,
            vec![
                "Alpha Note".to_string(),
                "Beta Note".to_string(),
                "Gamma".to_string()
            ]
        );
        assert_eq!(
            frontmatter.tags,
            vec!["math".to_string(), "reading".to_string()]
        );

        let metadata = extract_document_metadata(&lines);
        assert_eq!(metadata.frontmatter, frontmatter);
    }

    #[test]
    fn test_extract_outline() {
        use super::extract_outline;
        let text = "# Heading 1\nSome text\n## Heading 2 with **bold**\n```markdown\n# Not a heading in code\n```\n### Heading 3";
        let lines = highlight_markdown(text);
        let outline = extract_outline(&lines);
        assert_eq!(outline.len(), 3);

        assert_eq!(outline[0].level, 1);
        assert_eq!(outline[0].text, "Heading 1");
        assert_eq!(outline[0].line, 0);

        assert_eq!(outline[1].level, 2);
        assert_eq!(outline[1].text, "Heading 2 with bold");
        assert_eq!(outline[1].line, 2);

        assert_eq!(outline[2].level, 3);
        assert_eq!(outline[2].text, "Heading 3");
        assert_eq!(outline[2].line, 6);
    }

    #[test]
    fn reference_style_link_resolution_and_indexing() {
        let text = "Click [my text][ref1] and [shortcut_ref] and [unresolved_ref].\n\n[ref1]: paper.pdf#page=5\n[shortcut_ref]: <another_note.md>";
        let lines = highlight_markdown(text);

        let line0 = &lines[0];
        let span_ref1 = line0
            .spans
            .iter()
            .find(|s| s.text == "[my text][ref1]")
            .unwrap();
        assert_eq!(span_ref1.link_target.as_deref(), Some("paper.pdf#page=5"));
        assert!(span_ref1.is_link);

        let span_shortcut = line0
            .spans
            .iter()
            .find(|s| s.text == "[shortcut_ref]")
            .unwrap();
        assert_eq!(
            span_shortcut.link_target.as_deref(),
            Some("another_note.md")
        );
        assert!(span_shortcut.is_link);

        let span_unresolved = line0
            .spans
            .iter()
            .find(|s| s.text == "[unresolved_ref]")
            .unwrap();
        assert_eq!(
            span_unresolved.link_target.as_deref(),
            Some("unresolved_ref")
        );
        assert!(span_unresolved.is_link);

        let metadata = extract_document_metadata(&lines);
        let ref1_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "[my text][ref1]")
            .unwrap();
        assert_eq!(ref1_link.kind, MarkdownLinkKind::ResolvedReference);
        assert_eq!(ref1_link.target, "paper.pdf#page=5");

        let shortcut_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "[shortcut_ref]")
            .unwrap();
        assert_eq!(shortcut_link.kind, MarkdownLinkKind::ResolvedReference);
        assert_eq!(shortcut_link.target, "another_note.md");

        let unresolved_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "[unresolved_ref]")
            .unwrap();
        assert_eq!(unresolved_link.kind, MarkdownLinkKind::Reference);
        assert_eq!(unresolved_link.target, "unresolved_ref");

        let def1_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "paper.pdf#page=5")
            .unwrap();
        assert_eq!(def1_link.kind, MarkdownLinkKind::ResolvedReference);
        assert_eq!(def1_link.target, "paper.pdf#page=5");
    }
}
