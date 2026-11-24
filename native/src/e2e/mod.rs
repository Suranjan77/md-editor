//! Headless end-to-end tests: the real program (`main::program`) running in
//! `iced_test`'s emulator — real update loop, tasks, subscriptions and
//! rendering, no window and no human.
//!
//! Every session gets its own vault in a temp directory, and the whole test
//! process uses a private data directory, so a run never reads or writes the
//! user's settings. Telemetry (`telemetry.rs`) records the same spans iced's
//! Comet debugger shows, through the same beacon protocol, and writes a report.
//!
//! Inputs: widget-level clicks and typing go through the emulator as real
//! events. App-level shortcuts are handled by a subscription, which the
//! emulator does not feed raw keys to, so scenarios send their messages
//! (`Message::KeyboardShortcut(..)`) directly.
//!
//! Artifacts land in `target/e2e/`: screenshots per step and `telemetry.md`.

// The harness API is ahead of the scenarios written so far; the planned
// suite is in docs/e2e-bug-discovery-plan.md.
#![allow(dead_code)]

mod scenarios;
mod telemetry;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use iced::mouse;
use iced_program::Program;
use iced_test::Instruction;
use iced_test::emulator::{self, Emulator, Mode};
use iced_test::futures::futures::channel::mpsc;
use iced_test::instruction::{Expectation, Interaction, Keyboard, Mouse, Target};

use crate::app::MdEditor;
use crate::messages::Message;

/// The app state as of the last update, captured by [`telemetry::Observe`].
/// Sessions run one at a time (`SERIAL`), so one slot is enough.
static LATEST: Mutex<Option<crate::app::Probe>> = Mutex::new(None);

impl telemetry::Observe for MdEditor {
    fn observe(&self) {
        *LATEST.lock().unwrap_or_else(|p| p.into_inner()) = Some(self.probe());
    }
}

/// The app state after the most recent update.
pub(crate) fn latest_probe() -> crate::app::Probe {
    LATEST
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .expect("the app has booted")
}

/// Default per-update time budget for the invariant check (debug build).
const UPDATE_BUDGET: Duration = Duration::from_millis(100);

/// Viewport every session runs at; matches the app's default window size.
pub(crate) const VIEWPORT: iced::Size = iced::Size::new(1200.0, 800.0);

/// Emulator sessions share the settings database and the beacon, so they run
/// one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// Where screenshots and the telemetry report are written.
pub(crate) fn artifacts_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("e2e");
    std::fs::create_dir_all(&dir).expect("create target/e2e");
    dir
}

fn isolate_data_dir() {
    crate::test_support::isolate_data_dir();
}

/// A throwaway vault on disk.
pub(crate) struct Vault {
    pub root: PathBuf,
}

impl Vault {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "md-editor-e2e-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create vault");
        Self { root }
    }

    pub fn file(self, rel: &str, contents: &str) -> Self {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        self
    }

    /// Copy a file from `tests-fixtures/`.
    pub fn fixture(self, rel: &str, fixture: &str) -> Self {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests-fixtures")
            .join(fixture);
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(from, path).expect("copy fixture");
        self
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root.join(rel)).unwrap_or_default()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).exists()
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One running app.
pub(crate) struct Session<P: Program<State = MdEditor, Message = Message>> {
    name: String,
    program: P,
    emulator: Emulator<P>,
    events: mpsc::Receiver<emulator::Event<P>>,
    failures: Vec<String>,
    /// Our own renderer for screenshots. `Emulator::screenshot` in
    /// iced_test 0.14.0 takes the emulator's UI cache and never returns it, so
    /// any interaction after it panics; rendering separately avoids that.
    renderer: P::Renderer,
    shot: usize,
    mark: usize,
    /// The action being settled, for invariant failure messages.
    step: String,
    /// Whether an error toast is expected (cleared after each check).
    expect_error: bool,
    update_budget: Duration,
    /// Updates already reported as slow, so each is reported once.
    slow_seen: usize,
    _serial: MutexGuard<'static, ()>,
}

