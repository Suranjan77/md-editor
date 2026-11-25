//! The PDF pane: toolbar, search bar, the continuous page list and its overlay canvas.

use iced::widget::{Space, button, canvas, column, container, row, stack, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

use crate::design::fade;
use crate::design::metrics::Elevation;
use crate::design::metrics::{BORDER_WIDTH, ControlSize, RADIUS_SM, SPACE_1, SPACE_2};
use crate::design::{ContentHue, TextRole, tokens};
use crate::messages::{EditorMessage, Message, PdfMessage, SearchMessage, Shortcut};
use crate::ui::{
    Button, Icon, IconButton, TextField, TooltipPosition, empty_state, toolbar_frame, with_tooltip,
};
use crate::views::interactive_pdf::{InteractivePdf, PageEvents, PdfSelection};
use crate::views::viewer_toolbar::{self, PositionField, ViewerToolbar, ZoomControls};

pub(crate) const PDF_PAGE_LIST_PADDING: f32 = 20.0;
pub(crate) const PDF_PAGE_SPACING: f32 = 20.0;
pub const PDF_SEARCH_INPUT_ID: &str = "pdf_search_input";

fn empty_pdf_state_message(load_error: Option<&str>) -> &str {
    load_error.unwrap_or("Loading PDF...")
}

#[derive(Debug, Clone)]
struct OverlayRect {
    rect: md_editor_core::pdf::PdfRect,
    color: Color,
    border_color: Option<Color>,
    /// When true, the minimum-size clamp is skipped so the rect can render as a
    /// hairline (used for reference underlines). Highlights leave this false so
    /// they stay legibly tall.
    thin: bool,
}

#[derive(Debug, Clone)]
struct PdfOverlay {
    rects: Vec<OverlayRect>,
    /// The page size the rects were computed for. If layout gives the page
    /// less room, the bitmap underneath is squeezed to fit, so the rects are
    /// scaled the same way instead of drifting off the text.
    laid_out_for: (f32, f32),
}

impl<Message> canvas::Program<Message> for PdfOverlay {
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
        let sx = bounds.width / self.laid_out_for.0.max(1.0);
        let sy = bounds.height / self.laid_out_for.1.max(1.0);

        for item in &self.rects {
            let (width, height) = (item.rect.width * sx, item.rect.height * sy);
            let rect = Rectangle {
                x: item.rect.x * sx,
                y: item.rect.y * sy,
                width: if item.thin { width } else { width.max(3.0) },
                height: if item.thin { height } else { height.max(8.0) },
            };
            frame.fill_rectangle(
                Point::new(rect.x, rect.y),
                iced::Size::new(rect.width, rect.height),
                item.color,
            );
            if let Some(border_color) = item.border_color {
                frame.stroke_rectangle(
                    Point::new(rect.x, rect.y),
                    iced::Size::new(rect.width, rect.height),
                    canvas::Stroke::default()
                        .with_color(border_color)
                        .with_width(1.5),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

fn search_rect_to_view_rect(
    rect: &md_editor_core::pdf::PdfRect,
    page_height: f32,
    zoom: f32,
) -> md_editor_core::pdf::PdfRect {
    md_editor_core::pdf::PdfRect {
        x: rect.x * zoom,
        y: (page_height - rect.y - rect.height) * zoom,
        width: rect.width * zoom,
        height: rect.height * zoom,
    }
}

pub fn search_bar<'a>(
    query: &'a str,
    regex: bool,
    match_case: bool,
    loose: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    let counter = match active_match_index {
        Some(index) if current_match_count > 0 => {
            format!("{} of {}", index + 1, current_match_count)
        }
        _ if query.is_empty() => String::new(),
        _ if current_match_count >= md_editor_core::pdf::PDF_SEARCH_MATCH_LIMIT => {
            format!("{current_match_count}+ matches")
        }
        _ => format!("{current_match_count} matches"),
    };
    let role = TextRole::MonoCaption;
    toolbar_frame(
        vec![
            TextField::search("Find in PDF", query)
                .id(iced::advanced::widget::Id::new(PDF_SEARCH_INPUT_ID))
                .size(ControlSize::Sm)
                .on_input(|value| Message::Search(SearchMessage::QueryChanged(value)))
                .on_submit(Message::Search(SearchMessage::Next))
                .on_clear(Message::Search(SearchMessage::QueryChanged(String::new())))
                .build(),
            IconButton::new(Icon::TextAa, "Match case")
                .size(ControlSize::Sm)
                .selected(match_case)
                .on_press(Message::Search(SearchMessage::MatchCaseToggled(
                    !match_case,
                )))
                .build(),
            IconButton::new(Icon::Regex, "Regular expression")
                .size(ControlSize::Sm)
                .selected(regex)
                .on_press(Message::Search(SearchMessage::RegexToggled(!regex)))
                .build(),
            IconButton::new(Icon::TextLoose, "Ignore line breaks and extra spaces")
                .size(ControlSize::Sm)
                .selected(loose)
                .on_press(Message::Pdf(PdfMessage::SearchLooseToggled(!loose)))
                .build(),
            text(counter)
                .size(role.size())
                .font(role.font())
                .color(tokens().fg_secondary)
                .into(),
            IconButton::new(Icon::ChevronUp, "Previous match")
                .size(ControlSize::Sm)
                .shortcut("Shift Enter")
                .on_press(Message::Search(SearchMessage::Previous))
                .build(),
            IconButton::new(Icon::ChevronDown, "Next match")
                .size(ControlSize::Sm)
                .shortcut("Enter")
                .on_press(Message::Search(SearchMessage::Next))
                .build(),
            IconButton::new(Icon::X, "Close find")
                .size(ControlSize::Sm)
                .shortcut("Esc")
                .on_press(Message::Search(SearchMessage::Close))
                .build(),
        ],
        false,
    )
}

/// What the PDF toolbar shows; see [`toolbar`].
pub struct PdfToolbarProps<'a> {
    pub current_page: u16,
    pub total_pages: u16,
    pub zoom: f32,
    pub fit_to_width: bool,
    /// Text being typed into the page / zoom fields.
    pub page_input: Option<&'a str>,
    pub zoom_input: Option<&'a str>,
    pub toc_visible: bool,
    pub has_selection: bool,
    pub has_annotations: bool,
    pub focused_annotation: Option<&'a md_editor_core::pdf::PdfAnnotation>,
}

/// The content-palette hue a PDF highlight colour is drawn in.
pub fn annotation_hue(color: md_editor_core::pdf::PdfAnnotationColor) -> ContentHue {
    use md_editor_core::pdf::PdfAnnotationColor as C;
    match color {
        C::Yellow => ContentHue::Yellow,
        C::Green => ContentHue::Green,
        C::Blue => ContentHue::Blue,
        C::Pink => ContentHue::Pink,
        C::Orange => ContentHue::Orange,
    }
}

fn swatch<'a>(
    color: md_editor_core::pdf::PdfAnnotationColor,
) -> Element<'a, Message, Theme, Renderer> {
    let hue = annotation_hue(color);
    let fill = hue.color(tokens().is_dark);
    let size = ControlSize::Sm.height();
    let chip = button(Space::new())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .on_press(Message::Pdf(PdfMessage::CreateHighlight(color)))
        .style(move |_, status| {
            let t = tokens();
            button::Style {
                background: Some(iced::Background::Color(fill)),
                border: iced::Border {
                    color: if matches!(status, button::Status::Hovered) {
                        t.fg_primary
                    } else {
                        t.border_control
                    },
                    width: BORDER_WIDTH,
                    radius: RADIUS_SM.into(),
                },
                ..Default::default()
            }
        });
    with_tooltip(
        chip,
        &format!("Highlight {}", hue.label().to_lowercase()),
        None,
        TooltipPosition::Bottom,
    )
}

/// Selection tools: one swatch per highlight colour, quick highlight, dismiss.
fn selection_tools<'a>() -> Vec<Element<'a, Message, Theme, Renderer>> {
    use md_editor_core::pdf::PdfAnnotationColor as C;
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = vec![label("Highlight")];
    for color in [C::Yellow, C::Green, C::Blue, C::Pink, C::Orange] {
        items.push(swatch(color));
    }
    items.push(
        IconButton::new(Icon::Refresh, "Quick highlight (next colour)")
            .size(ControlSize::Sm)
            .on_press(Message::Pdf(PdfMessage::QuickHighlight))
            .build(),
    );
    items.push(
        IconButton::new(Icon::X, "Clear selection")
            .size(ControlSize::Sm)
            .on_press(Message::Pdf(PdfMessage::SelectionCleared))
            .build(),
    );
    vec![
        row(items)
            .spacing(SPACE_2)
            .align_y(Alignment::Center)
            .into(),
    ]
}

/// Tools for the focused highlight: note, linked note, copy, delete.
fn annotation_tools<'a>(
    ann: &'a md_editor_core::pdf::PdfAnnotation,
) -> Vec<Element<'a, Message, Theme, Renderer>> {
    let note = Button::ghost("Note")
        .size(ControlSize::Sm)
        .icon(Icon::FileText)
        .on_press(Message::Pdf(PdfMessage::RightClicked(
            ann.page_index,
            -1.0,
            -1.0,
        )))
        .build();
    let linked = match ann.linked_note_path.as_deref().filter(|p| !p.is_empty()) {
        Some(path) => Button::ghost("Open note")
            .size(ControlSize::Sm)
            .icon(Icon::ArrowSquareOut)
            .on_press(Message::Pdf(PdfMessage::OpenLinkedNote(path.to_string())))
            .build(),
        None => Button::ghost("Link note")
            .size(ControlSize::Sm)
            .icon(Icon::Link)
            .on_press(Message::Pdf(PdfMessage::LinkNote(
                ann.id.clone(),
                String::new(),
            )))
            .build(),
    };
    let copy = IconButton::new(Icon::Copy, "Copy highlighted text")
        .size(ControlSize::Sm)
        .on_press(Message::Pdf(PdfMessage::CopyAnnotationText(ann.id.clone())))
        .build();
    let delete = IconButton::new(Icon::Trash, "Delete highlight")
        .size(ControlSize::Sm)
        .danger(true)
        .on_press(Message::Pdf(PdfMessage::DeleteHighlight(ann.id.clone())))
        .build();
    vec![
        row![label("Highlight"), note, linked, copy, delete]
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
    ]
}

