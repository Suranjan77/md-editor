//! Tests for `ink`.

use super::*;

fn surface() -> InkState {
    // Pretend the surface occupies the window below a toolbar.
    set_viewport(Rectangle {
        x: 200.0,
        y: 40.0,
        width: 800.0,
        height: 600.0,
    });
    InkState::new()
}

fn sample(phase: PenPhase, x: f32, y: f32, pressure: f32) -> PenSample {
    PenSample {
        phase,
        x,
        y,
        pressure,
        menu: false,
        eraser: false,
    }
}

#[test]
fn samples_map_into_surface_local_coordinates() {
    // At 2x scale, a client point of (600, 240) is logical (300, 120),
    // which is (100, 80) inside a surface starting at (200, 40).
    set_viewport(Rectangle {
        x: 200.0,
        y: 40.0,
        width: 800.0,
        height: 600.0,
    });
    let mapped = map_to_canvas(&sample(PenPhase::Down, 600.0, 240.0, 0.5), 2.0, true).unwrap();
    assert!((mapped.x - 100.0).abs() < 0.01);
    assert!((mapped.y - 80.0).abs() < 0.01);
}

/// A surface sitting below a toolbar and right of a sidebar.
fn surface_bounds() -> Rectangle {
    Rectangle {
        x: 200.0,
        y: 90.0,
        width: 800.0,
        height: 600.0,
    }
}

#[test]
fn pen_presses_off_the_canvas_are_left_for_the_ui() {
    // The pen hook only claims input that lands on the surface; anywhere
    // else it falls through so the pen can press toolbar buttons.
    let bounds = surface_bounds();
    assert!(
        point_is_on_canvas(400.0, 300.0, bounds, 1.0),
        "middle of the canvas"
    );
    assert!(
        !point_is_on_canvas(400.0, 40.0, bounds, 1.0),
        "up on the toolbar"
    );
    assert!(
        !point_is_on_canvas(80.0, 300.0, bounds, 1.0),
        "over the sidebar"
    );
    assert!(
        !point_is_on_canvas(400.0, 800.0, bounds, 1.0),
        "below the canvas"
    );
}

#[test]
fn the_canvas_hit_test_accounts_for_display_scaling() {
    let bounds = surface_bounds();
    // Physical (800, 400) is logical (400, 200) — inside.
    assert!(point_is_on_canvas(800.0, 400.0, bounds, 2.0));
    // Physical (400, 100) is logical (200, 50) — above the surface.
    assert!(!point_is_on_canvas(400.0, 100.0, bounds, 2.0));
}

#[test]
fn a_nonsense_scale_factor_does_not_break_the_hit_test() {
    let bounds = surface_bounds();
    assert!(point_is_on_canvas(400.0, 300.0, bounds, 0.0));
}

#[test]
fn a_stroke_starting_outside_the_surface_is_ignored() {
    set_viewport(Rectangle {
        x: 200.0,
        y: 40.0,
        width: 800.0,
        height: 600.0,
    });
    // Over the sidebar, left of the surface.
    assert!(map_to_canvas(&sample(PenPhase::Down, 50.0, 100.0, 0.5), 1.0, true).is_none());
    // The same point mid-stroke is allowed through.
    assert!(map_to_canvas(&sample(PenPhase::Move, 50.0, 100.0, 0.5), 1.0, false).is_some());
}

#[test]
fn a_full_pen_gesture_commits_one_stroke() {
    let mut ink = surface();
    let samples = vec![
        sample(PenPhase::Down, 300.0, 140.0, 0.4),
        sample(PenPhase::Move, 320.0, 150.0, 0.6),
        sample(PenPhase::Move, 340.0, 160.0, 0.8),
        sample(PenPhase::Up, 360.0, 170.0, 0.2),
    ];

    assert!(ink.apply_samples(&samples, 1.0));
    assert_eq!(ink.document.strokes().len(), 1);
    assert!(
        ink.live_points().is_empty(),
        "live stroke should be committed"
    );
    assert!(ink.dirty);
}

