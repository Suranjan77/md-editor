//! Windows pen capture via the pointer input stack (`WM_POINTER*`).
//!
//! iced cannot deliver pen pressure: winit reads it, but `iced_core`'s touch
//! event carries only an id and a position, so the force is dropped in
//! conversion. Rather than fork iced, this module subclasses the window
//! procedure and reads the pointer stack directly.
//!
//! Two details matter for the feel of the result:
//!
//! * **Coalesced history.** The digitizer samples far faster than the window
//!   is repainted. `GetPointerPenInfoHistory` returns the samples that arrived
//!   since the last message; ignoring them turns fast handwriting into visible
//!   straight-line segments.
//! * **Sub-pixel positions.** `ptPixelLocation` is rounded to whole pixels,
//!   which makes slow strokes look like staircases. The himetric location is
//!   full resolution and is rescaled here against the device/display rects,
//!   the same approach winit uses.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::System::DataExchange::GlobalAddAtomW;
use windows_sys::Win32::UI::Input::Pointer::{
    GetPointerDeviceRects, GetPointerPenInfo, GetPointerPenInfoHistory, GetPointerType,
    POINTER_FLAG_DOWN, POINTER_FLAG_UP, POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    PEN_FLAG_BARREL, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PEN_MASK_PRESSURE, POINTER_INPUT_TYPE,
    PT_PEN, SetPropW, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE,
};

use super::pen::{PenPhase, PenSample};

/// Arbitrary id distinguishing our subclass from any other on the window.
const SUBCLASS_ID: usize = 0x6D64_496E; // 'mdIn'

/// Pen pressure arrives on a 0..1024 scale through the pointer stack.
const PRESSURE_RANGE: f32 = 1024.0;

/// Used when the digitizer does not report pressure at all, so such a device
/// still draws a sensible mid-weight line.
const PRESSURE_UNAVAILABLE: f32 = 0.5;

// Tablet PC gesture opt-outs, set as a window property. Without these a
// press-and-hold becomes a right-click part-way through a stroke and paints
// the "ripple" feedback ring over the canvas, and edge flicks fire navigation
// gestures. Every ink app has to turn these off explicitly.
const TABLET_DISABLE_PRESSANDHOLD: u32 = 0x0000_0001;
const TABLET_DISABLE_PENTAPFEEDBACK: u32 = 0x0000_0008;
const TABLET_DISABLE_PENBARRELFEEDBACK: u32 = 0x0000_0010;
const TABLET_DISABLE_TOUCHUIFORCEON: u32 = 0x0000_0100;
const TABLET_DISABLE_FLICKS: u32 = 0x0001_0000;

/// Subclass `hwnd` so pen input is routed to the ink surface.
pub(super) fn install(raw_window_id: u64) -> Result<(), String> {
    let hwnd = raw_window_id as usize as HWND;
    if hwnd.is_null() {
        return Err("window handle was null".to_string());
    }

    // SAFETY: `hwnd` is a live window owned by the event loop, and
    // `subclass_proc` has the signature required by `SUBCLASSPROC`.
    let installed = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) };
    if installed == 0 {
        return Err("SetWindowSubclass failed".to_string());
    }

    suppress_tablet_gestures(hwnd);
    Ok(())
}

/// Opt out of the Tablet PC press-and-hold, tap feedback and flick gestures
/// for this window. Best effort: the app still works if it fails, it just
/// feels worse.
fn suppress_tablet_gestures(hwnd: HWND) {
    let name: Vec<u16> = OsStr::new("MicrosoftTabletPenServiceProperty")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `name` is a NUL-terminated UTF-16 buffer alive across the call.
    let atom = unsafe { GlobalAddAtomW(name.as_ptr()) };
    if atom == 0 {
        return;
    }

    let flags = TABLET_DISABLE_PRESSANDHOLD
        | TABLET_DISABLE_PENTAPFEEDBACK
        | TABLET_DISABLE_PENBARRELFEEDBACK
        | TABLET_DISABLE_TOUCHUIFORCEON
        | TABLET_DISABLE_FLICKS;

    // SAFETY: `MAKEINTATOM` is the documented way to name a property by atom —
    // the atom value occupies the low word of the pointer rather than pointing
    // at a string.
    unsafe {
        SetPropW(
            hwnd,
            atom as usize as *const u16,
            flags as usize as *mut core::ffi::c_void,
        );
    }
}