fn label<'a>(content: &'static str) -> Element<'a, Message, Theme, Renderer> {
    let role = TextRole::Caption;
    text(content)
        .size(role.size())
        .font(role.font())
        .color(tokens().fg_tertiary)
        .into()
}

/// The PDF pane's toolbar: the shared viewer toolbar with the PDF's study
/// tools in the middle.
pub fn toolbar<'a>(props: PdfToolbarProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    let mut context = if props.has_selection {
        selection_tools()
    } else if let Some(ann) = props.focused_annotation {
        annotation_tools(ann)
    } else {
        Vec::new()
    };
    // Only offer the orphan-anchor check when the document has annotations;
    // otherwise there is nothing to check.
    if props.has_annotations && !props.has_selection {
        context.push(
            Button::ghost("Check anchors")
                .size(ControlSize::Sm)
                .on_press(Message::Pdf(PdfMessage::OrphanReport))
                .tooltip("Find highlights whose text moved or vanished", None)
                .build(),
        );
    }

    viewer_toolbar::view(ViewerToolbar {
        outline: Some((props.toc_visible, Message::Editor(EditorMessage::ToggleTOC))),
        position: Some(PositionField {
            current: usize::from(props.current_page) + 1,
            total: usize::from(props.total_pages),
            draft: props.page_input,
            on_input: |raw| Message::Pdf(PdfMessage::PageInputChanged(raw)),
            on_submit: Message::Pdf(PdfMessage::PageInputSubmit),
            noun: "Page",
        }),
        zoom: Some(ZoomControls {
            percent: (props.zoom * 100.0).round() as u32,
            draft: props.zoom_input,
            on_input: Some(|raw| Message::Pdf(PdfMessage::ZoomInputChanged(raw))),
            on_submit: Some(Message::Pdf(PdfMessage::ZoomInputSubmit)),
            zoom_out: Message::Pdf(PdfMessage::ZoomChanged((props.zoom - 0.1).max(0.25))),
            zoom_in: Message::Pdf(PdfMessage::ZoomChanged((props.zoom + 0.1).min(4.0))),
            fit_width: Some((props.fit_to_width, Message::Pdf(PdfMessage::FitToWidth))),
            fit_page: Some((false, Message::Pdf(PdfMessage::FitToPage))),
        }),
        find: Some(Message::KeyboardShortcut(Shortcut::Search)),
        context,
        title: None,
    })
}

