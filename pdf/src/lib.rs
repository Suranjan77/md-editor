//! PDF engine for MD Editor: tile addressing, the byte-budget
//! LRU tile cache, and the cancellable render queue are pure logic, testable
//! without rendering anything. The impure half — pdfium wiring (ported from
//! v2 core, ADR-0002 re-affirmed) — lives in [`render`] behind the `pdfium`
//! cargo feature.

pub mod outline;
pub mod scroll;
pub mod select;
pub mod tile;

#[cfg(feature = "pdfium")]
pub mod render;

pub use outline::{OutlineEntry, section_at};
pub use scroll::{DocLayout, PlacedPage, PlacedTile};
pub use select::{CharBox, LinkBox, SelRect, TextSelection};
pub use tile::{RenderQueue, TILE_PX, TileCache, TileKey, zoom_bucket, zoom_bucket_scale};

#[cfg(feature = "pdfium")]
pub use render::{PdfError, PdfRenderer, RenderedPage, RenderedTile};
