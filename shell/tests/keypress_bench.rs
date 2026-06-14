//! Coarse p95 keypress -> layout latency gate (master plan §6, impl-plan
//! Phase 7.6). Each iteration drives the real shell keystroke cycle through
//! `MdSession::apply` over a large document: the incremental parse, restyle,
//! and shaped remeasure of the touched range. Paint is viewport-bounded and
//! not the editor cost this protects; this is the dominant per-keystroke work.
//!
//! The threshold is deliberately generous so a shared/cold CI runner does not
//! flake; it exists to catch an order-of-magnitude regression (e.g. a full
//! reparse creeping back onto the keystroke path), not to micro-optimize.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Instant;

use cosmic_text::FontSystem;
use md3_editor::buffer::{Command, Movement};
use md3_shell::gui::session::MdSession;
use md3_shell::gui::shaped_measurer::ShapedMeasurer;

fn large_document(lines: usize) -> String {
    let mut text = String::with_capacity(lines * 48);
    for i in 0..lines {
        match i % 8 {
            0 => text.push_str("## Section heading with **bold** and *italic* prose\n"),
            3 => text.push_str("- a bullet item with a [wikilink] and `inline code`\n"),
            6 => text.push_str("> a quoted line that wraps around the editor width nicely\n"),
            _ => text
                .push_str("The quick brown fox jumps over the lazy dog while typing fast here.\n"),
        }
    }
    text
}

fn p95(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((samples.len() as f64) * 0.95).ceil() as usize;
    samples[idx.saturating_sub(1).min(samples.len() - 1)]
}

#[test]
fn keypress_to_layout_p95_under_budget() {
    let measurer = ShapedMeasurer::new(Arc::new(Mutex::new(FontSystem::new())));
    let mut session = MdSession::new("bench.md", &large_document(5000), measurer);
    session.set_viewport(1000.0, 800.0);

    // Park the caret deep into the document so motion and edits exercise the
    // height tree at depth, not at the cheap top edge.
    session.apply(Command::SetCursor { line: 2500, col: 0 });

    // Warm up: first shapes can pay one-time font/cache costs unrelated to the
    // steady-state keystroke loop.
    for _ in 0..200 {
        session.apply(Command::Insert("x".to_string()));
        session.apply(Command::DeleteBackward);
    }

    let mut samples = Vec::with_capacity(2000);

    // Steady-state typing: insert a character, then a caret motion, timing the
    // full apply each time.
    for i in 0..1000 {
        let start = Instant::now();
        session.apply(Command::Insert("a".to_string()));
        samples.push(start.elapsed().as_secs_f64() * 1000.0);

        let movement = if i % 2 == 0 {
            Movement::Right
        } else {
            Movement::Down
        };
        let start = Instant::now();
        session.apply(Command::Move {
            movement,
            extend: false,
        });
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let p95_ms = p95(samples);
    // 16ms is a 60fps frame; allow a wide multiple for shared CI hardware. A
    // healthy incremental cycle is well under 1ms locally.
    let budget_ms = 16.0;
    assert!(
        p95_ms < budget_ms,
        "p95 keypress->layout was {p95_ms:.3}ms, over the {budget_ms}ms budget \
         (an order-of-magnitude regression — suspect a full reparse/remeasure \
         on the keystroke path)"
    );
    eprintln!("keypress->layout p95 = {p95_ms:.3}ms (budget {budget_ms}ms)");
}