/// Everything the continuous PDF view renders from.
pub struct ContinuousProps<'a> {
    pub pages: &'a [Option<iced::widget::image::Handle>],
    pub zoom: f32,
    pub dimensions: &'a [Option<(u32, u32)>],
    pub page_sizes: &'a [Option<(f32, f32)>],
    pub placeholder_page_size: Option<(f32, f32)>,
    pub search_matches: &'a [md_editor_core::pdf::PdfSearchMatch],
    pub active_search_index: Option<usize>,
    pub page_texts: &'a std::collections::HashMap<u16, md_editor_core::pdf::PdfPageText>,
    pub annotations: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::PdfAnnotation>>,
    pub references: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::LinkInfo>>,
    pub active_selection: Option<PdfSelection>,
    pub focused_annotation_id: Option<&'a str>,
    pub load_error: Option<&'a str>,
}

pub fn view_continuous<'a>(props: ContinuousProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    if props.pages.is_empty() {
        return empty_viewer(props.load_error);
    }
    let zoom = props.zoom;

    let mut page_list = column![]
        .spacing(PDF_PAGE_SPACING)
        .padding(PDF_PAGE_LIST_PADDING)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    let placeholder_display_size = props
        .placeholder_page_size
        .map(|(w, h)| (w * zoom, h * zoom))
        .or_else(|| {
            props
                .page_sizes
                .first()
                .and_then(|s| *s)
                .map(|(w, h)| (w * zoom, h * zoom))
        })
        .or_else(|| {
            props
                .dimensions
                .first()
                .and_then(|d| d.map(|(w, h)| (w as f32, h as f32)))
        })
        .unwrap_or((612.0 * zoom, 792.0 * zoom));

    let (pw, ph) = (
        placeholder_display_size.0 / zoom.max(0.01),
        placeholder_display_size.1 / zoom.max(0.01),
    );

    for (i, page_opt) in props.pages.iter().enumerate() {
        let (page_width, page_height) = props
            .page_sizes
            .get(i)
            .and_then(|size| *size)
            .unwrap_or((pw, ph));
        let display_size = (page_width * zoom, page_height * zoom);
        page_list = page_list.push(match page_opt {
            Some(handle) => {
                let page_index = i as u16;
                let overlay = props.overlay_rects(page_index, page_height);
                rendered_page(
                    handle,
                    page_index,
                    display_size,
                    props.page_texts.get(&page_index),
                    overlay,
                )
            }
            None => placeholder_page(i, display_size),
        });
    }

    container(page_list)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(tokens().viewer_ground)),
            ..Default::default()
        })
        .into()
}

