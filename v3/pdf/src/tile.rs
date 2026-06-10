//! Tile-based rendering model (plan §3.3): pages render as zoom-appropriate
//! tiles in an LRU byte-budget cache; tiles re-render on zoom-threshold
//! crossings so displayed content is never upscaled by more than 1.4×; the
//! render queue supports cancellation so offscreen requests are dropped, not
//! rendered.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

/// Zoom buckets are powers of 1.4 (the plan's upscale ceiling). A tile
/// rendered at its bucket's scale is displayed at ≤ 1.4× magnification.
const BUCKET_BASE: f32 = 1.4;
/// Bucket index for zoom = 1.0.
const BUCKET_ZERO: i8 = 0;

/// The bucket a zoom level falls into: smallest `n` with `1.4^n >= zoom`.
pub fn zoom_bucket(zoom: f32) -> i8 {
    let zoom = zoom.clamp(0.05, 64.0);
    let mut n = BUCKET_ZERO;
    let mut scale = 1.0_f32;
    if zoom > 1.0 {
        while scale < zoom && n < 12 {
            scale *= BUCKET_BASE;
            n += 1;
        }
    } else {
        while scale / BUCKET_BASE >= zoom && n > -12 {
            scale /= BUCKET_BASE;
            n -= 1;
        }
    }
    n
}

/// The render scale for a bucket (`1.4^bucket`).
pub fn zoom_bucket_scale(bucket: i8) -> f32 {
    BUCKET_BASE.powi(bucket as i32)
}

/// Address of one rendered tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub page: u32,
    pub bucket: i8,
    pub col: u16,
    pub row: u16,
}

#[derive(Debug)]
struct CacheEntry {
    bytes: usize,
    /// Monotonic recency stamp; larger = more recent.
    stamp: u64,
}

/// LRU cache bounded by total bytes, not entry count — tiles at high zoom are
/// much larger than at low zoom.
#[derive(Debug)]
pub struct TileCache {
    budget: usize,
    used: usize,
    clock: u64,
    entries: HashMap<TileKey, CacheEntry>,
}

impl TileCache {
    pub fn new(budget_bytes: usize) -> TileCache {
        TileCache {
            budget: budget_bytes,
            used: 0,
            clock: 0,
            entries: HashMap::new(),
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a rendered tile, evicting least-recently-used tiles until the
    /// budget holds. Returns the evicted keys (so the shell can drop pixmaps).
    pub fn insert(&mut self, key: TileKey, bytes: usize) -> Vec<TileKey> {
        self.clock += 1;
        if let Some(old) = self.entries.insert(
            key,
            CacheEntry {
                bytes,
                stamp: self.clock,
            },
        ) {
            self.used -= old.bytes;
        }
        self.used += bytes;
        let mut evicted = Vec::new();
        while self.used > self.budget {
            let lru = self
                .entries
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, e)| e.stamp)
                .map(|(k, _)| *k);
            match lru {
                Some(victim) => {
                    if let Some(e) = self.entries.remove(&victim) {
                        self.used -= e.bytes;
                    }
                    evicted.push(victim);
                }
                // Only the just-inserted tile remains and it alone exceeds
                // the budget: keep it (something must be displayable).
                None => break,
            }
        }
        evicted
    }