/// Window procedure hook. Pen messages are consumed while the ink surface is
/// active; everything else — including all pen input when ink is not on
/// screen — continues down the original chain untouched.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    // The pen left hover range entirely; drop any aiming indicator.
    if msg == WM_POINTERLEAVE && HOVERING_CANVAS.swap(false, Ordering::Relaxed) {
        super::pen::emit(PenSample {
            phase: PenPhase::Leave,
            x: 0.0,
            y: 0.0,
            pressure: 0.0,
            eraser: false,
            barrel: false,
        });
    }

    if super::pen::is_capturing()
        && matches!(msg, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP)
    {
        // SAFETY: called from the window procedure for `hwnd` with that
        // message's own `wparam`.
        if unsafe { handle_pointer(hwnd, msg, wparam) } {
            // Returning 0 without chaining suppresses the legacy mouse
            // messages Windows would otherwise synthesize from this pen
            // input, which would otherwise also land in the editor as clicks.
            return 0;
        }
    }

    // SAFETY: forwarding the untouched message to the rest of the chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Pointer id of the stroke in progress, or [`NO_POINTER`].
///
/// A stroke that starts on the canvas keeps its claim even if it wanders over
/// the toolbar, mirroring how mouse capture works. Without it, writing off the
/// edge of the surface would start clicking buttons mid-word.
static CAPTURED_POINTER: AtomicU32 = AtomicU32::new(NO_POINTER);

/// Sentinel for "no stroke in progress". Real pointer ids are never 0.
const NO_POINTER: u32 = 0;

/// Read every sample behind one pointer message. Returns whether the message
/// was pen input that we consumed.
unsafe fn handle_pointer(hwnd: HWND, msg: u32, wparam: WPARAM) -> bool {
    let pointer_id = (wparam & 0xFFFF) as u32;

    // Claim pen only. Touch and mouse keep their normal behaviour so the rest
    // of the UI still works while the ink surface is open.
    let mut kind: POINTER_INPUT_TYPE = 0;
    // SAFETY: out-param is a live local.
    if unsafe { GetPointerType(pointer_id, &mut kind) } == 0 || kind != PT_PEN {
        return false;
    }

    let captured = CAPTURED_POINTER.load(Ordering::Relaxed);

    if msg == WM_POINTERDOWN {
        // A press outside the surface belongs to whatever widget is there —
        // let it through so the pen can operate the toolbar.
        // SAFETY: reads the current pointer's position via the pointer API.
        if !unsafe { press_is_on_canvas(hwnd, pointer_id) } {
            return false;
        }
        CAPTURED_POINTER.store(pointer_id, Ordering::Relaxed);
    } else if captured != pointer_id {
        // No stroke in progress. An update here is the pen hovering in range:
        // report it over the canvas so the surface can show where the nib is
        // aimed, and leave it alone elsewhere so the toolbar keeps its own
        // hover behaviour.
        // SAFETY: reads the current pointer's position via the pointer API.
        if msg == WM_POINTERUPDATE && unsafe { report_hover(hwnd, pointer_id) } {
            return true;
        }
        return false;
    }

    if msg == WM_POINTERUP && captured == pointer_id {
        CAPTURED_POINTER.store(NO_POINTER, Ordering::Relaxed);
    }

    // First call sizes the backlog, second fills it.
    let mut count: u32 = 0;
    // SAFETY: a null buffer with a live count pointer is the documented way to
    // query the required entry count.
    if unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, std::ptr::null_mut()) } == 0
        || count == 0
    {
        return false;
    }

    let mut history: Vec<POINTER_PEN_INFO> = vec![unsafe { std::mem::zeroed() }; count as usize];
    // SAFETY: `history` has room for `count` entries, as just reported.
    if unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, history.as_mut_ptr()) } == 0 {
        return false;
    }
    history.truncate(count as usize);

    // The history is newest-first; replay it in the order the pen drew it.
    for info in history.iter().rev() {
        // SAFETY: `info` points into a buffer the API just filled.
        let Some((x, y)) = (unsafe { client_position(hwnd, info) }) else {
            continue;
        };

        let flags = info.pointerInfo.pointerFlags;
        let phase = if flags & POINTER_FLAG_UP != 0 {
            PenPhase::Up
        } else if flags & POINTER_FLAG_DOWN != 0 {
            PenPhase::Down
        } else {
            PenPhase::Move
        };

        let pressure = if info.penMask & PEN_MASK_PRESSURE != 0 {
            (info.pressure as f32 / PRESSURE_RANGE).clamp(0.0, 1.0)
        } else {
            PRESSURE_UNAVAILABLE
        };

        super::pen::emit(PenSample {
            phase,
            x,
            y,
            pressure,
            eraser: info.penFlags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0,
            barrel: info.penFlags & PEN_FLAG_BARREL != 0,
        });
    }

    true
}

