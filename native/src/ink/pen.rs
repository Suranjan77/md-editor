//! Pen input plumbing: the sample type, the capture toggle, and the stream
//! that carries captured samples into the iced runtime.
//!
//! Capture itself is platform specific — see [`super::pen_win`] for the
//! Windows implementation. Everything here is platform neutral so the rest of
//! the app never sees a `#[cfg]`.

use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::messages::Message;

/// Where a sample sits in a stroke's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenPhase {
    /// Pen touched down: start a new stroke.
    Down,
    /// Pen moved while down.
    Move,
    /// Pen lifted: commit the stroke.
    Up,
    /// Pen is in range but not touching.
    ///
    /// On an opaque tablet this is how you aim: your eyes are on the screen
    /// while your hand works blind, so knowing where the nib *will* land is
    /// what makes the surface usable. Every one of these was previously
    /// discarded.
    Hover,
    /// Pen left hover range, so any aiming indicator should disappear.
    Leave,
}

/// One digitizer sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenSample {
    pub phase: PenPhase,
    /// Position relative to the window client area, in **physical** pixels.
    /// The canvas divides by the window scale factor to reach logical
    /// coordinates.
    pub x: f32,
    pub y: f32,
    /// Normalized pen pressure, `0.0..=1.0`.
    pub pressure: f32,
    /// The pen is inverted (eraser end) — erase instead of draw.
    pub eraser: bool,
    /// A barrel (side) button is held. Used to summon the radial menu at the
    /// nib rather than making the user travel to the toolbar.
    pub barrel: bool,
}

/// Whether the platform hook should claim pen messages.
///
/// The hook stays installed for the window's whole life but only swallows
/// input while the ink surface is on screen; otherwise pen events fall through
/// to winit and behave like ordinary mouse input everywhere else in the app.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Whether a platform hook is installed at all. Drives the "pen unavailable"
/// messaging in the UI.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The window the hook is currently attached to.
///
/// The handwriting canvas lives in its own window, which is created and
/// destroyed as it is opened and closed — so the hook has to follow it rather
/// than being installed once for the life of the process.
static HOOKED_WINDOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

type Channel = (
    tokio::sync::mpsc::UnboundedSender<PenSample>,
    Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<PenSample>>>,
);

/// Unbounded so the platform hook — which runs on the UI thread inside the
/// window procedure — never blocks. Samples are small and drained every frame.
static CHANNEL: LazyLock<Channel> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, Mutex::new(Some(rx)))
});

/// Hand a captured sample to the runtime. Called from the platform hook.
pub(crate) fn emit(sample: PenSample) {
    let _ = CHANNEL.0.send(sample);
}

pub(crate) fn is_capturing() -> bool {
    CAPTURING.load(Ordering::Relaxed)
}

/// Start or stop claiming pen input. Called when the ink surface is shown or
/// hidden.
pub fn set_capturing(enabled: bool) {
    CAPTURING.store(enabled, Ordering::Relaxed);
}

/// Whether pen capture was successfully installed for the window.
pub fn is_available() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// Install the platform pen hook on the window identified by `raw_window_id`
/// (the `HWND` on Windows, as returned by `iced::window::raw_id`).
///
/// Installing on the same window twice is a no-op; a different window moves
/// the hook.
pub fn install(raw_window_id: u64) -> Result<(), String> {
    if raw_window_id == 0 {
        return Err("window handle was null".to_string());
    }
    if HOOKED_WINDOW.load(Ordering::Relaxed) == raw_window_id {
        return Ok(());
    }

    #[cfg(windows)]
    {
        super::pen_win::install(raw_window_id)?;
        HOOKED_WINDOW.store(raw_window_id, Ordering::Relaxed);
        INSTALLED.store(true, Ordering::Relaxed);
        Ok(())
    }

    #[cfg(not(windows))]
    {
        Err("Handwriting input is only implemented on Windows".to_string())
    }
}

/// Stream of captured pen samples, batched per wake-up.
///
/// Samples arrive in bursts — the platform hook replays the digitizer's
/// coalesced backlog, which can be several samples per frame — so they are
/// batched into one message rather than driving a separate update cycle each.
pub fn stream() -> std::pin::Pin<Box<dyn iced::futures::Stream<Item = Message> + Send>> {
    use iced::futures::SinkExt;

    Box::pin(iced::stream::channel(
        256,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            // The receiver is single-consumer; if the subscription is somehow
            // started twice, the second one idles rather than stealing samples.
            let Some(mut rx) = CHANNEL.1.lock().ok().and_then(|mut guard| guard.take()) else {
                return;
            };

            loop {
                let Some(first) = rx.recv().await else {
                    break; // senders dropped
                };

                let mut batch = vec![first];
                // Drain whatever else is already queued so a burst becomes one
                // update rather than one per sample.
                while let Ok(next) = rx.try_recv() {
                    batch.push(next);
                }

                if output.send(Message::InkPenSamples(batch)).await.is_err() {
                    break; // subscription dropped
                }
            }
        },
    ))
}
