//! The ink drawing surface.
//!
//! Rendering is split in two so that writing stays cheap: every finished
//! stroke lives in a retained [`canvas::Cache`] that is only rebuilt when the
//! document or the view changes, while the stroke currently under the pen is
//! re-tessellated each frame. Without that split, every pen sample would
//! re-tessellate the whole page.
//!
//! Pan and zoom are baked into the cached geometry rather than applied to it
//! afterwards — iced offers no way to transform geometry once built — so
//! moving the view does invalidate the cache. Culling to the visible page area
//! keeps the cost proportional to what is on screen rather than to the size of
//! the document.

use iced::widget::canvas;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, Vector, mouse};

use super::Tool;
use super::camera::Camera;
use super::document::{InkDocument, PaperStyle, StrokeKind};
use super::stroke::{self, InkPoint, StrokeOptions, Vec2};
use crate::messages::Message;

/// Pressure used for mouse input, which reports none. Mid-range so mouse
/// strokes sit visually between a light and a heavy pen stroke.
pub const MOUSE_PRESSURE: f32 = 0.55;

/// Extra page-space margin around the viewport when culling, so a stroke whose
/// path starts just off-screen still contributes the width that reaches into
/// view.
const CULL_MARGIN: f32 = 64.0;

/// Colour used for everything indicating a selection — the tint on selected
/// strokes, the lasso, and the bounding box — so they read as one system.
const SELECTION_HUE: Color = Color::from_rgb(0.55, 0.78, 0.98);

/// Everything the surface needs to draw one frame.
pub struct InkSurface<'a> {
    pub document: &'a InkDocument,
    /// The stroke currently under the pen, in page coordinates.
    pub live: &'a [InkPoint],
    /// The lasso being drawn, in page coordinates.
    pub lasso: &'a [Vec2],
    /// Indices of selected strokes.
    pub selection: &'a [usize],
    /// Page-space bounding box of the selection, if any.
    pub selection_bounds: Option<(Vec2, Vec2)>,
    pub cache: &'a canvas::Cache,
    pub camera: Camera,
    /// Base options for committed strokes; each stroke's kind refines them.
    pub options: StrokeOptions,
    /// Options for the stroke currently under the pen, already refined for the
    /// active tool.
    pub live_options: StrokeOptions,
    pub color: Color,
    pub tool: Tool,
    pub eraser_radius: f32,
    /// A stroke is currently being laid down, by pen or mouse.
    pub drawing: bool,
    /// Where the pen is hovering, in surface-local screen pixels.
    pub hover: Option<Vec2>,
    /// The radial menu, while it is open.
    pub radial: Option<super::radial::RadialMenu>,
}

/// Tracks in-progress *mouse* interaction. Pen input never reaches here — it is
/// captured at the window level — so this only backs the fallback path plus
/// view navigation, which is mouse/trackpad driven either way.
#[derive(Debug, Default)]
pub struct MouseState {
    drawing: bool,
    panning: bool,
    last_cursor: Option<Point>,
}

pub fn view<'a>(surface: InkSurface<'a>) -> Element<'a, Message, Theme, Renderer> {
    canvas(surface)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

