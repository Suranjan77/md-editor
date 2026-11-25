//! Space (4px grid), control heights, radii, elevation shadows, icon sizes and layout constants (MASTER §4).
#![allow(dead_code)] // Full spacing/size scale (MASTER §4); not every step has a user yet.

use iced::{Color, Shadow, Vector};

use super::interface_scale;

// ── Spacing (4px grid, MASTER §4.1) ──────────────────────────────────

pub const SPACE_0: f32 = 0.0;
pub const SPACE_PX: f32 = 1.0;
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;
pub const SPACE_6: f32 = 32.0;
pub const SPACE_7: f32 = 48.0;

// ── Control sizes (MASTER §4.2, scaled by interface size) ────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlSize {
    Sm,
    Md,
    Lg,
}

impl ControlSize {
    pub const fn raw_height(self) -> f32 {
        match self {
            Self::Sm => 24.0,
            Self::Md => 28.0,
            Self::Lg => 36.0,
        }
    }

    pub fn height(self) -> f32 {
        (self.raw_height() * interface_scale()).round().max(24.0)
    }

    pub const fn horizontal_padding(self) -> f32 {
        match self {
            Self::Sm => SPACE_2,
            Self::Md => SPACE_3,
            Self::Lg => SPACE_4,
        }
    }
}

pub fn control_sm() -> f32 {
    ControlSize::Sm.height()
}

pub fn control_md() -> f32 {
    ControlSize::Md.height()
}

pub fn control_lg() -> f32 {
    ControlSize::Lg.height()
}

// ── Radius (MASTER §4.3) ─────────────────────────────────────────────

pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 10.0;
pub const RADIUS_XL: f32 = 14.0;
pub const RADIUS_FULL: f32 = 999.0;

// ── Icon sizes (MASTER §4.6, scaled by interface size) ───────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    Sm,
    Md,
    Lg,
}

impl IconSize {
    pub const fn raw_px(self) -> f32 {
        match self {
            Self::Sm => 14.0,
            Self::Md => 16.0,
            Self::Lg => 20.0,
        }
    }

    pub fn px(self) -> f32 {
        (self.raw_px() * interface_scale()).round().max(12.0)
    }
}

pub fn icon_sm() -> f32 {
    IconSize::Sm.px()
}

pub fn icon_md() -> f32 {
    IconSize::Md.px()
}

pub fn icon_lg() -> f32 {
    IconSize::Lg.px()
}

// ── Focus ring (MASTER §7) ───────────────────────────────────────────

pub const FOCUS_RING_WIDTH: f32 = 2.0;
pub const BORDER_WIDTH: f32 = 1.0;

// ── Elevation shadows (MASTER §4.4) ──────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    Flat,
    Popover,
    Dialog,
    Toast,
}

impl Elevation {
    pub const fn shadow(self) -> Shadow {
        match self {
            Self::Flat => Shadow {
                color: Color::TRANSPARENT,
                offset: Vector::new(0.0, 0.0),
                blur_radius: 0.0,
            },
            Self::Popover => Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.30,
                },
                offset: Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            Self::Dialog => Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.40,
                },
                offset: Vector::new(0.0, 12.0),
                blur_radius: 32.0,
            },
            Self::Toast => Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.40,
                },
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
        }
    }
}

// ── Shell layout dimensions ──────────────────────────────────────────

pub const TOOLBAR_HEIGHT: f32 = 40.0;
pub const TAB_STRIP_HEIGHT: f32 = 36.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const SPLIT_HANDLE_THICKNESS: f32 = 6.0;

pub const SIDEBAR_DEFAULT_WIDTH: f32 = 260.0;
pub const TOC_DEFAULT_WIDTH: f32 = 250.0;
pub const BACKLINKS_DEFAULT_WIDTH: f32 = 240.0;
pub const INSPECTOR_DEFAULT_WIDTH: f32 = 260.0;
pub const PANEL_MIN_WIDTH: f32 = 180.0;
pub const PANEL_MAX_WIDTH: f32 = 480.0;
/// Drag limits for the vault sidebar (`SplitHandle`, persisted in `layout_sidebar_width`).
pub const SIDEBAR_MIN_WIDTH: f32 = PANEL_MIN_WIDTH;
pub const SIDEBAR_MAX_WIDTH: f32 = PANEL_MAX_WIDTH;
/// Right-hand side panel (outline / backlinks / inspector) default and drag limits.
pub const SIDE_PANEL_DEFAULT_WIDTH: f32 = INSPECTOR_DEFAULT_WIDTH;
pub const SIDE_PANEL_MIN_WIDTH: f32 = PANEL_MIN_WIDTH;
pub const SIDE_PANEL_MAX_WIDTH: f32 = PANEL_MAX_WIDTH;

pub const DIALOG_WIDTH_SM: f32 = 420.0;
pub const DIALOG_WIDTH_MD: f32 = 540.0;
pub const DIALOG_WIDTH_LG: f32 = 640.0;
pub const PALETTE_WIDTH: f32 = 580.0;
/// Tallest the command palette's result list grows before it scrolls.
pub const PALETTE_MAX_LIST_HEIGHT: f32 = 360.0;
/// Size of the app mark on the welcome screen.
pub const WELCOME_LOGO_PX: f32 = 64.0;
pub const POPOVER_MAX_WIDTH: f32 = 420.0;
/// Height of the scrolling folder/note list in the link-note picker dialog.
pub const PICKER_LIST_HEIGHT: f32 = 240.0;
