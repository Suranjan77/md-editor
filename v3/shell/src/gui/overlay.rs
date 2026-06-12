//! Modal overlays: command palette, quick-open, vault search, find,
//! PDF zoom/page input. State lives here; the kernel only knows *that* an
//! overlay is open (the scope fence) — escape/enter resolve to
//! `overlay.close`/`overlay.confirm` through the keymap, and everything
//! else reaches the overlay as raw input. No iced text_input widget: the
//! input line is fed by the same single keystroke path as the editors.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Fill, Task};
use md3_kernel::CommandRegistry;
use md3_vault::Hit;

use super::Message;
use super::editor_canvas::palette as colors;
use super::tokens;

/// One `pdf.find` match: where it lives (page points, ready to tint) and
/// what the hit list shows.
#[derive(Debug, Clone)]
pub struct PdfFindHit {
    pub page: u32,
    pub quads: Vec<md3_pdf::SelRect>,
    pub text: String,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Palette {
        input: String,
        selected: usize,
    },
    QuickOpen {
        input: String,
        selected: usize,
    },
    Search {
        input: String,
        selected: usize,
        hits: Vec<Hit>,
    },
    Find {
        input: String,
    },
    PdfFind {
        input: String,
        selected: usize,
        hits: Vec<PdfFindHit>,
    },
    /// Table of contents: `entries` are `(indented title, 0-based page)`
    /// snapshotted from the focused PDF's outline at open.
    PdfToc {
        input: String,
        selected: usize,
        entries: Vec<(String, u32)>,
    },
    /// Referrers of the focused note (vault-relative paths), snapshotted
    /// from the link graph at open; `input` filters them.
    Backlinks {
        input: String,
        selected: usize,
        referrers: Vec<String>,
    },
    PdfZoom {
        input: String,
    },
    PdfPage {
        input: String,
    },
    /// Note text for the focused PDF's picked annotation (pre-filled with
    /// the existing note; confirm overwrites it).
    AnnotationNote {
        input: String,
    },
    /// Read-only report: documents whose annotations no longer match any
    /// vault file's content hash — `(last seen path, "N annotations")`.
    OrphanReport {
        rows: Vec<(String, String)>,
    },
    PdfLinkPreview {
        dest_page: u32,
        dest_y: Option<f32>,
        image: iced::widget::image::Handle,
        width: u32,
        height: u32,
    },
}

impl Overlay {
    /// The name the kernel sees (status / debugging only — the fence is the
    /// same for all overlays).
    pub fn kernel_name(&self) -> &'static str {
        match self {
            Overlay::Palette { .. } => "palette",
            Overlay::QuickOpen { .. } => "quick-open",
            Overlay::Search { .. } => "search",
            Overlay::Find { .. } => "find",
            Overlay::PdfFind { .. } => "pdf-find",
            Overlay::PdfToc { .. } => "pdf-toc",
            Overlay::Backlinks { .. } => "backlinks",
            Overlay::PdfZoom { .. } => "pdf-zoom",
            Overlay::PdfPage { .. } => "pdf-page",
            Overlay::AnnotationNote { .. } => "annotation-note",
            Overlay::OrphanReport { .. } => "orphan-report",
            Overlay::PdfLinkPreview { .. } => "pdf-link-preview",
        }
    }

    pub fn input_mut(&mut self) -> Option<&mut String> {
        match self {
            Overlay::Palette { input, .. }
            | Overlay::QuickOpen { input, .. }
            | Overlay::Search { input, .. }
            | Overlay::Find { input }
            | Overlay::PdfFind { input, .. }
            | Overlay::PdfToc { input, .. }
            | Overlay::Backlinks { input, .. }
            | Overlay::PdfZoom { input }
            | Overlay::PdfPage { input }
            | Overlay::AnnotationNote { input } => Some(input),
            Overlay::OrphanReport { .. } | Overlay::PdfLinkPreview { .. } => None,
        }
    }

    pub fn selected_mut(&mut self) -> Option<&mut usize> {
        match self {
            Overlay::Palette { selected, .. }
            | Overlay::QuickOpen { selected, .. }
            | Overlay::Search { selected, .. }
            | Overlay::PdfFind { selected, .. }
            | Overlay::PdfToc { selected, .. }
            | Overlay::Backlinks { selected, .. } => Some(selected),
            _ => None,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Overlay::Palette { .. } => "Command Palette",
            Overlay::QuickOpen { .. } => "Quick Open",
            Overlay::Search { .. } => "Search Vault",
            Overlay::Find { .. } => "Find in Note",
            Overlay::PdfFind { .. } => "Find in PDF",
            Overlay::PdfToc { .. } => "Table of Contents",
            Overlay::Backlinks { .. } => "Backlinks",
            Overlay::PdfZoom { .. } => "Zoom (%)",
            Overlay::PdfPage { .. } => "Go to Page",
            Overlay::AnnotationNote { .. } => "Annotation Note",
            Overlay::OrphanReport { .. } => "Orphaned Annotations",
            Overlay::PdfLinkPreview { .. } => "Reference",
        }
    }
}

/// Stable id for the overlay's hit list, so keyboard navigation can keep
/// the selected row visible (`snap_selected`) across view rebuilds.
pub fn list_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("overlay-list")
}

/// Keep `selected` visible in the list scrollable. A relative offset of
/// `selected / (rows − 1)` puts the row fully in view for any viewport at
/// least one row tall: the row's top is ≥ the scroll position and its
/// bottom is ≤ position + viewport, for every ratio.
pub fn snap_selected(rows: usize, selected: usize) -> Task<Message> {
    if rows < 2 {
        return Task::none();
    }
    let y = selected as f32 / (rows - 1) as f32;
    iced::widget::operation::snap_to(
        list_scroll_id(),
        iced::widget::operation::RelativeOffset { x: 0.0, y },
    )
}

