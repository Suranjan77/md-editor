//! Phosphor SVG icon set (MASTER §4.6): regular stroke icons + filled variants for selected states.

use iced::widget::{container, svg};
use iced::{Color, Element, Length, Renderer, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Clock,
    Command,
    File,
    FileText,
    FilePdf,
    FilePlus,
    Folder,
    FolderOpen,
    FolderPlus,
    Image,
    Eraser,
    EraserFill,
    Highlighter,
    HighlighterFill,
    Lasso,
    LassoFill,
    LayoutPanelLeft,
    PanelRight,
    ListTree,
    Presentation,
    Link,
    Network,
    Pen,
    PenFill,
    Pencil,
    Redo,
    Undo,
    Search,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    ChevronLeft,
    Split,
    Trash,
    X,
    Plus,
    Minus,
    Maximize,
    FitWidth,
    FitPage,
    Refresh,
    Play,
    PlayFill,
    Pause,
    Pin,
    PinFill,
    Target,
    Gear,
    Sun,
    Moon,
    Desktop,
    Check,
    CheckCircle,
    Warning,
    WarningCircle,
    Info,
    DotsThree,
    Copy,
    Scissors,
    Clipboard,
    ArrowSquareOut,
    TextB,
    TextItalic,
    Code,
    Table,
    Math,
    CheckSquare,
    TextAa,
    Regex,
    TextLoose,
    Spinner,
    Eye,
}

impl Icon {
    /// Filled counterpart for a selected state, when one exists (MASTER §4.6).
    pub const fn selected_variant(self) -> Self {
        match self {
            Self::Pen => Self::PenFill,
            Self::Highlighter => Self::HighlighterFill,
            Self::Eraser => Self::EraserFill,
            Self::Lasso => Self::LassoFill,
            Self::Play => Self::PlayFill,
            Self::Pin => Self::PinFill,
            other => other,
        }
    }

