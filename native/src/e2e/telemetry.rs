//! Comet-style telemetry without Comet's window.
//!
//! iced's debugger works in two halves: the app reports spans (each update
//! with its message, each view, …) through `iced_debug`, and Comet receives
//! them as a beacon server. Here the app half is `Traced`, a `Program` wrapper
//! that opens the same spans around update and view (the emulator has no
//! windowed runtime to do it), and the server half is `Recorder`, which runs
//! `iced_beacon::run()` on a thread and keeps every span for a report.
//!
//! If the beacon port (127.0.0.1:9167, Comet's default) is taken — say a real
//! Comet is open — the recorder logs nothing and tests still run.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use iced_program::Program;

/// State the harness inspects after boot and after every update.
pub(crate) trait Observe {
    fn observe(&self);
}

/// The app wrapped so its update and view report `iced_debug` spans, and its
/// state is observed after every update.
pub(crate) struct Traced<P>(pub P);

impl<P: Program> Program for Traced<P>
where
    P::Message: std::fmt::Debug,
    P::State: Observe,
{
    type State = P::State;
    type Message = P::Message;
    type Theme = P::Theme;
    type Renderer = P::Renderer;
    type Executor = P::Executor;

    fn name() -> &'static str {
        P::name()
    }

    fn settings(&self) -> iced::Settings {
        self.0.settings()
    }

    fn window(&self) -> Option<iced::window::Settings> {
        self.0.window()
    }

    fn boot(&self) -> (Self::State, iced::Task<Self::Message>) {
        let span = iced_debug::boot();
        let booted = self.0.boot();
        span.finish();
        booted.0.observe();
        booted
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> iced::Task<Self::Message> {
        let span = iced_debug::update(&message);
        let task = self.0.update(state, message);
        span.finish();
        state.observe();
        task
    }

    fn view<'a>(
        &self,
        state: &'a Self::State,
        window: iced::window::Id,
    ) -> iced::Element<'a, Self::Message, Self::Theme, Self::Renderer> {
        let span = iced_debug::view(window);
        let element = self.0.view(state, window);
        span.finish();
        element
    }

    fn title(&self, state: &Self::State, window: iced::window::Id) -> String {
        self.0.title(state, window)
    }

    fn subscription(&self, state: &Self::State) -> iced::Subscription<Self::Message> {
        self.0.subscription(state)
    }

    fn theme(&self, state: &Self::State, window: iced::window::Id) -> Option<Self::Theme> {
        self.0.theme(state, window)
    }

    fn style(&self, state: &Self::State, theme: &Self::Theme) -> iced::theme::Style {
        self.0.style(state, theme)
    }

    fn scale_factor(&self, state: &Self::State, window: iced::window::Id) -> f32 {
        self.0.scale_factor(state, window)
    }
}

#[derive(Debug, Clone)]
struct Record {
    stage: &'static str,
    message: Option<String>,
    duration: Duration,
}

pub(crate) struct Recorder {
    records: Arc<Mutex<Vec<Record>>>,
    connected: Arc<AtomicBool>,
}

/// The process-wide recorder, started on first use.
pub(crate) fn recorder() -> &'static Recorder {
    static RECORDER: OnceLock<Recorder> = OnceLock::new();
    RECORDER.get_or_init(start)
}

