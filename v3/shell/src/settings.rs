//! User settings v1: keymap overrides (plan §3.1 "user remapping is a JSON
//! file reusing the same table"). The kernel deliberately stays serde-free —
//! it exposes `Keymap::apply_override`/`remove`; this module owns the file
//! format and applies it at startup.
//!
//! `<vault>/.md3/keymap.json`:
//!
//! ```json
//! {
//!   "bindings": [
//!     { "scope": "markdown",  "chord": "ctrl+d", "command": "editor.select-all" },
//!     { "scope": "workspace", "chord": "ctrl+w", "command": null }
//!   ]
//! }
//! ```
//!
//! `command: null` unbinds the chord in that scope. Commands must name a
//! registered command (the registry is the source of `CommandId`s, which
//! keeps ids `'static` and typo-proof). Bad rows are skipped with a
//! warning, never fatal — a broken keymap file must not brick startup.

use std::path::Path;

use md3_kernel::input::{Binding, Chord, EditorKind, Keymap, Scope};
use md3_kernel::{CommandId, CommandRegistry};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct KeymapFile {
    #[serde(default)]
    bindings: Vec<BindingRow>,
}

#[derive(Debug, Deserialize)]
struct BindingRow {
    scope: String,
    chord: String,
    /// `None` (JSON `null` or absent) unbinds.
    #[serde(default)]
    command: Option<String>,
}

/// What applying the overrides file did — the caller decides how loudly to
/// report (the binary prints warnings to stderr; tests assert on them).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OverrideReport {
    pub applied: usize,
    pub removed: usize,
    pub warnings: Vec<String>,
}

/// Load `<root>/.md3/keymap.json` (if present) and apply it to `keymap`.
pub fn apply_keymap_overrides(
    root: &Path,
    registry: &CommandRegistry,
    keymap: &mut Keymap,
) -> OverrideReport {
    let path = root.join(".md3/keymap.json");
    let mut report = OverrideReport::default();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return report, // no overrides file — the common case
    };
    let file: KeymapFile = match serde_json::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            report.warnings.push(format!("keymap.json: {e}"));
            return report;
        }
    };
    for (i, row) in file.bindings.iter().enumerate() {
        let Some(scope) = parse_scope(&row.scope) else {
            report.warnings.push(format!(
                "keymap.json row {i}: unknown scope `{}`",
                row.scope
            ));
            continue;
        };
        let chord = match Chord::parse(&row.chord) {
            Ok(c) => c,
            Err(e) => {
                report.warnings.push(format!(
                    "keymap.json row {i}: bad chord `{}`: {e}",
                    row.chord
                ));
                continue;
            }
        };
        match &row.command {
            None => {
                keymap.remove(scope, chord);
                report.removed += 1;
            }
            Some(name) => {
                let Some(id) = command_id(registry, name) else {
                    report
                        .warnings
                        .push(format!("keymap.json row {i}: unknown command `{name}`"));
                    continue;
                };
                keymap.apply_override(Binding::new(scope, chord, id));
                report.applied += 1;
            }
        }
    }
    report
}

/// Resolve a command name against the registry so the binding carries the
/// registered `'static` id.
fn command_id(registry: &CommandRegistry, name: &str) -> Option<CommandId> {
    registry.specs().map(|s| s.id).find(|id| id.0 == name)
}

