//! InputRouter (plan §3.1): one declarative keymap, `(scope, chord) → command`,
//! resolved against the focused editor's scope stack, innermost scope wins.
//! Widgets never bind keys. Conflicts are detected statically when the keymap
//! is built — and therefore in CI, because a test builds the default keymap.
//!
//! This module is the structural fix for BUG-A (v2's Ctrl+Z collision): v2 had
//! a global `keyboard::listen()` *and* per-widget bindings firing in parallel
//! with no arbitration. Here there is exactly one table and one resolver.

use std::collections::HashMap;
use std::fmt;

use crate::command::CommandId;

/// Modifier keys held as part of a [`Chord`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        ..Mods::NONE
    };
    pub const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        ..Mods::NONE
    };
}

/// A logical, layout-independent key. Characters are stored lowercased; the
/// shell is responsible for normalizing toolkit events into this form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

/// A complete key chord, e.g. `ctrl+shift+p`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

impl Chord {
    pub const fn new(mods: Mods, key: Key) -> Chord {
        Chord { mods, key }
    }

    /// Convenience for the common `ctrl+<letter>` case.
    pub const fn ctrl(c: char) -> Chord {
        Chord {
            mods: Mods::CTRL,
            key: Key::Char(c),
        }
    }

    /// Parse the canonical textual form used by keymap files and docs:
    /// `"ctrl+shift+p"`, `"escape"`, `"f5"`. Case-insensitive; `+`-separated;
    /// the last segment is the key, everything before it a modifier.
    pub fn parse(s: &str) -> Result<Chord, ChordParseError> {
        let mut mods = Mods::NONE;
        let mut key: Option<Key> = None;
        let segments: Vec<&str> = s.split('+').map(str::trim).collect();
        let last = segments.len().saturating_sub(1);
        for (i, seg) in segments.iter().enumerate() {
            let seg_lower = seg.to_ascii_lowercase();
            if i < last {
                match seg_lower.as_str() {
                    "ctrl" | "control" => mods.ctrl = true,
                    "shift" => mods.shift = true,
                    "alt" => mods.alt = true,
                    "meta" | "cmd" | "super" => mods.meta = true,
                    _ => return Err(ChordParseError::UnknownModifier(seg.to_string())),
                }
            } else {
                key = Some(
                    parse_key(&seg_lower)
                        .ok_or_else(|| ChordParseError::UnknownKey(seg.to_string()))?,
                );
            }
        }
        match key {
            Some(key) => Ok(Chord { mods, key }),
            None => Err(ChordParseError::Empty),
        }
    }
}

fn parse_key(s: &str) -> Option<Key> {
    let key = match s {
        "" => return None,
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "space" => Key::Space,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        _ => {
            if let Some(n) = s.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                Key::F(n)
            } else {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Key::Char(c.to_ascii_lowercase()),
                    _ => return None,
                }
            }
        }
    };
    Some(key)
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.ctrl {
            write!(f, "ctrl+")?;
        }
        if self.mods.shift {
            write!(f, "shift+")?;
        }
        if self.mods.alt {
            write!(f, "alt+")?;
        }
        if self.mods.meta {
            write!(f, "meta+")?;
        }
        match self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::Enter => write!(f, "enter"),
            Key::Escape => write!(f, "escape"),
            Key::Tab => write!(f, "tab"),
            Key::Backspace => write!(f, "backspace"),
            Key::Delete => write!(f, "delete"),
            Key::Space => write!(f, "space"),
            Key::Up => write!(f, "up"),
            Key::Down => write!(f, "down"),
            Key::Left => write!(f, "left"),
            Key::Right => write!(f, "right"),
            Key::Home => write!(f, "home"),
            Key::End => write!(f, "end"),
            Key::PageUp => write!(f, "pageup"),
            Key::PageDown => write!(f, "pagedown"),
            Key::F(n) => write!(f, "f{n}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChordParseError {
    #[error("empty chord")]
    Empty,
    #[error("unknown modifier `{0}`")]
    UnknownModifier(String),
    #[error("unknown key `{0}`")]
    UnknownKey(String),
}

/// The kind of editor a pane tab hosts. An "editor" is a *view onto a
/// document* (plan §3.1); the kind selects which `Scope::Editor` bindings
/// apply while it is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EditorKind {
    Markdown,
    Pdf,
    Image,
    Graph,
}

/// Keymap scope, ordered outermost → innermost:
/// `Global < Workspace < Pane < Editor(kind) < Overlay`.
///
/// `Overlay` is a **modal fence**: while an overlay is on the scope stack,
/// resolution consults only `Overlay` and `Global` bindings. Workspace, pane
/// and editor bindings are unreachable — a go-to-page overlay can never leak
/// Ctrl+Z into the editor underneath (the BUG-A failure mode), and unbound
/// printable keys fall through to the overlay's text input as raw input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    Global,
    Workspace,
    Pane,
    Editor(EditorKind),
    Overlay,
}

/// One row of the declarative keymap table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub scope: Scope,
    pub chord: Chord,
    pub command: CommandId,
}

