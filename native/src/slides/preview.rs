//! Renders a presentation's slides to PNG files with iced's software
//! renderer, through the same widgets the viewer shows them with — for
//! comparing against another renderer's output without opening a window.
//!
//! Run with
//! `PPTX_PREVIEW_FILE=<deck.pptx> PPTX_PREVIEW_DIR=<dir> cargo test -p md-editor-native slide_preview -- --ignored`.
//! `PPTX_PREVIEW_WIDTH` sets the slide width in pixels (default 960) and
//! `PPTX_PREVIEW_LIMIT` the number of slides (default all).

use std::path::Path;
use std::sync::Arc;

use iced::advanced::layout::{Layout, Limits};
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::Tree;
use iced::{Element, Rectangle, Size, Theme, mouse};

type R = iced::Renderer;

#[test]
#[ignore = "writes PNG previews; run explicitly with PPTX_PREVIEW_FILE and PPTX_PREVIEW_DIR set"]
fn slide_preview() {
    let file = std::env::var("PPTX_PREVIEW_FILE").expect("set PPTX_PREVIEW_FILE");
    let dir = std::env::var("PPTX_PREVIEW_DIR").expect("set PPTX_PREVIEW_DIR");
    let width: f32 = std::env::var("PPTX_PREVIEW_WIDTH")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(960.0);
    let limit: usize = std::env::var("PPTX_PREVIEW_LIMIT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(usize::MAX);

    let started = std::time::Instant::now();
    let deck = Arc::new(super::load_deck(Path::new(&file)).expect("deck loads"));
    eprintln!(
        "loaded {} slides in {:?}",
        deck.slides.len(),
        started.elapsed()
    );
    let height = width * deck.height / deck.width;

    for index in 0..deck.slides.len().min(limit) {
        // A fresh renderer per slide: a headless renderer keeps what it has
        // drawn, so reusing one paints every slide over the ones before it.
        let mut renderer = iced::futures::executor::block_on(<R as Headless>::new(
            iced::Font::DEFAULT,
            16.0.into(),
            Some("tiny-skia"),
        ))
        .expect("software renderer");
        let mut element: Element<'_, (), Theme, R> =
            crate::views::pptx_viewer::slide_page(&deck, index, width, height);
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &Limits::new(Size::ZERO, Size::new(width, height)),
        );
        let bounds = node.bounds();
        element.as_widget().draw(
            &tree,
            &mut renderer,
            &Theme::Dark,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &Rectangle { ..bounds },
        );
        let size = Size::new(bounds.width as u32, bounds.height as u32);
        let pixels = renderer.screenshot(size, 1.0, crate::theme::BG_PRIMARY);
        let path = format!("{dir}/slide-{:02}.png", index + 1);
        image::save_buffer(
            &path,
            &pixels,
            size.width,
            size.height,
            image::ColorType::Rgba8,
        )
        .expect("write png");
    }
}
