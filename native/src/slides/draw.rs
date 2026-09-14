//! Painting prepared slides onto iced canvases.
//!
//! Positions in a deck are points; a canvas is however many pixels wide the
//! viewer shows the slide at, so every coordinate is scaled on the way out.
//! The same prepared slide therefore paints crisply at any zoom, and each
//! canvas's cache keeps its geometry until the slide's size changes.
//!
//! A slide is several canvases stacked ([`SlidePart`]); see
//! [`super::layer_ranges`] for why.

use std::cell::Cell;
use std::sync::Arc;

use iced::advanced::text::{Alignment, LineHeight, Shaping};
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, LineCap, LineDash, LineJoin, Path, Stroke};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};
use md_editor_core::pptx::{
    Dash, Element, ElementKind, Fill, Gradient, Line, PathCommand, Rgba, Shape, ShapePath, Table,
    TextBody,
};

use super::text::{LaidTable, LaidText, anchor_offset};
use super::{LoadedDeck, Prepared};

/// The box standing in for content the viewer cannot draw. It paints on the
/// slide, over the deck's own colours, so it uses translucent neutral greys
/// rather than the app's chrome palette.
const UNSUPPORTED_FILL: Color = Color::from_rgba(0.5, 0.5, 0.5, 0.12);
const UNSUPPORTED_EDGE: Color = Color::from_rgba(0.35, 0.35, 0.35, 0.6);
const UNSUPPORTED_TEXT: Color = Color::from_rgba(0.25, 0.25, 0.25, 0.9);
/// Label size in that box, in points on the slide.
const UNSUPPORTED_LABEL_SIZE: f32 = 12.0;
/// Arrowhead length as a multiple of its line's width.
const ARROW_LENGTH: f32 = 3.0;
/// Shortest arrowhead, in points, so hairline arrows stay visible.
const MIN_ARROW_LENGTH: f32 = 4.0;

/// Which part of a slide a canvas paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlidePart {
    /// A picture background, which must sit under every layer's shapes.
    Background,
    Layer(usize),
}

/// One canvas of a slide.
pub struct SlideLayer {
    deck: Arc<LoadedDeck>,
    slide: usize,
    part: SlidePart,
}

impl SlideLayer {
    pub fn new(deck: Arc<LoadedDeck>, slide: usize, part: SlidePart) -> Self {
        Self { deck, slide, part }
    }
}

#[derive(Default)]
pub struct LayerCache {
    cache: canvas::Cache,
    /// What the cache holds. The widget tree reuses canvas state by
    /// position, so the same state can meet a different slide or deck.
    drawn: Cell<Option<(u64, usize, SlidePart)>>,
}

impl<Message> canvas::Program<Message> for SlideLayer {
    type State = LayerCache;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let key = Some((self.deck.id, self.slide, self.part));
        if state.drawn.get() != key {
            state.cache.clear();
            state.drawn.set(key);
        }
        vec![state.cache.draw(renderer, bounds.size(), |frame| {
            paint_part(frame, &self.deck, self.slide, self.part);
        })]
    }
}

fn paint_part(frame: &mut Frame<Renderer>, deck: &LoadedDeck, index: usize, part: SlidePart) {
    let (Some(slide), Some(prepared)) = (deck.slides.get(index), deck.prepared.get(index)) else {
        return;
    };
    let scale = frame.width() / deck.width.max(1.0);
    let page = Rectangle::new(Point::ORIGIN, frame.size());
    let page_path = Path::rectangle(Point::ORIGIN, frame.size());

    match part {
        SlidePart::Background => {
            frame.fill(&page_path, Color::WHITE);
            paint_fill(
                frame,
                &page_path,
                &slide.background,
                page,
                prepared.background.as_deref(),
                deck,
            );
        }
        SlidePart::Layer(layer) => {
            let Some(range) = prepared.layers.get(layer).cloned() else {
                return;
            };
            if layer == 0 && prepared.background.is_none() {
                frame.fill(&page_path, Color::WHITE);
                paint_fill(frame, &page_path, &slide.background, page, None, deck);
            }
            for (element, prepared) in slide.elements[range.clone()]
                .iter()
                .zip(&prepared.elements[range])
            {
                paint_element(frame, element, prepared, deck, scale);
            }
        }
    }
}

