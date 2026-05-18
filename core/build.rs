#[path = "build_pdfium.rs"]
mod build_pdfium;

fn main() {
    build_pdfium::setup_pdfium();
}