impl canvas::Program<Message> for InkSurface<'_> {
    type State = MouseState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::Event;
        use mouse::Button::{Left, Middle};
        use mouse::Event::{ButtonPressed, ButtonReleased, CursorMoved, WheelScrolled};

        // Handled before the position check: a drag that ends outside the
        // surface must still finish cleanly.
        match event {
            Event::Mouse(ButtonReleased(Left)) if state.drawing => {
                state.drawing = false;
                return Some(canvas::Action::publish(Message::InkPointerUp).and_capture());
            }
            Event::Mouse(ButtonReleased(Middle)) if state.panning => {
                state.panning = false;
                return Some(canvas::Action::capture());
            }
            _ => {}
        }

        match event {
            Event::Mouse(WheelScrolled { delta }) => {
                let position = cursor.position_in(bounds)?;
                let notches = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    // Pixel deltas are far finer grained; scale them into
                    // something comparable to a wheel notch.
                    mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                };
                Some(
                    canvas::Action::publish(Message::InkZoom {
                        x: position.x,
                        y: position.y,
                        notches,
                    })
                    .and_capture(),
                )
            }
            Event::Mouse(ButtonPressed(Middle)) => {
                let position = cursor.position_in(bounds)?;
                state.panning = true;
                state.last_cursor = Some(position);
                Some(canvas::Action::capture())
            }
            Event::Mouse(ButtonPressed(Left)) => {
                let position = cursor.position_in(bounds)?;
                state.drawing = true;
                state.last_cursor = Some(position);
                Some(
                    canvas::Action::publish(Message::InkPointerDown(position.x, position.y))
                        .and_capture(),
                )
            }
            Event::Mouse(CursorMoved { .. }) => {
                let position = cursor.position_in(bounds)?;
                let previous = state.last_cursor.replace(position);

                if state.panning {
                    let last = previous.unwrap_or(position);
                    return Some(
                        canvas::Action::publish(Message::InkPan {
                            dx: position.x - last.x,
                            dy: position.y - last.y,
                        })
                        .and_capture(),
                    );
                }
                if state.drawing {
                    return Some(
                        canvas::Action::publish(Message::InkPointerMove(position.x, position.y))
                            .and_capture(),
                    );
                }
                // Feed the mouse into the same hover state the pen uses, so
                // the aiming indicators track whichever device moved last.
                Some(canvas::Action::publish(Message::InkHoverMoved(
                    position.x, position.y,
                )))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        // Unused: pen input never reaches iced, so every aiming indicator is
        // positioned from `hover` instead.
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // Pen samples arrive from the window procedure, not through iced, so
        // they carry window-client coordinates. Recording the surface's
        // position here — the one place it is reliably known — lets the app map
        // them into surface-local space.
        super::set_viewport(bounds);

        let visible = self
            .camera
            .visible_page_box(Vec2::new(bounds.width, bounds.height));
        let visible = (
            Vec2::new(visible.0.x - CULL_MARGIN, visible.0.y - CULL_MARGIN),
            Vec2::new(visible.1.x + CULL_MARGIN, visible.1.y + CULL_MARGIN),
        );

        let committed = self.cache.draw(renderer, bounds.size(), |frame| {
            self.with_camera(frame, |frame| {
                draw_paper(frame, self.document.paper, visible, self.camera.zoom);

                // Two passes so highlighter always lands under writing,
                // whatever order the strokes were drawn in. Within each pass
                // the document's own order is preserved.
                for pass in [StrokeKind::Highlighter, StrokeKind::Pen] {
                    for (index, stroke) in self.document.strokes().iter().enumerate() {
                        if stroke.kind != pass {
                            continue;
                        }
                        // Skip anything the viewport cannot show; on a long
                        // page this is most of the document.
                        if let Some(stroke_box) = stroke.path_bounds() {
                            if !stroke::boxes_overlap(stroke_box, visible) {
                                continue;
                            }
                        }

                        let selected = self.selection.contains(&index);
                        let color = if selected {
                            // Tint the selection rather than outlining it: an
                            // outline around handwriting is unreadable.
                            SELECTION_HUE
                        } else {
                            Color::from_rgba(
                                stroke.color[0],
                                stroke.color[1],
                                stroke.color[2],
                                stroke.color[3],
                            )
                        };

                        let options = super::options_for(stroke.kind, self.options);
                        fill_stroke(frame, &stroke.ink_points(), stroke.size, color, &options);
                    }
                }
            });
        });

        let mut overlay = canvas::Frame::new(renderer, bounds.size());

        self.with_camera(&mut overlay, |frame| {
            if !self.live.is_empty() {
                fill_stroke(
                    frame,
                    self.live,
                    self.live_options.size,
                    self.color,
                    &self.live_options,
                );
            }

            if self.lasso.len() >= 2 {
                let path = canvas::Path::new(|builder| {
                    builder.move_to(Point::new(self.lasso[0].x, self.lasso[0].y));
                    for vertex in &self.lasso[1..] {
                        builder.line_to(Point::new(vertex.x, vertex.y));
                    }
                    builder.close();
                });
                frame.fill(&path, Color { a: 0.10, ..SELECTION_HUE });
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(Color { a: 0.85, ..SELECTION_HUE })
                        // Keep the outline one screen pixel wide whatever the
                        // zoom, since the frame is scaled.
                        .with_width(1.0 / self.camera.zoom.max(0.001)),
                );
            }
        });

        // The selection box is drawn in screen space so its outline stays one
        // pixel wide at any zoom, which is what makes it read as a UI
        // affordance rather than as more ink.
        if let Some((min, max)) = self.selection_bounds {
            let top_left = self.camera.to_screen(min);
            let bottom_right = self.camera.to_screen(max);
            let pad = 4.0;
            let rect = Rectangle {
                x: top_left.x - pad,
                y: top_left.y - pad,
                width: (bottom_right.x - top_left.x) + pad * 2.0,
                height: (bottom_right.y - top_left.y) + pad * 2.0,
            };
            overlay.stroke(
                &canvas::Path::rectangle(
                    Point::new(rect.x, rect.y),
                    iced::Size::new(rect.width, rect.height),
                ),
                canvas::Stroke::default()
                    .with_color(Color { a: 0.9, ..SELECTION_HUE })
                    .with_width(1.0),
            );
        }

        // Drawn outside the camera transform so the ring stays the same size
        // on screen regardless of zoom — it indicates a screen-space radius.
        // Positioned from `hover` rather than iced's cursor: pen input never
        // reaches iced, so its cursor keeps returning the last place the
        // *mouse* was and the ring would sit frozen there.
        if self.tool == Tool::Eraser {
            if let Some(position) = self.hover {
                overlay.stroke(
                    &canvas::Path::circle(Point::new(position.x, position.y), self.eraser_radius),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.85, 0.85, 0.9, 0.55))
                        .with_width(1.0),
                );
            }
        }

        // Where the nib will land. On an opaque tablet this is the whole
        // aiming mechanism, and it shows the *mark* rather than a pointer:
        // sized to the current brush at the current zoom.
        if let (Some(position), false) = (self.hover, self.drawing) {
            if self.tool != Tool::Eraser && self.radial.is_none() {
                let radius = (self.live_options.size * 0.5 * self.camera.zoom).max(2.0);
                let centre = Point::new(position.x, position.y);
                overlay.fill(
                    &canvas::Path::circle(centre, radius),
                    Color {
                        a: 0.35,
                        ..self.color
                    },
                );
                overlay.stroke(
                    &canvas::Path::circle(centre, radius + 1.0),
                    canvas::Stroke::default()
                        .with_color(Color { a: 0.7, ..self.color })
                        .with_width(1.0),
                );
            }
        }

        if let Some(menu) = self.radial {
            draw_radial(&mut overlay, &menu);
        }

        vec![committed, overlay.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.panning {
            return mouse::Interaction::Grabbing;
        }

        // While a stroke is being laid down the nib is the pointer. On an
        // absolute-mapped tablet the system cursor tracks the pen exactly, so
        // leaving it visible parks an arrow on top of the ink being drawn.
        if self.drawing {
            return mouse::Interaction::Hidden;
        }

        if !cursor.is_over(bounds) {
            return mouse::Interaction::None;
        }

        match self.tool {
            // The eraser draws its own ring showing what it will take; a
            // second cursor on top of that is just noise.
            Tool::Eraser => mouse::Interaction::Hidden,
            _ => mouse::Interaction::Crosshair,
        }
    }
}