fn paint_element(
    frame: &mut Frame<Renderer>,
    element: &Element,
    prepared: &Prepared,
    deck: &LoadedDeck,
    scale: f32,
) {
    let bounds = element.bounds;
    let size = Size::new(bounds.width * scale, bounds.height * scale);
    frame.with_save(|frame| {
        // Origin at the element's top-left corner, rotated about its centre.
        frame.translate(Vector::new(
            (bounds.x + bounds.width / 2.0) * scale,
            (bounds.y + bounds.height / 2.0) * scale,
        ));
        if element.rotation.abs() > f32::EPSILON {
            frame.rotate(element.rotation.to_radians());
        }
        frame.translate(Vector::new(-size.width / 2.0, -size.height / 2.0));
        let area = Rectangle::new(Point::ORIGIN, size);

        match (&element.kind, prepared) {
            (ElementKind::Shape(shape), Prepared::Shape { text, image }) => {
                paint_shape(frame, element, shape, image.as_deref(), deck, scale);
                if let (Some(body), Some(laid)) = (&shape.text, text) {
                    paint_text(frame, body, laid, Point::ORIGIN, bounds.height, scale);
                }
            }
            (ElementKind::Picture(picture), Prepared::Picture(key)) => {
                if let Some(handle) = key.as_deref().and_then(|key| deck.images.get(key)) {
                    frame.draw_image(area, canvas::Image::new(handle.clone()));
                }
                if let Some(line) = &picture.line {
                    let dashes = dash_pattern(line, scale);
                    frame.stroke(
                        &Path::rectangle(Point::ORIGIN, size),
                        stroke(line, scale, &dashes),
                    );
                }
            }
            (ElementKind::Table(table), Prepared::Table(laid)) => {
                paint_table(frame, table, laid, scale);
            }
            (ElementKind::Unsupported { label }, _) => {
                paint_unsupported(frame, label, area, scale);
            }
            _ => {}
        }
    });
}

fn paint_shape(
    frame: &mut Frame<Renderer>,
    element: &Element,
    shape: &Shape,
    image: Option<&str>,
    deck: &LoadedDeck,
    scale: f32,
) {
    let (width, height) = (element.bounds.width, element.bounds.height);
    let area = Rectangle::new(Point::ORIGIN, Size::new(width * scale, height * scale));
    for shape_path in &shape.paths {
        // Flips mirror the outline, never the text, so they are applied to
        // the path's points rather than to the frame.
        let map = |x: f32, y: f32| {
            Point::new(
                if element.flip_h { width - x } else { x } * scale,
                if element.flip_v { height - y } else { y } * scale,
            )
        };
        let path = Path::new(|builder| {
            for command in &shape_path.commands {
                match *command {
                    PathCommand::MoveTo(x, y) => builder.move_to(map(x, y)),
                    PathCommand::LineTo(x, y) => builder.line_to(map(x, y)),
                    PathCommand::CubicTo(ax, ay, bx, by, x, y) => {
                        builder.bezier_curve_to(map(ax, ay), map(bx, by), map(x, y))
                    }
                    PathCommand::QuadTo(ax, ay, x, y) => {
                        builder.quadratic_curve_to(map(ax, ay), map(x, y))
                    }
                    PathCommand::Close => builder.close(),
                }
            }
        });
        if shape_path.filled {
            paint_fill(frame, &path, &shape.fill, area, image, deck);
        }
        if shape_path.stroked
            && let Some(line) = &shape.line
        {
            let dashes = dash_pattern(line, scale);
            frame.stroke(&path, stroke(line, scale, &dashes));
            paint_arrowheads(frame, shape_path, line, &map, scale);
        }
    }
}

fn paint_fill(
    frame: &mut Frame<Renderer>,
    path: &Path,
    fill: &Fill,
    area: Rectangle,
    image: Option<&str>,
    deck: &LoadedDeck,
) {
    match fill {
        Fill::None => {}
        Fill::Solid(rgba) => frame.fill(path, color(*rgba)),
        Fill::Gradient(gradient) => frame.fill(path, linear_gradient(gradient, area)),
        Fill::Picture { .. } => {
            if let Some(handle) = image.and_then(|key| deck.images.get(key)) {
                frame.draw_image(area, canvas::Image::new(handle.clone()));
            }
        }
    }
}

