//! Tests for `pen_wayland`.

use super::*;
use TabletToolPhase::*;

#[test]
fn a_stroke_on_the_canvas_is_claimed_until_it_lifts_even_off_the_canvas() {
    let mut router = Router::new();
    assert_eq!(router.route(Down, true), (vec![PenPhase::Down], true));
    assert_eq!(router.route(Move, false), (vec![PenPhase::Move], true));
    assert_eq!(router.route(Up, false), (vec![PenPhase::Up], true));
    assert_eq!(router, Router::new());
}

#[test]
fn a_press_off_the_canvas_falls_through_to_the_widgets() {
    let mut router = Router::new();
    assert_eq!(router.route(Down, false), (vec![], false));
    assert_eq!(router.route(Up, false), (vec![], false));
}

#[test]
fn hover_is_claimed_on_the_canvas_and_its_departure_announced_once() {
    let mut router = Router::new();
    assert_eq!(
        router.route(ProximityIn, true),
        (vec![PenPhase::Hover], true)
    );
    assert_eq!(router.route(Hover, true), (vec![PenPhase::Hover], true));
    assert_eq!(router.route(Hover, false), (vec![PenPhase::Leave], false));
    assert_eq!(router.route(Hover, false), (vec![], false));
}

#[test]
fn leaving_range_mid_stroke_commits_the_stroke_and_is_never_claimed() {
    let mut router = Router::new();
    router.route(Hover, true);
    router.route(Down, true);
    assert_eq!(
        router.route(ProximityOut, true),
        (vec![PenPhase::Up], false)
    );
    assert_eq!(router, Router::new());

    router.route(Hover, true);
    assert_eq!(
        router.route(ProximityOut, true),
        (vec![PenPhase::Leave], false)
    );
}

fn tool_event(barrel: bool, barrel2: bool, eraser: bool) -> TabletToolEvent {
    TabletToolEvent {
        window_id: winit::window::WindowId::from(1),
        phase: Down,
        position: winit::dpi::PhysicalPosition::new(10.0, 20.0),
        pressure: Some(0.4),
        tilt: None,
        eraser,
        barrel,
        barrel2,
    }
}

#[test]
fn the_lower_side_button_erases_and_the_upper_one_opens_the_menu() {
    let plain = sample_for(&tool_event(false, false, false), PenPhase::Down);
    assert!(!plain.eraser && !plain.menu);
    assert_eq!(plain.pressure, 0.4);

    let lower = sample_for(&tool_event(true, false, false), PenPhase::Down);
    assert!(lower.eraser && !lower.menu, "BTN_STYLUS erases");

    let upper = sample_for(&tool_event(false, true, false), PenPhase::Down);
    assert!(!upper.eraser && upper.menu, "BTN_STYLUS2 opens the menu");

    let inverted = sample_for(&tool_event(false, false, true), PenPhase::Down);
    assert!(inverted.eraser, "the eraser end still erases");
}

#[test]
fn a_tool_without_a_pressure_sensor_draws_at_mid_weight() {
    let mut event = tool_event(false, false, false);
    event.pressure = None;
    assert_eq!(
        sample_for(&event, PenPhase::Down).pressure,
        PRESSURE_UNAVAILABLE
    );
}