    /// Cache hit check; bumps recency on hit.
    pub fn touch(&mut self, key: TileKey) -> bool {
        self.clock += 1;
        match self.entries.get_mut(&key) {
            Some(e) => {
                e.stamp = self.clock;
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, key: TileKey) -> bool {
        self.entries.contains_key(&key)
    }
}

/// FIFO render queue with cancellation: scheduling is cheap, and a viewport
/// change drops every request that is no longer visible (plan §3.3 "offscreen
/// requests dropped").
#[derive(Debug, Default)]
pub struct RenderQueue {
    pending: VecDeque<TileKey>,
    queued: HashSet<TileKey>,
}

impl RenderQueue {
    pub fn new() -> RenderQueue {
        RenderQueue::default()
    }

    /// Enqueue a tile for rendering (idempotent while pending).
    pub fn schedule(&mut self, key: TileKey) {
        if self.queued.insert(key) {
            self.pending.push_back(key);
        }
    }

    /// Cancel everything not in the visible set. Returns how many requests
    /// were dropped without being rendered.
    pub fn retain_visible(&mut self, visible: &HashSet<TileKey>) -> usize {
        let before = self.pending.len();
        self.pending.retain(|k| visible.contains(k));
        self.queued.retain(|k| visible.contains(k));
        before - self.pending.len()
    }

    /// Next tile to render, in schedule order.
    pub fn pop(&mut self) -> Option<TileKey> {
        let key = self.pending.pop_front()?;
        self.queued.remove(&key);
        Some(key)
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(page: u32, col: u16) -> TileKey {
        TileKey {
            page,
            bucket: 0,
            col,
            row: 0,
        }
    }

    #[test]
    fn displayed_content_is_never_upscaled_beyond_the_ceiling() {
        // For any zoom, the bucket's render scale is >= zoom / 1.4 — i.e.
        // magnification of a rendered tile never exceeds 1.4×.
        let mut z = 0.05_f32;
        while z < 16.0 {
            let scale = zoom_bucket_scale(zoom_bucket(z));
            let magnification = z / scale;
            assert!(
                magnification <= BUCKET_BASE * 1.001,
                "zoom {z}: bucket scale {scale} ⇒ upscale {magnification}"
            );
            z *= 1.07;
        }
        assert_eq!(zoom_bucket(1.0), 0);
        assert_eq!(zoom_bucket_scale(0), 1.0);
    }

    #[test]
    fn zoom_within_a_bucket_does_not_invalidate_tiles() {
        // Small zoom wiggles stay in the same bucket → cache stays warm.
        assert_eq!(zoom_bucket(1.05), zoom_bucket(1.3));
        // Crossing the threshold re-renders.
        assert_ne!(zoom_bucket(1.3), zoom_bucket(1.5));
    }

    #[test]
    fn cache_evicts_lru_to_hold_byte_budget() {
        let mut cache = TileCache::new(100);
        cache.insert(key(0, 0), 40);
        cache.insert(key(0, 1), 40);
        assert!(cache.touch(key(0, 0)), "bump page-0 tile so col-1 is LRU");
        let evicted = cache.insert(key(1, 0), 40);
        assert_eq!(evicted, vec![key(0, 1)]);
        assert!(cache.contains(key(0, 0)));
        assert!(cache.used_bytes() <= 100);
    }

    #[test]
    fn oversized_single_tile_is_kept_rather_than_thrashed() {
        let mut cache = TileCache::new(10);
        let evicted = cache.insert(key(0, 0), 50);
        assert!(evicted.is_empty());
        assert!(cache.contains(key(0, 0)));
    }

    #[test]
    fn reinserting_a_tile_replaces_its_size_accounting() {
        let mut cache = TileCache::new(100);
        cache.insert(key(0, 0), 60);
        cache.insert(key(0, 0), 30);
        assert_eq!(cache.used_bytes(), 30);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn queue_drops_offscreen_requests() {
        let mut q = RenderQueue::new();
        for c in 0..6 {
            q.schedule(key(0, c));
        }
        q.schedule(key(0, 3)); // duplicate while pending: ignored
        assert_eq!(q.len(), 6);

        let visible: HashSet<TileKey> = [key(0, 4), key(0, 5)].into_iter().collect();
        let dropped = q.retain_visible(&visible);
        assert_eq!(dropped, 4);
        assert_eq!(q.pop(), Some(key(0, 4)));
        assert_eq!(q.pop(), Some(key(0, 5)));
        assert_eq!(q.pop(), None);

        // After draining, a re-schedule works (queued set was cleaned up).
        q.schedule(key(0, 4));
        assert_eq!(q.len(), 1);
    }
}
