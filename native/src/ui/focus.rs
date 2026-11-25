//! Region focus ring model (`F6` / `Shift+F6` cycling) and focus-border styling (MASTER §7).

use iced::{Border, Color};

use crate::design;
use crate::design::metrics::{BORDER_WIDTH, FOCUS_RING_WIDTH};

/// Top-level workspace regions cycled with `F6` / `Shift+F6` (UI overhaul §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusRegion {
    Toolbar,
    Sidebar,
    #[default]
    Main,
    SidePanel,
    StatusBar,
}

impl FocusRegion {
    pub const ORDER: [Self; 5] = [
        Self::Toolbar,
        Self::Sidebar,
        Self::Main,
        Self::SidePanel,
        Self::StatusBar,
    ];

    pub fn step(
        self,
        forward: bool,
        sidebar_open: bool,
        side_panel_open: bool,
        status_open: bool,
    ) -> Self {
        let available: Vec<Self> = Self::ORDER
            .into_iter()
            .filter(|r| match r {
                Self::Toolbar | Self::Main => true,
                Self::Sidebar => sidebar_open,
                Self::SidePanel => side_panel_open,
                Self::StatusBar => status_open,
            })
            .collect();
        if available.is_empty() {
            return Self::Main;
        }
        let pos = available.iter().position(|&r| r == self).unwrap_or(0);
        let delta: isize = if forward { 1 } else { -1 };
        let next = (pos as isize + delta).rem_euclid(available.len() as isize) as usize;
        available[next]
    }
}

/// Border with the 2px `focus.ring` when `focused` is true, else `rest_color` at 1px (or 0px).
pub fn ring_border(focused: bool, radius: f32, rest_color: Option<Color>) -> Border {
    if focused {
        Border {
            color: design::tokens().focus_ring,
            width: FOCUS_RING_WIDTH,
            radius: radius.into(),
        }
    } else if let Some(color) = rest_color {
        Border {
            color,
            width: BORDER_WIDTH,
            radius: radius.into(),
        }
    } else {
        Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius.into(),
        }
    }
}