impl InkSurface<'_> {
    /// Run `f` with the frame transformed from page space into screen space.
    fn with_camera(&self, frame: &mut canvas::Frame, f: impl FnOnce(&mut canvas::Frame)) {
        frame.with_save(|frame| {
            frame.translate(Vector::new(self.camera.pan.x, self.camera.pan.y));
            frame.scale(self.camera.zoom);
            f(frame);
        });
    }
}

/// Spacing between ruled lines, in page units. Roughly the height of
/// comfortable handwriting at the default stroke size.
const RULE_SPACING: f32 = 52.0;

/// Grid and dot spacing, in page units. Tighter than the ruled lines because a
/// grid has to work in both axes, but still generous enough to write between.
const GRID_SPACING: f32 = 38.0;

/// Ruling closer together than this on screen stops reading as a ruled page
/// and starts reading as a grey haze — while costing thousands of primitives
/// to draw. Below it the paper is simply omitted.
const MIN_RULE_SCREEN_SPACING: f32 = 8.0;

/// Draw the page ruling across the visible region.
///
/// Only the lines inside `visible` are emitted — the page is unbounded, so
/// drawing the ruling everywhere is not an option.
fn draw_paper(frame: &mut canvas::Frame, paper: PaperStyle, visible: (Vec2, Vec2), zoom: f32) {
    let spacing = match paper {
        PaperStyle::Plain => return,
        PaperStyle::Ruled => RULE_SPACING,
        PaperStyle::Grid | PaperStyle::Dotted => GRID_SPACING,
    };

    let zoom = zoom.max(0.001);
    if spacing * zoom < MIN_RULE_SCREEN_SPACING {
        return;
    }

    let (min, max) = visible;
    let line_color = Color::from_rgba(1.0, 1.0, 1.0, 0.07);
    let dot_color = Color::from_rgba(1.0, 1.0, 1.0, 0.16);

    // The frame is scaled by the camera, so a page-space width of 1 renders as
    // `zoom` screen pixels. Dividing keeps the ruling a hairline at every zoom;
    // anything thicker competes with the handwriting.
    let width = 1.0 / zoom;

    let horizontals = |frame: &mut canvas::Frame| {
        let mut y = (min.y / spacing).floor() * spacing;
        while y <= max.y {
            frame.stroke(
                &canvas::Path::line(Point::new(min.x, y), Point::new(max.x, y)),
                canvas::Stroke::default()
                    .with_color(line_color)
                    .with_width(width),
            );
            y += spacing;
        }
    };

    match paper {
        PaperStyle::Plain => {}
        PaperStyle::Ruled => horizontals(frame),
        PaperStyle::Grid => {
            horizontals(frame);
            let mut x = (min.x / spacing).floor() * spacing;
            while x <= max.x {
                frame.stroke(
                    &canvas::Path::line(Point::new(x, min.y), Point::new(x, max.y)),
                    canvas::Stroke::default()
                        .with_color(line_color)
                        .with_width(width),
                );
                x += spacing;
            }
        }
        PaperStyle::Dotted => {
            // Squares rather than circles: at one or two pixels the shapes are
            // indistinguishable, and a rectangle is two triangles against a
            // circle's tessellated fan — which matters when a zoomed-out view
            // holds tens of thousands of them.
            let size = 1.6 / zoom;
            let offset = size * 0.5;
            let mut y = (min.y / spacing).floor() * spacing;
            while y <= max.y {
                let mut x = (min.x / spacing).floor() * spacing;
                while x <= max.x {
                    frame.fill_rectangle(
                        Point::new(x - offset, y - offset),
                        iced::Size::new(size, size),
                        dot_color,
                    );
                    x += spacing;
                }
                y += spacing;
            }
        }
    }
}

