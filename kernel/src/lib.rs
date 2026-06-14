//! Workspace kernel for md-editor v3 (plan §3.1).
//!
//! UI-free by construction: no toolkit types appear in this crate. The shell
//! translates toolkit events into [`input::Chord`]s, asks the kernel to resolve
//! them against the focused editor's scope stack, and executes the resulting
//! [`command::CommandId`]s.
//!
//! The four pillars, each killing a class of v2 bug by construction:
//! - [`command`]: every action is a registered command (palette/menus/docs are
//!   generated from the registry, never hand-maintained).
//! - [`input`]: one declarative keymap with scope-stack resolution and static
//!   conflict detection (kills BUG-A: stolen shortcuts).
//! - [`pane`]: PaneTree — documents open in tabs in panes; split is a layout
//!   choice, never a requirement (kills BUG-C: PDF-needs-split).
//! - [`focus`]: exactly one focused editor; all input flows through it first.

pub mod command;
pub mod defaults;
pub mod focus;
pub mod input;
pub mod pane;
pub mod workspace;

pub use command::{CommandBus, CommandId, CommandRegistry, CommandSpec, Invocation};
pub use focus::FocusModel;
pub use input::{Binding, Chord, EditorKind, Key, Keymap, KeymapError, Mods, Scope};
pub use pane::{DocumentId, DocumentStore, PaneId, PaneTree, SplitAxis, TabId};
pub use workspace::Workspace;
