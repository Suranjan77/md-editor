//! Pen input on `InkState`: captured samples, the stroke lifecycle (begin, extend, finish, cancel), erasing, the radial menu and page advance.

use super::*;

impl InkState {
    /// Feed a batch of captured pen samples. Returns whether anything changed
    /// and the view needs redrawing.
    pub fn apply_samples(&mut self, samples: &[PenSample], scale_factor: f32) -> bool {
        let mut changed = false;

        for sample in samples {
            if sample.phase == PenPhase::Leave {
                changed |= self.hover.take().is_some();
                continue;
            }

            let starting = sample.phase == PenPhase::Down;
            let Some(position) = map_to_canvas(sample, scale_factor, starting) else {
                continue;
            };

            match sample.phase {
                PenPhase::Hover => {
                    // Cheap and constant, so it is worth a redraw every time:
                    // this indicator is how the surface is aimed.
                    self.hover = Some(position);
                    changed = true;
                }
                PenPhase::Down => {
                    if sample.barrel {
                        // The side button summons the menu instead of drawing.
                        self.radial = Some(radial::RadialMenu::new(position));
                    } else {
                        self.begin(position, sample.pressure, sample.eraser);
                    }
                    changed = true;
                }
                PenPhase::Move => {
                    self.hover = Some(position);
                    if let Some(menu) = &mut self.radial {
                        menu.cursor = position;
                        changed = true;
                    } else {
                        changed |= self.extend(position, sample.pressure);
                    }
                }
                PenPhase::Up => {
                    if self.radial.is_some() {
                        changed |= self.commit_radial();
                    } else {
                        changed |= self.extend(position, sample.pressure);
                        changed |= self.finish();
                    }
                }
                PenPhase::Leave => unreachable!("handled above"),
            }
        }

        changed
    }

    /// Where the pointing device is, in surface-local screen pixels.
    ///
    /// Fed by both the pen (hover samples) and the mouse, so the aiming
    /// indicators track whichever moved last.
    pub fn hover(&self) -> Option<Vec2> {
        self.hover
    }

    pub fn set_hover(&mut self, position: Vec2) {
        self.hover = Some(position);
    }

    pub fn radial_menu(&self) -> Option<radial::RadialMenu> {
        self.radial
    }

    /// Apply whatever the open radial menu is pointing at and close it.
    fn commit_radial(&mut self) -> bool {
        let Some(menu) = self.radial.take() else {
            return false;
        };

        match menu.highlighted() {
            Some(radial::RadialAction::Tool(tool)) => {
                if tool != Tool::Lasso {
                    self.clear_selection();
                }
                self.tool = tool;
            }
            Some(radial::RadialAction::Color(color)) => {
                if self.tool == Tool::Highlighter {
                    self.highlighter_color = color;
                } else {
                    self.color = color;
                }
            }
            // Lifted in the dead zone or flicked away: cancelled.
            None => {}
        }
        true
    }

    /// Pull the page up once a stroke has ended in the bottom margin, so the
    /// next line has room without stopping to pan.
    ///
    /// Only ever between strokes. Panning mid-stroke moves the page under a nib
    /// that stays put on the glass, so every later sample lands further down the
    /// page: the stroke is smeared across the gap, and because each sample in
    /// the margin panned again, the page ran away while writing (the "page
    /// split" bug). Note apps that scroll while writing all do it at pen-up.
    ///
    /// Takes a **screen** position because the margin is a property of the
    /// window, not of the page.
    fn advance_if_near_bottom(&mut self, screen_y: f32) -> bool {
        let Some(bounds) = viewport() else {
            return false;
        };
        let threshold = bounds.height - ADVANCE_MARGIN;
        if threshold <= 0.0 || screen_y <= threshold {
            return false;
        }

        // Lift the line just written to the threshold, leaving the margin free
        // for the next one.
        self.camera.pan_by(Vec2::new(0.0, threshold - screen_y));
        self.cache.clear();
        true
    }

    /// Begin an interaction at a **screen**-space position.
    ///
    /// What that means depends on the tool: drawing a stroke, erasing, drawing
    /// a lasso, or — when the press lands on an existing selection — starting
    /// to drag it.
    pub fn begin(&mut self, screen: Vec2, pressure: f32, inverted: bool) {
        let position = self.camera.to_page(screen);

        // Keep the aiming indicators pinned to the nib from the first sample,
        // not from wherever the last hover left them.
        self.hover = Some(screen);
        self.live.clear();
        self.drag_from = None;
        self.drag_total = Vec2::new(0.0, 0.0);

        // An inverted pen erases regardless of the selected tool.
        if inverted || self.tool == Tool::Eraser {
            self.erasing = true;
            self.drawing = false;
            // The whole drag is one undoable gesture; without this, undo would
            // rewind an eraser sweep one input sample at a time.
            self.document.begin_batch();
            self.erase_at(position);
            return;
        }

        self.erasing = false;

        if self.tool == Tool::Lasso {
            self.drawing = false;
            if self.selection_contains(position) {
                // Grabbing the existing selection moves it instead of starting
                // a new lasso.
                self.drag_from = Some(position);
            } else {
                self.selection.clear();
                self.lasso.clear();
                self.lasso.push(position);
            }
            return;
        }

        self.drawing = true;
        self.last_screen = Some(screen);
        self.live.push(InkPoint {
            pos: position,
            pressure,
        });
    }

