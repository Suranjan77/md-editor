//! PDF engine logic for md-editor v3 (plan §3.3), UI- and pdfium-free: tile
//! addressing, the byte-budget LRU tile cache, and the cancellable render
//! queue are pure logic, testable without rendering anything. pdfium wiring
//! (ported from v2 core, ADR-0002 re-affirmed) lands in a later session.

pub mod tile;

pub use tile::{RenderQueue, TileCache, TileKey, zoom_bucket, zoom_bucket_scale};
