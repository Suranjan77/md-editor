//! Non-destructive PDF margin ink toolbar controls and per-page vector stroke canvas overlay.

use iced::widget::{Space, button, canvas, row};
use iced::{Alignment, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

use md_editor_core::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfInkStroke};

use crate::design::metrics::{BORDER_WIDTH, ControlSize, RADIUS_FULL, SPACE_1, SPACE_2};
use crate::design::tokens;
use crate::ink::stroke::{self, InkPoint, StrokeOptions};
use crate::messages::{Message, PdfMessage};
use crate::pdf_pane::PdfInkTool;
use crate::ui::{Icon, IconButton, TooltipPosition, with_tooltip};
use crate::views::pdf_viewer::annotation_hue;

#[cfg(test)]
mod tests;

/// Default stroke diameter as a fraction of page width (~2.3pt on US Letter).
pub const DEFAULT_INK_WIDTH_FRAC: f32 = 0.0038;
/// Default pressure assigned to mouse strokes on PDF pages.
const MOUSE_PRESSURE: f32 = 0.55;
/// Eraser hit radius in normalized page coordinates.
pub const ERASE_HIT_RADIUS_FRAC: f32 = 0.025;
/// Minimum screen distance between recorded pointer samples (logical pixels).
const MIN_SAMPLE_DIST_PX: f32 = 1.5;

/// Clamp normalized `[x_frac, y_frac, pressure]` samples into `[0.0..=1.0]` and drop non-finite points.
pub fn clamp_fraction_points(raw: &[[f32; 3]]) -> Vec<[f32; 3]> {
    raw.iter()
        .copied()
        .filter(|p| p[0].is_finite() && p[1].is_finite() && p[2].is_finite())
        .map(|p| {
            [
                p[0].clamp(0.0, 1.0),
                p[1].clamp(0.0, 1.0),
                p[2].clamp(0.05, 1.0),
            ]
        })
        .collect()
}

/// Whether normalized point `(x_frac, y_frac)` lies within `radius` of any segment of `stroke`.
pub fn hit_test_ink_stroke(stroke: &PdfInkStroke, x_frac: f32, y_frac: f32, radius: f32) -> bool {
    if stroke.points.is_empty() || !x_frac.is_finite() || !y_frac.is_finite() {
        return false;
    }
    let eff_radius = radius.max(stroke.size * 0.6);
    let r2 = eff_radius * eff_radius;
    if stroke.points.len() == 1 {
        let p = stroke.points[0];
        let dx = x_frac - p[0];
        let dy = y_frac - p[1];
        return dx * dx + dy * dy <= r2;
    }
    for pair in stroke.points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if dist_sq_to_segment(x_frac, y_frac, a[0], a[1], b[0], b[1]) <= r2 {
            return true;
        }
    }
    false
}

fn dist_sq_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq <= 1e-12 {
        let dx = px - ax;
        let dy = py - ay;
        return dx * dx + dy * dy;
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0);
    let cx = ax + abx * t;
    let cy = ay + aby * t;
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy
}

fn color_dot<'a>(
    color: PdfAnnotationColor,
    selected: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let hue = annotation_hue(color);
    let fill = hue.color(false);
    let size = ControlSize::Sm.height() - 6.0;
    let btn = button(Space::new())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .on_press(Message::Pdf(PdfMessage::InkColorSelected(color)))
        .style(move |_, status| {
            let t = tokens();
            button::Style {
                background: Some(iced::Background::Color(fill)),
                border: iced::Border {
                    color: if selected || matches!(status, button::Status::Hovered) {
                        t.fg_primary
                    } else {
                        t.border_control
                    },
                    width: if selected {
                        BORDER_WIDTH * 2.0
                    } else {
                        BORDER_WIDTH
                    },
                    radius: RADIUS_FULL.into(),
                },
                ..Default::default()
            }
        });
    with_tooltip(
        btn,
        &format!("Ink {}", hue.label().to_lowercase()),
        None,
        TooltipPosition::Bottom,
    )
}

