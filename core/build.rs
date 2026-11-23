//! Build script: fetches and places the PDFium shared library (see `build_pdfium.rs`).

#[path = "build_pdfium.rs"]
mod build_pdfium;

fn main() {
    build_pdfium::setup_pdfium();
}