#[test]
fn an_inverted_pen_erases_without_switching_tools() {
    let mut ink = surface();
    ink.apply_samples(
        &[
            sample(PenPhase::Down, 300.0, 140.0, 0.5),
            sample(PenPhase::Move, 400.0, 140.0, 0.5),
            sample(PenPhase::Up, 500.0, 140.0, 0.5),
        ],
        1.0,
    );
    assert_eq!(ink.document.strokes().len(), 1);

    let mut erase = sample(PenPhase::Down, 400.0, 140.0, 0.5);
    erase.eraser = true;
    ink.apply_samples(&[erase], 1.0);

    assert_eq!(
        ink.document.strokes().len(),
        0,
        "the inverted end should erase"
    );
    assert_eq!(
        ink.tool,
        Tool::Pen,
        "erasing by inversion must not change the tool"
    );
}

#[test]
fn hiding_the_surface_stops_capture_and_drops_a_partial_stroke() {
    let mut ink = surface();
    ink.set_visible(true);
    assert!(pen::is_capturing());

    ink.apply_samples(&[sample(PenPhase::Down, 300.0, 140.0, 0.5)], 1.0);
    assert!(!ink.live_points().is_empty());

    ink.set_visible(false);
    assert!(!pen::is_capturing());
    assert!(ink.live_points().is_empty());
    assert_eq!(
        ink.document.strokes().len(),
        0,
        "an abandoned stroke should not commit"
    );
}

#[test]
fn undo_after_a_stroke_restores_the_blank_page() {
    let mut ink = surface();
    ink.begin(Vec2::new(10.0, 10.0), 0.5, false);
    ink.extend(Vec2::new(40.0, 10.0), 0.5);
    ink.finish();
    assert_eq!(ink.document.strokes().len(), 1);

    assert!(ink.undo());
    assert!(ink.document.is_empty());
    assert!(ink.redo());
    assert_eq!(ink.document.strokes().len(), 1);
}

#[test]
fn a_tap_leaves_a_dot() {
    let mut ink = surface();
    ink.begin(Vec2::new(10.0, 10.0), 0.7, false);
    assert!(ink.finish(), "a single sample should still commit");
    assert_eq!(ink.document.strokes().len(), 1);
}

/// Draw a horizontal stroke in screen space from `(x0, y)` to `(x1, y)`.
fn draw_line(ink: &mut InkState, x0: f32, x1: f32, y: f32) {
    ink.begin(Vec2::new(x0, y), 0.8, false);
    let steps = 12;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        ink.extend(Vec2::new(x0 + (x1 - x0) * t, y), 0.8);
    }
    ink.finish();
}

#[test]
fn strokes_are_stored_in_page_space_so_zoom_does_not_move_them() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 100.0, 200.0, 50.0);
    let before = ink.document.strokes()[0].clone();

    ink.zoom_by_wheel(Vec2::new(0.0, 0.0), 4.0);

    assert_eq!(
        ink.document.strokes()[0],
        before,
        "zooming must not rewrite the document"
    );
}

#[test]
fn drawing_while_zoomed_lands_where_the_pen_is() {
    let mut ink = InkState::new();
    // Zoom 2x about the origin: screen (200, 100) is page (100, 50).
    ink.camera.zoom_about(Vec2::new(0.0, 0.0), 2.0);

    ink.begin(Vec2::new(200.0, 100.0), 0.8, false);
    ink.finish();

    let point = ink.document.strokes()[0].points[0];
    assert!((point[0] - 100.0).abs() < 0.01, "got x = {}", point[0]);
    assert!((point[1] - 50.0).abs() < 0.01, "got y = {}", point[1]);
}

#[test]
fn the_eraser_covers_a_constant_area_on_screen() {
    let mut ink = InkState::new();
    let at_1x = ink.eraser_radius();

    ink.camera.zoom_about(Vec2::new(0.0, 0.0), 2.0);
    let at_2x = ink.eraser_radius();

    assert!(
        (at_2x - at_1x / 2.0).abs() < 0.001,
        "the eraser tip should stay the same size on screen: {at_1x} then {at_2x}"
    );
}

