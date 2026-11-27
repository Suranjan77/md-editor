//! Graphics tablet input (`zwp_tablet_v2`).
//!
//! Once a client binds the tablet manager, the compositor stops emulating a
//! pointer for the pen and sends tablet events instead. Each tool frame is
//! offered to the handler installed with
//! [`crate::platform::wayland::set_tablet_tool_handler`]; whatever the handler
//! does not claim is turned back into ordinary cursor and left-button events,
//! so the pen still operates every widget the way a mouse does.

use std::sync::Mutex;

use sctk::compositor::SurfaceData;
use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{
    delegate_dispatch, event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use sctk::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use sctk::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use sctk::reexports::protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};

use crate::dpi::{LogicalPosition, PhysicalPosition};
use crate::event::{ElementState, MouseButton, WindowEvent};
use crate::platform::wayland::{TabletToolEvent, TabletToolPhase};
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::{self, DeviceId};

/// Linux input codes for the pen's barrel buttons (`linux/input-event-codes.h`).
const BTN_STYLUS: u32 = 0x14b;
const BTN_STYLUS2: u32 = 0x14c;

/// Full scale of `zwp_tablet_tool_v2.pressure`.
const PRESSURE_RANGE: f64 = 65535.0;

/// The bound tablet manager, plus the cursor-shape manager used to give the pen
/// a cursor (tablet tools have their own cursor, separate from the pointer's).
#[derive(Debug)]
pub struct TabletManagerState {
    manager: ZwpTabletManagerV2,
    cursor_shape: Option<WpCursorShapeManagerV1>,
}

impl TabletManagerState {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        // Version 1 is all that is used; it also avoids pad dials (since 2).
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        let cursor_shape = globals.bind(queue_handle, 1..=1, GlobalData).ok();
        crate::platform::wayland::TABLET_AVAILABLE
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(Self { manager, cursor_shape })
    }

    /// Start receiving tablets on `seat`.
    pub fn tablet_seat(
        &self,
        seat: &WlSeat,
        queue_handle: &QueueHandle<WinitState>,
    ) -> ZwpTabletSeatV2 {
        self.manager.get_tablet_seat(seat, queue_handle, GlobalData)
    }
}

/// Per-tool state, accumulated between `frame` events.
#[derive(Debug, Default)]
pub struct TabletToolData {
    inner: Mutex<ToolState>,
}

#[derive(Debug, Default)]
struct ToolState {
    eraser: bool,
    reports_pressure: bool,
    reports_tilt: bool,

    /// The surface the tool is in proximity of, and the serial to set its
    /// cursor with.
    surface: Option<WlSurface>,
    serial: u32,
    /// Surface-local logical position.
    position: (f64, f64),
    pressure: f64,
    tilt: (f64, f64),
    down: bool,
    barrel: bool,
    barrel2: bool,

    // Changes since the last frame.
    entered: bool,
    left: bool,
    pressed: bool,
    released: bool,
    moved: bool,

    /// The emulated cursor is inside the window, so leaving it must say so.
    emulating: bool,
    shape_device: Option<WpCursorShapeDeviceV1>,
    /// Last cursor requested: `Some(true)` hidden, `Some(false)` default arrow.
    cursor_hidden: Option<bool>,
}

impl Dispatch<ZwpTabletManagerV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        _: &ZwpTabletManagerV2,
        _: <ZwpTabletManagerV2 as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        // No events.
    }
}

impl Dispatch<ZwpTabletSeatV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        _: &ZwpTabletSeatV2,
        _: zwp_tablet_seat_v2::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        // The new objects arrive with their user data already attached below.
    }

    event_created_child!(WinitState, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, GlobalData),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, TabletToolData::default()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, GlobalData),
    ]);
}

impl Dispatch<ZwpTabletV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        tablet: &ZwpTabletV2,
        event: <ZwpTabletV2 as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        // Tools carry everything needed; the tablet itself is only metadata.
        if let sctk::reexports::protocols::wp::tablet::zv2::client::zwp_tablet_v2::Event::Removed =
            event
        {
            tablet.destroy();
        }
    }
}

