use pdfium_render::prelude::*;
use std::sync::OnceLock;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

pub fn test() {
    let p = PDFIUM.get().unwrap();
    let _doc = p.load_pdf_from_file("test", None);
}