#[test]
fn a_lasso_selects_the_strokes_it_encloses() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0); // inside
    draw_line(&mut ink, 10.0, 60.0, 400.0); // far below

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(80.0, 0.0), 0.0);
    ink.extend(Vec2::new(80.0, 60.0), 0.0);
    ink.extend(Vec2::new(0.0, 60.0), 0.0);
    ink.finish();

    assert_eq!(ink.selection(), &[0], "only the enclosed stroke");
    assert!(ink.has_selection());
}

#[test]
fn dragging_a_selection_moves_it_as_one_undo_step() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);
    let original = ink.document.strokes()[0].clone();

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(80.0, 0.0), 0.0);
    ink.extend(Vec2::new(80.0, 60.0), 0.0);
    ink.extend(Vec2::new(0.0, 60.0), 0.0);
    ink.finish();
    assert_eq!(ink.selection(), &[0]);

    // Grab inside the selection and drag in two steps.
    ink.begin(Vec2::new(30.0, 20.0), 0.0, false);
    ink.extend(Vec2::new(50.0, 20.0), 0.0);
    ink.extend(Vec2::new(70.0, 25.0), 0.0);
    ink.finish();

    let moved = ink.document.strokes()[0].points[0];
    assert!((moved[0] - (original.points[0][0] + 40.0)).abs() < 0.01);
    assert!((moved[1] - (original.points[0][1] + 5.0)).abs() < 0.01);

    // One undo should return the whole drag, not just its last step.
    // Compared approximately: translating in place is float arithmetic, so
    // a round trip lands within rounding error rather than bit-identical.
    assert!(ink.undo());
    let restored = &ink.document.strokes()[0];
    assert_eq!(restored.points.len(), original.points.len());
    for (after, before) in restored.points.iter().zip(&original.points) {
        assert!(
            (after[0] - before[0]).abs() < 0.001 && (after[1] - before[1]).abs() < 0.001,
            "undo should put the stroke back: {after:?} vs {before:?}"
        );
    }
}

#[test]
fn deleting_a_selection_removes_it_and_clears_the_highlight() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(80.0, 0.0), 0.0);
    ink.extend(Vec2::new(80.0, 60.0), 0.0);
    ink.extend(Vec2::new(0.0, 60.0), 0.0);
    ink.finish();

    assert!(ink.delete_selection());
    assert!(ink.document.is_empty());
    assert!(!ink.has_selection());
    assert!(!ink.delete_selection(), "nothing left to delete");
}

#[test]
fn undo_drops_a_selection_whose_indices_would_be_stale() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(80.0, 0.0), 0.0);
    ink.extend(Vec2::new(80.0, 60.0), 0.0);
    ink.extend(Vec2::new(0.0, 60.0), 0.0);
    ink.finish();
    assert!(ink.has_selection());

    ink.undo();
    assert!(
        !ink.has_selection(),
        "indices into a changed document must not survive"
    );
}

#[test]
fn a_lasso_with_too_few_points_selects_nothing() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(1.0, 1.0), 0.0);
    ink.finish();

    assert!(!ink.has_selection());
}

#[test]
fn a_highlighter_stroke_is_translucent_flat_and_wide() {
    let mut ink = InkState::new();
    ink.tool = Tool::Highlighter;
    draw_line(&mut ink, 10.0, 60.0, 20.0);

    let stroke = &ink.document.strokes()[0];
    assert_eq!(stroke.kind, document::StrokeKind::Highlighter);
    assert!(
        (stroke.color[3] - HIGHLIGHTER_ALPHA).abs() < 0.001,
        "highlighter ink must be translucent"
    );
    assert!(
        stroke.size > StrokeOptions::default().size * 2.0,
        "a highlighter should be much broader than the pen"
    );

    // Pressure must not modulate the width, or it reads as a fat pen.
    let options = options_for(stroke.kind, ink.options);
    assert_eq!(options.thinning, 0.0);
    assert_eq!(options.taper_start, 0.0);
    assert_eq!(options.taper_end, 0.0);
}

