//! # Wayland
//!
//! **Note:** Windows don't appear on Wayland until you draw/present to them.
//!
//! By default, Winit loads system libraries using `dlopen`. This can be
//! disabled by disabling the `"wayland-dlopen"` cargo feature.
//!
//! ## Client-side decorations
//!
//! Winit provides client-side decorations by default, but the behaviour can
//! be controlled with the following feature flags:
//!
//! * `wayland-csd-adwaita` (default).
//! * `wayland-csd-adwaita-crossfont`.
//! * `wayland-csd-adwaita-notitle`.

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::event_loop::{ActiveEventLoop, EventLoop, EventLoopBuilder};
use crate::monitor::MonitorHandle;
use crate::window::{Window, WindowAttributes};

pub use crate::window::Theme;

/// Additional methods on [`ActiveEventLoop`] that are specific to Wayland.
pub trait ActiveEventLoopExtWayland {
    /// True if the [`ActiveEventLoop`] uses Wayland.
    fn is_wayland(&self) -> bool;
}

impl ActiveEventLoopExtWayland for ActiveEventLoop {
    #[inline]
    fn is_wayland(&self) -> bool {
        self.p.is_wayland()
    }
}

/// Additional methods on [`EventLoop`] that are specific to Wayland.
pub trait EventLoopExtWayland {
    /// True if the [`EventLoop`] uses Wayland.
    fn is_wayland(&self) -> bool;
}

impl<T: 'static> EventLoopExtWayland for EventLoop<T> {
    #[inline]
    fn is_wayland(&self) -> bool {
        self.event_loop.is_wayland()
    }
}

/// Additional methods on [`EventLoopBuilder`] that are specific to Wayland.
pub trait EventLoopBuilderExtWayland {
    /// Force using Wayland.
    fn with_wayland(&mut self) -> &mut Self;

    /// Whether to allow the event loop to be created off of the main thread.
    ///
    /// By default, the window is only allowed to be created on the main
    /// thread, to make platform compatibility easier.
    fn with_any_thread(&mut self, any_thread: bool) -> &mut Self;
}

impl<T> EventLoopBuilderExtWayland for EventLoopBuilder<T> {
    #[inline]
    fn with_wayland(&mut self) -> &mut Self {
        self.platform_specific.forced_backend = Some(crate::platform_impl::Backend::Wayland);
        self
    }

    #[inline]
    fn with_any_thread(&mut self, any_thread: bool) -> &mut Self {
        self.platform_specific.any_thread = any_thread;
        self
    }
}

/// Additional methods on [`Window`] that are specific to Wayland.
///
/// [`Window`]: crate::window::Window
pub trait WindowExtWayland {
    /// Returns `xdg_toplevel` of the window or [`None`] if the window is X11 window.
    fn xdg_toplevel(&self) -> Option<NonNull<c_void>>;
}

impl WindowExtWayland for Window {
    #[inline]
    fn xdg_toplevel(&self) -> Option<NonNull<c_void>> {
        #[allow(clippy::single_match)]
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => None,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.xdg_toplevel(),
        }
    }
}

/// Additional methods on [`WindowAttributes`] that are specific to Wayland.
pub trait WindowAttributesExtWayland {
    /// Build window with the given name.
    ///
    /// The `general` name sets an application ID, which should match the `.desktop`
    /// file distributed with your program. The `instance` is a `no-op`.
    ///
    /// For details about application ID conventions, see the
    /// [Desktop Entry Spec](https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html#desktop-file-id)
    fn with_name(self, general: impl Into<String>, instance: impl Into<String>) -> Self;
}

impl WindowAttributesExtWayland for WindowAttributes {
    #[inline]
    fn with_name(mut self, general: impl Into<String>, instance: impl Into<String>) -> Self {
        self.platform_specific.name =
            Some(crate::platform_impl::ApplicationName::new(general.into(), instance.into()));
        self
    }
}

/// Additional methods on `MonitorHandle` that are specific to Wayland.
pub trait MonitorHandleExtWayland {
    /// Returns the inner identifier of the monitor.
    fn native_id(&self) -> u32;
}

impl MonitorHandleExtWayland for MonitorHandle {
    #[inline]
    fn native_id(&self) -> u32 {
        self.inner.native_identifier()
    }
}

/// Where a tablet tool (pen) is in its interaction with a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabletToolPhase {
    /// The tool came into range over the window.
    ProximityIn,
    /// The tool moved while in range but not touching.
    Hover,
    /// The tool touched the surface.
    Down,
    /// The tool moved while touching.
    Move,
    /// The tool lifted off the surface.
    Up,
    /// The tool left range, or left the window.
    ProximityOut,
}

/// One tablet tool sample on a window, delivered once per protocol frame
/// (`zwp_tablet_tool_v2.frame`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabletToolEvent {
    /// The window the tool is over.
    pub window_id: crate::window::WindowId,
    pub phase: TabletToolPhase,
    /// Position relative to the window's surface, in physical pixels.
    pub position: crate::dpi::PhysicalPosition<f64>,
    /// Normalised pressure, `0.0..=1.0`, or `None` if the tool has no pressure
    /// sensor.
    pub pressure: Option<f64>,
    /// Tilt from the perpendicular in degrees, `(x, y)`, if the tool reports it.
    pub tilt: Option<(f64, f64)>,
    /// The tool is an eraser (or the eraser end of a pen).
    pub eraser: bool,
    /// The first barrel button (`BTN_STYLUS`) is held.
    pub barrel: bool,
    /// The second barrel button (`BTN_STYLUS2`) is held.
    pub barrel2: bool,
}

/// A handler for tablet tool events. Returns whether it claimed the event.
pub type TabletToolHandler = Box<dyn FnMut(&TabletToolEvent) -> bool + Send>;

static TABLET_TOOL_HANDLER: std::sync::Mutex<Option<TabletToolHandler>> =
    std::sync::Mutex::new(None);

pub(crate) static TABLET_AVAILABLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Receive graphics-tablet tool input on Wayland (`zwp_tablet_v2`): position
/// with pressure, tilt, eraser and barrel buttons, which winit's ordinary
/// pointer events cannot carry.
///
/// The handler runs on the event-loop thread for every tool frame. Return
/// `true` to claim the event; an unclaimed one is turned into the
/// `CursorEntered`/`CursorMoved`/`MouseInput(Left)`/`CursorLeft` events a mouse
/// would produce, so the pen keeps working on ordinary widgets. Over a surface
/// that claims hover, the system cursor is hidden.
///
/// The handler must not call this function. Pass `None` to remove it, after
/// which every tool event is emulated as pointer input.
pub fn set_tablet_tool_handler(handler: Option<TabletToolHandler>) {
    if let Ok(mut slot) = TABLET_TOOL_HANDLER.lock() {
        *slot = handler;
    }
}

/// Whether the compositor offered the tablet protocol, so
/// [`set_tablet_tool_handler`] will receive anything. Meaningful once an event
/// loop has been created.
pub fn is_tablet_input_available() -> bool {
    TABLET_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Offer an event to the installed handler; `false` when there is none.
#[cfg(wayland_platform)]
pub(crate) fn dispatch_tablet_tool_event(event: &TabletToolEvent) -> bool {
    match TABLET_TOOL_HANDLER.lock() {
        Ok(mut slot) => slot.as_mut().is_some_and(|handler| handler(event)),
        Err(_) => false,
    }
}