/// Boot the app on `vault` as a vault, then open `open` (vault-relative) from
/// the sidebar, so paths stay relative to the vault root.
pub(crate) fn boot(
    name: &str,
    vault: &Vault,
    open: Option<&str>,
) -> Session<impl Program<State = MdEditor, Message = Message> + 'static> {
    let mut session = boot_with_path(name, vault.root.clone());
    if let Some(rel) = open {
        session.send(Message::Vault(
            crate::messages::VaultMessage::SidebarFileClicked(rel.to_string()),
        ));
    }
    session
}

/// Boot the app exactly as `md-editor <path>` would. A file path makes its
/// folder the vault (unless it is inside the remembered vault).
pub(crate) fn boot_with_path(
    name: &str,
    startup: PathBuf,
) -> Session<impl Program<State = MdEditor, Message = Message> + 'static> {
    let serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    isolate_data_dir();
    *LATEST.lock().unwrap_or_else(|p| p.into_inner()) = None;
    let mark = telemetry::recorder().mark();

    let window = iced::window::Settings {
        size: VIEWPORT,
        ..Default::default()
    };
    let program = telemetry::Traced(crate::program(window, Some(startup)));

    let (sender, events) = mpsc::channel(1024);
    let emulator = Emulator::new(sender, &program, Mode::Zen, VIEWPORT);
    let renderer = headless_renderer(&program);
    let mut session = Session {
        name: name.to_string(),
        program,
        emulator,
        events,
        failures: Vec::new(),
        renderer,
        shot: 0,
        mark,
        step: "boot".to_string(),
        expect_error: false,
        update_budget: UPDATE_BUDGET,
        slow_seen: 0,
        _serial: serial,
    };
    session.settle();
    session
}

fn headless_renderer<P: Program>(program: &P) -> P::Renderer {
    use iced_test::core::renderer::Headless;
    let settings = program.settings();
    iced_test::futures::futures::executor::block_on(P::Renderer::new(
        settings.default_font,
        settings.default_text_size,
        None,
    ))
    .expect("create a headless renderer")
}