#[test]
fn switching_tools_keeps_each_tools_colour_and_width() {
    let mut ink = InkState::new();
    let pen_color = ink.color;
    let pen_size = ink.options.size;

    ink.tool = Tool::Highlighter;
    ink.highlighter_color = COLOR_PRESETS[3].1;
    ink.highlighter_size = 30.0;

    ink.tool = Tool::Pen;
    assert_eq!(ink.color, pen_color, "the pen keeps its own colour");
    assert_eq!(ink.options.size, pen_size, "and its own width");
}

#[test]
fn an_eraser_sweep_is_one_undo_step_through_the_state_layer() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 0.0, 50.0, 10.0);
    draw_line(&mut ink, 0.0, 50.0, 40.0);
    assert_eq!(ink.document.strokes().len(), 2);

    ink.tool = Tool::Eraser;
    ink.eraser_mode = EraserMode::Stroke;
    // A single drag across both lines.
    ink.begin(Vec2::new(25.0, 10.0), 0.0, false);
    ink.extend(Vec2::new(25.0, 25.0), 0.0);
    ink.extend(Vec2::new(25.0, 40.0), 0.0);
    ink.finish();
    assert!(ink.document.is_empty());

    assert!(ink.undo());
    assert_eq!(
        ink.document.strokes().len(),
        2,
        "one undo should restore the whole sweep, not one stroke"
    );
}

#[test]
fn abandoning_an_eraser_sweep_does_not_strand_the_batch() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 0.0, 50.0, 10.0);

    ink.tool = Tool::Eraser;
    ink.eraser_mode = EraserMode::Stroke;
    ink.begin(Vec2::new(25.0, 10.0), 0.0, false);
    // Surface dismissed mid-sweep.
    ink.set_visible(false);

    assert!(ink.document.is_empty());
    assert!(
        ink.undo(),
        "the interrupted sweep should still be on the history"
    );
    assert_eq!(ink.document.strokes().len(), 1);
}

#[test]
fn the_partial_eraser_leaves_the_rest_of_the_stroke() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 0.0, 100.0, 20.0);

    ink.tool = Tool::Eraser;
    ink.eraser_mode = EraserMode::Partial;
    ink.begin(Vec2::new(50.0, 20.0), 0.0, false);
    ink.finish();

    assert_eq!(
        ink.document.strokes().len(),
        2,
        "a partial erase should cut the line, not remove it"
    );
}

#[test]
fn a_stray_pen_up_does_not_clear_the_selection() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(80.0, 0.0), 0.0);
    ink.extend(Vec2::new(80.0, 60.0), 0.0);
    ink.extend(Vec2::new(0.0, 60.0), 0.0);
    ink.finish();
    assert!(ink.has_selection());

    // A lift with no matching press — the press landed off-surface and was
    // rejected, so no lasso was ever started.
    assert!(!ink.finish(), "nothing happened, so nothing changed");
    assert!(
        ink.has_selection(),
        "the selection must survive an unmatched pen-up"
    );
}

#[test]
fn a_lasso_tap_deselects() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);

    ink.tool = Tool::Lasso;
    ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
    ink.extend(Vec2::new(80.0, 0.0), 0.0);
    ink.extend(Vec2::new(80.0, 60.0), 0.0);
    ink.extend(Vec2::new(0.0, 60.0), 0.0);
    ink.finish();
    assert!(ink.has_selection());

    // Tap on empty space: a lasso is started but never becomes a loop.
    ink.begin(Vec2::new(300.0, 300.0), 0.0, false);
    assert!(ink.finish());
    assert!(!ink.has_selection(), "a tap should drop the selection");
}

#[test]
fn the_starting_colour_is_one_of_the_swatches() {
    // Regression: the default was the theme's 8-bit text colour while the
    // first preset was an f32 literal, so no swatch ever read as selected.
    let ink = InkState::new();
    assert!(
        COLOR_PRESETS
            .iter()
            .any(|(_, preset)| same_color(ink.color, *preset)),
        "the palette must show a selection from the first frame"
    );
}

#[test]
fn every_preset_matches_only_itself() {
    for (name, a) in COLOR_PRESETS {
        let matches = COLOR_PRESETS
            .iter()
            .filter(|(_, b)| same_color(a, *b))
            .count();
        assert_eq!(
            matches, 1,
            "{name} should be distinguishable from every other swatch"
        );
    }
}

