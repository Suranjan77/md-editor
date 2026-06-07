#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppShellPane {
    None,
    Markdown,
    Pdf,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppShellMode {
    NoVault,
    EmptyVault,
    EditorOnly,
    PdfOnly,
    ImageOnly,
    SplitResearch,
    SearchHeavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowSidebarTab {
    None,
    Backlinks,
    Annotations,
    Outline,
    Tracker,
}

impl WorkflowSidebarTab {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Backlinks => "backlinks",
            Self::Annotations => "annotations",
            Self::Outline => "outline",
            Self::Tracker => "tracker",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "backlinks" => Some(Self::Backlinks),
            "annotations" => Some(Self::Annotations),
            "outline" => Some(Self::Outline),
            "tracker" => Some(Self::Tracker),
            _ => None,
        }
    }
}

impl AppShellPane {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Markdown => "markdown",
            Self::Pdf => "pdf",
            Self::Image => "image",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "markdown" => Some(Self::Markdown),
            "pdf" => Some(Self::Pdf),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AppShellPersistence {
    pub sidebar_width: f32,
    pub reference_width: f32,
    pub workflow_width: f32,
    pub split_ratio: f32,
    pub sidebar_collapsed: bool,
    pub reference_collapsed: bool,
    pub workflow_collapsed: bool,
    pub active_workflow_tab: WorkflowSidebarTab,
    pub last_focused_pane: AppShellPane,
    pub theme: crate::theme::AppTheme,
    pub reduce_motion: bool,
}

impl Default for AppShellPersistence {
    fn default() -> Self {
        Self {
            sidebar_width: 260.0,
            reference_width: 360.0,
            workflow_width: 280.0,
            split_ratio: 0.5,
            sidebar_collapsed: false,
            reference_collapsed: false,
            workflow_collapsed: true,
            active_workflow_tab: WorkflowSidebarTab::None,
            last_focused_pane: AppShellPane::None,
            theme: crate::theme::AppTheme::Dark,
            reduce_motion: false,
        }
    }
}

impl AppShellPersistence {
    pub(crate) fn serialize(self) -> String {
        format!(
            "sidebar_width={};reference_width={};workflow_width={};split_ratio={};sidebar_collapsed={};reference_collapsed={};workflow_collapsed={};active_workflow_tab={};last_focused_pane={};theme={};reduce_motion={}",
            self.sidebar_width,
            self.reference_width,
            self.workflow_width,
            self.split_ratio,
            self.sidebar_collapsed,
            self.reference_collapsed,
            self.workflow_collapsed,
            self.active_workflow_tab.as_str(),
            self.last_focused_pane.as_str(),
            self.theme.as_str(),
            self.reduce_motion
        )
    }

    pub(crate) fn deserialize(value: &str) -> Option<Self> {
        let mut persistence = Self::default();
        let mut found_any = false;

        for part in value.split(';') {
            let (key, raw_value) = part.split_once('=')?;
            found_any = true;
            match key {
                "sidebar_width" => persistence.sidebar_width = raw_value.parse().ok()?,
                "reference_width" => persistence.reference_width = raw_value.parse().ok()?,
                "workflow_width" => persistence.workflow_width = raw_value.parse().ok()?,
                "split_ratio" => persistence.split_ratio = raw_value.parse().ok()?,
                "sidebar_collapsed" => {
                    persistence.sidebar_collapsed = parse_bool(raw_value)?;
                }
                "reference_collapsed" => {
                    persistence.reference_collapsed = parse_bool(raw_value)?;
                }
                "workflow_collapsed" => {
                    persistence.workflow_collapsed = parse_bool(raw_value)?;
                }
                "active_workflow_tab" => {
                    persistence.active_workflow_tab = WorkflowSidebarTab::from_str(raw_value)?;
                }
                "last_focused_pane" => {
                    persistence.last_focused_pane = AppShellPane::from_str(raw_value)?;
                }
                "theme" => {
                    persistence.theme = crate::theme::AppTheme::from_str(raw_value)?;
                }
                "reduce_motion" => {
                    persistence.reduce_motion = parse_bool(raw_value)?;
                }
                _ => {}
            }
        }

        found_any.then_some(persistence)
    }