/// The pane before any page exists: loading, or why the PDF failed to open.
fn empty_viewer<'a>(load_error: Option<&str>) -> Element<'a, Message, Theme, Renderer> {
    let body = match load_error {
        Some(error) => empty_state(
            Icon::WarningCircle,
            "This PDF could not be opened",
            empty_pdf_state_message(Some(error)),
            None,
        ),
        None => empty_state(
            Icon::FilePdf,
            "Loading PDF…",
            "Pages appear as they render.",
            None,
        ),
    };
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(tokens().viewer_ground)),
            ..Default::default()
        })
        .into()
}

impl ContinuousProps<'_> {
    /// Everything drawn over one rendered page, bottom to top: highlights,
    /// search hits, the active hit, the text selection and reference underlines.
    fn overlay_rects(&self, page_index: u16, page_height: f32) -> Vec<OverlayRect> {
        let zoom = self.zoom;
        let to_view =
            |rect: &md_editor_core::pdf::PdfRect| search_rect_to_view_rect(rect, page_height, zoom);
        let fill = |rect, color| OverlayRect {
            rect,
            color,
            border_color: None,
            thin: false,
        };
        let mut overlay_rects = Vec::new();
        for ann in self.annotations.get(&page_index).into_iter().flatten() {
            // Content-palette tints: the same on dark and light chrome,
            // since they sit on the white page.
            let color = annotation_hue(ann.color).pdf_highlight();
            let border_color =
                (self.focused_annotation_id == Some(ann.id.as_str())).then(|| tokens().focus_ring);
            for rect in &ann.rects {
                overlay_rects.push(OverlayRect {
                    rect: to_view(rect),
                    color,
                    border_color,
                    thin: false,
                });
            }
        }
        for (idx, result) in self.search_matches.iter().enumerate() {
            if result.page_index == page_index && Some(idx) != self.active_search_index {
                overlay_rects.extend(
                    result
                        .rects
                        .iter()
                        .map(|rect| fill(to_view(rect), tokens().doc_find)),
                );
            }
        }
        if let Some(result) = self
            .active_search_index
            .and_then(|idx| self.search_matches.get(idx))
            .filter(|result| result.page_index == page_index)
        {
            overlay_rects.extend(
                result
                    .rects
                    .iter()
                    .map(|rect| fill(to_view(rect), tokens().doc_find_active)),
            );
        }
        if let (Some(sel), Some(page_text)) =
            (self.active_selection, self.page_texts.get(&page_index))
            && sel.page_index == page_index
        {
            let start = sel.anchor_idx.min(sel.focus_idx);
            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
            let selected_chars = page_text
                .chars
                .iter()
                .filter(|c| c.text_index >= start && c.text_index < end)
                .cloned()
                .collect::<Vec<_>>();
            overlay_rects.extend(
                md_editor_core::pdf::merge_char_rects(&selected_chars)
                    .into_iter()
                    .map(|rect| {
                        fill(
                            search_rect_to_view_rect(&rect, page_text.page_height, zoom),
                            fade(ContentHue::Blue.color(true), 0.45),
                        )
                    }),
            );
        }
        // Underline recognised internal references so they read as
        // clickable. `LinkInfo.bbox` is already top-left origin in PDF
        // points, so it converts to view space with a plain zoom scale (no
        // y-flip, unlike the bottom-left annotation/search rects above).
        if let Some(refs) = self.references.get(&page_index) {
            // Hairline underline: fixed ~1px, not scaled by zoom (a
            // zoom-scaled rule reads as a heavy bar when zoomed in).
            let thickness = 1.0;
            for link in refs {
                let b = &link.bbox;
                overlay_rects.push(OverlayRect {
                    rect: md_editor_core::pdf::PdfRect {
                        x: b.x * zoom,
                        y: (b.y + b.height) * zoom - thickness,
                        width: b.width * zoom,
                        height: thickness,
                    },
                    color: fade(tokens().accent, 0.5),
                    border_color: None,
                    thin: true,
                });
            }
        }
        overlay_rects
    }
}