#[test]
fn colour_matching_tolerates_eight_bit_rounding() {
    // The same colour authored two ways must still compare equal.
    let from_bytes = Color::from_rgb8(227, 229, 237);
    let from_floats = Color::from_rgb(0.890, 0.898, 0.929);
    assert!(same_color(from_bytes, from_floats));
    assert!(!same_color(from_bytes, COLOR_PRESETS[1].1));
}

fn hover_sample(x: f32, y: f32) -> PenSample {
    PenSample {
        phase: PenPhase::Hover,
        x,
        y,
        pressure: 0.0,
        eraser: false,
        menu: false,
    }
}

#[test]
fn hovering_reports_where_the_nib_is_aimed() {
    let mut ink = surface();
    assert!(ink.hover().is_none());

    assert!(ink.apply_samples(&[hover_sample(300.0, 140.0)], 1.0));
    let aim = ink.hover().expect("hover should be tracked");
    // Surface starts at (200, 40), so client (300, 140) is local (100, 100).
    assert!((aim.x - 100.0).abs() < 0.01);
    assert!((aim.y - 100.0).abs() < 0.01);

    // Hovering must not draw anything.
    assert!(ink.document.is_empty());
}

#[test]
fn the_aim_indicator_follows_an_eraser_drag() {
    // Regression: the eraser ring was positioned from iced's mouse cursor,
    // which pen input never updates, so it sat frozen wherever the mouse
    // happened to be while the pen erased elsewhere.
    let mut ink = surface();
    ink.tool = Tool::Eraser;

    ink.begin(Vec2::new(50.0, 60.0), 0.0, false);
    assert_eq!(ink.hover(), Some(Vec2::new(50.0, 60.0)));

    ink.extend(Vec2::new(180.0, 210.0), 0.0);
    assert_eq!(
        ink.hover(),
        Some(Vec2::new(180.0, 210.0)),
        "the ring must track the tip through the drag"
    );
}

#[test]
fn a_mouse_move_updates_the_same_aim_state_as_the_pen() {
    let mut ink = surface();
    ink.apply_samples(&[hover_sample(300.0, 140.0)], 1.0);
    assert_eq!(ink.hover(), Some(Vec2::new(100.0, 100.0)));

    // Mouse positions arrive already surface-local.
    ink.set_hover(Vec2::new(400.0, 320.0));
    assert_eq!(ink.hover(), Some(Vec2::new(400.0, 320.0)));
}

#[test]
fn leaving_range_clears_the_aim_indicator() {
    let mut ink = surface();
    ink.apply_samples(&[hover_sample(300.0, 140.0)], 1.0);
    assert!(ink.hover().is_some());

    let leave = PenSample {
        phase: PenPhase::Leave,
        ..hover_sample(0.0, 0.0)
    };
    assert!(ink.apply_samples(&[leave], 1.0));
    assert!(ink.hover().is_none());
}

#[test]
fn ending_a_stroke_in_the_bottom_margin_pulls_the_page_up() {
    // Surface is 600 tall, so the advance threshold is 600 - 120 = 480.
    let mut ink = surface();
    let before = ink.camera.pan.y;

    ink.begin(Vec2::new(100.0, 400.0), 0.8, false);
    ink.extend(Vec2::new(100.0, 540.0), 0.8);
    assert_eq!(
        ink.camera.pan.y, before,
        "the page never moves while the pen is down"
    );

    ink.finish();
    assert!(
        (ink.camera.pan.y - (before - 60.0)).abs() < 0.01,
        "pen-up in the margin lifts the line to the threshold, got {}",
        ink.camera.pan.y
    );
}

#[test]
fn a_stroke_ending_above_the_margin_does_not_advance() {
    let mut ink = surface();
    let before = ink.camera.pan.y;
    ink.begin(Vec2::new(100.0, 540.0), 0.8, false);
    ink.extend(Vec2::new(100.0, 300.0), 0.8);
    ink.finish();
    assert_eq!(ink.camera.pan.y, before);
}