    /// Continue the current interaction. Returns whether anything changed.
    pub fn extend(&mut self, screen: Vec2, pressure: f32) -> bool {
        let position = self.camera.to_page(screen);
        // Covers a mouse drag too, which never produces hover samples.
        self.hover = Some(screen);

        if self.erasing {
            return self.erase_at(position);
        }

        if self.tool == Tool::Lasso {
            if let Some(from) = self.drag_from {
                // Apply the move incrementally so the strokes follow the pen;
                // the whole gesture is recorded once on release.
                let delta = position.sub(from);
                if delta.x == 0.0 && delta.y == 0.0 {
                    return false;
                }
                let selection = self.selection.clone();
                self.document.translate_without_history(&selection, delta);
                self.drag_total = self.drag_total.add(delta);
                self.drag_from = Some(position);
                self.cache.clear();
                return true;
            }
            if !self.lasso.is_empty() {
                self.lasso.push(position);
                return true;
            }
            return false;
        }

        if !self.drawing {
            return false;
        }
        self.live.push(InkPoint {
            pos: position,
            pressure,
        });
        self.last_screen = Some(screen);
        true
    }

    /// End the current interaction, committing whatever it produced. Returns
    /// whether anything changed.
    pub fn finish(&mut self) -> bool {
        if self.erasing {
            self.erasing = false;
            self.drawing = false;
            self.document.end_batch();
            return false;
        }

        if self.tool == Tool::Lasso {
            if self.drag_from.take().is_some() {
                let moved = self.drag_total;
                self.drag_total = Vec2::new(0.0, 0.0);
                if moved.x == 0.0 && moved.y == 0.0 {
                    return false;
                }
                // The strokes already sit where the drag left them, so the
                // history only needs the total to undo by.
                let selection = self.selection.clone();
                self.document.record_move(&selection, moved);
                self.dirty = true;
                return true;
            }

            // Only a gesture that actually drew a lasso resolves to a
            // selection. Without this, a stray pen-up — one whose press landed
            // outside the surface and was rejected — would silently clear a
            // selection the user is still working with.
            if self.lasso.is_empty() {
                return false;
            }

            self.selection = if self.lasso.len() >= 3 {
                self.document.select_in_polygon(&self.lasso)
            } else {
                // A tap rather than a loop: treat it as "deselect".
                Vec::new()
            };
            self.lasso.clear();
            return true;
        }

        if !self.drawing {
            return false;
        }

        self.drawing = false;

        // A single sample is a legitimate dot — the tittle on an `i`.
        if self.live.is_empty() {
            return false;
        }

        let (size, color, kind) = self.active_stroke_style();
        let stroke = InkStroke::from_points(&self.live, size, color, kind);
        self.document.push(stroke);
        self.live.clear();
        self.invalidate();
        // The stroke is committed in page space, so moving the view now leaves
        // it exactly where it was written.
        if let Some(screen) = self.last_screen.take() {
            self.advance_if_near_bottom(screen.y);
        }
        true
    }

    /// Size, colour and kind the next committed stroke will carry.
    ///
    /// The highlighter's alpha is baked into the stored colour rather than
    /// applied at render time, so a page keeps its appearance even if the
    /// default opacity is retuned later.
    pub fn active_stroke_style(&self) -> (f32, [f32; 4], document::StrokeKind) {
        if self.tool == Tool::Highlighter {
            let mut color = rgba(self.highlighter_color);
            color[3] = HIGHLIGHTER_ALPHA;
            (
                self.highlighter_size,
                color,
                document::StrokeKind::Highlighter,
            )
        } else {
            (
                self.options.size,
                rgba(self.color),
                document::StrokeKind::Pen,
            )
        }
    }

    /// Render options and colour for the stroke currently under the pen.
    pub fn live_style(&self) -> (StrokeOptions, Color) {
        let (size, color, kind) = self.active_stroke_style();
        let options = options_for(
            kind,
            StrokeOptions {
                size,
                ..self.options
            },
        );
        (
            options,
            Color::from_rgba(color[0], color[1], color[2], color[3]),
        )
    }

    /// Set the page's background ruling.
    pub fn set_paper(&mut self, paper: document::PaperStyle) {
        if self.document.paper != paper {
            self.document.paper = paper;
            self.dirty = true;
            self.cache.clear();
        }
    }

    /// Drop an in-progress stroke or lasso without committing it.
    pub fn cancel_stroke(&mut self) {
        self.live.clear();
        self.lasso.clear();
        self.radial = None;
        self.hover = None;
        if self.erasing {
            // Close the gesture rather than leaving the document collecting
            // edits into a batch nothing will ever commit.
            self.document.end_batch();
        }
        self.drawing = false;
        self.erasing = false;
        self.last_screen = None;
        // A selection drag moves the strokes as it goes, with no history until
        // release. Cancelling must put them back, or the page changes with
        // nothing to undo and no unsaved marker.
        if self.drag_from.take().is_some() {
            let moved = self.drag_total;
            if moved.x != 0.0 || moved.y != 0.0 {
                let selection = self.selection.clone();
                self.document
                    .translate_without_history(&selection, Vec2::new(0.0, 0.0).sub(moved));
                self.cache.clear();
            }
        }
        self.drag_total = Vec2::new(0.0, 0.0);
    }

    fn erase_at(&mut self, position: Vec2) -> bool {
        let radius = self.eraser_radius();
        let erased = match self.eraser_mode {
            EraserMode::Partial => self.document.erase_partial_at(position, radius),
            EraserMode::Stroke => self.document.erase_at(position, radius),
        };
        if erased {
            // Removing strokes shifts every later index, so any selection
            // pointing at them is now meaningless.
            self.selection.clear();
            self.invalidate();
            true
        } else {
            false
        }
    }
}
