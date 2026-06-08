use crate::messages::{CitationItem, Message};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum CitationMessage {
    PaletteToggle,
    PaletteQueryChanged(String),
    PaletteSubmitFirst,
    PaletteChoose(CitationItem),
    ExcerptModeToggle,
    ExcerptQueueAdd(CitationItem),
    ExcerptQueueRemove(usize),
    ExcerptQueueClear,
    ExcerptQueueInsertBatch,
}

#[allow(dead_code, non_snake_case, non_upper_case_globals)]
impl Message {
    pub(crate) const CitationPaletteToggle: Self = Self::Citation(CitationMessage::PaletteToggle);
    pub(crate) const CitationPaletteSubmitFirst: Self =
        Self::Citation(CitationMessage::PaletteSubmitFirst);
    pub(crate) const ExcerptModeToggle: Self = Self::Citation(CitationMessage::ExcerptModeToggle);
    pub(crate) const ExcerptQueueClear: Self = Self::Citation(CitationMessage::ExcerptQueueClear);
    pub(crate) const ExcerptQueueInsertBatch: Self =
        Self::Citation(CitationMessage::ExcerptQueueInsertBatch);

    pub(crate) fn CitationPaletteQueryChanged(query: String) -> Self {
        Self::Citation(CitationMessage::PaletteQueryChanged(query))
    }

    pub(crate) fn CitationPaletteChoose(item: CitationItem) -> Self {
        Self::Citation(CitationMessage::PaletteChoose(item))
    }

    pub(crate) fn ExcerptQueueAdd(item: CitationItem) -> Self {
        Self::Citation(CitationMessage::ExcerptQueueAdd(item))
    }

    pub(crate) fn ExcerptQueueRemove(index: usize) -> Self {
        Self::Citation(CitationMessage::ExcerptQueueRemove(index))
    }
}