    pub(crate) fn clamp_for_window(mut self, window_width: f32) -> Self {
        let narrow = window_width < 720.0;
        self.sidebar_width = self.sidebar_width.clamp(180.0, 360.0);
        self.reference_width = self.reference_width.clamp(260.0, 640.0);
        self.workflow_width = self.workflow_width.clamp(240.0, 420.0);
        self.split_ratio = self.split_ratio.clamp(0.25, 0.75);

        if narrow {
            self.reference_collapsed = true;
            self.workflow_collapsed = true;
        }

        self
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppShellInputs {
    pub vault_open: bool,
    pub vault_has_entries: bool,
    pub markdown_open: bool,
    pub pdf_open: bool,
    pub image_open: bool,
    pub split_requested: bool,
    pub search_visible: bool,
    pub command_palette_visible: bool,
    pub citation_palette_visible: bool,
    pub active_pane: AppShellPane,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AppShellState {
    pub mode: AppShellMode,
    pub active_pane: AppShellPane,
    pub persistence: AppShellPersistence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveStatus {
    NoDocument,
    Saved,
    Unsaved,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppShellStatusInputs {
    pub document_open: bool,
    pub document_dirty: bool,
    pub global_search_searching: bool,
    pub global_search_status: Option<String>,
    pub global_search_visible: bool,
    pub active_pane: AppShellPane,
    pub toast: Option<String>,
    pub background_error: Option<String>,
    pub background_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppShellStatus {
    pub save_status: SaveStatus,
    pub search_status: Option<String>,
    pub active_pane: AppShellPane,
    pub message: Option<String>,
}

impl AppShellStatus {
    pub(crate) fn derive(inputs: AppShellStatusInputs) -> Self {
        let save_status = if !inputs.document_open {
            SaveStatus::NoDocument
        } else if inputs.document_dirty {
            SaveStatus::Unsaved
        } else {
            SaveStatus::Saved
        };

        // Only show search status when the global search overlay is closed.
        // When the overlay is open it shows its own status; no need to duplicate.
        let search_status = if inputs.global_search_visible {
            None
        } else if inputs.global_search_searching {
            Some(
                inputs
                    .global_search_status
                    .clone()
                    .unwrap_or_else(|| "Searching...".to_string()),
            )
        } else {
            inputs.global_search_status.clone()
        };

        Self {
            save_status,
            search_status,
            active_pane: inputs.active_pane,
            message: inputs
                .toast
                .or(inputs.background_error)
                .or(inputs.background_status),
        }
    }
}

impl AppShellState {
    pub(crate) fn derive(inputs: AppShellInputs, persistence: AppShellPersistence) -> Self {
        let mode = if !inputs.vault_open {
            AppShellMode::NoVault
        } else if !inputs.vault_has_entries {
            AppShellMode::EmptyVault
        } else if inputs.search_visible
            || inputs.command_palette_visible
            || inputs.citation_palette_visible
        {
            AppShellMode::SearchHeavy
        } else if inputs.split_requested && inputs.markdown_open && inputs.pdf_open {
            AppShellMode::SplitResearch
        } else if inputs.pdf_open
            && (!inputs.markdown_open || inputs.active_pane == AppShellPane::Pdf)
        {
            AppShellMode::PdfOnly
        } else if inputs.image_open
            && (!inputs.markdown_open || inputs.active_pane == AppShellPane::Image)
        {
            AppShellMode::ImageOnly
        } else {
            AppShellMode::EditorOnly
        };

        let active_pane = match mode {
            AppShellMode::NoVault | AppShellMode::EmptyVault => AppShellPane::None,
            AppShellMode::EditorOnly => AppShellPane::Markdown,
            AppShellMode::PdfOnly => AppShellPane::Pdf,
            AppShellMode::ImageOnly => AppShellPane::Image,
            AppShellMode::SplitResearch => {
                if matches!(persistence.last_focused_pane, AppShellPane::Pdf) {
                    AppShellPane::Pdf
                } else {
                    AppShellPane::Markdown
                }
            }
            AppShellMode::SearchHeavy => persistence.last_focused_pane,
        };

        Self {
            mode,
            active_pane,
            persistence,
        }
    }

    pub(crate) fn command_groups(self) -> &'static [CommandGroup] {
        match self.mode {
            AppShellMode::NoVault | AppShellMode::EmptyVault => {
                &[CommandGroup::File, CommandGroup::View]
            }
            AppShellMode::EditorOnly => &[
                CommandGroup::File,
                CommandGroup::Edit,
                CommandGroup::Navigation,
                CommandGroup::View,
                CommandGroup::Research,
                CommandGroup::Search,
            ],
            AppShellMode::PdfOnly => &[
                CommandGroup::File,
                CommandGroup::Navigation,
                CommandGroup::View,
                CommandGroup::Annotation,
                CommandGroup::Search,
            ],
            AppShellMode::ImageOnly => &[
                CommandGroup::File,
                CommandGroup::Navigation,
                CommandGroup::View,
            ],
            AppShellMode::SplitResearch | AppShellMode::SearchHeavy => &[
                CommandGroup::File,
                CommandGroup::Edit,
                CommandGroup::Navigation,
                CommandGroup::View,
                CommandGroup::Research,
                CommandGroup::Annotation,
                CommandGroup::Search,
            ],
        }
    }

    pub(crate) fn uses_split_research_layout(self) -> bool {
        matches!(self.mode, AppShellMode::SplitResearch)
    }

    pub(crate) fn shows_pdf_document(self) -> bool {
        matches!(
            self.mode,
            AppShellMode::PdfOnly | AppShellMode::SplitResearch | AppShellMode::SearchHeavy
        )
    }

    pub(crate) fn shows_image_document(self) -> bool {
        matches!(self.mode, AppShellMode::ImageOnly)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandGroup {
    File,
    Edit,
    Navigation,
    View,
    Research,
    Annotation,
    Search,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> AppShellInputs {
        AppShellInputs {
            vault_open: true,
            vault_has_entries: true,
            markdown_open: false,
            pdf_open: false,
            image_open: false,
            split_requested: false,
            search_visible: false,
            command_palette_visible: false,
            citation_palette_visible: false,
            active_pane: AppShellPane::Markdown,
        }
    }

    #[test]
    fn derives_primary_layout_modes() {
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    vault_open: false,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::NoVault
        );
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    vault_has_entries: false,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::EmptyVault
        );
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    markdown_open: true,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::EditorOnly
        );
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    pdf_open: true,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::PdfOnly
        );
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    image_open: true,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::ImageOnly
        );
    }

    #[test]
    fn split_research_requires_markdown_and_pdf() {
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    markdown_open: true,
                    pdf_open: true,
                    split_requested: true,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::SplitResearch
        );
        assert_eq!(
            AppShellState::derive(
                AppShellInputs {
                    markdown_open: true,
                    split_requested: true,
                    ..inputs()
                },
                AppShellPersistence::default()
            )
            .mode,
            AppShellMode::EditorOnly
        );
    }

    #[test]
    fn search_and_palette_modes_override_document_layout() {
        for input in [
            AppShellInputs {
                search_visible: true,
                ..inputs()
            },
            AppShellInputs {
                command_palette_visible: true,
                ..inputs()
            },
            AppShellInputs {
                citation_palette_visible: true,
                ..inputs()
            },
        ] {
            assert_eq!(
                AppShellState::derive(input, AppShellPersistence::default()).mode,
                AppShellMode::SearchHeavy
            );
        }
    }

    #[test]
    fn persistence_clamps_widths_and_narrow_window_collapses_sidebars() {
        let persistence = AppShellPersistence {
            sidebar_width: 80.0,
            reference_width: 900.0,
            workflow_width: 900.0,
            split_ratio: 0.9,
            reference_collapsed: false,
            workflow_collapsed: false,
            ..AppShellPersistence::default()
        }
        .clamp_for_window(600.0);

        assert_eq!(persistence.sidebar_width, 180.0);
        assert_eq!(persistence.reference_width, 640.0);
        assert_eq!(persistence.workflow_width, 420.0);
        assert_eq!(persistence.split_ratio, 0.75);
        assert!(persistence.reference_collapsed);
        assert!(persistence.workflow_collapsed);
    }

    #[test]
    fn persistence_serializes_round_trip_leniently() {
        let persistence = AppShellPersistence {
            sidebar_width: 320.0,
            reference_width: 420.0,
            workflow_width: 300.0,
            split_ratio: 0.6,
            sidebar_collapsed: true,
            reference_collapsed: false,
            workflow_collapsed: false,
            active_workflow_tab: WorkflowSidebarTab::Outline,
            last_focused_pane: AppShellPane::Pdf,
            theme: crate::theme::AppTheme::Dark,
            reduce_motion: true,
        };

        let serialized = persistence.serialize();
        assert_eq!(
            AppShellPersistence::deserialize(&serialized),
            Some(persistence)
        );
        assert_eq!(
            AppShellPersistence::deserialize("unknown=value;split_ratio=0.4")
                .map(|saved| saved.split_ratio),
            Some(0.4)
        );
        assert!(AppShellPersistence::deserialize("sidebar_collapsed=maybe").is_none());
    }

    #[test]
    fn command_groups_match_layout_context() {
        let editor = AppShellState::derive(
            AppShellInputs {
                markdown_open: true,
                ..inputs()
            },
            AppShellPersistence::default(),
        );
        assert!(editor.command_groups().contains(&CommandGroup::Research));
        assert!(!editor.command_groups().contains(&CommandGroup::Annotation));

        let pdf = AppShellState::derive(
            AppShellInputs {
                pdf_open: true,
                ..inputs()
            },
            AppShellPersistence::default(),
        );
        assert!(pdf.command_groups().contains(&CommandGroup::Annotation));
        assert!(!pdf.command_groups().contains(&CommandGroup::Edit));
    }

    #[test]
    fn document_visibility_predicates_follow_layout_mode() {
        let split = AppShellState::derive(
            AppShellInputs {
                markdown_open: true,
                pdf_open: true,
                split_requested: true,
                ..inputs()
            },
            AppShellPersistence::default(),
        );
        assert!(split.uses_split_research_layout());
        assert!(split.shows_pdf_document());
        assert!(!split.shows_image_document());

        let image = AppShellState::derive(
            AppShellInputs {
                image_open: true,
                ..inputs()
            },
            AppShellPersistence::default(),
        );
        assert!(!image.uses_split_research_layout());
        assert!(!image.shows_pdf_document());
        assert!(image.shows_image_document());
    }

    #[test]
    fn status_model_reports_document_search_and_errors() {
        let status = AppShellStatus::derive(AppShellStatusInputs {
            document_open: true,
            document_dirty: true,
            global_search_searching: true,
            global_search_status: Some("Searched 3 PDFs".to_string()),
            global_search_visible: false,
            active_pane: AppShellPane::Pdf,
            toast: None,
            background_error: Some("Index failed".to_string()),
            background_status: Some("Indexing...".to_string()),
        });

        assert_eq!(status.save_status, SaveStatus::Unsaved);
        assert_eq!(status.search_status.as_deref(), Some("Searched 3 PDFs"));
        assert_eq!(status.active_pane, AppShellPane::Pdf);
        assert_eq!(status.message.as_deref(), Some("Index failed"));
    }

    #[test]
    fn status_model_prefers_toast_and_handles_no_document() {
        let status = AppShellStatus::derive(AppShellStatusInputs {
            document_open: false,
            document_dirty: false,
            global_search_searching: false,
            global_search_status: None,
            global_search_visible: false,
            active_pane: AppShellPane::None,
            toast: Some("Saved".to_string()),
            background_error: Some("Hidden".to_string()),
            background_status: Some("Also hidden".to_string()),
        });

        assert_eq!(status.save_status, SaveStatus::NoDocument);
        assert!(status.search_status.is_none());
        assert_eq!(status.message.as_deref(), Some("Saved"));
    }

    #[test]
    fn status_model_suppresses_search_status_when_overlay_visible() {
        let status = AppShellStatus::derive(AppShellStatusInputs {
            document_open: true,
            document_dirty: false,
            global_search_searching: true,
            global_search_status: Some("Searching...".to_string()),
            global_search_visible: true,
            active_pane: AppShellPane::Markdown,
            toast: None,
            background_error: None,
            background_status: None,
        });

        // When overlay is open, status bar should NOT duplicate the status text.
        assert!(status.search_status.is_none());
    }
}
