use iced::Rectangle;

pub fn clip_viewport(viewport: Rectangle, clip: Rectangle) -> Rectangle {
    let x = viewport.x.max(clip.x);
    let y = viewport.y.max(clip.y);
    let width = (viewport.x + viewport.width).min(clip.x + clip.width) - x;
    let height = (viewport.y + viewport.height).min(clip.y + clip.height) - y;
    if width <= 0.0 || height <= 0.0 {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    } else {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }
}

pub fn normalized_selection(
    anchor: Option<(usize, usize)>,
    focus: Option<(usize, usize)>,
) -> Option<((usize, usize), (usize, usize))> {
    match (anchor, focus) {
        (Some(a), Some(f)) => {
            if a <= f {
                Some((a, f))
            } else {
                Some((f, a))
            }
        }
        _ => None,
    }
}
