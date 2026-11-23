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
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use iced::mouse;
use iced_program::Program;
use iced_test::Instruction;
use iced_test::emulator::{self, Emulator, Mode};
use iced_test::futures::futures::channel::mpsc;
use iced_test::instruction::{Expectation, Interaction, Keyboard, Mouse, Target};

use crate::app::MdEditor;
use crate::messages::Message;

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
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DATA_DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("md-editor-e2e-data-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create e2e data dir");
        dir
    });
    assert!(
        md_editor_core::state::override_data_dir(dir.clone()),
        "another data directory was pinned before the e2e tests"
    );
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
    _serial: MutexGuard<'static, ()>,
}

/// Boot the real app on `vault`, opening `open` (vault-relative) if given.
pub(crate) fn boot(
    name: &str,
    vault: &Vault,
    open: Option<&str>,
) -> Session<impl Program<State = MdEditor, Message = Message> + 'static> {
    let serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    isolate_data_dir();
    let mark = telemetry::recorder().mark();

    let startup = match open {
        Some(rel) => vault.root.join(rel),
        None => vault.root.clone(),
    };
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
    }

    /// Run the event loop for a fixed time regardless of activity.
    pub fn wait(&mut self, duration: Duration) {
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

    /// Deliver a message as if the app had produced it, then settle.
    pub fn send(&mut self, message: Message) {
        self.emulator.update(&self.program, message);
        self.settle();
    }

    fn instruct(&mut self, instruction: Instruction) -> Result<(), String> {
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