/// Regression for the "page split": writing along a line in the margin used
/// to pan on every sample, so the page ran away and the stroke smeared.
#[test]
fn writing_along_the_margin_keeps_the_stroke_contiguous() {
    let mut ink = surface();
    ink.begin(Vec2::new(100.0, 540.0), 0.8, false);
    for step in 1..=50 {
        ink.extend(Vec2::new(100.0 + step as f32 * 2.0, 540.0), 0.8);
    }

    let points = ink.live_points();
    for pair in points.windows(2) {
        let gap = pair[1].pos.sub(pair[0].pos);
        assert!(
            gap.x.abs() <= 2.01 && gap.y.abs() < 0.01,
            "samples 2px apart on screen must stay 2px apart on the page, got {gap:?}"
        );
    }

    ink.finish();
    ink.begin(Vec2::new(100.0, 400.0), 0.8, false);
    ink.finish();
    assert!(
        (ink.camera.pan.y + 60.0).abs() < 0.01,
        "one stroke in the margin advances once, got {}",
        ink.camera.pan.y
    );
}

#[test]
fn advancing_keeps_the_stroke_where_it_was_written() {
    let mut ink = surface();
    ink.begin(Vec2::new(100.0, 540.0), 0.8, false);
    ink.extend(Vec2::new(110.0, 545.0), 0.8);
    let written: Vec<_> = ink.live_points().iter().map(|p| p.pos).collect();
    ink.finish();

    let stored = ink.document.strokes().last().unwrap().ink_points();
    assert_eq!(stored.first().map(|p| p.pos), written.first().copied());
    assert_eq!(
        stored.last().map(|p| p.pos),
        written.last().copied(),
        "the page moves under the ink, not the other way round"
    );
}

#[test]
fn cancelling_a_selection_drag_puts_the_strokes_back() {
    let mut ink = surface();
    ink.begin(Vec2::new(100.0, 100.0), 0.8, false);
    ink.extend(Vec2::new(140.0, 100.0), 0.8);
    ink.finish();
    ink.dirty = false;
    let original = ink.document.strokes()[0].path_bounds();

    ink.tool = Tool::Lasso;
    for point in [(80.0, 80.0), (160.0, 80.0), (160.0, 120.0), (80.0, 120.0)] {
        if ink.lasso().is_empty() {
            ink.begin(Vec2::new(point.0, point.1), 0.5, false);
        } else {
            ink.extend(Vec2::new(point.0, point.1), 0.5);
        }
    }
    ink.finish();
    assert!(ink.has_selection());

    ink.begin(Vec2::new(120.0, 100.0), 0.5, false);
    ink.extend(Vec2::new(220.0, 300.0), 0.5);
    assert_ne!(ink.document.strokes()[0].path_bounds(), original);

    ink.cancel_stroke();
    assert_eq!(ink.document.strokes()[0].path_bounds(), original);
    assert!(!ink.dirty);
}

#[test]
fn the_barrel_button_opens_the_menu_instead_of_drawing() {
    let mut ink = surface();
    let mut press = hover_sample(300.0, 140.0);
    press.phase = PenPhase::Down;
    press.menu = true;
    press.pressure = 0.7;

    ink.apply_samples(&[press], 1.0);
    assert!(
        ink.radial_menu().is_some(),
        "the side button summons the ring"
    );
    assert!(ink.live_points().is_empty(), "and lays down no ink");
}

#[test]
fn the_menu_applies_the_slot_the_pen_lifts_on() {
    let mut ink = surface();
    assert_eq!(ink.tool, Tool::Pen);

    let mut press = hover_sample(300.0, 240.0);
    press.phase = PenPhase::Down;
    press.menu = true;
    ink.apply_samples(&[press], 1.0);

    // Flick right, into the second tool slot, then lift.
    let mut moved = hover_sample(300.0 + radial::TOOL_RADIUS, 240.0);
    moved.phase = PenPhase::Move;
    let mut lifted = moved;
    lifted.phase = PenPhase::Up;
    ink.apply_samples(&[moved, lifted], 1.0);

    assert!(ink.radial_menu().is_none(), "lifting closes the menu");
    assert_eq!(ink.tool, Tool::Highlighter);
    assert!(
        ink.document.is_empty(),
        "the gesture must not leave a stroke"
    );
}

