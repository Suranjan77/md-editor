use pdfium_render::prelude::*;
use image::DynamicImage;

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

pub struct PdfRenderer {
    pdfium: Pdfium,
}

impl PdfRenderer {
    pub fn new() -> Result<Self, String> {
        let pdfium = Pdfium::new(
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name())
                .map_err(|e| format!("Failed to bind to pdfium library: {:?}", e))?
        );
        Ok(Self { pdfium })
    }

    /// Render a specific page of a PDF file to a DynamicImage.
    pub fn render_page(&self, path: &str, page_index: u16, scale: f32) -> Result<DynamicImage, String> {
        let document = self.pdfium.load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
        
        let pages = document.pages();
        if i32::from(page_index) >= pages.len() {
            return Err("Page index out of bounds".to_string());
        }

        let page = pages.get(page_index as i32)
            .map_err(|e| format!("Failed to get page: {:?}", e))?;
        
        let render_config = PdfRenderConfig::new()
            .set_target_width((page.width().value * scale) as i32)
            .set_target_height((page.height().value * scale) as i32);

        let bitmap = page.render_with_config(&render_config)
            .map_err(|e| format!("Failed to render page: {:?}", e))?;
        
        bitmap.as_image().map_err(|e| format!("Failed to convert to image: {:?}", e))
    }

    /// Get total page count of a PDF.
    pub fn page_count(&self, path: &str) -> Result<u16, String> {
        let document = self.pdfium.load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
        Ok(document.pages().len() as u16)
    }
}