fn parse_scope(s: &str) -> Option<Scope> {
    Some(match s {
        "global" => Scope::Global,
        "workspace" => Scope::Workspace,
        "pane" => Scope::Pane,
        "markdown" => Scope::Editor(EditorKind::Markdown),
        "pdf" => Scope::Editor(EditorKind::Pdf),
        "image" => Scope::Editor(EditorKind::Image),
        "graph" => Scope::Editor(EditorKind::Graph),
        "overlay" => Scope::Overlay,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use md3_kernel::defaults::default_registry;

    fn setup() -> (tempfile::TempDir, CommandRegistry, Keymap) {
        let dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        };
        let registry = match default_registry() {
            Ok(r) => r,
            Err(e) => panic!("registry: {e}"),
        };
        let keymap = match registry.keymap() {
            Ok(k) => k,
            Err(e) => panic!("keymap: {e}"),
        };
        (dir, registry, keymap)
    }

    fn write_overrides(root: &Path, json: &str) {
        let dir = root.join(".md3");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            panic!("mkdir: {e}");
        }
        if let Err(e) = std::fs::write(dir.join("keymap.json"), json) {
            panic!("write: {e}");
        }
    }

    fn md_stack() -> Vec<Scope> {
        vec![
            Scope::Global,
            Scope::Workspace,
            Scope::Pane,
            Scope::Editor(EditorKind::Markdown),
        ]
    }

    #[test]
    fn missing_file_changes_nothing() {
        let (dir, registry, mut keymap) = setup();
        let before = keymap.bindings();
        let report = apply_keymap_overrides(dir.path(), &registry, &mut keymap);
        assert_eq!(report, OverrideReport::default());
        assert_eq!(keymap.bindings(), before);
    }

    #[test]
    fn overrides_rebind_unbind_and_skip_bad_rows() {
        let (dir, registry, mut keymap) = setup();
        write_overrides(
            dir.path(),
            r#"{ "bindings": [
                { "scope": "markdown",  "chord": "ctrl+d", "command": "editor.select-all" },
                { "scope": "workspace", "chord": "ctrl+w", "command": null },
                { "scope": "starship",  "chord": "ctrl+x", "command": "editor.undo" },
                { "scope": "markdown",  "chord": "ctrl+",  "command": "editor.undo" },
                { "scope": "markdown",  "chord": "ctrl+y", "command": "no.such.command" }
            ] }"#,
        );
        let report = apply_keymap_overrides(dir.path(), &registry, &mut keymap);
        assert_eq!(report.applied, 1);
        assert_eq!(report.removed, 1);
        assert_eq!(report.warnings.len(), 3, "{:#?}", report.warnings);

        // The rebind resolves in a markdown editor…
        let resolved = keymap.resolve(&md_stack(), Chord::ctrl('d'));
        assert_eq!(resolved.map(|c| c.0), Some("editor.select-all"));
        // …the unbind removed close-tab…
        assert_eq!(keymap.resolve(&md_stack(), Chord::ctrl('w')), None);
        // …and defaults the file never mentioned are untouched.
        let undo = keymap.resolve(&md_stack(), Chord::ctrl('z'));
        assert_eq!(undo.map(|c| c.0), Some("editor.undo"));
    }

    #[test]
    fn override_beats_a_default_on_the_same_chord() {
        let (dir, registry, mut keymap) = setup();
        write_overrides(
            dir.path(),
            r#"{ "bindings": [
                { "scope": "markdown", "chord": "ctrl+z", "command": "editor.redo" }
            ] }"#,
        );
        apply_keymap_overrides(dir.path(), &registry, &mut keymap);
        let resolved = keymap.resolve(&md_stack(), Chord::ctrl('z'));
        assert_eq!(
            resolved.map(|c| c.0),
            Some("editor.redo"),
            "user mapping replaces the default in that scope"
        );
        // The PDF scope's ctrl+z is untouched (scoping survives overrides).
        let pdf_stack = vec![
            Scope::Global,
            Scope::Workspace,
            Scope::Pane,
            Scope::Editor(EditorKind::Pdf),
        ];
        let resolved = keymap.resolve(&pdf_stack, Chord::ctrl('z'));
        assert_eq!(resolved.map(|c| c.0), Some("pdf.zoom-input"));
    }

    #[test]
    fn corrupt_json_is_a_warning_not_a_crash() {
        let (dir, registry, mut keymap) = setup();
        write_overrides(dir.path(), "{ not json");
        let before = keymap.bindings();
        let report = apply_keymap_overrides(dir.path(), &registry, &mut keymap);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(keymap.bindings(), before, "keymap untouched on parse error");
    }

    #[test]
    fn the_documented_example_parses() {
        // The module doc's example JSON must stay valid.
        let (dir, registry, mut keymap) = setup();
        write_overrides(
            dir.path(),
            r#"{
              "bindings": [
                { "scope": "markdown",  "chord": "ctrl+d", "command": "editor.select-all" },
                { "scope": "workspace", "chord": "ctrl+w", "command": null }
              ]
            }"#,
        );
        let report = apply_keymap_overrides(dir.path(), &registry, &mut keymap);
        assert!(report.warnings.is_empty(), "{:#?}", report.warnings);
        assert_eq!(report.applied + report.removed, 2);
    }
}
