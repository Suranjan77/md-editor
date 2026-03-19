use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::piece_table::PieceTable;

/// Wraps Tree-sitter for incremental markdown parsing.
pub struct MarkdownParser {
    parser: Parser,
    tree: Option<Tree>,
}

impl MarkdownParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        let language = tree_sitter_md::LANGUAGE.into();
        parser
            .set_language(&language)
            .expect("Failed to set tree-sitter-markdown language");
        MarkdownParser { parser, tree: None }
    }

    /// Full parse of the document — used on file open.
    pub fn parse_full(&mut self, text: &str) -> Option<Tree> {
        let tree = self.parser.parse(text, None)?;
        self.tree = Some(tree.clone());
        Some(tree)
    }

    /// Incremental re-parse after an edit.
    /// Applies the InputEdit to the existing tree, then re-parses only the changed region.
    pub fn parse_incremental(
        &mut self,
        piece_table: &PieceTable,
        byte_offset: usize,
        old_end_byte: usize,
        new_end_byte: usize,
    ) -> Option<Tree> {
        let text = piece_table.to_string();

        if let Some(ref mut old_tree) = self.tree {
            let start_point = piece_table.byte_offset_to_point(byte_offset);
            let old_end_point = piece_table.byte_offset_to_point(old_end_byte.min(text.len()));
            let new_end_point = piece_table.byte_offset_to_point(new_end_byte.min(text.len()));

            let input_edit = InputEdit {
                start_byte: byte_offset,
                old_end_byte,
                new_end_byte,
                start_position: Point {
                    row: start_point.0,
                    column: start_point.1,
                },
                old_end_position: Point {
                    row: old_end_point.0,
                    column: old_end_point.1,
                },
                new_end_position: Point {
                    row: new_end_point.0,
                    column: new_end_point.1,
                },
            };

            old_tree.edit(&input_edit);

            let new_tree = self.parser.parse(&text, Some(old_tree))?;
            self.tree = Some(new_tree.clone());
            Some(new_tree)
        } else {
            self.parse_full(&text)
        }
    }

    /// Get a reference to the current tree.
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full() {
        let mut parser = MarkdownParser::new();
        let tree = parser.parse_full("# Hello\n\nSome text.");
        assert!(tree.is_some());
        let tree = tree.unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), "document");
    }

    #[test]
    fn test_parse_incremental() {
        let mut parser = MarkdownParser::new();
        let mut pt = PieceTable::new("# Hello\n\nSome text.");

        parser.parse_full(&pt.to_string());

        // Insert " World" after "Hello"
        pt.insert(7, " World");
        let tree = parser.parse_incremental(&pt, 7, 7, 13);
        assert!(tree.is_some());
    }
}