/// A rendered page: the interactive image with its overlay canvas on top.
fn rendered_page<'a>(
    handle: &iced::widget::image::Handle,
    page_index: u16,
    (w, h): (f32, f32),
    page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
    overlay_rects: Vec<OverlayRect>,
) -> Element<'a, Message, Theme, Renderer> {
    let interactive = InteractivePdf::new(
        handle.clone(),
        w,
        h,
        page_index,
        page_text,
        PageEvents {
            on_left_click: Box::new(move |x, y, modifiers| {
                Message::Pdf(PdfMessage::LeftClicked(page_index, x, y, modifiers))
            }),
            on_right_click: Box::new(move |x, y| {
                Message::Pdf(PdfMessage::RightClicked(page_index, x, y))
            }),
            on_selection_changed: Box::new(|page, start, end| {
                Message::Pdf(PdfMessage::SelectionChanged(page, start, end))
            }),
            on_selection_finished: Box::new(|page, start, end| {
                Message::Pdf(PdfMessage::SelectionFinished(page, start, end))
            }),
            on_selection_cleared: Box::new(|| Message::Pdf(PdfMessage::SelectionCleared)),
        },
    );

    container(stack![
        interactive,
        canvas(PdfOverlay {
            rects: overlay_rects,
            laid_out_for: (w, h),
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h)),
    ])
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .style(|_| container::Style {
        background: Some(iced::Background::Color(iced::Color::WHITE)),
        shadow: Elevation::Popover.shadow(),
        ..Default::default()
    })
    .into()
}

/// A page-sized placeholder, so nothing jumps when the page renders.
fn placeholder_page<'a>(index: usize, (w, h): (f32, f32)) -> Element<'a, Message, Theme, Renderer> {
    let role = TextRole::Caption;
    container(
        text(format!("Page {}", index + 1))
            .size(role.size())
            .font(role.font())
            .color(tokens().fg_tertiary),
    )
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .center_x(Length::Fixed(w))
    .center_y(Length::Fixed(h))
    .style(|_| container::Style {
        background: Some(iced::Background::Color(tokens().bg_raised)),
        border: iced::Border {
            color: tokens().border_subtle,
            width: BORDER_WIDTH,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::empty_pdf_state_message;

    #[test]
    fn failed_empty_pdf_shows_error_instead_of_loading() {
        assert_eq!(empty_pdf_state_message(None), "Loading PDF...");
        assert_eq!(
            empty_pdf_state_message(Some("Could not open 'locked.pdf': Password required")),
            "Could not open 'locked.pdf': Password required"
        );
    }
}