impl Dispatch<ZwpTabletToolV2, TabletToolData, WinitState> for TabletManagerState {
    fn event(
        state: &mut WinitState,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        data: &TabletToolData,
        _: &Connection,
        queue_handle: &QueueHandle<WinitState>,
    ) {
        use zwp_tablet_tool_v2::Event;

        let mut tool_state = data.inner.lock().unwrap();
        match event {
            Event::Type { tool_type } => {
                tool_state.eraser =
                    matches!(tool_type, WEnum::Value(zwp_tablet_tool_v2::Type::Eraser));
            },
            Event::Capability { capability } => match capability {
                WEnum::Value(zwp_tablet_tool_v2::Capability::Pressure) => {
                    tool_state.reports_pressure = true
                },
                WEnum::Value(zwp_tablet_tool_v2::Capability::Tilt) => {
                    tool_state.reports_tilt = true
                },
                _ => {},
            },
            Event::ProximityIn { serial, surface, .. } => {
                tool_state.surface = Some(surface);
                tool_state.serial = serial;
                tool_state.entered = true;
                tool_state.cursor_hidden = None;
                if tool_state.shape_device.is_none() {
                    tool_state.shape_device =
                        state.tablet_manager.as_ref().and_then(|m| m.cursor_shape.as_ref()).map(
                            |manager| manager.get_tablet_tool_v2(tool, queue_handle, GlobalData),
                        );
                }
            },
            Event::ProximityOut => {
                tool_state.left = true;
            },
            Event::Down { serial } => {
                tool_state.serial = serial;
                tool_state.down = true;
                tool_state.pressed = true;
            },
            Event::Up => {
                tool_state.down = false;
                tool_state.released = true;
            },
            Event::Motion { x, y } => {
                tool_state.position = (x, y);
                tool_state.moved = true;
            },
            Event::Pressure { pressure } => {
                tool_state.pressure = (f64::from(pressure) / PRESSURE_RANGE).clamp(0.0, 1.0);
                tool_state.moved = true;
            },
            Event::Tilt { tilt_x, tilt_y } => {
                tool_state.tilt = (tilt_x, tilt_y);
            },
            Event::Button { serial, button, state: button_state } => {
                tool_state.serial = serial;
                let pressed = matches!(
                    button_state,
                    WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed)
                );
                match button {
                    BTN_STYLUS => tool_state.barrel = pressed,
                    BTN_STYLUS2 => tool_state.barrel2 = pressed,
                    _ => {},
                }
            },
            Event::Frame { .. } => {
                let frame = std::mem::take(&mut *tool_state);
                *tool_state = state.tablet_frame(tool, frame);
            },
            Event::Removed => {
                if let Some(device) = tool_state.shape_device.take() {
                    device.destroy();
                }
                tool.destroy();
            },
            // Distance, rotation, slider, wheel and hardware ids are not used.
            _ => {},
        }
    }
}

impl WinitState {
    /// Deliver one tool frame. Takes the tool's state by value and hands back
    /// what persists into the next frame.
    fn tablet_frame(&mut self, tool: &ZwpTabletToolV2, mut frame: ToolState) -> ToolState {
        let mut next = ToolState {
            eraser: frame.eraser,
            reports_pressure: frame.reports_pressure,
            reports_tilt: frame.reports_tilt,
            surface: frame.surface.clone(),
            serial: frame.serial,
            position: frame.position,
            pressure: frame.pressure,
            tilt: frame.tilt,
            down: frame.down,
            barrel: frame.barrel,
            barrel2: frame.barrel2,
            emulating: frame.emulating,
            shape_device: frame.shape_device.take(),
            cursor_hidden: frame.cursor_hidden,
            ..Default::default()
        };

        let Some(surface) = frame.surface.clone() else {
            return next;
        };

        // Only the window's own surface: decorations (subsurfaces) are left to
        // the compositor, which hands the pen to them as a pointer anyway.
        let is_main_surface = surface
            .data::<SurfaceData>()
            .is_some_and(|data| data.parent_surface().is_none());
        let window_id = wayland::make_wid(&surface);
        let scale_factor = match self.windows.get_mut().get(&window_id) {
            Some(window) if is_main_surface => window.lock().unwrap().scale_factor(),
            _ => {
                if frame.left {
                    next.surface = None;
                }
                return next;
            },
        };

        let position: PhysicalPosition<f64> =
            LogicalPosition::new(frame.position.0, frame.position.1).to_physical(scale_factor);

        let mut phases = Vec::with_capacity(4);
        if frame.entered {
            phases.push(TabletToolPhase::ProximityIn);
        }
        if frame.pressed {
            phases.push(TabletToolPhase::Down);
        } else if frame.moved && !frame.released {
            phases.push(if frame.down { TabletToolPhase::Move } else { TabletToolPhase::Hover });
        }
        if frame.released {
            phases.push(TabletToolPhase::Up);
        }
        if frame.left {
            phases.push(TabletToolPhase::ProximityOut);
        }

        let mut claimed_any = false;
        for phase in phases {
            let event = TabletToolEvent {
                window_id: crate::window::WindowId(window_id),
                phase,
                position,
                pressure: frame.reports_pressure.then_some(frame.pressure),
                tilt: frame.reports_tilt.then_some(frame.tilt),
                eraser: frame.eraser,
                barrel: frame.barrel,
                barrel2: frame.barrel2,
            };
            let claimed = crate::platform::wayland::dispatch_tablet_tool_event(&event);
            claimed_any |= claimed;
            if !claimed {
                self.emulate_pointer(&mut next, window_id, phase, position);
            } else if next.emulating && phase == TabletToolPhase::ProximityOut {
                // Claimed or not, a cursor that entered must leave.
                self.emulate_pointer(&mut next, window_id, phase, position);
            }
        }

        // Over a surface that draws its own nib indicator, hide the system
        // cursor; elsewhere show the default arrow.
        if let Some(device) = next.shape_device.as_ref() {
            if !frame.left && next.cursor_hidden != Some(claimed_any) {
                if claimed_any {
                    tool.set_cursor(next.serial, None, 0, 0);
                } else {
                    device.set_shape(next.serial, Shape::Default);
                }
                next.cursor_hidden = Some(claimed_any);
            }
        }

        if frame.left {
            next.surface = None;
            next.down = false;
            next.cursor_hidden = None;
        }
        next
    }

