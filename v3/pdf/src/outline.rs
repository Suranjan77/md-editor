//! Document outline (plan §3.3 "TOC with section tracking"): the pure half.
//! [`crate::render`] flattens pdfium's bookmark tree into depth-tagged
//! entries in reading order; everything a TOC view or a "current section"
//! pill needs from them is plain math over that list, testable without a
//! document.

/// One outline (bookmark) entry, prefix order, `depth` 0 at the root level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    pub title: String,
    /// 0-based destination page.
    pub page: u32,
    pub depth: u8,
}

/// The section `page` is inside: the index of the *last* entry that starts
/// on or before it. `None` before the first section (or with no outline).
/// Entries are in document order, but page numbers need not be monotonic
/// (a malformed tree won't panic, it just answers by the order given).
pub fn section_at(entries: &[OutlineEntry], page: u32) -> Option<usize> {
    let mut current = None;
    for (i, e) in entries.iter().enumerate() {
        if e.page <= page {
            current = Some(i);
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, page: u32, depth: u8) -> OutlineEntry {
        OutlineEntry {
            title: title.to_string(),
            page,
            depth,
        }
    }

    #[test]
    fn section_is_the_last_entry_at_or_before_the_page() {
        let toc = [
            entry("Ch 1", 0, 0),
            entry("Ch 1.1", 1, 1),
            entry("Ch 2", 3, 0),
        ];
        assert_eq!(section_at(&toc, 0), Some(0));
        assert_eq!(section_at(&toc, 1), Some(1));
        assert_eq!(section_at(&toc, 2), Some(1), "between entries: previous");
        assert_eq!(section_at(&toc, 3), Some(2));
        assert_eq!(section_at(&toc, 99), Some(2), "past the end: last");
    }

    #[test]
    fn pages_before_the_first_section_have_none() {
        let toc = [entry("Ch 1", 2, 0)];
        assert_eq!(section_at(&toc, 0), None);
        assert_eq!(section_at(&toc, 1), None);
        assert_eq!(section_at(&[], 5), None, "no outline at all");
    }

    #[test]
    fn non_monotonic_page_order_still_answers_by_document_order() {
        // Malformed but seen in the wild: an entry pointing backwards.
        let toc = [entry("A", 5, 0), entry("B", 2, 0)];
        assert_eq!(section_at(&toc, 3), Some(1), "B is the last that started");
        assert_eq!(section_at(&toc, 6), Some(1));
    }
}
