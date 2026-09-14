//! Presentation viewer sub-state.
//!
//! Owns the open `.pptx` deck: its prepared slides, which load is current,
//! the zoom, and the scroll position the toolbar's slide counter is read
//! from. Presentations are view-only, so unlike the PDF pane there is no
//! selection, annotation or render queue: a deck is parsed and laid out once,
//! off the UI thread (see [`crate::slides`]), and every slide draws from that
//! one prepared copy at whatever size it is shown.
//!
//! Part of the `MdEditor` decomposition: the shell owns cross-pane
//! coordination, each sub-state owns its own domain.

use std::sync::Arc;

use iced::Task;
use iced::widget::operation::{self, AbsoluteOffset};

use crate::messages::Message;
use crate::slides::LoadedDeck;
use crate::views::pptx_viewer::{PPTX_SCROLLABLE_ID, SLIDE_LIST_PADDING, slide_metrics};

/// Zoom levels the toolbar steps through, as multiples of fitting the pane.
pub const ZOOM_STEPS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

pub struct PptxPane {
    /// Vault-relative path of the open deck, set as soon as it starts loading.
    pub active_path: Option<String>,
    pub deck: Option<Arc<LoadedDeck>>,
    /// Why the open deck could not be read.
    pub error: Option<String>,
    /// Bumped by every load and by closing, so the result of a slow parse
    /// the reader has since moved away from is dropped when it lands.
    pub generation: u64,
    /// Multiple of the slide width that fits the pane.
    pub zoom: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub viewport_height: f32,
}

impl PptxPane {
    pub fn new() -> Self {
        Self {
            active_path: None,
            deck: None,
            error: None,
            generation: 0,
            zoom: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            viewport_height: 0.0,
        }
    }

