//! Design system (UX-A): tokens, semantic palette, motion primitives.
//!
//! Views consume these instead of raw literals. Architecture rules: `design/`
//! imports nothing from `features/`; raw `Color::from_rgb` outside `design/`
//! (and the legacy `theme.rs` it wraps) is ratcheted down in `budgets.toml`.

pub(crate) mod motion;
pub(crate) mod palette;
pub(crate) mod tokens;