    pub const fn svg_bytes(self) -> &'static [u8] {
        match self {
            Self::Clock => include_bytes!("../../assets/icons/clock.svg"),
            Self::Command => include_bytes!("../../assets/icons/command.svg"),
            Self::File => include_bytes!("../../assets/icons/file.svg"),
            Self::FileText => include_bytes!("../../assets/icons/file-text.svg"),
            Self::FilePdf => include_bytes!("../../assets/icons/file-pdf.svg"),
            Self::FilePlus => include_bytes!("../../assets/icons/file-plus.svg"),
            Self::Folder => include_bytes!("../../assets/icons/folder.svg"),
            Self::FolderOpen => include_bytes!("../../assets/icons/folder-open.svg"),
            Self::FolderPlus => include_bytes!("../../assets/icons/folder-plus.svg"),
            Self::Image => include_bytes!("../../assets/icons/image.svg"),
            Self::Eraser => include_bytes!("../../assets/icons/eraser.svg"),
            Self::EraserFill => include_bytes!("../../assets/icons/eraser-fill.svg"),
            Self::Highlighter => include_bytes!("../../assets/icons/highlighter.svg"),
            Self::HighlighterFill => include_bytes!("../../assets/icons/highlighter-fill.svg"),
            Self::Lasso => include_bytes!("../../assets/icons/lasso.svg"),
            Self::LassoFill => include_bytes!("../../assets/icons/lasso-fill.svg"),
            Self::LayoutPanelLeft => include_bytes!("../../assets/icons/sidebar.svg"),
            Self::PanelRight => include_bytes!("../../assets/icons/panel-right.svg"),
            Self::ListTree => include_bytes!("../../assets/icons/list-tree.svg"),
            Self::Presentation => include_bytes!("../../assets/icons/presentation.svg"),
            Self::Link => include_bytes!("../../assets/icons/link.svg"),
            Self::Network => include_bytes!("../../assets/icons/network.svg"),
            Self::Pen => include_bytes!("../../assets/icons/pen.svg"),
            Self::PenFill => include_bytes!("../../assets/icons/pen-fill.svg"),
            Self::Pencil => include_bytes!("../../assets/icons/pencil.svg"),
            Self::Redo => include_bytes!("../../assets/icons/redo.svg"),
            Self::Undo => include_bytes!("../../assets/icons/undo.svg"),
            Self::Search => include_bytes!("../../assets/icons/search.svg"),
            Self::ChevronDown => include_bytes!("../../assets/icons/chevron-down.svg"),
            Self::ChevronRight => include_bytes!("../../assets/icons/chevron-right.svg"),
            Self::ChevronUp => include_bytes!("../../assets/icons/chevron-up.svg"),
            Self::ChevronLeft => include_bytes!("../../assets/icons/chevron-left.svg"),
            Self::Split => include_bytes!("../../assets/icons/split.svg"),
            Self::Trash => include_bytes!("../../assets/icons/trash.svg"),
            Self::X => include_bytes!("../../assets/icons/x.svg"),
            Self::Plus => include_bytes!("../../assets/icons/plus.svg"),
            Self::Minus => include_bytes!("../../assets/icons/minus.svg"),
            Self::Maximize => include_bytes!("../../assets/icons/maximize.svg"),
            Self::FitWidth => include_bytes!("../../assets/icons/fit-width.svg"),
            Self::FitPage => include_bytes!("../../assets/icons/fit-page.svg"),
            Self::Refresh => include_bytes!("../../assets/icons/refresh.svg"),
            Self::Play => include_bytes!("../../assets/icons/play.svg"),
            Self::PlayFill => include_bytes!("../../assets/icons/play-fill.svg"),
            Self::Pause => include_bytes!("../../assets/icons/pause.svg"),
            Self::Pin => include_bytes!("../../assets/icons/pin.svg"),
            Self::PinFill => include_bytes!("../../assets/icons/pin-fill.svg"),
            Self::Target => include_bytes!("../../assets/icons/target.svg"),
            Self::Gear => include_bytes!("../../assets/icons/gear.svg"),
            Self::Sun => include_bytes!("../../assets/icons/sun.svg"),
            Self::Moon => include_bytes!("../../assets/icons/moon.svg"),
            Self::Desktop => include_bytes!("../../assets/icons/desktop.svg"),
            Self::Check => include_bytes!("../../assets/icons/check.svg"),
            Self::CheckCircle => include_bytes!("../../assets/icons/check-circle.svg"),
            Self::Warning => include_bytes!("../../assets/icons/warning.svg"),
            Self::WarningCircle => include_bytes!("../../assets/icons/warning-circle.svg"),
            Self::Info => include_bytes!("../../assets/icons/info.svg"),
            Self::DotsThree => include_bytes!("../../assets/icons/dots-three.svg"),
            Self::Copy => include_bytes!("../../assets/icons/copy.svg"),
            Self::Scissors => include_bytes!("../../assets/icons/scissors.svg"),
            Self::Clipboard => include_bytes!("../../assets/icons/clipboard.svg"),
            Self::ArrowSquareOut => include_bytes!("../../assets/icons/arrow-square-out.svg"),
            Self::TextB => include_bytes!("../../assets/icons/text-b.svg"),
            Self::TextItalic => include_bytes!("../../assets/icons/text-italic.svg"),
            Self::Code => include_bytes!("../../assets/icons/code.svg"),
            Self::Table => include_bytes!("../../assets/icons/table.svg"),
            Self::Math => include_bytes!("../../assets/icons/math.svg"),
            Self::CheckSquare => include_bytes!("../../assets/icons/check-square.svg"),
            Self::TextAa => include_bytes!("../../assets/icons/text-aa.svg"),
            Self::Regex => include_bytes!("../../assets/icons/regex.svg"),
            Self::TextLoose => include_bytes!("../../assets/icons/text-loose.svg"),
            Self::Spinner => include_bytes!("../../assets/icons/spinner.svg"),
            Self::Eye => include_bytes!("../../assets/icons/eye.svg"),
        }
    }

    pub fn for_vault_path(path: &str, is_dir: bool, expanded: bool) -> Self {
        if is_dir {
            return if expanded {
                Self::FolderOpen
            } else {
                Self::Folder
            };
        }
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".pdf") {
            Self::FilePdf
        } else if lower.ends_with(".pptx") {
            Self::Presentation
        } else if lower.ends_with(".ink") {
            Self::Pen
        } else if lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".gif")
            || lower.ends_with(".bmp")
            || lower.ends_with(".webp")
            || lower.ends_with(".svg")
            || lower.ends_with(".avif")
        {
            Self::Image
        } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
            Self::FileText
        } else {
            Self::File
        }
    }
}

/// Render a Phosphor SVG icon at `size` logical pixels tinted with `color`.
pub fn icon<'a, Message: 'a>(
    which: Icon,
    size: f32,
    color: Color,
) -> Element<'a, Message, Theme, Renderer> {
    let solid = Color { a: 1.0, ..color };
    let opacity = color.a.clamp(0.0, 1.0);
    let handle = svg::Handle::from_memory(which.svg_bytes());
    container(
        svg(handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .opacity(opacity)
            .style(move |_theme, _status| svg::Style { color: Some(solid) }),
    )
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .center_x(Length::Fixed(size))
    .center_y(Length::Fixed(size))
    .into()
}
