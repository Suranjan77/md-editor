//! Bounded LRU cache for rendered PDF page images.
//!
//! Previously the app stored every rendered page as
//! `pdf_pages: Vec<Option<Handle>>` and never evicted entries, causing memory
//! to grow without bound on long reading sessions of large PDFs.
//!
//! [`PdfPageCache`] keeps recently rendered pages in a `HashMap` keyed by
//! page index, with an LRU `VecDeque` recording recency. Eviction triggers on
//! either:
//!   * exceeding `max_pages` distinct page entries, or
//!   * exceeding `max_bytes` of cumulative RGBA pixel data.
//!
//! The visible range is honoured when picking eviction victims: pages within
//! the protected range stay alive even if they are old, so re-rendering is not
//! triggered every frame for the very pages the user is reading.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

use iced::widget::image::Handle;

#[derive(Debug, Clone)]
pub struct CachedPage {
    pub handle: Handle,
    pub dimensions: (u32, u32),
    pub byte_size: usize,
}

/// Default cap on the number of cached page images.
pub const DEFAULT_MAX_PAGES: usize = 30;
/// Default cap on cumulative RGBA bytes (~512 MiB).
pub const DEFAULT_MAX_BYTES: usize = 512 * 1024 * 1024;
/// How far on each side of the visible range we refuse to evict.
pub const VISIBLE_GUARD_PAGES: u16 = 3;

#[derive(Debug, Clone)]
pub struct PdfPageCache {
    pages: HashMap<u16, CachedPage>,
    /// Most-recently-used pages live near the back of the deque.
    lru_order: VecDeque<u16>,
    max_pages: usize,
    max_bytes: usize,
    total_bytes: usize,
    /// Currently visible page range (inclusive). Eviction will avoid removing
    /// any page within this range expanded by [`VISIBLE_GUARD_PAGES`].
    visible_range: Option<(u16, u16)>,
}

impl Default for PdfPageCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PAGES, DEFAULT_MAX_BYTES)
    }
}

impl PdfPageCache {
    pub fn new(max_pages: usize, max_bytes: usize) -> Self {
        Self {
            pages: HashMap::new(),
            lru_order: VecDeque::new(),
            max_pages: max_pages.max(1),
            max_bytes: max_bytes.max(1),
            total_bytes: 0,
            visible_range: None,
        }
    }

