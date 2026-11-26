//! Decoding image files (raster formats and SVG) to RGBA for the image viewer and images in notes.

use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Rasterised SVGs are drawn at this multiple of their size, so they stay
/// sharp on HiDPI screens and when zoomed in a little.
const SVG_DENSITY: f32 = 2.0;
/// Longest side of a rasterised SVG, in pixels.
const SVG_MAX_SIDE: f32 = 4096.0;

/// A decoded image: straight (not premultiplied) RGBA pixels, and the size to
/// lay it out at, which for an SVG is smaller than the pixel size.
pub struct Decoded {
    pub rgba: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub width: f32,
    pub height: f32,
}

impl Decoded {
    pub fn handle(self) -> (iced::widget::image::Handle, f32, f32) {
        let handle =
            iced::widget::image::Handle::from_rgba(self.pixel_width, self.pixel_height, self.rgba);
        (handle, self.width, self.height)
    }
}

/// Decode `path`: SVG and handwritten `.ink` pages by extension (any case),
/// everything else by the `image` crate.
pub fn decode(path: &Path) -> Result<Decoded, String> {
    let has_ext = |want: &str| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(want))
    };
    if has_ext("svg") {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        return decode_svg(&bytes);
    }
    if has_ext("ink") {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let doc = crate::ink::document::InkDocument::from_json(&raw)?;
        return crate::ink::raster::render(&doc).ok_or_else(|| "The page has no ink yet".into());
    }
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let (pixel_width, pixel_height) = (img.width(), img.height());
    Ok(Decoded {
        rgba: img.into_rgba8().into_raw(),
        pixel_width,
        pixel_height,
        width: pixel_width as f32,
        height: pixel_height as f32,
    })
}

/// System fonts for text in SVGs, loaded once.
fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

pub fn decode_svg(bytes: &[u8]) -> Result<Decoded, String> {
    let options = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (width, height) = (size.width(), size.height());
    let scale = SVG_DENSITY.min(SVG_MAX_SIDE / width.max(height).max(1.0));
    let pixel_width = (width * scale).ceil().max(1.0) as u32;
    let pixel_height = (height * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_width, pixel_height)
        .ok_or_else(|| "SVG has no drawable size".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let c = pixel.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(Decoded {
        rgba,
        pixel_width,
        pixel_height,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_renders_at_double_density_with_its_own_layout_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
            <rect width="40" height="20" fill="#ff0000"/></svg>"##;
        let decoded = decode_svg(svg).unwrap();
        assert_eq!((decoded.width, decoded.height), (40.0, 20.0));
        assert_eq!((decoded.pixel_width, decoded.pixel_height), (80, 40));
        assert_eq!(&decoded.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn uppercase_extensions_and_misnamed_files_decode() {
        let dir = std::env::temp_dir().join(format!("md_raster_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("PHOTO.PNG");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]))
            .save_with_format(&png, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode(&png).unwrap().pixel_width, 3);
        // A PNG saved as .jpg still opens: the format comes from the bytes.
        let misnamed = dir.join("x.jpg");
        std::fs::copy(&png, &misnamed).unwrap();
        assert_eq!(decode(&misnamed).unwrap().pixel_height, 2);
        let svg = dir.join("D.SVG");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#,
        )
        .unwrap();
        assert_eq!(decode(&svg).unwrap().width, 4.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