    /// Turn an unclaimed tool phase into the pointer events a mouse would
    /// have produced, so the pen operates ordinary widgets.
    fn emulate_pointer(
        &mut self,
        tool: &mut ToolState,
        window_id: crate::platform_impl::wayland::WindowId,
        phase: TabletToolPhase,
        position: PhysicalPosition<f64>,
    ) {
        let device_id = crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(DeviceId));
        let sink = &mut self.events_sink;
        let mut push = |event: WindowEvent| sink.push_window_event(event, window_id);

        if !tool.emulating && phase != TabletToolPhase::ProximityOut {
            tool.emulating = true;
            push(WindowEvent::CursorEntered { device_id });
        }
        match phase {
            TabletToolPhase::ProximityIn | TabletToolPhase::Hover | TabletToolPhase::Move => {
                push(WindowEvent::CursorMoved { device_id, position });
            },
            TabletToolPhase::Down => {
                push(WindowEvent::CursorMoved { device_id, position });
                push(WindowEvent::MouseInput {
                    device_id,
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                });
            },
            TabletToolPhase::Up => {
                push(WindowEvent::MouseInput {
                    device_id,
                    state: ElementState::Released,
                    button: MouseButton::Left,
                });
            },
            TabletToolPhase::ProximityOut => {
                if tool.emulating {
                    tool.emulating = false;
                    push(WindowEvent::CursorLeft { device_id });
                }
            },
        }
    }
}

impl Dispatch<ZwpTabletPadV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        pad: &ZwpTabletPadV2,
        event: zwp_tablet_pad_v2::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        // Pad buttons, rings and strips are not used yet.
        if let zwp_tablet_pad_v2::Event::Removed = event {
            pad.destroy();
        }
    }

    event_created_child!(WinitState, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, GlobalData),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
    }

    event_created_child!(WinitState, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, GlobalData),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, GlobalData),
    ]);
}

impl Dispatch<ZwpTabletPadRingV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        _: &ZwpTabletPadRingV2,
        _: <ZwpTabletPadRingV2 as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadStripV2, GlobalData, WinitState> for TabletManagerState {
    fn event(
        _: &mut WinitState,
        _: &ZwpTabletPadStripV2,
        _: <ZwpTabletPadStripV2 as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
    }
}

delegate_dispatch!(WinitState: [ZwpTabletManagerV2: GlobalData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletSeatV2: GlobalData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletV2: GlobalData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletToolV2: TabletToolData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletPadV2: GlobalData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletPadGroupV2: GlobalData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletPadRingV2: GlobalData] => TabletManagerState);
delegate_dispatch!(WinitState: [ZwpTabletPadStripV2: GlobalData] => TabletManagerState);
