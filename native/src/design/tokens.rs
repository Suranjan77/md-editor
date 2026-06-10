//! Non-color design tokens: spacing, radius, type scale, motion durations,
//! easing, elevation, z-layers.
//!
//! The legacy spacing/radius/font constants in `theme.rs` remain as aliases
//! during the UXA.T2a–g migration; new code uses these.

#![allow(dead_code)] // consumed incrementally as views migrate (UXA.T2a–g)

use std::time::Duration;

// ── Spacing scale (4/8/12/16/24/32) ─────────────────────────────────
pub(crate) const SPACE_1: f32 = 4.0;
pub(crate) const SPACE_2: f32 = 8.0;
pub(crate) const SPACE_3: f32 = 12.0;
pub(crate) const SPACE_4: f32 = 16.0;
pub(crate) const SPACE_6: f32 = 24.0;
pub(crate) const SPACE_8: f32 = 32.0;

// ── Radius scale ─────────────────────────────────────────────────────
pub(crate) const RADIUS_S: f32 = 2.0;
pub(crate) const RADIUS_M: f32 = 4.0;
pub(crate) const RADIUS_L: f32 = 8.0;

// ── Type scale: (size, line_height) — line heights are 4px multiples ─
pub(crate) const TYPE_CAPTION: (u16, f32) = (12, 16.0);
pub(crate) const TYPE_BODY_S: (u16, f32) = (13, 20.0);
pub(crate) const TYPE_BODY: (u16, f32) = (14, 20.0);
pub(crate) const TYPE_BODY_L: (u16, f32) = (16, 24.0);
pub(crate) const TYPE_TITLE_S: (u16, f32) = (20, 28.0);
pub(crate) const TYPE_TITLE: (u16, f32) = (24, 32.0);
pub(crate) const TYPE_DISPLAY: (u16, f32) = (32, 40.0);

// ── Motion duration tokens ───────────────────────────────────────────
pub(crate) const DURATION_FAST: Duration = Duration::from_millis(90);
pub(crate) const DURATION_BASE: Duration = Duration::from_millis(160);
pub(crate) const DURATION_SLOW: Duration = Duration::from_millis(240);

// ── Elevation (shadow blur radii; color comes from palette) ──────────
pub(crate) const ELEVATION_1: f32 = 4.0;
pub(crate) const ELEVATION_2: f32 = 12.0;
pub(crate) const ELEVATION_3: f32 = 24.0;

// ── Z-layers (drawing order contracts for overlays) ──────────────────
pub(crate) const Z_CONTENT: u8 = 0;
pub(crate) const Z_PANEL: u8 = 10;
pub(crate) const Z_OVERLAY: u8 = 20;
pub(crate) const Z_MODAL: u8 = 30;
pub(crate) const Z_TOAST: u8 = 40;

// ── Focus & hairlines (shared with legacy theme.rs values) ───────────
pub(crate) const FOCUS_RING_WIDTH: f32 = 2.0;
pub(crate) const FOCUS_RING_OFFSET: f32 = 1.0;
pub(crate) const HAIRLINE: f32 = 1.0;

// ── Standard interaction timings ─────────────────────────────────────
pub(crate) const TOOLTIP_DELAY: Duration = Duration::from_millis(600);