impl Binding {
    pub const fn new(scope: Scope, chord: Chord, command: CommandId) -> Binding {
        Binding {
            scope,
            chord,
            command,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    /// Two different commands claim the same chord in the same scope. Detected
    /// statically when the keymap is built; the default keymap is built in a
    /// CI test, so a conflicting default binding cannot merge.
    #[error("keymap conflict: {scope:?} `{chord}` bound to both `{first}` and `{second}`")]
    Conflict {
        scope: Scope,
        chord: Chord,
        first: CommandId,
        second: CommandId,
    },
}

/// The single keymap. There is no other place key bindings live.
#[derive(Debug, Default, Clone)]
pub struct Keymap {
    map: HashMap<(Scope, Chord), CommandId>,
}

impl Keymap {
    /// Build from bindings, statically rejecting conflicts. Identical
    /// duplicate rows are tolerated (idempotent registration).
    pub fn from_bindings<I>(bindings: I) -> Result<Keymap, KeymapError>
    where
        I: IntoIterator<Item = Binding>,
    {
        let mut map = HashMap::new();
        for b in bindings {
            if let Some(&existing) = map.get(&(b.scope, b.chord)) {
                if existing != b.command {
                    return Err(KeymapError::Conflict {
                        scope: b.scope,
                        chord: b.chord,
                        first: existing,
                        second: b.command,
                    });
                }
                continue;
            }
            map.insert((b.scope, b.chord), b.command);
        }
        Ok(Keymap { map })
    }

    /// User remapping (plan §3.1: "a JSON file reusing the same table").
    /// An override *replaces* whatever was bound — it cannot conflict.
    /// Parsing the keymap file is the shell's job; it feeds rows in here.
    pub fn apply_override(&mut self, binding: Binding) {
        self.map
            .insert((binding.scope, binding.chord), binding.command);
    }

    /// Remove a binding (user maps a chord to "nothing").
    pub fn remove(&mut self, scope: Scope, chord: Chord) -> Option<CommandId> {
        self.map.remove(&(scope, chord))
    }

    /// Resolve a chord against the active scope stack (outermost first, e.g.
    /// `[Global, Workspace, Pane, Editor(Markdown)]`). Innermost scope wins.
    ///
    /// Modal fence: if the stack's innermost scope is `Overlay`, only
    /// `Overlay` then `Global` are consulted (see [`Scope`] docs).
    pub fn resolve(&self, stack: &[Scope], chord: Chord) -> Option<CommandId> {
        let modal = stack.last() == Some(&Scope::Overlay);
        for &scope in stack.iter().rev() {
            if modal && !matches!(scope, Scope::Overlay | Scope::Global) {
                continue;
            }
            if let Some(&cmd) = self.map.get(&(scope, chord)) {
                return Some(cmd);
            }
        }
        None
    }

    /// All rows, for generated docs/palette hints. Sorted for determinism.
    pub fn bindings(&self) -> Vec<Binding> {
        let mut rows: Vec<Binding> = self
            .map
            .iter()
            .map(|(&(scope, chord), &command)| Binding {
                scope,
                chord,
                command,
            })
            .collect();
        rows.sort_by_key(|a| (a.scope, a.chord, a.command));
        rows
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_parse_round_trips_display() {
        for s in [
            "ctrl+z",
            "ctrl+shift+p",
            "escape",
            "f5",
            "alt+enter",
            "meta+k",
        ] {
            let chord = Chord::parse(s).unwrap_or_else(|e| panic!("parse {s}: {e}"));
            assert_eq!(chord.to_string(), s);
        }
    }

    #[test]
    fn chord_parse_rejects_garbage() {
        assert!(Chord::parse("").is_err());
        assert!(Chord::parse("hyper+z").is_err());
        assert!(Chord::parse("ctrl+zz").is_err());
    }

    #[test]
    fn conflict_is_detected_statically() {
        let undo = CommandId("editor.undo");
        let zoom = CommandId("pdf.zoom-input");
        let same_scope = [
            Binding::new(Scope::Editor(EditorKind::Markdown), Chord::ctrl('z'), undo),
            Binding::new(Scope::Editor(EditorKind::Markdown), Chord::ctrl('z'), zoom),
        ];
        match Keymap::from_bindings(same_scope) {
            Err(KeymapError::Conflict { first, second, .. }) => {
                assert_eq!(first, undo);
                assert_eq!(second, zoom);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn same_chord_in_different_editor_scopes_is_not_a_conflict() {
        let rows = [
            Binding::new(
                Scope::Editor(EditorKind::Markdown),
                Chord::ctrl('z'),
                CommandId("editor.undo"),
            ),
            Binding::new(
                Scope::Editor(EditorKind::Pdf),
                Chord::ctrl('z'),
                CommandId("pdf.zoom-input"),
            ),
        ];
        assert!(Keymap::from_bindings(rows).is_ok());
    }

    #[test]
    fn override_replaces_instead_of_conflicting() {
        let mut keymap = Keymap::from_bindings([Binding::new(
            Scope::Workspace,
            Chord::ctrl('p'),
            CommandId("file.quick-open"),
        )])
        .unwrap_or_else(|e| panic!("{e}"));
        keymap.apply_override(Binding::new(
            Scope::Workspace,
            Chord::ctrl('p'),
            CommandId("palette.open"),
        ));
        assert_eq!(
            keymap.resolve(&[Scope::Global, Scope::Workspace], Chord::ctrl('p')),
            Some(CommandId("palette.open"))
        );
    }
}
