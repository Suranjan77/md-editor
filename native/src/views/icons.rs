use iced::widget::{canvas, container};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Icon {
    Clock,
    Command,
    File,
    FileText,
    Folder,
    FolderOpen,
    Image,
    LayoutPanelLeft,
    ListTree,
    Search,
    ChevronLeft,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    Split,
    Trash,
    X,
}

pub(crate) fn view<'a, Message: 'a>(
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
        let s = bounds.width.min(bounds.height);
        let scale = s / 24.0;
        let p = |x: f32, y: f32| Point::new(x * scale, y * scale);
        let stroke = canvas::Stroke::default()
            .with_color(self.color)
            .with_width((2.0 * scale).max(1.25))
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);

        match self.icon {
            Icon::Search => {
                frame.stroke(&canvas::Path::circle(p(10.5, 10.5), 5.5 * scale), stroke);
                stroke_line(&mut frame, p(15.0, 15.0), p(21.0, 21.0), stroke);
            }
            Icon::X => {
                stroke_line(&mut frame, p(6.0, 6.0), p(18.0, 18.0), stroke);
                stroke_line(&mut frame, p(18.0, 6.0), p(6.0, 18.0), stroke);
            }
            Icon::ChevronUp => {
                stroke_line(&mut frame, p(6.0, 15.0), p(12.0, 9.0), stroke);
                stroke_line(&mut frame, p(12.0, 9.0), p(18.0, 15.0), stroke);
            }
            Icon::ChevronDown => {
                stroke_line(&mut frame, p(6.0, 9.0), p(12.0, 15.0), stroke);
                stroke_line(&mut frame, p(12.0, 15.0), p(18.0, 9.0), stroke);
            }
            Icon::ChevronLeft => {
                stroke_line(&mut frame, p(15.0, 6.0), p(9.0, 12.0), stroke);
                stroke_line(&mut frame, p(9.0, 12.0), p(15.0, 18.0), stroke);
            }
            Icon::ChevronRight => {
                stroke_line(&mut frame, p(9.0, 6.0), p(15.0, 12.0), stroke);
                stroke_line(&mut frame, p(15.0, 12.0), p(9.0, 18.0), stroke);
            }
            Icon::File | Icon::FileText => {
                let file = canvas::Path::new(|path| {
                    path.move_to(p(6.0, 3.0));
                    path.line_to(p(14.0, 3.0));
                    path.line_to(p(19.0, 8.0));
                    path.line_to(p(19.0, 21.0));
                    path.line_to(p(6.0, 21.0));
                    path.close();
                    path.move_to(p(14.0, 3.0));
                    path.line_to(p(14.0, 8.0));
                    path.line_to(p(19.0, 8.0));
                });
                frame.stroke(&file, stroke);
                if matches!(self.icon, Icon::FileText) {
                    stroke_line(&mut frame, p(9.0, 13.0), p(16.0, 13.0), stroke);
                    stroke_line(&mut frame, p(9.0, 17.0), p(15.0, 17.0), stroke);
                }
            }
            Icon::Folder | Icon::FolderOpen => {
                let folder = canvas::Path::new(|path| {
                    path.move_to(p(3.0, 7.0));
                    path.line_to(p(9.0, 7.0));
                    path.line_to(p(11.0, 9.0));
                    path.line_to(p(21.0, 9.0));
                    path.line_to(p(21.0, 19.0));
                    path.line_to(p(3.0, 19.0));
                    path.close();
                });
                frame.stroke(&folder, stroke);
                if matches!(self.icon, Icon::FolderOpen) {
                    stroke_line(&mut frame, p(5.0, 12.0), p(19.0, 12.0), stroke);
                }
            }
            Icon::Image => {
                frame.stroke(
                    &canvas::Path::rounded_rectangle(
                        p(4.0, 5.0),
                        iced::Size::new(16.0 * scale, 14.0 * scale),
                        (2.0 * scale).into(),
                    ),
                    stroke,
                );
                frame.stroke(&canvas::Path::circle(p(9.0, 10.0), 1.5 * scale), stroke);
                stroke_line(&mut frame, p(6.0, 17.0), p(11.0, 13.0), stroke);
                stroke_line(&mut frame, p(11.0, 13.0), p(14.0, 16.0), stroke);
                stroke_line(&mut frame, p(14.0, 16.0), p(18.0, 12.0), stroke);
            }
            Icon::Trash => {
                stroke_line(&mut frame, p(5.0, 7.0), p(19.0, 7.0), stroke);
                stroke_line(&mut frame, p(10.0, 11.0), p(10.0, 17.0), stroke);
                stroke_line(&mut frame, p(14.0, 11.0), p(14.0, 17.0), stroke);
                let bin = canvas::Path::new(|path| {
                    path.move_to(p(8.0, 7.0));
                    path.line_to(p(8.0, 20.0));
                    path.line_to(p(16.0, 20.0));
                    path.line_to(p(16.0, 7.0));
                    path.move_to(p(10.0, 4.0));
                    path.line_to(p(14.0, 4.0));
                    path.line_to(p(15.0, 7.0));
                });
                frame.stroke(&bin, stroke);
            }
            Icon::LayoutPanelLeft => {
                let outer = canvas::Path::rounded_rectangle(
                    p(3.0, 4.0),
                    iced::Size::new(18.0 * scale, 16.0 * scale),
                    (2.0 * scale).into(),
                );
                frame.stroke(&outer, stroke);
                stroke_line(&mut frame, p(9.0, 4.0), p(9.0, 20.0), stroke);
            }
            Icon::Command => {
                for (x, y) in [(7.0, 7.0), (17.0, 7.0), (7.0, 17.0), (17.0, 17.0)] {
                    frame.stroke(
                        &canvas::Path::rounded_rectangle(
                            p(x - 3.0, y - 3.0),
                            iced::Size::new(6.0 * scale, 6.0 * scale),
                            (2.0 * scale).into(),
                        ),
                        stroke,
                    );
                }
                stroke_line(&mut frame, p(7.0, 10.0), p(7.0, 14.0), stroke);
                stroke_line(&mut frame, p(17.0, 10.0), p(17.0, 14.0), stroke);
                stroke_line(&mut frame, p(10.0, 7.0), p(14.0, 7.0), stroke);
                stroke_line(&mut frame, p(10.0, 17.0), p(14.0, 17.0), stroke);
            }
            Icon::ListTree => {
                stroke_line(&mut frame, p(7.0, 6.0), p(21.0, 6.0), stroke);
                stroke_line(&mut frame, p(7.0, 12.0), p(21.0, 12.0), stroke);
                stroke_line(&mut frame, p(13.0, 18.0), p(21.0, 18.0), stroke);
                stroke_line(&mut frame, p(3.0, 6.0), p(3.0, 18.0), stroke);
                stroke_line(&mut frame, p(3.0, 18.0), p(9.0, 18.0), stroke);
            }
            Icon::Split => {
                let outer = canvas::Path::rounded_rectangle(
                    p(4.0, 5.0),
                    iced::Size::new(16.0 * scale, 14.0 * scale),
                    (2.0 * scale).into(),
                );
                frame.stroke(&outer, stroke);
                stroke_line(&mut frame, p(12.0, 5.0), p(12.0, 19.0), stroke);
            }
            Icon::Clock => {
                frame.stroke(&canvas::Path::circle(p(12.0, 12.0), 8.0 * scale), stroke);
                stroke_line(&mut frame, p(12.0, 8.0), p(12.0, 12.5), stroke);
                stroke_line(&mut frame, p(12.0, 12.5), p(15.5, 15.0), stroke);
            }
        }

        vec![frame.into_geometry()]
    }
}

fn stroke_line(
    frame: &mut canvas::Frame<Renderer>,
    from: Point,
    to: Point,
    stroke: canvas::Stroke<'_>,
) {
    frame.stroke(&canvas::Path::line(from, to), stroke);
}