impl<P: Program<State = MdEditor, Message = Message> + 'static> Session<P> {
    /// Run the event loop until nothing has happened for a moment: tasks
    /// done, debounces (autosave, search) fired, the vault watcher quiet.
    pub fn settle(&mut self) {
        self.pump(Duration::from_millis(600), Duration::from_secs(15));
        self.check_invariants();
    }

    /// The next settle may show an error toast without failing the scenario.
    pub fn expect_error_toast(&mut self) {
        self.expect_error = true;
    }

    /// Raise (or lower) the per-update budget for this session.
    pub fn set_update_budget(&mut self, budget: Duration) {
        self.update_budget = budget;
    }

    /// The app state as of the last update.
    pub fn probe(&self) -> crate::app::Probe {
        latest_probe()
    }

    /// Checks that must hold whenever the app is quiet. Panics naming the step
    /// that broke them, so a failure points at the action, not the assertion.
    fn check_invariants(&mut self) {
        let probe = latest_probe();
        let violations = invariant_violations(&probe, self.expect_error);
        self.expect_error = false;

        let slow = self.slow_updates(self.update_budget);
        let new_slow: Vec<_> = slow.iter().skip(self.slow_seen).cloned().collect();
        self.slow_seen = slow.len();

        let mut problems = violations;
        for (duration, message) in new_slow {
            problems.push(format!(
                "update took {duration:.1?} (budget {:?}): {message}",
                self.update_budget
            ));
        }
        if !problems.is_empty() {
            let shot = self.screenshot("invariant-failure");
            panic!(
                "[{}] invariants broken after `{}`:\n  - {}\n(screenshot: {})",
                self.name,
                self.step,
                problems.join("\n  - "),
                shot.display()
            );
        }
    }

    /// Run the event loop for a fixed time regardless of activity.
    pub fn wait(&mut self, duration: Duration) {
        self.step = format!("wait {duration:?}");
        self.pump(duration, duration);
    }

    fn pump(&mut self, quiet: Duration, max: Duration) {
        let start = Instant::now();
        let mut last_event = Instant::now();
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    last_event = Instant::now();
                    match event {
                        emulator::Event::Action(action) => {
                            self.emulator.perform(&self.program, action)
                        }
                        emulator::Event::Failed(instruction) => {
                            self.failures.push(instruction.to_string())
                        }
                        emulator::Event::Ready => {}
                    }
                }
                Err(mpsc::TryRecvError::Closed) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    if last_event.elapsed() >= quiet || start.elapsed() >= max {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }

    /// Deliver a message without letting the event loop run — so, for
    /// example, edits are still unsaved (autosave waits 400 ms) when the next
    /// message arrives.
    pub fn send_quick(&mut self, message: Message) {
        self.step = format!("send_quick {message:?}");
        self.emulator.update(&self.program, message);
        // Long enough for the tasks it spawns (a toast) to land, well short
        // of the 400 ms autosave debounce.
        self.pump(Duration::from_millis(15), Duration::from_millis(100));
    }

    /// Insert text at the caret of the open note, as typing would.
    pub fn type_in_note(&mut self, text: &str) {
        self.send(Message::Editor(crate::messages::EditorMessage::Command(
            crate::editor::buffer::EditorCommand::InsertText(text.to_string()),
        )));
    }

    /// Deliver a message as if the app had produced it, then settle.
    pub fn send(&mut self, message: Message) {
        self.step = format!("send {message:?}");
        self.emulator.update(&self.program, message);
        self.settle();
    }

    fn instruct(&mut self, instruction: Instruction) -> Result<(), String> {
        self.step = instruction.to_string();
        let before = self.failures.len();
        self.emulator.run(&self.program, instruction);
        self.settle();
        match self.failures.get(before) {
            Some(failure) => Err(failure.clone()),
            None => Ok(()),
        }
    }

    /// Click the widget showing `text`.
    pub fn click(&mut self, text: &str) -> Result<(), String> {
        self.instruct(Instruction::Interact(Interaction::Mouse(Mouse::Click {
            button: mouse::Button::Left,
            target: Some(Target::Text(text.to_string())),
        })))
    }

    /// Click a point of the viewport.
    pub fn click_at(&mut self, x: f32, y: f32) {
        let _ = self.instruct(Instruction::Interact(Interaction::Mouse(Mouse::Click {
            button: mouse::Button::Left,
            target: Some(Target::Point(iced::Point::new(x, y))),
        })));
    }

    /// Type text into whatever has keyboard focus.
    pub fn type_text(&mut self, text: &str) {
        let _ = self.instruct(Instruction::Interact(Interaction::Keyboard(
            Keyboard::Typewrite(text.to_string()),
        )));
    }

    /// Whether some widget currently shows `text`.
    pub fn shows(&mut self, text: &str) -> bool {
        self.instruct(Instruction::Expect(Expectation::Text(text.to_string())))
            .is_ok()
    }

    /// Render the current frame to `target/e2e/<session>-<n>-<label>.png`.
    pub fn screenshot(&mut self, label: &str) -> PathBuf {
        use iced_test::core::renderer::Headless as _;
        use iced_test::core::{Event, clipboard, renderer, theme, window};
        use iced_test::runtime::user_interface::{Cache, UserInterface};

        let theme = self
            .emulator
            .theme(&self.program)
            .expect("the app always sets a theme");
        let style = theme::Base::base(&theme);
        let size = iced::Size::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
        let mut cache = Cache::default();
        let mut rgba = Vec::new();
        // Two frames: on the first, text widgets have only just shaped their
        // paragraphs and a fresh renderer draws none of them.
        for _ in 0..2 {
            let mut ui = UserInterface::build(
                self.emulator.view(&self.program),
                VIEWPORT,
                cache,
                &mut self.renderer,
            );
            let _ = ui.update(
                &[Event::Window(
                    window::Event::RedrawRequested(Instant::now()),
                )],
                mouse::Cursor::Unavailable,
                &mut self.renderer,
                &mut clipboard::Null,
                &mut Vec::new(),
            );
            ui.draw(
                &mut self.renderer,
                &theme,
                &renderer::Style {
                    text_color: style.text_color,
                },
                mouse::Cursor::Unavailable,
            );
            cache = ui.into_cache();
            rgba = self.renderer.screenshot(size, 1.0, style.background_color);
        }

        self.shot += 1;
        let path = artifacts_dir().join(format!("{}-{:02}-{label}.png", self.name, self.shot));
        image::RgbaImage::from_raw(size.width, size.height, rgba)
            .expect("screenshot buffer matches its size")
            .save(&path)
            .expect("write screenshot");
        path
    }

    /// End the session: write its telemetry and hand back the final state.
    pub fn finish(self) -> crate::app::Probe {
        telemetry::recorder().report(&self.name, self.mark);
        self.emulator.into_state().0.probe()
    }

    /// Updates since this session started that took longer than `budget`.
    pub fn slow_updates(&self, budget: Duration) -> Vec<(Duration, String)> {
        telemetry::recorder().slow_updates(self.mark, budget)
    }
}

