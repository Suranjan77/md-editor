//! Renders the editor to PNG files with iced's software renderer, for looking
//! at rendering changes without opening a window.
//!
//! Run with `RENDER_PREVIEW_DIR=<dir> cargo test -p md-editor-native render_preview -- --ignored`.

use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{Layout, Limits};
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Shell, clipboard};
use iced::{Point, Rectangle, Size, mouse};

use crate::editor::buffer::{Affinity, DocBuffer, EditorCommand};
use crate::editor::highlight::highlight_markdown;
use crate::editor::renderer::{Editor, ImageCache, MathCache, MathRender};

const DOC: &str = r#"# A heading that wraps when the column is narrow
Styled **bold words** and *italic ones* with `inline code` and [a link](http://example.com) that wraps across several rows of the editor column here.
A plain paragraph that is long enough to wrap across several rows of the editor column without any styling at all, to check alignment.
- [ ] an unchecked task with enough words to wrap onto a second row of the column
- [x] a checked task
> a quoted line with **bold** text
Inline $a^2$ math sits on the baseline and $x$ wraps with the text around it nicely.
```rust
fn main() { let a_really_long_identifier_name_to_force_horizontal_scrolling = "some string value"; }
```
| Name | Value | Notes |
|---|---|---|
| alpha | 1 | first |
| beta | 2 | second |
| gamma | 3 | third |
$$
\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6} \qquad \int_0^1 x^2\,dx = \frac{1}{3} \qquad e^{i\pi} + 1 = 0
$$
---
Trailing paragraph.
![The app icon](md-editor.png)
After the image."#;

type R = iced::Renderer;

fn bitmap(w: u32, h: u32, rgba: [u8; 4]) -> iced::widget::image::Handle {
    let pixels: Vec<u8> = (0..w * h).flat_map(|_| rgba).collect();
    iced::widget::image::Handle::from_rgba(w, h, pixels)
}

/// Device pixels per logical pixel in the rendered PNGs, and the density
/// equations are rasterized for.
const PREVIEW_SCALE: f32 = 2.0;

/// Every equation in `lines`, rendered by the app's own LaTeX renderer and
/// keyed the way the app keys them. One that fails to render shows as a flat
/// placeholder.
fn math(lines: &[crate::editor::highlight::StyledLine]) -> MathCache {
    let mut cache = MathCache::new();
    for span in lines.iter().flat_map(|line| &line.spans) {
        if !span.is_math {
            continue;
        }
        let tex = span
            .visible_text(false)
            .trim_matches('$')
            .trim()
            .to_string();
        if tex.is_empty() || cache.contains_key(&tex) {
            continue;
        }
        let render =
            crate::editor_state::render_latex_task(&tex, PREVIEW_SCALE).unwrap_or_else(|_| {
                let handle = bitmap(40, 20, [230, 200, 120, 255]);
                MathRender {
                    inline_handle: handle.clone(),
                    block_handle: handle,
                    width: 40.0,
                    height: 20.0,
                }
            });
        cache.insert(tex, render);
    }
    cache
}

/// Every image in `lines`, loaded from the repository root.
fn images(lines: &[crate::editor::highlight::StyledLine]) -> ImageCache {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut cache = ImageCache::new();
    for span in lines.iter().flat_map(|line| &line.spans) {
        if let Some(path) = span.image_path.as_ref().filter(|_| span.is_image)
            && let Ok(img) = image::open(root.join(path))
        {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            let handle = iced::widget::image::Handle::from_rgba(width, height, rgba.into_raw());
            cache.insert(path.clone(), (handle, width as f32, height as f32));
        }
    }
    cache
}

fn editor<'a>(
    buffer: &'a DocBuffer,
    lines: &'a [crate::editor::highlight::StyledLine],
    images: &'a ImageCache,
    math: &'a MathCache,
) -> Editor<'a, ()> {
    Editor::new(buffer, lines, images, math, |_| (), |_| (), |_| (), |_| ())
}

struct Scene {
    name: &'static str,
    width: f32,
    cursor: Option<(usize, usize)>,
    selection: Option<(usize, usize, usize, usize)>,
}

