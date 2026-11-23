//! Tests for `editor_state`.

use super::*;
use crate::editor::buffer::EditorCommand;

/// The layout revision changes exactly when something the editor lays out
/// does: the lines, an arriving image or equation, or the math scale.
#[test]
fn layout_revision_names_the_lines_and_resources_laid_out() {
    let mut pane = EditorPane::new();
    pane.buffer = crate::editor::buffer::DocBuffer::from_text("$$\nx\n$$\n![a](i.png)");
    pane.refresh_highlighting();
    let initial = pane.layout_revision();
    assert_eq!(
        pane.layout_revision(),
        initial,
        "stable while nothing changes"
    );

    let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]);
    pane.math_cache.insert(
        "x".into(),
        crate::editor::renderer::MathRender {
            inline_handle: handle.clone(),
            block_handle: handle.clone(),
            width: 10.0,
            height: 10.0,
        },
    );
    let with_math = pane.layout_revision();
    assert_ne!(with_math, initial, "an equation arrived");

    pane.image_cache.insert("i.png".into(), (handle, 1.0, 1.0));
    let with_image = pane.layout_revision();
    assert_ne!(with_image, with_math, "an image arrived");

    pane.math_scale_factor = 2.0;
    let rescaled = pane.layout_revision();
    assert_ne!(rescaled, with_image, "the math scale changed");

    pane.buffer.insert_at_cursor("y");
    pane.refresh_highlighting();
    assert_ne!(pane.layout_revision(), rescaled, "the lines changed");
}

fn edited(text: &str) -> DocBuffer {
    let mut buffer = DocBuffer::from_text(text);
    buffer.execute(EditorCommand::InsertText("!".to_string()));
    buffer
}

/// Parking a buffer and picking it up again must return the same document
/// *with its history* — that is the whole point of the registry.
#[test]
fn retained_buffer_round_trip_keeps_undo_history() {
    let mut pane = EditorPane::new();
    let buffer = edited("hello");
    let text = buffer.text();

    pane.retain_buffer("notes/a.md".to_string(), buffer, 128.0);

    let (mut resumed, scroll) = pane
        .take_retained_matching("notes/a.md", &text)
        .expect("buffer matching disk should be resumable");
    assert_eq!(scroll, 128.0, "scroll position comes back with the buffer");

    assert!(resumed.execute(EditorCommand::Undo).text_changed);
    assert_eq!(
        resumed.text(),
        "hello",
        "undo history survived the round trip"
    );
}

/// If the file changed on disk while parked, the history no longer
/// describes that file and must be discarded rather than replayed onto it.
#[test]
fn retained_buffer_is_dropped_when_disk_content_diverges() {
    let mut pane = EditorPane::new();
    pane.retain_buffer("notes/a.md".to_string(), DocBuffer::from_text("old"), 0.0);

    assert!(
        pane.take_retained_matching("notes/a.md", "changed elsewhere")
            .is_none(),
        "a diverged file must not resume stale history"
    );
    // The stale entry is evicted, not left to match a later coincidence.
    assert!(pane.take_retained_matching("notes/a.md", "old").is_none());
}

/// A deleted file must not hand its history to a new file of the same name.
#[test]
fn forgetting_a_path_drops_its_buffer() {
    let mut pane = EditorPane::new();
    pane.retain_buffer("notes/a.md".to_string(), DocBuffer::from_text("x"), 0.0);
    pane.forget_retained("notes/a.md");
    assert!(pane.take_retained_matching("notes/a.md", "x").is_none());
}

/// Session memory stays bounded on a large vault.
#[test]
fn registry_evicts_least_recently_used() {
    let mut pane = EditorPane::new();
    for i in 0..(MAX_RETAINED_BUFFERS + 5) {
        pane.retain_buffer(format!("n{i}.md"), DocBuffer::from_text("x"), 0.0);
    }
    assert_eq!(pane.retained.len(), MAX_RETAINED_BUFFERS);
    assert!(
        pane.take_retained_matching("n0.md", "x").is_none(),
        "oldest entry should have been evicted"
    );
    let newest = format!("n{}.md", MAX_RETAINED_BUFFERS + 4);
    assert!(pane.take_retained_matching(&newest, "x").is_some());
}

mod math_blocks {
    use super::super::render_latex_task;
    use crate::editor::highlight::highlight_markdown;

    #[test]
    fn nested_align_math_block_reaches_the_renderer_as_one_formula() {
        let markdown = "$$\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n$$\nAfter";
        let lines = highlight_markdown(markdown);
        let tex = lines[0].spans[0].visible_text(false);

        assert_eq!(tex, "\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}");
        let crate::editor::renderer::MathRender { width, height, .. } =
            render_latex_task(tex, 1.0).expect("align block should render");
        assert!(width > 0.0);
        assert!(height > 24.0, "both align rows should have visible height");
        assert!(!lines[6].is_math_block);
    }

    #[test]
    fn align_nonumber_with_spaced_environment_braces_renders() {
        let markdown = "\\begin {align}\na &= b \\nonumber\n\\end {align}\nAfter";
        let lines = highlight_markdown(markdown);
        let tex = lines[0].spans[0].visible_text(false);

        assert_eq!(tex, "\\begin {align}\na &= b \\nonumber\n\\end {align}");
        let crate::editor::renderer::MathRender { width, height, .. } =
            render_latex_task(tex, 1.0).expect("align with nonumber should render");
        assert!(width > 0.0);
        assert!(height > 0.0);
        assert!(!lines[3].is_math_block);
    }

    #[test]
    fn linalg_align_block_with_nonumber_on_every_row_renders() {
        let markdown = r"$$
\begin{align}
TS(u_k) &= T \left( \sum_{i=1}^n A_{i,k}\;\;v_i \right) \nonumber \\
        &= \sum_{i=1}^n A_{i,k}\;\;Tv_i \nonumber \\
        &= \sum_{i=1}^n A_{i,k}\;\;\sum_{j=1}^m B_{j,i}\;\;w_j   \nonumber \\
        &= \sum_{j=1}^m \left(\sum_{i=1}^n B_{j,i}\; A_{i,k} \right)\;w_j \nonumber
\end{align}
$$
After";
        let lines = highlight_markdown(markdown);
        let tex = lines[0].spans[0].visible_text(false);

        assert_eq!(tex.lines().count(), 6);
        let crate::editor::renderer::MathRender { width, height, .. } =
            render_latex_task(tex, 1.0).expect("the linalg align block should render");
        assert!(width > 0.0);
        assert!(height > 80.0, "all four align rows should be visible");
        assert!(!lines[8].is_math_block);
    }
}
