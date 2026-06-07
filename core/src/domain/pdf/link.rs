use super::geometry::PdfRect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct LinkInfo {
    pub bbox: PdfRect,
    pub dest_page: Option<u32>,
    pub dest_y: Option<f32>,
    pub uri: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LinkPreviewResult {
    pub image_data: Vec<u8>,
    pub center_ratio: f32,
}
