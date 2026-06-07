use super::geometry::PdfRect;
use super::text::PdfTextRange;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum PdfAnnotationKind {
    Highlight,
    Note,
    Underline,
    Strike,
    AreaNote,
    FreeNote,
}

impl PdfAnnotationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Note => "Note",
            Self::Underline => "Underline",
            Self::Strike => "Strike",
            Self::AreaNote => "AreaNote",
            Self::FreeNote => "FreeNote",
        }
    }
}

impl std::str::FromStr for PdfAnnotationKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Highlight" => Ok(Self::Highlight),
            "Note" => Ok(Self::Note),
            "Underline" => Ok(Self::Underline),
            "Strike" => Ok(Self::Strike),
            "AreaNote" => Ok(Self::AreaNote),
            "FreeNote" => Ok(Self::FreeNote),
            _ => Err(format!("Unknown annotation kind: {s}")),
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum PdfAnnotationColor {
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
    Red,
    Purple,
}

impl PdfAnnotationColor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Pink => "Pink",
            Self::Orange => "Orange",
            Self::Red => "Red",
            Self::Purple => "Purple",
        }
    }
}

impl std::str::FromStr for PdfAnnotationColor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Yellow" => Ok(Self::Yellow),
            "Green" => Ok(Self::Green),
            "Blue" => Ok(Self::Blue),
            "Pink" => Ok(Self::Pink),
            "Orange" => Ok(Self::Orange),
            "Red" => Ok(Self::Red),
            "Purple" => Ok(Self::Purple),
            _ => Err(format!("Unknown annotation color: {s}")),
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum PdfAnnotationStatus {
    Unresolved,
    Resolved,
}

impl PdfAnnotationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unresolved => "Unresolved",
            Self::Resolved => "Resolved",
        }
    }
}

impl std::str::FromStr for PdfAnnotationStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Unresolved" => Ok(Self::Unresolved),
            "Resolved" => Ok(Self::Resolved),
            _ => Err(format!("Unknown annotation status: {s}")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfAnnotation {
    pub id: String,
    pub document_id: String,
    pub page_index: u16,
    pub kind: PdfAnnotationKind,
    pub color: PdfAnnotationColor,
    pub selected_text: String,
    pub ranges: Vec<PdfTextRange>,
    pub rects: Vec<PdfRect>,
    pub note: Option<String>,
    pub linked_note_path: Option<String>,
    pub markdown_anchor: Option<String>,
    pub tags: Vec<String>,
    pub status: PdfAnnotationStatus,
    pub created_at: i64,
    pub updated_at: i64,
}