    /// Forget the open deck, releasing its slides and pictures.
    pub fn close(&mut self) {
        self.active_path = None;
        self.deck = None;
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Start loading `path` and return the generation its result must carry.
    ///
    /// Reloading the deck already on screen — because it changed on disk —
    /// keeps it showing, at the same scroll position and zoom, until the new
    /// copy is ready; opening a different deck starts from its first slide.
    pub fn begin_load(&mut self, path: &str) -> u64 {
        let reload = self.active_path.as_deref() == Some(path) && self.deck.is_some();
        if !reload {
            self.deck = None;
            self.zoom = 1.0;
            self.scroll_x = 0.0;
            self.scroll_y = 0.0;
        }
        self.active_path = Some(path.to_string());
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    /// The slide the reader is on: the one under a line a third of the way
    /// down the viewport, or the last slide once scrolled to the very end.
    pub fn current_slide(&self, available_width: f32) -> usize {
        let Some(deck) = &self.deck else {
            return 0;
        };
        let count = deck.slides.len();
        if count == 0 {
            return 0;
        }
        let metrics = slide_metrics(deck, available_width, self.zoom);
        if self.scroll_y > 0.0
            && self.scroll_y + self.viewport_height >= metrics.content_height(count) - 1.0
        {
            return count - 1;
        }
        let probe = self.scroll_y + self.viewport_height / 3.0 - SLIDE_LIST_PADDING;
        ((probe / metrics.pitch).floor().max(0.0) as usize).min(count - 1)
    }

    pub fn update(&mut self, message: Message, available_width: f32) -> Task<Message> {
        match message {
            Message::PptxLoaded(generation, result) => {
                if generation != self.generation {
                    return Task::none();
                }
                match result {
                    Ok(deck) => {
                        self.deck = Some(deck);
                        self.error = None;
                    }
                    // A reload that fails keeps the deck already showing: the
                    // file is most likely still being written.
                    Err(err) => {
                        if self.deck.is_none() {
                            self.error = Some(err);
                        }
                    }
                }
                Task::none()
            }
            Message::PptxScrolled {
                x,
                y,
                viewport_height,
            } => {
                self.scroll_x = x;
                self.scroll_y = y;
                self.viewport_height = viewport_height;
                Task::none()
            }
            Message::PptxZoomIn => self.set_zoom(step_zoom(self.zoom, true), available_width),
            Message::PptxZoomOut => self.set_zoom(step_zoom(self.zoom, false), available_width),
            Message::PptxZoomFit => self.set_zoom(1.0, available_width),
            Message::PptxStep(delta) => {
                let Some(count) = self.deck.as_ref().map(|deck| deck.slides.len()) else {
                    return Task::none();
                };
                if count == 0 {
                    return Task::none();
                }
                let current = self.current_slide(available_width) as i64;
                let target = (current + i64::from(delta)).clamp(0, count as i64 - 1) as usize;
                self.scroll_to_slide(target, 0.0, available_width)
            }
            _ => Task::none(),
        }
    }

    /// Scroll by `delta` pixels, for the arrow keys.
    pub fn scroll_by(&mut self, delta: f32, available_width: f32) -> Task<Message> {
        let Some(deck) = self.deck.clone() else {
            return Task::none();
        };
        let metrics = slide_metrics(&deck, available_width, self.zoom);
        let max = (metrics.content_height(deck.slides.len()) - self.viewport_height).max(0.0);
        let y = (self.scroll_y + delta).clamp(0.0, max);
        self.scroll_y = y;
        operation::scroll_to(
            iced::advanced::widget::Id::new(PPTX_SCROLLABLE_ID),
            AbsoluteOffset {
                x: self.scroll_x,
                y,
            },
        )
    }

    /// Change the zoom, keeping the slide being read at the same place in
    /// the viewport rather than letting the content slide out from under it.
    fn set_zoom(&mut self, zoom: f32, available_width: f32) -> Task<Message> {
        let Some(deck) = self.deck.clone() else {
            self.zoom = zoom;
            return Task::none();
        };
        if (zoom - self.zoom).abs() < f32::EPSILON {
            return Task::none();
        }
        let before = slide_metrics(&deck, available_width, self.zoom);
        let index = self.current_slide(available_width);
        let slide_top = SLIDE_LIST_PADDING + index as f32 * before.pitch;
        let fraction = (self.scroll_y - slide_top) / before.pitch;
        self.zoom = zoom;
        if zoom <= 1.0 {
            self.scroll_x = 0.0;
        }
        self.scroll_to_slide(index, fraction, available_width)
    }

    fn scroll_to_slide(
        &mut self,
        index: usize,
        fraction: f32,
        available_width: f32,
    ) -> Task<Message> {
        let Some(deck) = self.deck.clone() else {
            return Task::none();
        };
        let metrics = slide_metrics(&deck, available_width, self.zoom);
        let max = (metrics.content_height(deck.slides.len()) - self.viewport_height).max(0.0);
        let y = (SLIDE_LIST_PADDING + (index as f32 + fraction) * metrics.pitch).clamp(0.0, max);
        self.scroll_y = y;
        operation::scroll_to(
            iced::advanced::widget::Id::new(PPTX_SCROLLABLE_ID),
            AbsoluteOffset {
                x: self.scroll_x,
                y,
            },
        )
    }
}

/// The next zoom step above or below `current`.
fn step_zoom(current: f32, up: bool) -> f32 {
    if up {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|step| *step > current + 0.01)
            .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < current - 0.01)
            .unwrap_or(ZOOM_STEPS[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slides::PreparedSlide;
    use md_editor_core::pptx::{Fill, Slide};
    use std::collections::HashMap;

    fn deck(count: usize) -> Arc<LoadedDeck> {
        let slide = |number| Slide {
            number,
            hidden: false,
            title: None,
            background: Fill::None,
            elements: Vec::new(),
        };
        Arc::new(LoadedDeck {
            id: 1,
            width: 720.0,
            height: 405.0,
            slides: (1..=count).map(slide).collect(),
            prepared: (0..count)
                .map(|_| PreparedSlide {
                    background: None,
                    elements: Vec::new(),
                    layers: std::iter::once(0..0).collect(),
                })
                .collect(),
            images: HashMap::new(),
        })
    }

    #[test]
    fn zoom_steps_stop_at_the_ends() {
        assert_eq!(step_zoom(1.0, true), 1.25);
        assert_eq!(step_zoom(1.0, false), 0.75);
        assert_eq!(step_zoom(3.0, true), 3.0);
        assert_eq!(step_zoom(0.5, false), 0.5);
        // A zoom between steps moves to the nearest step in that direction.
        assert_eq!(step_zoom(1.1, true), 1.25);
        assert_eq!(step_zoom(1.1, false), 1.0);
    }

    #[test]
    fn current_slide_follows_the_scroll_position() {
        let mut pane = PptxPane::new();
        pane.begin_load("deck.pptx");
        let generation = pane.generation;
        let _ = pane.update(Message::PptxLoaded(generation, Ok(deck(10))), 1000.0);
        pane.viewport_height = 600.0;
        assert_eq!(pane.current_slide(1000.0), 0);

        let pitch = slide_metrics(pane.deck.as_ref().unwrap(), 1000.0, 1.0).pitch;
        pane.scroll_y = SLIDE_LIST_PADDING + 3.0 * pitch;
        assert_eq!(pane.current_slide(1000.0), 3);

        pane.scroll_y = 1.0e6;
        assert_eq!(pane.current_slide(1000.0), 9);
    }

    #[test]
    fn results_from_an_abandoned_load_are_dropped() {
        let mut pane = PptxPane::new();
        let stale = pane.begin_load("first.pptx");
        pane.begin_load("second.pptx");
        let _ = pane.update(Message::PptxLoaded(stale, Ok(deck(3))), 1000.0);
        assert!(pane.deck.is_none());

        let stale = pane.generation;
        pane.close();
        let _ = pane.update(Message::PptxLoaded(stale, Err("late".to_string())), 1000.0);
        assert!(pane.error.is_none());
    }
}
