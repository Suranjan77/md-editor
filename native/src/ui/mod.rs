//! Shared UI component library (MASTER §6), built exclusively from `crate::design` tokens.

#![allow(dead_code)] // Components are wired across views incrementally; gallery exercises every one.

pub mod badge;
pub mod button;
pub mod card;
pub mod context_anchor;
pub mod dialog;
pub mod empty_state;
pub mod focus;
pub mod icon;
pub mod icon_button;
pub mod kbd;
pub mod list_row;
pub mod menu;
pub mod panel;
pub mod popover;
pub mod segmented;
pub mod split_handle;
pub mod status_bar;
pub mod tabs;
pub mod text_field;
pub mod toast;
pub mod toggle;
pub mod toolbar;
pub mod tooltip;
pub mod tree_row;

#[cfg(test)]
mod gallery;

// Re-exported for views as they adopt each component; the gallery covers all of them.
#[allow(unused_imports)]
pub use badge::{BadgeTone, badge, filter_chip};
#[allow(unused_imports)]
pub use button::{Button, ButtonVariant};
#[allow(unused_imports)]
pub use card::{card, stat_card};
#[allow(unused_imports)]
pub use context_anchor::context_anchor;
#[allow(unused_imports)]
pub use dialog::dialog;
#[allow(unused_imports)]
pub use empty_state::{empty_state, progress_bar};
#[allow(unused_imports)]
pub use focus::{FocusRegion, ring_border};
#[allow(unused_imports)]
pub use icon::{Icon, icon};
#[allow(unused_imports)]
pub use icon_button::IconButton;
#[allow(unused_imports)]
pub use kbd::kbd;
#[allow(unused_imports)]
pub use list_row::{ListRowProps, ellipsize_single_line, list_row};
#[allow(unused_imports)]
pub use menu::{MenuEntry, menu, positioned_context_menu};
#[allow(unused_imports)]
pub use panel::{PanelSide, panel};
#[allow(unused_imports)]
pub use popover::popover;
#[allow(unused_imports)]
pub use segmented::{SegmentOption, segmented, segmented_choice};
#[allow(unused_imports)]
pub use split_handle::split_handle;
#[allow(unused_imports)]
pub use status_bar::{SaveStatus, StatusBarProps, status_bar};
#[allow(unused_imports)]
pub use tabs::{DocTabItem, TabStripEvents, UnderlineTab, tab_strip, underline_tabs};
#[allow(unused_imports)]
pub use text_field::TextField;
#[allow(unused_imports)]
pub use toast::{ToastItem, ToastKind, toast};
#[allow(unused_imports)]
pub use toggle::{CheckState, checkbox, toggle_switch};
#[allow(unused_imports)]
pub use toolbar::{toolbar_frame, toolbar_separator};
#[allow(unused_imports)]
pub use tooltip::{TooltipPosition, with_tooltip};
#[allow(unused_imports)]
pub use tree_row::{TreeRowProps, tree_row};