#[test]
fn lifting_in_the_dead_zone_cancels_the_menu() {
    let mut ink = surface();
    ink.tool = Tool::Eraser;

    let mut press = hover_sample(300.0, 240.0);
    press.phase = PenPhase::Down;
    press.menu = true;
    ink.apply_samples(&[press], 1.0);

    let mut lifted = press;
    lifted.phase = PenPhase::Up;
    ink.apply_samples(&[lifted], 1.0);

    assert!(ink.radial_menu().is_none());
    assert_eq!(ink.tool, Tool::Eraser, "no travel means no change");
}

#[test]
fn the_menu_can_set_a_colour_without_touching_the_tool() {
    let mut ink = surface();
    let target = COLOR_PRESETS[3].1;

    let mut press = hover_sample(400.0, 340.0);
    press.phase = PenPhase::Down;
    press.menu = true;
    ink.apply_samples(&[press], 1.0);

    // Straight up into the outer ring is the first colour slot; walk round
    // to the fourth by angle instead.
    let menu = ink.radial_menu().unwrap();
    let slot = radial::slot_position(menu.center, 3, COLOR_PRESETS.len(), radial::COLOR_RADIUS);
    let mut moved = hover_sample(slot.x + 200.0, slot.y + 40.0);
    moved.phase = PenPhase::Move;
    let mut lifted = moved;
    lifted.phase = PenPhase::Up;
    ink.apply_samples(&[moved, lifted], 1.0);

    assert_eq!(
        ink.tool,
        Tool::Pen,
        "picking a colour must not change tools"
    );
    assert!(same_color(ink.color, target));
}

#[test]
fn changing_paper_marks_the_page_unsaved() {
    let mut ink = InkState::new();
    assert!(!ink.dirty);
    ink.set_paper(document::PaperStyle::Grid);
    assert_eq!(ink.document.paper, document::PaperStyle::Grid);
    assert!(ink.dirty);
}

#[test]
fn panning_does_not_alter_the_document() {
    let mut ink = InkState::new();
    draw_line(&mut ink, 10.0, 60.0, 20.0);
    let before = ink.document.strokes()[0].clone();

    assert!(ink.pan_by(Vec2::new(120.0, -40.0)));
    assert!(
        !ink.pan_by(Vec2::new(0.0, 0.0)),
        "a zero pan is not a change"
    );
    assert_eq!(ink.document.strokes()[0], before);

    assert!(ink.reset_view());
    assert!(!ink.reset_view(), "already at the identity view");
}

#[test]
fn vault_round_trip_preserves_the_page() {
    let dir = std::env::temp_dir().join(format!("md_ink_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.to_str().unwrap();

    let mut ink = InkState::new();
    ink.begin(Vec2::new(1.0, 2.0), 0.5, false);
    ink.extend(Vec2::new(9.0, 12.0), 0.9);
    ink.finish();

    save_to_vault(root, "notes/page.ink", &ink.document).unwrap();
    let loaded = load_from_vault(root, "notes/page.ink").unwrap();
    assert_eq!(loaded.strokes(), ink.document.strokes());

    save_to_vault(root, "notes/page.ink.svg", &ink.document).unwrap();
    let loaded_svg = load_from_vault(root, "notes/page.ink.svg").unwrap();
    assert_eq!(loaded_svg.strokes(), ink.document.strokes());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hovering_with_the_erase_button_aims_the_eraser_until_the_mouse_moves() {
    let mut ink = surface();
    let mut hover = hover_sample(300.0, 140.0);
    hover.eraser = true;
    ink.apply_samples(&[hover], 1.0);
    assert!(ink.pen_erases(), "the ring shows while the button is held");

    hover.eraser = false;
    ink.apply_samples(&[hover], 1.0);
    assert!(!ink.pen_erases(), "and goes once it is released");

    hover.eraser = true;
    ink.apply_samples(&[hover], 1.0);
    ink.set_hover(Vec2::new(10.0, 10.0));
    assert!(!ink.pen_erases(), "the mouse never erases by hovering");
}
