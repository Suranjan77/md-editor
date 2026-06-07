pub mod annotation;
pub mod geometry;
pub mod link;
pub mod text;

pub use annotation::*;
pub use geometry::*;
pub use link::*;
pub use text::*;

pub struct PdfState {
    pub current_page: u16,
    pub total_pages: u16,
    pub scale: f32,
    pub path: Option<String>,
}

impl PdfState {
    pub fn new() -> Self {
        Self {
            current_page: 0,
            total_pages: 0,
            scale: 1.5,
            path: None,
        }
    }
}

impl Default for PdfState {
    fn default() -> Self {
        Self::new()
    }
}