/// Draw the radial menu: a tool ring inside a colour ring, with whichever slot
/// the pen is over lit up.
fn draw_radial(frame: &mut canvas::Frame, menu: &super::radial::RadialMenu) {
    use super::radial::{self, RadialAction};

    let centre = Point::new(menu.center.x, menu.center.y);
    let highlighted = menu.highlighted();

    // A dimmed backing disc so the menu reads against whatever is beneath it.
    frame.fill(
        &canvas::Path::circle(centre, radial::OUTER_LIMIT * 0.86),
        Color::from_rgba(0.06, 0.07, 0.08, 0.82),
    );
    frame.stroke(
        &canvas::Path::circle(centre, radial::DEAD_ZONE),
        canvas::Stroke::default()
            .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.25))
            .with_width(1.0),
    );

    for (index, (label, tool)) in radial::TOOLS.iter().enumerate() {
        let at = radial::slot_position(
            menu.center,
            index,
            radial::TOOLS.len(),
            radial::TOOL_RADIUS,
        );
        let lit = highlighted == Some(RadialAction::Tool(*tool));

        frame.fill(
            &canvas::Path::circle(Point::new(at.x, at.y), 22.0),
            if lit {
                Color::from_rgba(0.55, 0.78, 0.98, 0.95)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.10)
            },
        );
        frame.fill_text(canvas::Text {
            content: (*label).to_string(),
            position: Point::new(at.x, at.y),
            color: if lit {
                Color::from_rgb(0.05, 0.06, 0.07)
            } else {
                Color::from_rgb(0.88, 0.89, 0.92)
            },
            size: 11.0.into(),
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
    }

    for (index, (_, preset)) in super::COLOR_PRESETS.iter().enumerate() {
        let at = radial::slot_position(
            menu.center,
            index,
            super::COLOR_PRESETS.len(),
            radial::COLOR_RADIUS,
        );
        let lit = highlighted == Some(RadialAction::Color(*preset));
        let point = Point::new(at.x, at.y);

        frame.fill(&canvas::Path::circle(point, if lit { 15.0 } else { 11.0 }), *preset);
        if lit {
            frame.stroke(
                &canvas::Path::circle(point, 18.0),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.95, 0.96, 0.98))
                    .with_width(2.0),
            );
        }
    }
}

/// Tessellate one stroke's variable-width outline and fill it.
fn fill_stroke(
    frame: &mut canvas::Frame,
    points: &[InkPoint],
    size: f32,
    color: Color,
    options: &StrokeOptions,
) {
    let options = StrokeOptions { size, ..*options };
    let polygon = stroke::outline(points, &options);

    // Fewer than three vertices encloses no area, so there is nothing to fill.
    if polygon.len() < 3 {
        return;
    }

    let path = canvas::Path::new(|builder| {
        builder.move_to(Point::new(polygon[0].x, polygon[0].y));
        for vertex in &polygon[1..] {
            builder.line_to(Point::new(vertex.x, vertex.y));
        }
        builder.close();
    });

    frame.fill(&path, color);
}
