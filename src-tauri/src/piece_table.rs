/// Which buffer a descriptor refers to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BufferKind {
    Original,
    Add,
}

/// A single entry in the piece table — points into either the original or add buffer.
#[derive(Debug, Clone)]
pub struct Descriptor {
    pub buffer: BufferKind,
    pub start: usize,
    pub length: usize,
}

/// Piece table: the single source of truth for document text.
///
/// - `original` buffer is immutable (the text loaded from disk).
/// - `add` buffer is append-only (all inserted text goes here).
/// - `pieces` is a list of descriptors that, when concatenated, yield the logical document.
#[derive(Debug)]
pub struct PieceTable {
    original: String,
    add: String,
    pieces: Vec<Descriptor>,
}

impl PieceTable {
    /// Create a new piece table from an initial document string.
    pub fn new(text: &str) -> Self {
        let pieces = if text.is_empty() {
            vec![]
        } else {
            vec![Descriptor {
                buffer: BufferKind::Original,
                start: 0,
                length: text.len(),
            }]
        };
        PieceTable {
            original: text.to_string(),
            add: String::new(),
            pieces,
        }
    }

    /// Total byte length of the logical text.
    pub fn len(&self) -> usize {
        self.pieces.iter().map(|d| d.length).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the buffer content for a descriptor.
    fn buffer_content(&self, kind: BufferKind) -> &str {
        match kind {
            BufferKind::Original => &self.original,
            BufferKind::Add => &self.add,
        }
    }

    /// Insert text at a logical byte offset.
    pub fn insert(&mut self, byte_offset: usize, text: &str) {
        if text.is_empty() {
            return;
        }

        let add_start = self.add.len();
        self.add.push_str(text);

        let new_piece = Descriptor {
            buffer: BufferKind::Add,
            start: add_start,
            length: text.len(),
        };

        // Find which piece the offset falls into
        let mut logical_pos = 0;
        let mut piece_idx = 0;

        while piece_idx < self.pieces.len() {
            let piece = &self.pieces[piece_idx];
            if logical_pos + piece.length > byte_offset {
                break;
            }
            logical_pos += piece.length;
            piece_idx += 1;
        }

        if piece_idx >= self.pieces.len() {
            // Insert at the end
            self.pieces.push(new_piece);
        } else if logical_pos == byte_offset {
            // Insert before this piece
            self.pieces.insert(piece_idx, new_piece);
        } else {
            // Split the current piece
            let piece = self.pieces[piece_idx].clone();
            let offset_in_piece = byte_offset - logical_pos;

            let left = Descriptor {
                buffer: piece.buffer,
                start: piece.start,
                length: offset_in_piece,
            };
            let right = Descriptor {
                buffer: piece.buffer,
                start: piece.start + offset_in_piece,
                length: piece.length - offset_in_piece,
            };

            self.pieces
                .splice(piece_idx..=piece_idx, [left, new_piece, right]);
        }
    }

    /// Delete `length` bytes starting at `byte_offset`.
    pub fn delete(&mut self, byte_offset: usize, length: usize) {
        if length == 0 {
            return;
        }

        let mut remaining = length;
        let delete_start = byte_offset;

        while remaining > 0 {
            let mut logical_pos = 0;
            let mut piece_idx = 0;

            // Find the piece that contains delete_start
            while piece_idx < self.pieces.len() {
                let piece = &self.pieces[piece_idx];
                if logical_pos + piece.length > delete_start {
                    break;
                }
                logical_pos += piece.length;
                piece_idx += 1;
            }

            if piece_idx >= self.pieces.len() {
                break;
            }

            let piece = self.pieces[piece_idx].clone();
            let offset_in_piece = delete_start - logical_pos;
            let can_delete = (piece.length - offset_in_piece).min(remaining);

            if offset_in_piece == 0 && can_delete == piece.length {
                // Remove the entire piece
                self.pieces.remove(piece_idx);
            } else if offset_in_piece == 0 {
                // Trim from the start
                self.pieces[piece_idx] = Descriptor {
                    buffer: piece.buffer,
                    start: piece.start + can_delete,
                    length: piece.length - can_delete,
                };
            } else if offset_in_piece + can_delete == piece.length {
                // Trim from the end
                self.pieces[piece_idx] = Descriptor {
                    buffer: piece.buffer,
                    start: piece.start,
                    length: offset_in_piece,
                };
            } else {
                // Split: delete from the middle
                let left = Descriptor {
                    buffer: piece.buffer,
                    start: piece.start,
                    length: offset_in_piece,
                };
                let right = Descriptor {
                    buffer: piece.buffer,
                    start: piece.start + offset_in_piece + can_delete,
                    length: piece.length - offset_in_piece - can_delete,
                };
                self.pieces.splice(piece_idx..=piece_idx, [left, right]);
            }

            remaining -= can_delete;
        }
    }

    /// Read a span of the logical text.
    pub fn slice(&self, byte_offset: usize, length: usize) -> String {
        let mut result = String::with_capacity(length);
        let mut logical_pos = 0;
        let mut remaining = length;
        let mut start = byte_offset;

        for piece in &self.pieces {
            if remaining == 0 {
                break;
            }

            let piece_end = logical_pos + piece.length;

            if piece_end <= start {
                logical_pos = piece_end;
                continue;
            }

            let offset_in_piece = if start > logical_pos {
                start - logical_pos
            } else {
                0
            };

            let can_read = (piece.length - offset_in_piece).min(remaining);
            let buf = self.buffer_content(piece.buffer);
            let buf_start = piece.start + offset_in_piece;
            result.push_str(&buf[buf_start..buf_start + can_read]);

            remaining -= can_read;
            start = piece_end;
            logical_pos = piece_end;
        }

        result
    }

    /// Materialise the full logical text.
    pub fn to_string(&self) -> String {
        let mut result = String::with_capacity(self.len());
        for piece in &self.pieces {
            let buf = self.buffer_content(piece.buffer);
            result.push_str(&buf[piece.start..piece.start + piece.length]);
        }
        result
    }

    /// Compute line/column (Point) for a byte offset — needed for Tree-sitter InputEdit.
    pub fn byte_offset_to_point(&self, byte_offset: usize) -> (usize, usize) {
        let text = self.slice(0, byte_offset);
        let mut row = 0;
        let mut col = 0;
        for ch in text.chars() {
            if ch == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (row, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_empty() {
        let pt = PieceTable::new("");
        assert_eq!(pt.len(), 0);
        assert_eq!(pt.to_string(), "");
    }

    #[test]
    fn test_new_with_text() {
        let pt = PieceTable::new("hello world");
        assert_eq!(pt.len(), 11);
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn test_insert_at_beginning() {
        let mut pt = PieceTable::new("world");
        pt.insert(0, "hello ");
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn test_insert_at_end() {
        let mut pt = PieceTable::new("hello");
        pt.insert(5, " world");
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn test_insert_in_middle() {
        let mut pt = PieceTable::new("helo world");
        pt.insert(3, "l");
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn test_delete_from_beginning() {
        let mut pt = PieceTable::new("hello world");
        pt.delete(0, 6);
        assert_eq!(pt.to_string(), "world");
    }

    #[test]
    fn test_delete_from_end() {
        let mut pt = PieceTable::new("hello world");
        pt.delete(5, 6);
        assert_eq!(pt.to_string(), "hello");
    }

    #[test]
    fn test_delete_from_middle() {
        let mut pt = PieceTable::new("hello beautiful world");
        pt.delete(5, 10);
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn test_slice() {
        let pt = PieceTable::new("hello world");
        assert_eq!(pt.slice(0, 5), "hello");
        assert_eq!(pt.slice(6, 5), "world");
        assert_eq!(pt.slice(3, 5), "lo wo");
    }

    #[test]
    fn test_multiple_operations() {
        let mut pt = PieceTable::new("hello");
        pt.insert(5, " world");
        pt.insert(5, " beautiful");
        assert_eq!(pt.to_string(), "hello beautiful world");
        pt.delete(5, 10);
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn test_byte_offset_to_point() {
        let pt = PieceTable::new("line1\nline2\nline3");
        assert_eq!(pt.byte_offset_to_point(0), (0, 0));
        assert_eq!(pt.byte_offset_to_point(5), (0, 5));
        assert_eq!(pt.byte_offset_to_point(6), (1, 0));
        assert_eq!(pt.byte_offset_to_point(11), (1, 5));
        assert_eq!(pt.byte_offset_to_point(12), (2, 0));
    }
}
