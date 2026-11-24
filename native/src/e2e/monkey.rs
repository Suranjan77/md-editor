//! Randomised sessions (e2e plan, Step 2): seeded sequences of real user actions with the quiet-state invariants checked after each, shrunk to a minimal repro on failure.
//!
//! `E2E_SEED` picks the first seed (default: fixed, so `cargo test` is
//! reproducible), `E2E_CASES` the number of sessions (default 1), `E2E_STEPS`
//! the actions per session (default 25). A failure prints the seed and the
//! shrunk action list; rerun with that seed to reproduce. A larger run belongs
//! before releases: `E2E_CASES=20 E2E_STEPS=80 cargo test -p md-editor-native monkey`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use super::scenarios::research_vault;
use super::{Vault, boot};
use crate::editor::buffer::{Affinity, EditorCommand};
use crate::messages::{
    EditorMessage, GraphMessage, Message, SearchMessage, ShellMessage, Shortcut, VaultMessage,
};

/// SplitMix64: tiny, seedable, good enough to pick actions.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

/// One concrete user action. Recorded as generated, so a replay sends exactly
/// the same thing.
#[derive(Debug, Clone)]
enum Action {
    Open(String),
    /// Type and let autosave run.
    Type(String),
    /// Type and immediately do the next thing (edits still unsaved).
    TypeQuick(String),
    MoveCaret(usize),
    Undo,
    Redo,
    Save,
    Bold,
    CreateNote(String),
    Rename {
        from: String,
        to_name: String,
    },
    Delete(String),
    /// Another program edits the file on disk.
    ExternalEdit(String),
    Search(String),
    Palette(String),
    ToggleGraph,
    ToggleSplit,
    Escape,
}

const WORDS: &[&str] = &[
    "alpha ",
    "beta ",
    "[[Index]] ",
    "**bold** ",
    "$x$ ",
    "\n- item\n",
    "# H\n",
];

fn generate(rng: &mut Rng, probe: &crate::app::Probe, serial: usize) -> Action {
    let notes: Vec<String> = probe
        .entries
        .iter()
        .filter(|e| e.ends_with(".md"))
        .cloned()
        .collect();
    let files: Vec<String> = probe
        .entries
        .iter()
        .filter(|e| e.contains('.'))
        .cloned()
        .collect();
    let word = || WORDS[serial % WORDS.len()].to_string();
    match rng.below(17) {
        0..=2 => rng
            .pick(&files)
            .map_or(Action::Escape, |f| Action::Open(f.clone())),
        3 | 4 => Action::Type(word()),
        5 | 6 => Action::TypeQuick(word()),
        7 => Action::MoveCaret(rng.below(8)),
        8 => Action::Undo,
        9 => Action::Redo,
        10 => {
            if rng.below(2) == 0 {
                Action::Save
            } else {
                Action::Bold
            }
        }
        11 => Action::CreateNote(format!("monkey{serial}.md")),
        12 => match rng.pick(&notes) {
            Some(from) if notes.len() > 2 => Action::Rename {
                from: from.clone(),
                to_name: format!("renamed{serial}.md"),
            },
            _ => Action::Undo,
        },
        13 => match rng.pick(&notes) {
            Some(path) if notes.len() > 2 => Action::Delete(path.clone()),
            _ => Action::Redo,
        },
        14 => rng
            .pick(&notes)
            .map_or(Action::Escape, |p| Action::ExternalEdit(p.clone())),
        15 => match rng.below(4) {
            0 => Action::Search("alpha".to_string()),
            1 => Action::Palette("att".to_string()),
            2 => Action::ToggleGraph,
            _ => Action::ToggleSplit,
        },
        _ => Action::Escape,
    }
}

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

