//! Modal overlays: command palette, quick-open, vault search, find,
//! PDF zoom/page input. State lives here; the kernel only knows *that* an
//! overlay is open (the scope fence) — escape/enter resolve to
//! `overlay.close`/`overlay.confirm` through the keymap, and everything
//! else reaches the overlay as raw input. No iced text_input widget: the
//! input line is fed by the same single keystroke path as the editors.

use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use md3_kernel::CommandRegistry;
use md3_vault::Hit;

use super::Message;
use super::editor_canvas::palette as colors;

#[derive(Debug, Clone)]
pub enum Overlay {
    Palette { input: String, selected: usize },
    QuickOpen { input: String, selected: usize },
    Search { input: String, selected: usize, hits: Vec<Hit> },
    Find { input: String },
    PdfZoom { input: String },
    PdfPage { input: String },
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
            Overlay::PdfZoom { .. } => "pdf-zoom",
            Overlay::PdfPage { .. } => "pdf-page",
        }
    }

    pub fn input_mut(&mut self) -> &mut String {
        match self {
            Overlay::Palette { input, .. }
            | Overlay::QuickOpen { input, .. }
            | Overlay::Search { input, .. }
            | Overlay::Find { input }
            | Overlay::PdfZoom { input }
            | Overlay::PdfPage { input } => input,
        }
    }

    pub fn selected_mut(&mut self) -> Option<&mut usize> {
        match self {
            Overlay::Palette { selected, .. }
            | Overlay::QuickOpen { selected, .. }
            | Overlay::Search { selected, .. } => Some(selected),
            _ => None,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Overlay::Palette { .. } => "Command Palette",
            Overlay::QuickOpen { .. } => "Quick Open",
            Overlay::Search { .. } => "Search Vault",
            Overlay::Find { .. } => "Find in Note",
            Overlay::PdfZoom { .. } => "Zoom (%)",
            Overlay::PdfPage { .. } => "Go to Page",
        }
    }
}

/// Rows the list-style overlays display, already filtered by input.
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
                .take(12)
                .map(|f| (f.clone(), String::new()))
                .collect()
        }
        Overlay::Search { hits, .. } => hits
            .iter()
            .take(12)
            .map(|h| (h.path.to_string_lossy().to_string(), h.snippet.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn view<'a>(
    overlay: &'a Overlay,
    registry: &'a CommandRegistry,
    files: &'a [String],
) -> Element<'a, Message> {
    let rows = list_rows(overlay, registry, files);
    let selected = match overlay {
        Overlay::Palette { selected, .. }
        | Overlay::QuickOpen { selected, .. }
        | Overlay::Search { selected, .. } => *selected,
        _ => 0,
    };

    let input_line = {
        let shown = format!("{}▏", overlay.input());
        row![
            text(overlay.title()).size(13).color(colors::MARKER),
            text(shown).size(15).color(colors::TEXT),
        ]
        .spacing(12)
    };

    let mut list = column![].spacing(2);
    for (i, (title, detail)) in rows.iter().enumerate() {
        let marker = if i == selected { "▸ " } else { "  " };
        let line = row![
            text(format!("{marker}{title}")).size(14).color(if i == selected {
                colors::HEADING
            } else {
                colors::TEXT
            }),
            text(detail.clone()).size(12).color(colors::MARKER),
        ]
        .spacing(10);
        list = list.push(
            iced::widget::mouse_area(line).on_press(Message::OverlayPick(i)),
        );
    }

    let card = container(column![input_line, list].spacing(10).padding(14))
        .width(560)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.16, 0.16, 0.22,
            ))),
            border: iced::Border {
                color: iced::Color::from_rgb(0.35, 0.35, 0.5),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        });

    container(card)
        .center_x(Fill)
        .padding([60, 0])
        .into()
}

impl Overlay {
    pub fn input(&self) -> &str {
        match self {
            Overlay::Palette { input, .. }
            | Overlay::QuickOpen { input, .. }
            | Overlay::Search { input, .. }
            | Overlay::Find { input }
            | Overlay::PdfZoom { input }
            | Overlay::PdfPage { input } => input,
        }
    }
}