/// Rows the list-style overlays display, already filtered by input. The
/// full match set — the list scrolls; what's shown is exactly what enter
/// can pick.
pub fn list_rows(
    overlay: &Overlay,
    registry: &CommandRegistry,
    files: &[String],
) -> Vec<(String, String)> {
    match overlay {
        Overlay::Palette { input, .. } => registry
            .palette(input)
            .into_iter()
            .map(|spec| (spec.title.to_string(), spec.id.0.to_string()))
            .collect(),
        Overlay::QuickOpen { input, .. } => {
            let needle = input.to_lowercase();
            files
                .iter()
                .filter(|f| f.to_lowercase().contains(&needle))
                .map(|f| (f.clone(), String::new()))
                .collect()
        }
        Overlay::Search { hits, .. } => hits
            .iter()
            .map(|h| (h.path.to_string_lossy().to_string(), h.snippet.clone()))
            .collect(),
        Overlay::PdfFind { hits, .. } => hits
            .iter()
            .map(|h| (format!("p. {}", h.page + 1), h.preview.clone()))
            .collect(),
        Overlay::PdfToc { input, entries, .. } => toc_matches(entries, input)
            .into_iter()
            .map(|(title, page)| (title.clone(), format!("p. {}", page + 1)))
            .collect(),
        Overlay::Backlinks {
            input, referrers, ..
        } => {
            let needle = input.to_lowercase();
            referrers
                .iter()
                .filter(|r| r.to_lowercase().contains(&needle))
                .map(|r| (r.clone(), String::new()))
                .collect()
        }
        Overlay::OrphanReport { rows } => rows.clone(),
        _ => Vec::new(),
    }
}

pub fn view<'a>(
    overlay: &'a Overlay,
    registry: &'a CommandRegistry,
    files: &'a [String],
) -> Element<'a, Message> {
    if let Overlay::PdfLinkPreview {
        dest_page, image, ..
    } = overlay
    {
        let card = container(
            column![
                row![
                    text(format!("Reference — p. {}", dest_page + 1))
                        .size(13)
                        .color(colors::marker()),
                ],
                iced::widget::image(image.clone()).width(540),
                row![
                    text("esc closes · enter navigates")
                        .size(12)
                        .color(colors::marker()),
                ],
            ]
            .spacing(10)
            .padding(14),
        )
        .width(560)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(tokens::dark().bg_secondary)),
            border: iced::Border {
                color: tokens::dark().border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        });

        return container(card).center_x(Fill).padding([60, 0]).into();
    }

    let rows = list_rows(overlay, registry, files);
    let selected = match overlay {
        Overlay::Palette { selected, .. }
        | Overlay::QuickOpen { selected, .. }
        | Overlay::Search { selected, .. }
        | Overlay::PdfFind { selected, .. }
        | Overlay::PdfToc { selected, .. }
        | Overlay::Backlinks { selected, .. } => *selected,
        _ => 0,
    };

    let input_line = {
        let shown = format!("{}▏", overlay.input());
        row![
            text(overlay.title()).size(13).color(colors::marker()),
            text(shown).size(15).color(colors::text()),
        ]
        .spacing(12)
    };

    let mut list = column![].spacing(2);
    for (i, (title, detail)) in rows.iter().enumerate() {
        let marker = if i == selected { "▸ " } else { "  " };
        let line = row![
            text(format!("{marker}{title}"))
                .size(14)
                .color(if i == selected {
                    colors::heading()
                } else {
                    colors::text()
                }),
            text(detail.clone()).size(12).color(colors::marker()),
        ]
        .spacing(10);
        list = list.push(iced::widget::mouse_area(line).on_press(Message::OverlayPick(i)));
    }

    // The hit list scrolls (wheel/drag here, snap_selected for the keyboard)
    // instead of truncating — every row enter can pick is reachable on screen.
    let list =
        container(scrollable(list).id(list_scroll_id()).width(Fill).spacing(4)).max_height(420);

    let card = container(column![input_line, list].spacing(10).padding(14))
        .width(560)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(tokens::dark().bg_secondary)),
            border: iced::Border {
                color: tokens::dark().border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        });

    container(card).center_x(Fill).padding([60, 0]).into()
}

impl Overlay {
    pub fn input(&self) -> &str {
        match self {
            Overlay::Palette { input, .. }
            | Overlay::QuickOpen { input, .. }
            | Overlay::Search { input, .. }
            | Overlay::Find { input }
            | Overlay::PdfFind { input, .. }
            | Overlay::PdfToc { input, .. }
            | Overlay::Backlinks { input, .. }
            | Overlay::PdfZoom { input }
            | Overlay::PdfPage { input }
            | Overlay::AnnotationNote { input } => input,
            Overlay::OrphanReport { .. } | Overlay::PdfLinkPreview { .. } => "",
        }
    }
}

/// TOC entries whose title contains the query (case-insensitive), in
/// document order — the display rows and confirm resolution share this so
/// the row the user sees is the row enter picks.
pub fn toc_matches<'a>(entries: &'a [(String, u32)], input: &str) -> Vec<&'a (String, u32)> {
    let needle = input.to_lowercase();
    entries
        .iter()
        .filter(|(title, _)| title.to_lowercase().contains(&needle))
        .collect()
}