fn render(scene: &Scene, out_dir: &str) {
    let doc = std::env::var("RENDER_PREVIEW_DOC").unwrap_or(DOC.to_string());
    let doc = doc.as_str();
    let mut buffer = DocBuffer::from_text(doc);
    let lines = highlight_markdown(doc);
    let images = images(&lines);
    let math = math(&lines);
    let mut renderer = iced::futures::executor::block_on(<R as Headless>::new(
        iced::Font::DEFAULT,
        16.0.into(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");

    let mut tree = {
        let editor = editor(&buffer, &lines, &images, &math);
        Tree {
            tag: <Editor<'_, ()> as Widget<(), iced::Theme, R>>::tag(&editor),
            state: <Editor<'_, ()> as Widget<(), iced::Theme, R>>::state(&editor),
            children: Vec::new(),
        }
    };
    let limits = Limits::new(Size::ZERO, Size::new(scene.width, f32::INFINITY));

    if let Some((line, col)) = scene.cursor {
        // Focus with a click, then place the caret.
        let node = {
            let mut editor = editor(&buffer, &lines, &images, &math);
            <Editor<'_, ()> as Widget<(), iced::Theme, R>>::layout(
                &mut editor,
                &mut tree,
                &renderer,
                &limits,
            )
        };
        let mut messages = Vec::new();
        let mut editor = editor(&buffer, &lines, &images, &math);
        <Editor<'_, ()> as Widget<(), iced::Theme, R>>::update(
            &mut editor,
            &mut tree,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(100.0, 30.0)),
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &node.bounds(),
        );
        drop(editor);
        buffer.execute(EditorCommand::SetCursor {
            line,
            col,
            affinity: Affinity::Downstream,
        });
    }
    if let Some((anchor_line, anchor_col, focus_line, focus_col)) = scene.selection {
        buffer.execute(EditorCommand::SetSelection {
            anchor_line,
            anchor_col,
            focus_line,
            focus_col,
            affinity: Affinity::Downstream,
        });
    }

    let mut editor = editor(&buffer, &lines, &images, &math);
    let node = <Editor<'_, ()> as Widget<(), iced::Theme, R>>::layout(
        &mut editor,
        &mut tree,
        &renderer,
        &limits,
    );
    let bounds = node.bounds();
    let viewport = Rectangle {
        height: bounds.height,
        ..bounds
    };
    <Editor<'_, ()> as Widget<(), iced::Theme, R>>::draw(
        &editor,
        &tree,
        &mut renderer,
        &iced::Theme::Dark,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &viewport,
    );

    let scale = PREVIEW_SCALE;
    let size = Size::new(
        (bounds.width * scale) as u32,
        (bounds.height * scale) as u32,
    );
    let pixels = renderer.screenshot(size, scale, crate::theme::BG_PRIMARY);
    let path = format!("{out_dir}/{}.png", scene.name);
    image::save_buffer(
        &path,
        &pixels,
        size.width,
        size.height,
        image::ColorType::Rgba8,
    )
    .expect("write png");
}

#[test]
#[ignore = "writes PNG previews; run explicitly with RENDER_PREVIEW_DIR set"]
fn render_preview() {
    let dir = std::env::var("RENDER_PREVIEW_DIR").expect("set RENDER_PREVIEW_DIR");
    for scene in [
        Scene {
            name: "narrow_caret_in_styled_paragraph",
            width: 420.0,
            cursor: Some((1, 60)),
            selection: None,
        },
        Scene {
            name: "narrow_caret_after_checkbox",
            width: 420.0,
            cursor: Some((3, 6)),
            selection: None,
        },
        Scene {
            name: "narrow_selection_across_rows",
            width: 420.0,
            cursor: Some((2, 0)),
            selection: Some((2, 10, 2, 110)),
        },
        Scene {
            name: "selection_across_rich_blocks",
            width: 560.0,
            cursor: Some((21, 5)),
            selection: Some((10, 0, 21, 5)),
        },
        Scene {
            name: "caret_in_equation_source",
            width: 560.0,
            cursor: Some((16, 12)),
            selection: None,
        },
        Scene {
            name: "selection_across_lines",
            width: 560.0,
            cursor: Some((4, 0)),
            selection: Some((1, 20, 4, 30)),
        },
        Scene {
            name: "wide_unfocused",
            width: 760.0,
            cursor: None,
            selection: None,
        },
    ] {
        render(&scene, &dir);
    }
}
