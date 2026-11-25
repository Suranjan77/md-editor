//! Linux pen capture through the Wayland tablet protocol (`zwp_tablet_v2`),
//! via the tablet hook in the vendored winit (`vendor/winit`).
//!
//! Once the app binds the tablet protocol, the compositor stops turning the pen
//! into a mouse. winit then offers each tool frame here first: frames on the
//! ink surface are claimed and fed to [`super::pen::emit`] with pressure,
//! eraser and barrel state. The rest go back to winit, which replays them as
//! ordinary cursor and left-button events so the pen still presses toolbar
//! buttons. These are the same rules as the Windows hook in `pen_win.rs`.

use std::sync::Mutex;

use winit::platform::wayland::{
    TabletToolEvent, TabletToolPhase, is_tablet_input_available, set_tablet_tool_handler,
};

use super::pen::{PenPhase, PenSample};

/// Pressure reported for a tool without a pressure sensor: mid-weight, as for
/// the mouse.
const PRESSURE_UNAVAILABLE: f32 = 0.5;

/// Install the tablet handler. It is process-wide and filters by window, so
/// moving the hook to a new ink window only means updating `HOOKED_WINDOW`
/// (done by the caller).
pub fn install() -> Result<(), String> {
    if !is_tablet_input_available() {
        return Err(
            "the compositor does not offer the Wayland tablet protocol (zwp_tablet_v2); \
             the pen works as a mouse"
                .to_string(),
        );
    }
    set_tablet_tool_handler(Some(Box::new(handle)));
    Ok(())
}

/// Stroke and hover bookkeeping between frames.
static ROUTER: Mutex<Router> = Mutex::new(Router::new());

fn handle(event: &TabletToolEvent) -> bool {
    if !super::pen::is_capturing() || u64::from(event.window_id) != super::pen::hooked_window() {
        return false;
    }
    let (x, y) = (event.position.x as f32, event.position.y as f32);
    let on_canvas = super::client_point_is_on_canvas(x, y);

    let Ok(mut router) = ROUTER.lock() else {
        return false;
    };
    let (phases, claimed) = router.route(event.phase, on_canvas);
    drop(router);

    for phase in phases {
        super::pen::emit(PenSample {
            phase,
            x,
            y,
            pressure: event
                .pressure
                .map_or(PRESSURE_UNAVAILABLE, |p| (p as f32).clamp(0.0, 1.0)),
            eraser: event.eraser,
            barrel: event.barrel,
        });
    }
    claimed
}

/// Decides, for each tool phase, which samples the ink surface sees and
/// whether the event is claimed from the rest of the UI.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Router {
    /// A stroke began on the canvas; it owns the pen until it lifts, wherever
    /// the pen goes.
    stroking: bool,
    /// The last hover was over the canvas, so leaving it is announced once.
    hovering: bool,
}

impl Router {
    pub(crate) const fn new() -> Self {
        Self {
            stroking: false,
            hovering: false,
        }
    }

    /// Samples to emit for `phase` and whether it is claimed.
    pub(crate) fn route(
        &mut self,
        phase: TabletToolPhase,
        on_canvas: bool,
    ) -> (Vec<PenPhase>, bool) {
        match phase {
            // A press off the canvas belongs to the widget under it.
            TabletToolPhase::Down if on_canvas => {
                self.stroking = true;
                self.hovering = false;
                (vec![PenPhase::Down], true)
            }
            TabletToolPhase::Move if self.stroking => (vec![PenPhase::Move], true),
            TabletToolPhase::Up if self.stroking => {
                self.stroking = false;
                (vec![PenPhase::Up], true)
            }
            TabletToolPhase::ProximityOut => {
                let mut phases = Vec::new();
                if std::mem::take(&mut self.stroking) {
                    // Lifted out of range mid-stroke: commit what was drawn.
                    phases.push(PenPhase::Up);
                }
                if std::mem::take(&mut self.hovering) {
                    phases.push(PenPhase::Leave);
                }
                // Never claimed, so an emulated cursor always leaves too.
                (phases, false)
            }
            // Hovering — or dragging after a press that landed elsewhere.
            TabletToolPhase::ProximityIn | TabletToolPhase::Hover | TabletToolPhase::Move => {
                if on_canvas {
                    self.hovering = true;
                    (vec![PenPhase::Hover], true)
                } else if std::mem::take(&mut self.hovering) {
                    (vec![PenPhase::Leave], false)
                } else {
                    (Vec::new(), false)
                }
            }
            TabletToolPhase::Down | TabletToolPhase::Up => (Vec::new(), false),
        }
    }
}

#[cfg(test)]
mod tests;