/// Toolbar controls for PDF margin ink: Pen toggle, Eraser toggle, color swatches, and Undo.
pub fn toolbar_controls<'a>(
    ink_tool: Option<PdfInkTool>,
    ink_color: PdfAnnotationColor,
    has_ink: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let toggle_to = |tool: PdfInkTool| {
        if ink_tool == Some(tool) {
            None
        } else {
            Some(tool)
        }
    };
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        IconButton::new(Icon::Pen, "Margin ink pen")
            .size(ControlSize::Sm)
            .selected(ink_tool == Some(PdfInkTool::Pen))
            .on_press(Message::Pdf(PdfMessage::InkToolToggled(toggle_to(
                PdfInkTool::Pen,
            ))))
            .build(),
        IconButton::new(Icon::Eraser, "Erase margin ink")
            .size(ControlSize::Sm)
            .selected(ink_tool == Some(PdfInkTool::Eraser))
            .on_press(Message::Pdf(PdfMessage::InkToolToggled(toggle_to(
                PdfInkTool::Eraser,
            ))))
            .build(),
    ];

    if ink_tool.is_some() {
        for c in [
            PdfAnnotationColor::Blue,
            PdfAnnotationColor::Green,
            PdfAnnotationColor::Yellow,
            PdfAnnotationColor::Pink,
            PdfAnnotationColor::Orange,
        ] {
            items.push(color_dot(c, ink_color == c));
        }
    }

    if has_ink || ink_tool.is_some() {
        items.push(
            IconButton::new(Icon::Undo, "Undo margin ink")
                .size(ControlSize::Sm)
                .on_press_maybe(has_ink.then_some(Message::Pdf(PdfMessage::UndoInkStroke)))
                .build(),
        );
    }

    row(items)
        .spacing(SPACE_1)
        .padding([0.0, SPACE_2])
        .align_y(Alignment::Center)
        .into()
}

/// Interactive transparent canvas stacked above a rendered PDF page.
pub fn canvas_layer<'a>(
    page_index: u16,
    (w, h): (f32, f32),
    active_tool: Option<PdfInkTool>,
    ink_color: PdfAnnotationColor,
    annotations: Option<&'a Vec<PdfAnnotation>>,
    live_ink: Option<&'a (u16, Vec<[f32; 3]>)>,
) -> Element<'a, Message, Theme, Renderer> {
    let anns: &'a [PdfAnnotation] = annotations.map_or(&[], Vec::as_slice);
    canvas(PdfInkCanvas {
        page_index,
        active_tool,
        ink_color,
        annotations: anns,
        live_ink,
    })
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .into()
}

struct PdfInkCanvas<'a> {
    page_index: u16,
    active_tool: Option<PdfInkTool>,
    ink_color: PdfAnnotationColor,
    annotations: &'a [PdfAnnotation],
    live_ink: Option<&'a (u16, Vec<[f32; 3]>)>,
}

#[derive(Default, Debug, Clone)]
struct CanvasState {
    drawing: bool,
    erasing: bool,
    points: Vec<[f32; 3]>,
}

