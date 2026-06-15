use iced::widget::{canvas, container};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Back,
    Close,
    Command,
    File,
    Find,
    FitWidth,
    FitPage,
    Folder,
    Forward,
    Help,
    ListTree,
    NewFolder,
    NewNote,
    Pdf,
    Redo,
    Refresh,
    Save,
    Search,
    Settings,
    Sidebar,
    Split,
    SplitDown,
    Tracker,
    Undo,
    ZoomIn,
    ZoomOut,
}

pub fn view<'a, Message: 'a>(
    icon: Icon,
    color: Color,
    size: f32,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        canvas(IconCanvas { icon, color })
            .width(Length::Fixed(size))
            .height(Length::Fixed(size)),
    )
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

#[derive(Debug, Clone, Copy)]
struct IconCanvas {
    icon: Icon,
    color: Color,
}

impl<Message> canvas::Program<Message> for IconCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let scale = bounds.width.min(bounds.height) / 24.0;
        let p = |x: f32, y: f32| Point::new(x * scale, y * scale);
        let stroke = canvas::Stroke::default()
            .with_color(self.color)
            .with_width((2.0 * scale).max(1.25))
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);

        match self.icon {
            Icon::Search | Icon::Find => {
                frame.stroke(&canvas::Path::circle(p(10.0, 10.0), 5.5 * scale), stroke);
                line(&mut frame, p(14.5, 14.5), p(20.0, 20.0), stroke);
            }
            Icon::Folder => {
                let path = canvas::Path::new(|path| {
                    path.move_to(p(3.0, 7.0));
                    path.line_to(p(9.0, 7.0));
                    path.line_to(p(11.0, 9.0));
                    path.line_to(p(21.0, 9.0));
                    path.line_to(p(21.0, 19.0));
                    path.line_to(p(3.0, 19.0));
                    path.close();
                });
                frame.stroke(&path, stroke);
            }
            Icon::File | Icon::Save => {
                let path = canvas::Path::new(|path| {
                    path.move_to(p(6.0, 3.0));
                    path.line_to(p(16.0, 3.0));
                    path.line_to(p(20.0, 7.0));
                    path.line_to(p(20.0, 21.0));
                    path.line_to(p(6.0, 21.0));
                    path.close();
                });
                frame.stroke(&path, stroke);
                if matches!(self.icon, Icon::Save) {
                    line(&mut frame, p(9.0, 4.0), p(9.0, 10.0), stroke);
                    line(&mut frame, p(9.0, 16.0), p(17.0, 16.0), stroke);
                }
            }
            Icon::Settings => {
                frame.stroke(&canvas::Path::circle(p(12.0, 12.0), 3.2 * scale), stroke);
                for (x1, y1, x2, y2) in [
                    (12.0, 2.0, 12.0, 5.0),
                    (12.0, 19.0, 12.0, 22.0),
                    (2.0, 12.0, 5.0, 12.0),
                    (19.0, 12.0, 22.0, 12.0),
                    (5.0, 5.0, 7.1, 7.1),
                    (16.9, 16.9, 19.0, 19.0),
                    (5.0, 19.0, 7.1, 16.9),
                    (16.9, 7.1, 19.0, 5.0),
                ] {
                    line(&mut frame, p(x1, y1), p(x2, y2), stroke);
                }
            }
            Icon::Sidebar => {
                frame.stroke(
                    &canvas::Path::rounded_rectangle(
                        p(3.0, 4.0),
                        iced::Size::new(18.0 * scale, 16.0 * scale),
                        (2.0 * scale).into(),
                    ),
                    stroke,
                );
                line(&mut frame, p(9.0, 4.0), p(9.0, 20.0), stroke);
            }
            Icon::Split => {
                frame.stroke(
                    &canvas::Path::rounded_rectangle(
                        p(4.0, 5.0),
                        iced::Size::new(16.0 * scale, 14.0 * scale),
                        (2.0 * scale).into(),
                    ),
                    stroke,
                );
                line(&mut frame, p(12.0, 5.0), p(12.0, 19.0), stroke);
            }
            Icon::SplitDown => {
                frame.stroke(
                    &canvas::Path::rounded_rectangle(
                        p(4.0, 5.0),
                        iced::Size::new(16.0 * scale, 14.0 * scale),
                        (2.0 * scale).into(),
                    ),
                    stroke,
                );
                line(&mut frame, p(4.0, 12.0), p(20.0, 12.0), stroke);
            }
            Icon::Close => {
                line(&mut frame, p(6.0, 6.0), p(18.0, 18.0), stroke);
                line(&mut frame, p(18.0, 6.0), p(6.0, 18.0), stroke);
            }
            Icon::Tracker => {
                frame.stroke(&canvas::Path::circle(p(12.0, 12.0), 8.0 * scale), stroke);
                line(&mut frame, p(12.0, 8.0), p(12.0, 12.0), stroke);
                line(&mut frame, p(12.0, 12.0), p(16.0, 15.0), stroke);
            }
            Icon::ListTree => {
                for y in [6.0, 12.0, 18.0] {
                    line(&mut frame, p(8.0, y), p(21.0, y), stroke);
                }
                line(&mut frame, p(3.0, 6.0), p(3.0, 18.0), stroke);
                line(&mut frame, p(3.0, 18.0), p(6.0, 18.0), stroke);
            }
            Icon::Command => {
                frame.stroke(
                    &canvas::Path::rounded_rectangle(
                        p(4.0, 4.0),
                        iced::Size::new(16.0 * scale, 16.0 * scale),
                        (4.0 * scale).into(),
                    ),
                    stroke,
                );
                line(&mut frame, p(8.0, 8.0), p(16.0, 16.0), stroke);
                line(&mut frame, p(16.0, 8.0), p(8.0, 16.0), stroke);
            }
            Icon::Undo | Icon::Redo => {
                let (start, end) = if matches!(self.icon, Icon::Undo) {
                    (p(18.0, 8.0), p(6.0, 8.0))
                } else {
                    (p(6.0, 8.0), p(18.0, 8.0))
                };
                line(&mut frame, start, end, stroke);
                line(&mut frame, end, p(12.0, 3.0), stroke);
                frame.stroke(
                    &canvas::Path::new(|path| {
                        path.move_to(end);
                        path.quadratic_curve_to(p(12.0, 22.0), p(20.0, 14.0));
                    }),
                    stroke,
                );
            }
            Icon::Back | Icon::Forward => {
                let (a, b, c) = if matches!(self.icon, Icon::Back) {
                    (p(15.0, 5.0), p(8.0, 12.0), p(15.0, 19.0))
                } else {
                    (p(9.0, 5.0), p(16.0, 12.0), p(9.0, 19.0))
                };
                line(&mut frame, a, b, stroke);
                line(&mut frame, b, c, stroke);
            }
            Icon::ZoomIn | Icon::ZoomOut => {
                frame.stroke(&canvas::Path::circle(p(10.0, 10.0), 6.0 * scale), stroke);
                line(&mut frame, p(14.5, 14.5), p(20.0, 20.0), stroke);
                line(&mut frame, p(6.5, 10.0), p(13.5, 10.0), stroke);
                if matches!(self.icon, Icon::ZoomIn) {
                    line(&mut frame, p(10.0, 6.5), p(10.0, 13.5), stroke);
                }
            }
            Icon::Help => {
                frame.stroke(&canvas::Path::circle(p(12.0, 12.0), 9.0 * scale), stroke);
                frame.fill(
                    &canvas::Path::circle(p(12.0, 17.5), 1.0 * scale),
                    self.color,
                );
                frame.stroke(
                    &canvas::Path::new(|path| {
                        path.move_to(p(9.0, 8.5));
                        path.quadratic_curve_to(p(12.0, 4.0), p(15.0, 8.5));
                        path.quadratic_curve_to(p(15.0, 12.0), p(12.0, 14.0));
                    }),
                    stroke,
                );
            }
            Icon::FitWidth => {
                line(&mut frame, p(3.0, 4.0), p(3.0, 20.0), stroke);
                line(&mut frame, p(21.0, 4.0), p(21.0, 20.0), stroke);
                line(&mut frame, p(3.0, 12.0), p(21.0, 12.0), stroke);
                line(&mut frame, p(3.0, 12.0), p(7.0, 8.0), stroke);
                line(&mut frame, p(3.0, 12.0), p(7.0, 16.0), stroke);
                line(&mut frame, p(21.0, 12.0), p(17.0, 8.0), stroke);
                line(&mut frame, p(21.0, 12.0), p(17.0, 16.0), stroke);
            }
            Icon::FitPage => {
                let path = canvas::Path::new(|path| {
                    path.move_to(p(7.0, 6.0));
                    path.line_to(p(14.0, 6.0));
                    path.line_to(p(17.0, 9.0));
                    path.line_to(p(17.0, 18.0));
                    path.line_to(p(7.0, 18.0));
                    path.close();
                });
                frame.stroke(&path, stroke);
                line(&mut frame, p(4.0, 7.0), p(4.0, 4.0), stroke);
                line(&mut frame, p(4.0, 4.0), p(7.0, 4.0), stroke);
                line(&mut frame, p(17.0, 4.0), p(20.0, 4.0), stroke);
                line(&mut frame, p(20.0, 4.0), p(20.0, 7.0), stroke);
                line(&mut frame, p(4.0, 17.0), p(4.0, 20.0), stroke);
                line(&mut frame, p(4.0, 20.0), p(7.0, 20.0), stroke);
                line(&mut frame, p(17.0, 20.0), p(20.0, 20.0), stroke);
                line(&mut frame, p(20.0, 20.0), p(20.0, 17.0), stroke);
            }
            Icon::Pdf => {
                let path = canvas::Path::new(|path| {
                    path.move_to(p(6.0, 3.0));
                    path.line_to(p(15.0, 3.0));
                    path.line_to(p(19.0, 7.0));
                    path.line_to(p(19.0, 21.0));
                    path.line_to(p(6.0, 21.0));
                    path.close();
                });
                frame.stroke(&path, stroke);
                line(&mut frame, p(9.0, 12.0), p(16.0, 12.0), stroke);
                line(&mut frame, p(9.0, 15.5), p(16.0, 15.5), stroke);
                line(&mut frame, p(9.0, 18.5), p(13.0, 18.5), stroke);
            }
            Icon::NewNote => {
                let path = canvas::Path::new(|path| {
                    path.move_to(p(5.0, 3.0));
                    path.line_to(p(12.0, 3.0));
                    path.line_to(p(15.0, 6.0));
                    path.line_to(p(15.0, 18.0));
                    path.line_to(p(5.0, 18.0));
                    path.close();
                });
                frame.stroke(&path, stroke);
                line(&mut frame, p(18.0, 15.0), p(18.0, 23.0), stroke);
                line(&mut frame, p(14.0, 19.0), p(22.0, 19.0), stroke);
            }
            Icon::NewFolder => {
                let path = canvas::Path::new(|path| {
                    path.move_to(p(3.0, 6.0));
                    path.line_to(p(8.0, 6.0));
                    path.line_to(p(10.0, 8.0));
                    path.line_to(p(16.0, 8.0));
                    path.line_to(p(16.0, 16.0));
                    path.line_to(p(3.0, 16.0));
                    path.close();
                });
                frame.stroke(&path, stroke);
                line(&mut frame, p(19.0, 14.0), p(19.0, 22.0), stroke);
                line(&mut frame, p(15.0, 18.0), p(23.0, 18.0), stroke);
            }
            Icon::Refresh => {
                frame.stroke(
                    &canvas::Path::new(|path| {
                        path.move_to(p(19.0, 6.0));
                        path.quadratic_curve_to(p(20.0, 14.0), p(13.0, 18.0));
                        path.quadratic_curve_to(p(5.0, 21.0), p(4.0, 12.0));
                        path.quadratic_curve_to(p(4.0, 4.0), p(13.0, 4.0));
                    }),
                    stroke,
                );
                line(&mut frame, p(19.0, 6.0), p(13.5, 6.5), stroke);
                line(&mut frame, p(19.0, 6.0), p(18.5, 11.5), stroke);
            }
        }
        vec![frame.into_geometry()]
    }
}

fn line(frame: &mut canvas::Frame<Renderer>, from: Point, to: Point, stroke: canvas::Stroke<'_>) {
    frame.stroke(&canvas::Path::line(from, to), stroke);
}
