#![allow(dead_code)]
use iced::Rectangle;

pub(super) type TextPosition = (usize, usize);
pub(super) type SelectionRange = (TextPosition, TextPosition);

pub(super) fn clip_viewport(viewport: Rectangle, clip: Rectangle) -> Rectangle {
    let x1 = viewport.x.max(clip.x);
    let y1 = viewport.y.max(clip.y);
    let x2 = (viewport.x + viewport.width).min(clip.x + clip.width);
    let y2 = (viewport.y + viewport.height).min(clip.y + clip.height);

    Rectangle {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

pub(super) fn normalized_selection(
    anchor: Option<TextPosition>,
    focus: Option<TextPosition>,
) -> Option<SelectionRange> {
    let anchor = anchor?;
    let focus = focus?;

    if anchor == focus {
        None
    } else if anchor <= focus {
        Some((anchor, focus))
    } else {
        Some((focus, anchor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_selection_orders_distinct_positions() {
        for anchor_line in 0..15 {
            for anchor_col in 0..10 {
                for focus_line in 0..15 {
                    for focus_col in 0..10 {
                        let anchor = (anchor_line, anchor_col);
                        let focus = (focus_line, focus_col);
                        let normalized = normalized_selection(Some(anchor), Some(focus));

                        if anchor == focus {
                            assert!(normalized.is_none());
                        } else {
                            let (start, end) = normalized.expect("distinct positions");
                            assert_eq!(start, anchor.min(focus));
                            assert_eq!(end, anchor.max(focus));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn normalized_selection_requires_both_positions() {
        assert!(normalized_selection(None, None).is_none());
        assert!(normalized_selection(Some((1, 1)), None).is_none());
        assert!(normalized_selection(None, Some((2, 2))).is_none());
    }

    #[test]
    fn clip_viewport_returns_overlap() {
        let viewport = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };

        let inside = clip_viewport(
            viewport,
            Rectangle {
                x: 20.0,
                y: 30.0,
                width: 50.0,
                height: 50.0,
            },
        );
        assert_eq!(
            inside,
            Rectangle {
                x: 20.0,
                y: 30.0,
                width: 50.0,
                height: 50.0,
            }
        );

        let disjoint = clip_viewport(
            viewport,
            Rectangle {
                x: 200.0,
                y: 300.0,
                width: 50.0,
                height: 50.0,
            },
        );
        assert_eq!(disjoint.width, 0.0);
        assert_eq!(disjoint.height, 0.0);

        let partial = clip_viewport(
            viewport,
            Rectangle {
                x: 50.0,
                y: 100.0,
                width: 100.0,
                height: 200.0,
            },
        );
        assert_eq!(
            partial,
            Rectangle {
                x: 50.0,
                y: 100.0,
                width: 60.0,
                height: 120.0,
            }
        );
    }
}
