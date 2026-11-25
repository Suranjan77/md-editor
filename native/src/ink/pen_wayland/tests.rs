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
