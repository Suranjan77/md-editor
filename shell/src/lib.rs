//! md-editor shell library (ADR-0100: the only crate that may know about iced).
//!
//! Split out of the binary so the update loop is integration-testable without
//! a window: tests construct [`gui::Shell`], feed it [`gui::Message`]s, and
//! assert on kernel state — the same code path a real keystroke takes.
//!
//! - [`gui`] — the iced application: one keyboard subscription feeds
//!   `Workspace::handle_key`; the view is generated from `PaneTree::layout()`;
//!   markdown paints through the engine's 3-phase layout on a canvas, PDFs
//!   through the tile renderer (behind the `pdfium` feature).
//! - [`headless`] — the CLI modes CI runs (`--dump-shortcuts`, `--palette`,
//!   `--demo`).
//! - [`settings`] — user keymap overrides from `<vault>/.md-editor/keymap.json`
//!   (plan §3.1), applied to the kernel keymap at startup.

pub mod desktop;
pub mod gui;
pub mod headless;
pub mod paths;
pub mod settings;
pub mod vault_picker;
