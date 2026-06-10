//! CommandBus + CommandRegistry (plan §3.1): every user action — menu item,
//! palette entry, key chord, click — dispatches a [`CommandId`]. The palette,
//! menus, and the shortcuts doc are *generated* from the registry; nothing is
//! hand-maintained in parallel (v2's drift between `commands.rs`, the global
//! key listener, and widget bindings is impossible here by construction).

use std::collections::HashMap;
use std::collections::VecDeque;
use std::fmt;

use crate::input::{Binding, Keymap, KeymapError};

/// Stable, human-readable command identifier, namespaced by surface:
/// `editor.undo`, `pdf.zoom-input`, `workspace.split-right`, `palette.open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommandId(pub &'static str);

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Declarative description of one command. Default bindings live here so the
/// keymap, palette hint, and docs are all views of the same row.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub id: CommandId,
    /// Palette/menu title, e.g. "Undo", "Split Right".
    pub title: &'static str,
    /// Palette grouping, e.g. "Editor", "Workspace", "PDF".
    pub category: &'static str,
    /// Default key bindings (may be empty; users can override all of them).
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("duplicate command id `{0}`")]
    DuplicateId(CommandId),
    #[error("binding on `{binding_for}` references a different command id `{refers_to}`")]
    ForeignBinding {
        binding_for: CommandId,
        refers_to: CommandId,
    },
}

/// The single source of truth for what the application can do.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    specs: Vec<CommandSpec>,
    index: HashMap<CommandId, usize>,
}

impl CommandRegistry {
    pub fn new() -> CommandRegistry {
        CommandRegistry::default()
    }

    pub fn register(&mut self, spec: CommandSpec) -> Result<(), RegistryError> {
        if self.index.contains_key(&spec.id) {
            return Err(RegistryError::DuplicateId(spec.id));
        }
        for b in &spec.bindings {
            if b.command != spec.id {
                return Err(RegistryError::ForeignBinding {
                    binding_for: spec.id,
                    refers_to: b.command,
                });
            }
        }
        self.index.insert(spec.id, self.specs.len());
        self.specs.push(spec);
        Ok(())
    }

    pub fn get(&self, id: CommandId) -> Option<&CommandSpec> {
        self.index.get(&id).and_then(|&i| self.specs.get(i))
    }

    /// All specs in registration order (stable for generated docs).
    pub fn specs(&self) -> impl Iterator<Item = &CommandSpec> {
        self.specs.iter()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Build the keymap from every spec's default bindings. Conflicts are
    /// reported here — at startup and in CI — never discovered at keypress
    /// time.
    pub fn keymap(&self) -> Result<Keymap, KeymapError> {
        Keymap::from_bindings(self.specs.iter().flat_map(|s| s.bindings.iter().copied()))
    }

    /// Palette query: case-insensitive subsequence match on title and id,
    /// title matches ranked before id-only matches. The palette is *free* —
    /// it is this method over the registry, no separate command list.
    pub fn palette(&self, query: &str) -> Vec<&CommandSpec> {
        let q = query.to_lowercase();
        let mut title_hits = Vec::new();
        let mut id_hits = Vec::new();
        for spec in &self.specs {
            if subsequence_match(&q, &spec.title.to_lowercase()) {
                title_hits.push(spec);
            } else if subsequence_match(&q, spec.id.0) {
                id_hits.push(spec);
            }
        }
        title_hits.extend(id_hits);
        title_hits
    }
}

fn subsequence_match(needle: &str, haystack: &str) -> bool {
    let mut hs = haystack.chars();
    needle.chars().all(|n| hs.any(|h| h == n))
}

/// A dispatched command instance. Args are free-form strings until the first
/// command needing structured payloads lands (shell M1 work).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub id: CommandId,
    pub args: Vec<String>,
}

/// FIFO bus: producers (keymap resolution, palette, menus) push invocations;
/// the application update loop drains and executes them. Single-threaded by
/// design — the kernel runs on the UI thread.
#[derive(Debug, Default)]
pub struct CommandBus {
    queue: VecDeque<Invocation>,
}

impl CommandBus {
    pub fn new() -> CommandBus {
        CommandBus::default()
    }

    pub fn dispatch(&mut self, id: CommandId) {
        self.queue.push_back(Invocation {
            id,
            args: Vec::new(),
        });
    }

    pub fn dispatch_with_args(&mut self, id: CommandId, args: Vec<String>) {
        self.queue.push_back(Invocation { id, args });
    }

    pub fn drain(&mut self) -> impl Iterator<Item = Invocation> + '_ {
        self.queue.drain(..)
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Chord, EditorKind, Scope};

    fn spec(id: &'static str, title: &'static str) -> CommandSpec {
        CommandSpec {
            id: CommandId(id),
            title,
            category: "Test",
            bindings: Vec::new(),
        }
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let mut reg = CommandRegistry::new();
        assert!(reg.register(spec("editor.undo", "Undo")).is_ok());
        assert_eq!(
            reg.register(spec("editor.undo", "Undo Again")),
            Err(RegistryError::DuplicateId(CommandId("editor.undo")))
        );
    }

    #[test]
    fn binding_must_reference_its_own_command() {
        let mut reg = CommandRegistry::new();
        let bad = CommandSpec {
            id: CommandId("editor.undo"),
            title: "Undo",
            category: "Editor",
            bindings: vec![Binding::new(
                Scope::Editor(EditorKind::Markdown),
                Chord::ctrl('z'),
                CommandId("editor.redo"),
            )],
        };
        assert!(matches!(
            reg.register(bad),
            Err(RegistryError::ForeignBinding { .. })
        ));
    }

    #[test]
    fn palette_ranks_title_matches_before_id_matches() {
        let mut reg = CommandRegistry::new();
        for s in [
            spec("editor.undo", "Undo"),
            spec("workspace.split-right", "Split Right"),
            spec("pdf.next-page", "Next Page"),
        ] {
            reg.register(s).unwrap_or_else(|e| panic!("{e}"));
        }
        let hits = reg.palette("split");
        assert_eq!(
            hits.first().map(|s| s.id),
            Some(CommandId("workspace.split-right"))
        );
        assert!(reg.palette("zzzz").is_empty());
    }

    #[test]
    fn bus_drains_in_dispatch_order() {
        let mut bus = CommandBus::new();
        bus.dispatch(CommandId("a"));
        bus.dispatch_with_args(CommandId("b"), vec!["42".to_string()]);
        let got: Vec<Invocation> = bus.drain().collect();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, CommandId("a"));
        assert_eq!(got[1].args, vec!["42".to_string()]);
        assert!(bus.is_empty());
    }
}