/// Whether the last hover we saw was over the canvas, so leaving it emits
/// exactly one [`PenPhase::Leave`] rather than one per sample.
static HOVERING_CANVAS: AtomicBool = AtomicBool::new(false);

/// Report a hovering pen. Returns whether the message was consumed.
///
/// Hover over the canvas is claimed so the surface can draw its own nib
/// indicator; anywhere else it passes through untouched.
unsafe fn report_hover(hwnd: HWND, pointer_id: u32) -> bool {
    let mut info: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: out-param is a live local.
    if unsafe { GetPointerPenInfo(pointer_id, &mut info) } == 0 {
        return false;
    }
    // SAFETY: `info` was just filled in by the API.
    let Some((x, y)) = (unsafe { client_position(hwnd, &info) }) else {
        return false;
    };

    if !super::client_point_is_on_canvas(x, y) {
        // Only announce the departure once.
        if HOVERING_CANVAS.swap(false, Ordering::Relaxed) {
            super::pen::emit(PenSample {
                phase: PenPhase::Leave,
                x,
                y,
                pressure: 0.0,
                eraser: false,
                barrel: false,
            });
        }
        return false;
    }

    HOVERING_CANVAS.store(true, Ordering::Relaxed);
    super::pen::emit(PenSample {
        phase: PenPhase::Hover,
        x,
        y,
        pressure: 0.0,
        eraser: info.penFlags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0,
        barrel: info.penFlags & PEN_FLAG_BARREL != 0,
    });
    true
}

/// Whether the pen's current position lands on the ink surface.
///
/// Uses the single current sample rather than the history: the decision is
/// about where the press happened, and the newest reading is that.
unsafe fn press_is_on_canvas(hwnd: HWND, pointer_id: u32) -> bool {
    let mut info: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: out-param is a live local.
    if unsafe { GetPointerPenInfo(pointer_id, &mut info) } == 0 {
        return false;
    }
    // SAFETY: `info` was just filled in by the API.
    match unsafe { client_position(hwnd, &info) } {
        Some((x, y)) => super::client_point_is_on_canvas(x, y),
        None => false,
    }
}

/// Convert a pen sample's position to sub-pixel client coordinates.
///
/// The himetric location carries the digitizer's full precision; rescaling it
/// by the ratio between the display rect and the device rect recovers a
/// fractional pixel position. Falls back to the whole-pixel location when the
/// device rects are unavailable.
unsafe fn client_position(hwnd: HWND, info: &POINTER_PEN_INFO) -> Option<(f32, f32)> {
    let pointer = &info.pointerInfo;

    let mut device_rect: RECT = unsafe { std::mem::zeroed() };
    let mut display_rect: RECT = unsafe { std::mem::zeroed() };

    // SAFETY: both out-params are live locals; `sourceDevice` comes from the
    // pointer info the API filled in.
    let have_rects = unsafe {
        GetPointerDeviceRects(pointer.sourceDevice, &mut device_rect, &mut display_rect)
    } != 0;

    let device_width = (device_rect.right - device_rect.left) as f64;
    let device_height = (device_rect.bottom - device_rect.top) as f64;

    let (screen_x, screen_y) = if have_rects && device_width > 0.0 && device_height > 0.0 {
        let ratio_x = (display_rect.right - display_rect.left) as f64 / device_width;
        let ratio_y = (display_rect.bottom - display_rect.top) as f64 / device_height;
        // The himetric origin is 0,0 regardless of monitor layout, so the
        // display rect's offset has to be added back.
        (
            display_rect.left as f64 + pointer.ptHimetricLocation.x as f64 * ratio_x,
            display_rect.top as f64 + pointer.ptHimetricLocation.y as f64 * ratio_y,
        )
    } else {
        (
            pointer.ptPixelLocation.x as f64,
            pointer.ptPixelLocation.y as f64,
        )
    };

    // `ScreenToClient` only speaks whole pixels, so translate the floor and
    // add the fraction back afterwards.
    let mut point = POINT {
        x: screen_x.floor() as i32,
        y: screen_y.floor() as i32,
    };
    // SAFETY: `point` is a live local and `hwnd` is the window being drawn on.
    if unsafe { ScreenToClient(hwnd, &mut point) } == 0 {
        return None;
    }

    Some((
        point.x as f32 + screen_x.fract() as f32,
        point.y as f32 + screen_y.fract() as f32,
    ))
}