/// Perform `action`. Returns false when it no longer applies (its target is
/// gone) — possible while shrinking, when earlier actions were removed.
fn perform<P>(app: &mut super::Session<P>, vault: &Vault, action: &Action) -> bool
where
    P: iced_program::Program<State = crate::app::MdEditor, Message = Message> + 'static,
{
    let exists = |path: &str| vault.exists(path);
    match action {
        Action::Open(path) => {
            if !exists(path) {
                return false;
            }
            app.send(Message::Vault(VaultMessage::SidebarFileClicked(
                path.clone(),
            )));
        }
        Action::Type(text) => {
            if app.probe().active_path.is_none() {
                return false;
            }
            app.send(edit(EditorCommand::InsertText(text.clone())));
        }
        Action::TypeQuick(text) => {
            if app.probe().active_path.is_none() {
                return false;
            }
            app.send_quick(edit(EditorCommand::InsertText(text.clone())));
        }
        Action::MoveCaret(line) => app.send(edit(EditorCommand::SetCursor {
            line: *line,
            col: 0,
            affinity: Affinity::Downstream,
        })),
        Action::Undo => app.send(edit(EditorCommand::Undo)),
        Action::Redo => app.send(edit(EditorCommand::Redo)),
        Action::Save => app.send(Message::KeyboardShortcut(Shortcut::Save)),
        Action::Bold => {
            app.send(edit(EditorCommand::SelectAll));
            app.send(edit(EditorCommand::FormatBold));
        }
        Action::CreateNote(name) => {
            if exists(name) {
                return false;
            }
            app.send(Message::Vault(VaultMessage::SidebarSelectionCleared));
            app.send(Message::Vault(VaultMessage::CreateFileDialog));
            app.send(Message::Vault(VaultMessage::NameModalSubmit(name.clone())));
        }
        Action::Rename { from, to_name } => {
            let parent = std::path::Path::new(from)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let target = if parent.is_empty() {
                to_name.clone()
            } else {
                format!("{parent}/{to_name}")
            };
            if !exists(from) || exists(&target) {
                return false;
            }
            app.send_quick(Message::Vault(VaultMessage::RenameEntryDialog(
                from.clone(),
            )));
            app.send(Message::Vault(VaultMessage::NameModalSubmit(
                to_name.clone(),
            )));
        }
        Action::Delete(path) => {
            if !exists(path) {
                return false;
            }
            app.send(Message::Vault(VaultMessage::DeleteFile(path.clone())));
        }
        Action::ExternalEdit(path) => {
            if !exists(path) {
                return false;
            }
            let full = vault.root.join(path);
            let mut text = std::fs::read_to_string(&full).unwrap_or_default();
            text.push_str("external line\n");
            std::fs::write(&full, text).unwrap();
            // Let the watcher deliver it.
            app.wait(Duration::from_millis(700));
            app.settle();
        }
        Action::Search(query) => {
            app.send(Message::Search(SearchMessage::GlobalSearchOpen));
            app.send(Message::Search(SearchMessage::QueryChanged(query.clone())));
            app.send(Message::KeyboardShortcut(Shortcut::Escape));
        }
        Action::Palette(query) => {
            app.send(Message::Shell(ShellMessage::CommandPaletteOpen));
            app.send(Message::Shell(ShellMessage::CommandPaletteQueryChanged(
                query.clone(),
            )));
            app.send(Message::Shell(ShellMessage::OverlaySubmitCurrent));
        }
        Action::ToggleGraph => app.send(Message::Graph(GraphMessage::Toggle)),
        Action::ToggleSplit => app.send(Message::Shell(ShellMessage::SplitViewToggle)),
        Action::Escape => app.send(Message::KeyboardShortcut(Shortcut::Escape)),
    }
    true
}

/// Run `actions` from a fresh vault; `Err` carries the failure message.
fn replay(name: &str, actions: &[Action]) -> Result<(), String> {
    let vault = research_vault();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut app = boot(name, &vault, Some("Index.md"));
        for action in actions {
            perform(&mut app, &vault, action);
        }
        app.finish();
    }));
    outcome.map_err(panic_message)
}

/// Generate and run a fresh session from `seed`, recording what was done.
fn explore(name: &str, seed: u64, steps: usize) -> (Vec<Action>, Result<(), String>) {
    let vault = research_vault();
    let mut done = Vec::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut rng = Rng(seed);
        let mut app = boot(name, &vault, Some("Index.md"));
        for serial in 0..steps {
            let action = generate(&mut rng, &app.probe(), serial);
            done.push(action.clone());
            perform(&mut app, &vault, &action);
        }
        app.finish();
    }));
    (done, outcome.map_err(panic_message))
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "panic".to_string())
}

/// Delta-debugging shrink: drop chunks of actions while the run still fails.
/// Bounded, since every attempt boots the app.
fn shrink(mut actions: Vec<Action>) -> Vec<Action> {
    let mut attempts = 0;
    let mut chunk = actions.len() / 2;
    while chunk >= 1 && attempts < 30 {
        let mut start = 0;
        let mut removed_any = false;
        while start < actions.len() && attempts < 30 {
            let mut candidate = actions.clone();
            let end = (start + chunk).min(candidate.len());
            candidate.drain(start..end);
            attempts += 1;
            if !candidate.is_empty() && replay("monkey-shrink", &candidate).is_err() {
                actions = candidate;
                removed_any = true;
            } else {
                start += chunk;
            }
        }
        if !removed_any {
            chunk /= 2;
        }
    }
    actions
}

fn env_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn monkey_sessions_keep_the_invariants() {
    let first = env_or("E2E_SEED", 0x5EED_0001);
    let cases = env_or("E2E_CASES", 1);
    let steps = env_or("E2E_STEPS", 25) as usize;

    for case in 0..cases {
        let seed = first.wrapping_add(case);
        let (actions, outcome) = explore(&format!("monkey-{seed:x}"), seed, steps);
        if let Err(failure) = outcome {
            let minimal = shrink(actions.clone());
            panic!(
                "monkey session failed — reproduce with E2E_SEED={seed} E2E_CASES=1 E2E_STEPS={steps}\n\
                 failure: {failure}\n\
                 shrunk to {} of {} actions:\n  {}",
                minimal.len(),
                actions.len(),
                minimal
                    .iter()
                    .map(|a| format!("{a:?}"))
                    .collect::<Vec<_>>()
                    .join("\n  ")
            );
        }
    }
}

#[test]
fn rng_is_deterministic_per_seed() {
    let mut a = Rng(42);
    let mut b = Rng(42);
    let xs: Vec<u64> = (0..5).map(|_| a.next()).collect();
    let ys: Vec<u64> = (0..5).map(|_| b.next()).collect();
    assert_eq!(xs, ys);
    assert_ne!(Rng(43).next(), xs[0]);
}