/// The quiet-state invariants (e2e plan, Step 0). Pure, so it is unit-tested.
pub(crate) fn invariant_violations(probe: &crate::app::Probe, expect_error: bool) -> Vec<String> {
    let mut problems = Vec::new();

    if let Some(toast) = &probe.toast
        && !expect_error
        && is_error_toast(toast)
    {
        problems.push(format!("unexpected error toast: {toast:?}"));
    }

    if let (Some(root), Some(path)) = (&probe.vault_root, &probe.active_path)
        && !probe.dirty
        && md_editor_core::vault::is_markdown_path(Path::new(path))
        && probe.pdf_path.is_none()
    {
        let on_disk = std::fs::read_to_string(Path::new(root).join(path));
        match on_disk {
            Ok(text) if text != probe.buffer => problems.push(format!(
                "{path} is clean but differs from disk: buffer {:?}, disk {:?}",
                truncate(&probe.buffer),
                truncate(&text)
            )),
            Err(error) => {
                problems.push(format!("{path} is open and clean but unreadable: {error}"))
            }
            Ok(_) => {}
        }
    }

    if let Some(root) = &probe.vault_root
        && let Ok(listed) = md_editor_core::vault::list_vault_entries(Path::new(root))
    {
        let mut on_disk: Vec<String> = listed.into_iter().map(|e| e.path).collect();
        let mut shown = probe.entries.clone();
        on_disk.sort();
        shown.sort();
        if on_disk != shown {
            let missing: Vec<_> = on_disk.iter().filter(|p| !shown.contains(p)).collect();
            let extra: Vec<_> = shown.iter().filter(|p| !on_disk.contains(p)).collect();
            problems.push(format!(
                "file tree out of sync with disk: missing {missing:?}, stale {extra:?}"
            ));
        }
    }

    if probe.pdf_pending_pages > 0 {
        problems.push(format!(
            "{} PDF page renders still pending after settling",
            probe.pdf_pending_pages
        ));
    }

    problems
}

/// Toasts are plain strings; error toasts say so.
fn is_error_toast(toast: &str) -> bool {
    let lower = toast.to_lowercase();
    ["fail", "error", "could not", "cannot", "can't", "unable"]
        .iter()
        .any(|word| lower.contains(word))
}

fn truncate(text: &str) -> String {
    text.chars().take(120).collect()
}

#[cfg(test)]
mod invariant_tests {
    use super::*;

    fn probe(root: &Path) -> crate::app::Probe {
        let mut probe = MdEditor::new_with_startup_file(None).0.probe();
        probe.vault_root = Some(root.to_string_lossy().into_owned());
        probe.entries = vec!["a.md".to_string()];
        probe
    }

    #[test]
    fn a_consistent_quiet_state_has_no_violations() {
        let vault = Vault::new("inv-ok").file("a.md", "hello");
        let mut p = probe(&vault.root);
        p.active_path = Some("a.md".to_string());
        p.buffer = "hello".to_string();
        assert!(invariant_violations(&p, false).is_empty());
    }

    #[test]
    fn each_broken_invariant_is_reported() {
        let vault = Vault::new("inv-bad").file("a.md", "on disk");
        let mut p = probe(&vault.root);
        p.active_path = Some("a.md".to_string());
        p.buffer = "in memory".to_string();
        p.entries.push("ghost.md".to_string());
        p.toast = Some("Save failed: permission denied".to_string());
        p.pdf_pending_pages = 2;
        let problems = invariant_violations(&p, false);
        assert_eq!(problems.len(), 4, "{problems:#?}");

        // An expected error toast is not a violation; a dirty buffer may differ.
        p.dirty = true;
        assert_eq!(invariant_violations(&p, true).len(), 2);
    }

    #[test]
    fn info_toasts_are_not_errors() {
        assert!(!is_error_toast("No unsaved changes"));
        assert!(is_error_toast("Autosave failed: disk full"));
        assert!(is_error_toast("Could not open vault"));
    }
}