fn start() -> Recorder {
    use iced_beacon::span::Span;
    use iced_test::futures::futures::StreamExt;

    let records = Arc::new(Mutex::new(Vec::new()));
    let connected = Arc::new(AtomicBool::new(false));
    {
        let records = records.clone();
        let connected = connected.clone();
        std::thread::spawn(move || {
            // The beacon server listens with tokio's TcpListener.
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("beacon runtime");
            runtime.block_on(iced_beacon::run().for_each(|event| {
                match event {
                    iced_beacon::Event::Connected { .. } => connected.store(true, Ordering::SeqCst),
                    iced_beacon::Event::SpanFinished { duration, span, .. } => {
                        let (stage, message) = match span {
                            Span::Boot => ("boot", None),
                            Span::Update { message, .. } => ("update", Some(message)),
                            Span::View { .. } => ("view", None),
                            Span::Layout { .. } => ("layout", None),
                            Span::Interact { .. } => ("interact", None),
                            Span::Draw { .. } => ("draw", None),
                            Span::Present { .. } => ("present", None),
                            Span::Custom { .. } => ("custom", None),
                        };
                        records.lock().unwrap().push(Record {
                            stage,
                            message,
                            duration,
                        });
                    }
                    _ => {}
                }
                async {}
            }));
        });
    }

    iced_debug::init(iced_debug::Metadata {
        name: "md-editor-e2e",
        theme: None,
        can_time_travel: false,
    });
    iced_debug::enable();

    // Spans logged before the client connects are dropped, so wait briefly.
    // A Comet window already holding the port means no connection; carry on.
    for _ in 0..200 {
        let _ = iced_debug::boot(); // touch the beacon so the client starts
        if connected.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    Recorder { records, connected }
}

impl Recorder {
    /// Position to report from.
    pub fn mark(&self) -> usize {
        self.records.lock().unwrap().len()
    }

    /// Append a summary of everything recorded since `mark` to
    /// `target/e2e/telemetry.md`, and print it.
    pub fn report(&self, session: &str, mark: usize) {
        // Spans travel over a socket; give the last ones a moment to land.
        std::thread::sleep(Duration::from_millis(150));
        let records: Vec<Record> = self.records.lock().unwrap()[mark..].to_vec();

        let mut out = String::new();
        let _ = writeln!(out, "## {session}\n");
        if !self.connected.load(Ordering::SeqCst) {
            let _ = writeln!(
                out,
                "_beacon not connected (port in use?) — no telemetry_\n"
            );
        }
        let _ = writeln!(
            out,
            "| stage | count | p50 | p95 | max |\n| --- | ---: | ---: | ---: | ---: |"
        );
        for stage in ["boot", "update", "view"] {
            let mut durations: Vec<Duration> = records
                .iter()
                .filter(|r| r.stage == stage)
                .map(|r| r.duration)
                .collect();
            if durations.is_empty() {
                continue;
            }
            durations.sort();
            let pct = |p: f64| durations[((durations.len() - 1) as f64 * p).round() as usize];
            let _ = writeln!(
                out,
                "| {stage} | {} | {:.2?} | {:.2?} | {:.2?} |",
                durations.len(),
                pct(0.5),
                pct(0.95),
                durations[durations.len() - 1]
            );
        }

        let mut updates: Vec<&Record> = records.iter().filter(|r| r.stage == "update").collect();
        updates.sort_by_key(|r| std::cmp::Reverse(r.duration));
        let _ = writeln!(out, "\nSlowest updates:\n");
        for record in updates.iter().take(5) {
            let message = record.message.as_deref().unwrap_or("");
            let message: String = message.chars().take(90).collect();
            let _ = writeln!(
                out,
                "- {:.2?} `{}`",
                record.duration,
                message.replace('`', "'")
            );
        }
        let _ = writeln!(out);

        println!("{out}");
        use std::io::Write as _;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(super::artifacts_dir().join("telemetry.md"))
        {
            let _ = file.write_all(out.as_bytes());
        }
    }

    /// Updates recorded since `mark` that took longer than `budget`.
    pub fn slow_updates(&self, mark: usize, budget: Duration) -> Vec<(Duration, String)> {
        let end = self.mark();
        self.slow_updates_between(mark, end, budget)
    }

    /// Updates recorded in `from..to` that took longer than `budget`.
    pub fn slow_updates_between(
        &self,
        from: usize,
        to: usize,
        budget: Duration,
    ) -> Vec<(Duration, String)> {
        self.records.lock().unwrap()[from..to]
            .iter()
            .filter(|r| r.stage == "update" && r.duration > budget)
            .map(|r| (r.duration, r.message.clone().unwrap_or_default()))
            .collect()
    }
}
