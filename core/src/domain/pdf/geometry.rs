use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PdfRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