impl canvas::Program<Message> for PdfInkCanvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::Event;
        use mouse::Button::Left;
        use mouse::Event::{ButtonPressed, ButtonReleased, CursorMoved};

        let Some(tool) = self.active_tool else {
            state.drawing = false;
            state.erasing = false;
            state.points.clear();
            return None;
        };

        let w = bounds.width.max(1.0);
        let h = bounds.height.max(1.0);
        let normalize = |pos: Point| {
            [
                (pos.x / w).clamp(0.0, 1.0),
                (pos.y / h).clamp(0.0, 1.0),
                MOUSE_PRESSURE,
            ]
        };

        match (tool, event) {
            (PdfInkTool::Pen, Event::Mouse(ButtonReleased(Left))) if state.drawing => {
                state.drawing = false;
                let points = std::mem::take(&mut state.points);
                Some(
                    canvas::Action::publish(Message::Pdf(PdfMessage::InkStrokeCommitted {
                        page: self.page_index,
                        points,
                    }))
                    .and_capture(),
                )
            }
            (PdfInkTool::Pen, Event::Mouse(ButtonPressed(Left))) => {
                let pos = cursor.position_in(bounds)?;
                state.drawing = true;
                state.points = vec![normalize(pos)];
                Some(
                    canvas::Action::publish(Message::Pdf(PdfMessage::InkLiveUpdated(Some((
                        self.page_index,
                        state.points.clone(),
                    )))))
                    .and_capture(),
                )
            }
            (PdfInkTool::Pen, Event::Mouse(CursorMoved { .. })) if state.drawing => {
                let abs = cursor.position()?;
                let rel = Point::new(abs.x - bounds.x, abs.y - bounds.y);
                let next = normalize(rel);
                if let Some(prev) = state.points.last() {
                    let dx = (next[0] - prev[0]) * w;
                    let dy = (next[1] - prev[1]) * h;
                    if dx * dx + dy * dy < MIN_SAMPLE_DIST_PX * MIN_SAMPLE_DIST_PX {
                        return Some(canvas::Action::capture());
                    }
                }
                state.points.push(next);
                Some(
                    canvas::Action::publish(Message::Pdf(PdfMessage::InkLiveUpdated(Some((
                        self.page_index,
                        state.points.clone(),
                    )))))
                    .and_capture(),
                )
            }
            (PdfInkTool::Eraser, Event::Mouse(ButtonReleased(Left))) if state.erasing => {
                state.erasing = false;
                Some(canvas::Action::capture())
            }
            (PdfInkTool::Eraser, Event::Mouse(ButtonPressed(Left))) => {
                let pos = cursor.position_in(bounds)?;
                state.erasing = true;
                let pt = normalize(pos);
                Some(
                    canvas::Action::publish(Message::Pdf(PdfMessage::InkEraseAt {
                        page: self.page_index,
                        x_frac: pt[0],
                        y_frac: pt[1],
                    }))
                    .and_capture(),
                )
            }
            (PdfInkTool::Eraser, Event::Mouse(CursorMoved { .. })) if state.erasing => {
                let pos = cursor.position_in(bounds)?;
                let pt = normalize(pos);
                Some(
                    canvas::Action::publish(Message::Pdf(PdfMessage::InkEraseAt {
                        page: self.page_index,
                        x_frac: pt[0],
                        y_frac: pt[1],
                    }))
                    .and_capture(),
                )
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
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for ann in self.annotations {
            if ann.kind != PdfAnnotationKind::Ink {
                continue;
            }
            if let Some(ref stroke) = ann.ink_stroke {
                draw_ink_stroke(
                    &mut frame,
                    bounds.size(),
                    stroke,
                    annotation_hue(ann.color).color(false),
                );
            }
        }
        if let Some((page, pts)) = self.live_ink
            && *page == self.page_index
            && pts.len() >= 2
        {
            let live_stroke = PdfInkStroke {
                size: DEFAULT_INK_WIDTH_FRAC,
                points: pts.clone(),
            };
            draw_ink_stroke(
                &mut frame,
                bounds.size(),
                &live_stroke,
                annotation_hue(self.ink_color).color(false),
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.active_tool.is_some() && cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::None
        }
    }
}

fn draw_ink_stroke(
    frame: &mut canvas::Frame,
    size: iced::Size,
    stroke: &PdfInkStroke,
    color: iced::Color,
) {
    if stroke.points.len() < 2 {
        return;
    }
    let w = size.width.max(1.0);
    let h = size.height.max(1.0);
    let ink_points: Vec<InkPoint> = stroke
        .points
        .iter()
        .map(|p| InkPoint::new(p[0] * w, p[1] * h, p[2]))
        .collect();
    let options = StrokeOptions {
        size: (stroke.size * w).max(1.5),
        ..Default::default()
    };
    let polygon = stroke::outline(&ink_points, &options);
    if polygon.len() < 3 {
        return;
    }
    let path = canvas::Path::new(|builder| {
        builder.move_to(Point::new(polygon[0].x, polygon[0].y));
        for v in &polygon[1..] {
            builder.line_to(Point::new(v.x, v.y));
        }
        builder.close();
    });
    frame.fill(&path, color);
}
