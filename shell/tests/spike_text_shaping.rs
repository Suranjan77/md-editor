use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};

#[test]
fn spike_cosmic_text_direct() {
    let mut font_system = FontSystem::new();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 24.0));

    let text = "This is a paragraph of text that we are using to test cosmic-text directly. We want to see if we can hit test and measure it.";

    // Set text and size
    buffer.set_text(
        &mut font_system,
        text,
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        None,
    );
    buffer.set_size(&mut font_system, Some(200.0), None);

    // Shape it
    buffer.shape_until_scroll(&mut font_system, false);

    // Print layout
    for run in buffer.layout_runs() {
        println!(
            "Run: line_i={}, text='{}', w={}, h={}, line_y={}",
            run.line_i, run.text, run.line_w, run.line_height, run.line_y
        );
    }

    // Hit test
    if let Some(cursor) = buffer.hit(50.0, 10.0) {
        println!(
            "Hit at 50,10: index={}, affinity={:?}",
            cursor.index, cursor.affinity
        );
    }

    assert!(!buffer.layout_runs().count() > 0);
}