    /// Drop every cached page.
    pub fn clear(&mut self) {
        self.pages.clear();
        self.lru_order.clear();
        self.total_bytes = 0;
        self.visible_range = None;
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Set the visible page range so eviction can protect those pages.
    /// Range is inclusive on both ends.
    pub fn set_visible_range(&mut self, range: Option<(u16, u16)>) {
        self.visible_range = range;
    }

    /// Look up a cached page handle without changing recency. Used by the view
    /// builder where touching every visible page on each frame would defeat
    /// the LRU policy.
    pub fn get_handle(&self, page: u16) -> Option<&Handle> {
        self.pages.get(&page).map(|c| &c.handle)
    }

    /// Look up the rendered logical dimensions of a cached page.
    pub fn get_dimensions(&self, page: u16) -> Option<(u32, u32)> {
        self.pages.get(&page).map(|c| c.dimensions)
    }

    pub fn contains(&self, page: u16) -> bool {
        self.pages.contains_key(&page)
    }

    pub fn keys(&self) -> impl Iterator<Item = &u16> {
        self.pages.keys()
    }

    /// Insert a freshly rendered page, evicting older entries if necessary.
    pub fn insert(&mut self, page: u16, handle: Handle, dimensions: (u32, u32), byte_size: usize) {
        if let Some(prev) = self.pages.remove(&page) {
            self.total_bytes = self.total_bytes.saturating_sub(prev.byte_size);
            self.lru_order.retain(|&p| p != page);
        }

        self.pages.insert(
            page,
            CachedPage {
                handle,
                dimensions,
                byte_size,
            },
        );
        self.total_bytes = self.total_bytes.saturating_add(byte_size);
        self.lru_order.push_back(page);

        self.evict_if_needed();
    }

    /// Mark a page as recently used. Caller is responsible for being judicious
    /// here (touching the same page many times per frame is wasteful but safe).
    pub fn touch(&mut self, page: u16) {
        if !self.pages.contains_key(&page) {
            return;
        }
        if let Some(pos) = self.lru_order.iter().position(|&p| p == page) {
            self.lru_order.remove(pos);
        }
        self.lru_order.push_back(page);
    }

    /// Touch every page in the visible range so they are treated as MRU.
    pub fn touch_visible(&mut self) {
        let Some((start, end)) = self.visible_range else {
            return;
        };
        for page in start..=end {
            self.touch(page);
        }
    }

    fn is_protected(&self, page: u16) -> bool {
        let Some((start, end)) = self.visible_range else {
            return false;
        };
        let lo = start.saturating_sub(VISIBLE_GUARD_PAGES);
        let hi = end.saturating_add(VISIBLE_GUARD_PAGES);
        page >= lo && page <= hi
    }

    fn evict_if_needed(&mut self) {
        // Phase 1: respect protection. Evict the oldest unprotected page until
        // we are within both the page and byte budgets.
        while self.over_budget() {
            let mut victim_pos: Option<usize> = None;
            for (idx, &page) in self.lru_order.iter().enumerate() {
                if !self.is_protected(page) {
                    victim_pos = Some(idx);
                    break;
                }
            }
            match victim_pos {
                Some(pos) => {
                    if let Some(page) = self.lru_order.remove(pos) {
                        if let Some(entry) = self.pages.remove(&page) {
                            self.total_bytes = self.total_bytes.saturating_sub(entry.byte_size);
                        }
                    }
                }
                None => break,
            }
        }

        // Phase 2: if the cache is still over the hard budget, the visible
        // window itself is too large. Evict from the LRU front regardless of
        // protection so the process does not OOM.
        while self.over_hard_budget() {
            let Some(page) = self.lru_order.pop_front() else {
                break;
            };
            if let Some(entry) = self.pages.remove(&page) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.byte_size);
            }
        }
    }

    fn over_budget(&self) -> bool {
        self.pages.len() > self.max_pages || self.total_bytes > self.max_bytes
    }

    /// Hard budget is double the byte limit; anything past this we will evict
    /// even visible pages. Page-count overage is handled by Phase 1 since the
    /// page-count budget alone never causes OOM.
    fn over_hard_budget(&self) -> bool {
        self.total_bytes > self.max_bytes.saturating_mul(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handle() -> Handle {
        // 1x1 transparent RGBA pixel; cheap to construct in tests.
        Handle::from_rgba(1, 1, vec![0u8, 0, 0, 0])
    }

    fn insert_n(cache: &mut PdfPageCache, n: u16, page_bytes: usize) {
        for page in 0..n {
            cache.insert(page, make_handle(), (1, 1), page_bytes);
        }
    }

    #[test]
    fn empty_cache_returns_none() {
        let cache = PdfPageCache::default();
        assert!(cache.is_empty());
        assert!(cache.get_handle(0).is_none());
        assert!(cache.get_dimensions(0).is_none());
    }

    #[test]
    fn insert_and_lookup_round_trip() {
        let mut cache = PdfPageCache::new(8, 1024);
        cache.insert(3, make_handle(), (640, 480), 64);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(3));
        assert_eq!(cache.get_dimensions(3), Some((640, 480)));
        assert_eq!(cache.total_bytes(), 64);
    }

    #[test]
    fn re_insert_replaces_and_does_not_double_count_bytes() {
        let mut cache = PdfPageCache::new(8, 1024);
        cache.insert(0, make_handle(), (1, 1), 100);
        cache.insert(0, make_handle(), (2, 2), 50);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_bytes(), 50);
        assert_eq!(cache.get_dimensions(0), Some((2, 2)));
    }

    #[test]
    fn evicts_lru_when_page_count_exceeded() {
        let mut cache = PdfPageCache::new(3, usize::MAX);
        insert_n(&mut cache, 5, 1);
        // Pages 0 and 1 should be evicted; pages 2..=4 retained.
        assert_eq!(cache.len(), 3);
        assert!(!cache.contains(0));
        assert!(!cache.contains(1));
        assert!(cache.contains(2));
        assert!(cache.contains(3));
        assert!(cache.contains(4));
    }

    #[test]
    fn evicts_when_bytes_exceeded() {
        let mut cache = PdfPageCache::new(usize::MAX, 100);
        cache.insert(0, make_handle(), (1, 1), 60);
        cache.insert(1, make_handle(), (1, 1), 60);
        // Inserting page 1 pushed total to 120 > 100, so page 0 was evicted.
        assert!(!cache.contains(0));
        assert!(cache.contains(1));
        assert_eq!(cache.total_bytes(), 60);
    }

    #[test]
    fn visible_range_protects_pages_from_eviction() {
        let mut cache = PdfPageCache::new(3, usize::MAX);
        cache.insert(20, make_handle(), (1, 1), 1);
        cache.insert(21, make_handle(), (1, 1), 1);
        cache.insert(10, make_handle(), (1, 1), 1); // 10 is the newest

        // Protect page 10 (and its guard pages, which don't include 20 or 21)
        cache.set_visible_range(Some((10, 10)));

        // Touch 20 to make it MRU. LRU order: 21, 10, 20
        cache.touch(20);

        // Insert 30. Normally 21 would be evicted since it's the oldest.
        cache.insert(30, make_handle(), (1, 1), 1);

        assert!(cache.contains(10), "protected page 10 must not be evicted");
        assert!(
            !cache.contains(21),
            "oldest unprotected page 21 should be evicted"
        );
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn visible_guard_extends_protection() {
        let mut cache = PdfPageCache::new(3, usize::MAX);
        insert_n(&mut cache, 3, 1);
        // Visible range is [10, 10]; with guard 3 the protected window is
        // [7, 13], so pages 0..=2 are NOT protected and may be evicted freely.
        cache.set_visible_range(Some((10, 10)));
        cache.insert(20, make_handle(), (1, 1), 1);
        assert!(!cache.contains(0));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn clear_drops_everything_and_zeros_total_bytes() {
        let mut cache = PdfPageCache::new(8, 1024);
        insert_n(&mut cache, 5, 10);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.total_bytes(), 0);
        assert!(cache.get_handle(0).is_none());
    }

    #[test]
    fn hard_budget_evicts_visible_pages_to_avoid_oom() {
        // max_bytes = 100, hard budget = 200. Visible range covers everything,
        // but a single 250-byte page would still need to evict.
        let mut cache = PdfPageCache::new(usize::MAX, 100);
        cache.set_visible_range(Some((0, 5)));
        for page in 0..6 {
            cache.insert(page, make_handle(), (1, 1), 60);
        }
        // Total would be 360; hard cap is 200. Phase 2 must have evicted
        // visible pages from the LRU front.
        assert!(
            cache.total_bytes() <= 200,
            "hard cap exceeded: total_bytes = {}",
            cache.total_bytes()
        );
    }

    #[test]
    fn touch_promotes_to_mru() {
        let mut cache = PdfPageCache::new(2, usize::MAX);
        cache.insert(0, make_handle(), (1, 1), 1);
        cache.insert(1, make_handle(), (1, 1), 1);
        // Page 0 is currently LRU. Touch it so it becomes MRU, then insert
        // page 2 — page 1 should be evicted instead of page 0.
        cache.touch(0);
        cache.insert(2, make_handle(), (1, 1), 1);
        assert!(cache.contains(0));
        assert!(!cache.contains(1));
        assert!(cache.contains(2));
    }
}
