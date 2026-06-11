//! md3 shell library (ADR-0100: the only v3 crate that may know about iced).
//!
//! Split out of the binary so the update loop is integration-testable without
//! a window: tests construct [`app::App`], feed it [`app::Message`]s, and
//! assert on kernel state — the same code path a real keystroke takes.
//!
//! - [`keys`] — iced keyboard events → kernel [`md3_kernel::Chord`]s.
//! - [`app`] — the iced application: one keyboard subscription feeds
//!   `Workspace::handle_key`; resolved commands go through the kernel
//!   `CommandBus`; the view is generated from `PaneTree::layout()`.
//! - [`headless`] — the CLI modes CI runs (`--dump-shortcuts`, `--palette`,
//!   `--demo`).

pub mod app;
pub mod headless;
pub mod keys;