fn linear_gradient(gradient: &Gradient, area: Rectangle) -> canvas::Fill {
    // iced has no radial fill; a radial gradient runs top to bottom instead,
    // which keeps its colours and the direction light usually falls.
    let angle = if gradient.radial {
        90.0_f32
    } else {
        gradient.angle
    }
    .to_radians();
    let (dx, dy) = (angle.cos(), angle.sin());
    let reach = (area.width * dx.abs() + area.height * dy.abs()) / 2.0;
    let center = area.center();
    let mut linear = canvas::gradient::Linear::new(
        Point::new(center.x - dx * reach, center.y - dy * reach),
        Point::new(center.x + dx * reach, center.y + dy * reach),
    );
    for stop in &gradient.stops {
        linear = linear.add_stop(stop.offset, color(stop.color));
    }
    linear.into()
}

fn dash_pattern(line: &Line, scale: f32) -> Vec<f32> {
    let w = (line.width * scale).max(1.0);
    match line.dash {
        Dash::Solid => Vec::new(),
        Dash::Dot => vec![w, w * 2.0],
        Dash::Dash => vec![w * 4.0, w * 3.0],
        Dash::LongDash => vec![w * 8.0, w * 3.0],
        Dash::DashDot => vec![w * 4.0, w * 3.0, w, w * 3.0],
    }
}

fn stroke<'a>(line: &Line, scale: f32, dashes: &'a [f32]) -> Stroke<'a> {
    Stroke {
        line_dash: LineDash {
            segments: dashes,
            offset: 0,
        },
        ..Stroke::default()
            .with_color(color(line.color))
            .with_width((line.width * scale).max(0.5))
            .with_line_cap(LineCap::Butt)
            .with_line_join(LineJoin::Miter)
    }
}

/// Arrowheads at the ends of an open path: the head at its first point, the
/// tail at its last, as DrawingML names them.
fn paint_arrowheads(
    frame: &mut Frame<Renderer>,
    path: &ShapePath,
    line: &Line,
    map: &dyn Fn(f32, f32) -> Point,
    scale: f32,
) {
    if !line.head_arrow && !line.tail_arrow {
        return;
    }
    let points: Vec<Point> = path
        .commands
        .iter()
        .filter_map(|command| match *command {
            PathCommand::MoveTo(x, y)
            | PathCommand::LineTo(x, y)
            | PathCommand::CubicTo(_, _, _, _, x, y)
            | PathCommand::QuadTo(_, _, x, y) => Some(map(x, y)),
            PathCommand::Close => None,
        })
        .collect();
    let [first, second, ..] = points[..] else {
        return;
    };
    let length = (line.width * ARROW_LENGTH).max(MIN_ARROW_LENGTH) * scale;
    let fill = color(line.color);
    if line.head_arrow {
        arrowhead(frame, second, first, length, fill);
    }
    if line.tail_arrow {
        arrowhead(
            frame,
            points[points.len() - 2],
            points[points.len() - 1],
            length,
            fill,
        );
    }
}

fn arrowhead(frame: &mut Frame<Renderer>, from: Point, tip: Point, length: f32, fill: Color) {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let distance = dx.hypot(dy);
    if distance < f32::EPSILON {
        return;
    }
    let (ux, uy) = (dx / distance, dy / distance);
    let base = Point::new(tip.x - ux * length, tip.y - uy * length);
    let (nx, ny) = (-uy * length / 2.0, ux * length / 2.0);
    let head = Path::new(|builder| {
        builder.move_to(tip);
        builder.line_to(Point::new(base.x + nx, base.y + ny));
        builder.line_to(Point::new(base.x - nx, base.y - ny));
        builder.close();
    });
    frame.fill(&head, fill);
}

/// Paint laid-out text whose box has its top-left corner at `origin` and is
/// `box_height` points tall.
fn paint_text(
    frame: &mut Frame<Renderer>,
    body: &TextBody,
    laid: &LaidText,
    origin: Point,
    box_height: f32,
    scale: f32,
) {
    let top = anchor_offset(body, box_height, laid.height);
    let at = |x: f32, y: f32| {
        Point::new(
            origin.x + (body.insets.left + x) * scale,
            origin.y + (top + y) * scale,
        )
    };
    for mark in &laid.highlights {
        frame.fill_rectangle(
            at(mark.x, mark.y),
            Size::new(mark.width * scale, mark.height * scale),
            color(mark.color),
        );
    }
    for run in &laid.runs {
        frame.fill_text(canvas::Text {
            content: run.text.clone(),
            position: at(run.x, run.y),
            max_width: f32::INFINITY,
            color: color(run.color),
            size: Pixels(run.size * scale),
            line_height: LineHeight::Relative(1.0),
            font: run.font,
            align_x: Alignment::Left,
            align_y: Vertical::Top,
            shaping: Shaping::Advanced,
        });
    }
    for rule in &laid.rules {
        frame.fill_rectangle(
            at(rule.x, rule.y),
            Size::new(rule.width * scale, (rule.height * scale).max(1.0)),
            color(rule.color),
        );
    }
}

