//! Bringing the caret into view.
//!
//! The widget can't scroll the scrollable it sits in, and the app can't see
//! where the caret is drawn. So the app asks — it bumps a request number — and
//! on the next frame, with layout final and the viewport known, the widget
//! answers with the caret's exact place. Nothing is estimated on either side.

use iced::Rectangle;
use iced::advanced::Shell;

use super::metrics::{body_row, content_bounds};
use super::{Editor, Measure, State};

/// How the caret should be brought into view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reveal {
    /// Scroll as little as keeps the caret a comfortable distance inside the
    /// viewport, as while typing or moving the caret.
    Nearest,
    /// Put the caret in the middle, as after jumping to a search match.
    Center,
}

/// Where the caret is, answering one reveal request. Every y is relative to
/// the top of the editor widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretView {
    pub request: u64,
    pub caret_top: f32,
    pub caret_bottom: f32,
    pub viewport_top: f32,
    pub viewport_height: f32,
    /// Height of the whole widget.
    pub height: f32,
}

impl CaretView {
    /// The viewport top that shows the caret the way `reveal` asks, or `None`
    /// when the caret is already shown that way.
    pub fn viewport_top_for(&self, reveal: Reveal) -> Option<f32> {
        let height = self.viewport_height;
        match reveal {
            Reveal::Center => Some((self.caret_top + self.caret_bottom - height) / 2.0),
            Reveal::Nearest => {
                let comfort = (2.0 * body_row()).min(height / 4.0).max(0.0);
                if self.caret_top < self.viewport_top + comfort {
                    Some(self.caret_top - comfort)
                } else if self.caret_bottom > self.viewport_top + height - comfort {
                    Some(self.caret_bottom + comfort - height)
                } else {
                    None
                }
            }
        }
    }
}

impl<Message> Editor<'_, Message> {
    /// Answer an outstanding reveal request with the caret's place. Called
    /// when a frame is about to be drawn, so layout is final.
    pub(super) fn report_caret<R: Measure>(
        &self,
        state: &mut State,
        layout_bounds: Rectangle,
        viewport: Rectangle,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(on_caret_view) = &self.on_caret_view else {
            return;
        };
        if state.revealed_request == self.reveal_request || self.lines.is_empty() {
            return;
        }
        state.revealed_request = self.reveal_request;

        let bounds = content_bounds(layout_bounds);
        let line_idx = self.buffer.cursor_line.min(self.lines.len() - 1);
        let focused = state.is_focused;
        let caret = self.caret_box::<R>(
            line_idx,
            self.buffer.cursor_col,
            self.buffer.cursor_affinity,
            bounds.width,
            self.is_block_editing(&self.lines[line_idx], focused),
            self.active_col(line_idx, focused),
        );
        let caret_top = self.line_body_top(line_idx, state) + caret.y;
        shell.publish(on_caret_view(CaretView {
            request: self.reveal_request,
            caret_top,
            caret_bottom: caret_top + caret.height,
            viewport_top: viewport.y - layout_bounds.y,
            viewport_height: viewport.height,
            height: layout_bounds.height,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(caret_top: f32, viewport_top: f32) -> CaretView {
        CaretView {
            request: 1,
            caret_top,
            caret_bottom: caret_top + 19.0,
            viewport_top,
            viewport_height: 600.0,
            height: 5000.0,
        }
    }

    #[test]
    fn nearest_leaves_a_comfortably_visible_caret_alone() {
        assert_eq!(view(1300.0, 1000.0).viewport_top_for(Reveal::Nearest), None);
    }

    #[test]
    fn nearest_scrolls_just_enough_to_restore_the_comfort_margin() {
        let comfort = 2.0 * body_row();
        let above = view(1010.0, 1000.0);
        assert_eq!(
            above.viewport_top_for(Reveal::Nearest),
            Some(1010.0 - comfort)
        );

        let below = view(1590.0, 1000.0);
        let top = below.viewport_top_for(Reveal::Nearest).expect("scrolls");
        assert_eq!(top + 600.0 - comfort, below.caret_bottom);
    }

    #[test]
    fn center_puts_the_caret_mid_viewport() {
        let v = view(3000.0, 0.0);
        let top = v.viewport_top_for(Reveal::Center).expect("scrolls");
        assert_eq!(top + 300.0, (v.caret_top + v.caret_bottom) / 2.0);
    }
}