fn paint_table(frame: &mut Frame<Renderer>, table: &Table, laid: &LaidTable, scale: f32) {
    let columns = table.columns.len();
    let rows = table.rows.len();
    let cell_rect = |r: usize, c: usize, row_span: usize, column_span: usize| {
        let (right, bottom) = ((c + column_span).min(columns), (r + row_span).min(rows));
        let (x0, x1) = (laid.column_x[c] * scale, laid.column_x[right] * scale);
        let (y0, y1) = (laid.row_y[r] * scale, laid.row_y[bottom] * scale);
        Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
    };
    let cells = || {
        table.rows.iter().enumerate().flat_map(|(r, row)| {
            row.cells
                .iter()
                .enumerate()
                .filter(move |(c, cell)| !cell.merged && *c < columns)
                .map(move |(c, cell)| (r, c, cell))
        })
    };

    for (r, c, cell) in cells() {
        let rect = cell_rect(r, c, cell.row_span, cell.column_span);
        let path = Path::rectangle(rect.position(), rect.size());
        match &cell.fill {
            Fill::Solid(rgba) => frame.fill(&path, color(*rgba)),
            Fill::Gradient(gradient) => frame.fill(&path, linear_gradient(gradient, rect)),
            Fill::None | Fill::Picture { .. } => {}
        }
        if let (Some(body), Some(Some(text))) =
            (&cell.text, laid.cells.get(r).and_then(|row| row.get(c)))
        {
            paint_text(
                frame,
                body,
                text,
                rect.position(),
                rect.height / scale,
                scale,
            );
        }
    }
    // Borders go on after every fill, so a neighbour's fill cannot cover them.
    for (r, c, cell) in cells() {
        let rect = cell_rect(r, c, cell.row_span, cell.column_span);
        let (left, top) = (rect.x, rect.y);
        let (right, bottom) = (rect.x + rect.width, rect.y + rect.height);
        let edges = [
            (&cell.borders.left, (left, top), (left, bottom)),
            (&cell.borders.top, (left, top), (right, top)),
            (&cell.borders.right, (right, top), (right, bottom)),
            (&cell.borders.bottom, (left, bottom), (right, bottom)),
        ];
        for (line, from, to) in edges {
            if let Some(line) = line {
                let dashes = dash_pattern(line, scale);
                frame.stroke(
                    &Path::line(Point::new(from.0, from.1), Point::new(to.0, to.1)),
                    stroke(line, scale, &dashes),
                );
            }
        }
    }
}

fn paint_unsupported(frame: &mut Frame<Renderer>, label: &str, area: Rectangle, scale: f32) {
    let path = Path::rectangle(area.position(), area.size());
    frame.fill(&path, UNSUPPORTED_FILL);
    let dash = (3.0 * scale).max(2.0);
    let dashes = [dash * 1.5, dash];
    frame.stroke(
        &path,
        Stroke {
            line_dash: LineDash {
                segments: &dashes,
                offset: 0,
            },
            ..Stroke::default()
                .with_color(UNSUPPORTED_EDGE)
                .with_width(1.0)
        },
    );
    frame.fill_text(canvas::Text {
        content: label.to_string(),
        position: area.center(),
        max_width: f32::INFINITY,
        color: UNSUPPORTED_TEXT,
        size: Pixels((UNSUPPORTED_LABEL_SIZE * scale).max(8.0)),
        line_height: LineHeight::Relative(1.2),
        font: iced::Font::DEFAULT,
        align_x: Alignment::Center,
        align_y: Vertical::Center,
        shaping: Shaping::Advanced,
    });
}

fn color(rgba: Rgba) -> Color {
    Color::from_rgba8(rgba.r, rgba.g, rgba.b, f32::from(rgba.a) / 255.0)
}
