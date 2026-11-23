use super::super::MathRender;
use super::super::testing::make_line;
use super::*;
use crate::editor::highlight::StyledSpan;

#[test]
fn table_scrollbar_gutter_is_reserved_only_after_last_table_row() {
    let mut first = make_line(1, vec![]);
    first.is_table_row = true;
    let mut second = make_line(1, vec![]);
    second.is_table_row = true;
    let plain = make_line(2, vec![StyledSpan::plain("after")]);
    let lines = vec![first, second, plain];

    assert_eq!(table_block_gutter_after(&lines, 0, false), 0.0);
    assert_eq!(
        table_block_gutter_after(&lines, 1, false),
        HORIZONTAL_SCROLLBAR_GUTTER
    );
    assert_eq!(table_block_gutter_after(&lines, 1, true), 0.0);
    assert_eq!(table_block_gutter_after(&lines, 2, false), 0.0);
}

#[test]
fn inactive_plain_line_height_does_not_create_cursorless_blank_gap() {
    let line = make_line(1, vec![StyledSpan::plain("short line")]);
    let image_cache = HashMap::new();
    let math_cache = HashMap::new();
    let mut seen_math_blocks = HashSet::new();

    let h = line_height_for::<iced::Renderer>(
        &line,
        &image_cache,
        &math_cache,
        900.0,
        false,
        None,
        &mut seen_math_blocks,
    );
    assert_eq!(h, body_row());
}

#[test]
fn line_visual_y_includes_single_table_scrollbar_gutter() {
    let mut header = make_line(1, vec![]);
    header.is_table_row = true;
    header.table_cells = vec![vec![StyledSpan::plain("A")], vec![StyledSpan::plain("B")]];
    let mut body = make_line(1, vec![]);
    body.is_table_row = true;
    body.table_cells = vec![vec![StyledSpan::plain("1")], vec![StyledSpan::plain("2")]];
    let after = make_line(2, vec![StyledSpan::plain("after")]);
    let lines = vec![header, body, after];
    let image_cache = HashMap::new();
    let math_cache = HashMap::new();

    let y_after_table =
        line_visual_y::<iced::Renderer>(&lines, &image_cache, &math_cache, 900.0, 0, 0, 2, false);

    assert_eq!(
        y_after_table,
        TOP_PAD
            + BLOCK_CAPTION_HEIGHT
            + TABLE_ROW_HEIGHT
            + TABLE_ROW_HEIGHT
            + HORIZONTAL_SCROLLBAR_GUTTER
    );
}

#[test]
fn test_renderer_line_height_permutations() {
    let mut lines = Vec::new();

    // 1. Plain text line
    lines.push(make_line(1, vec![StyledSpan::plain("Hello world")]));

    // 2. Code block line
    let mut code_line = make_line(
        2,
        vec![StyledSpan {
            text: "let x = 10;".to_string(),
            is_code: true,
            ..StyledSpan::plain("")
        }],
    );
    code_line.is_code_block = true;
    lines.push(code_line);

    // 3. Math block line (not editing)
    let mut math_line = make_line(
        3,
        vec![StyledSpan {
            text: "$$ E = mc^2 $$".to_string(),
            is_math: true,
            ..StyledSpan::plain("")
        }],
    );
    math_line.is_math_block = true;
    lines.push(math_line);

    // 4. Table row
    let mut table_line = make_line(4, vec![]);
    table_line.is_table_row = true;
    table_line.table_cells = vec![
        vec![StyledSpan::plain("Col A")],
        vec![StyledSpan::plain("Col B")],
    ];
    lines.push(table_line);

    // 5. Image line
    let img_line = make_line(
        5,
        vec![StyledSpan {
            text: "![alt](image.png)".to_string(),
            is_image: true,
            image_path: Some("image.png".to_string()),
            ..StyledSpan::plain("")
        }],
    );
    lines.push(img_line);

    // 6. Deep quote line
    let mut quote_line = make_line(6, vec![StyledSpan::plain("A quote")]);
    quote_line.is_blockquote = true;
    lines.push(quote_line);

    let mut image_cache = HashMap::new();
    let mut math_cache = HashMap::new();

    image_cache.insert(
        "image.png".to_string(),
        (
            iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
            400.0,
            300.0,
        ),
    );
    math_cache.insert(
        "E = mc^2".to_string(),
        MathRender {
            inline_handle: iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
            block_handle: iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
            width: 200.0,
            height: 50.0,
        },
    );

    let widths = vec![100.0, 200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0];
    let mut seen_math_blocks = HashSet::new();

    for &width in &widths {
        for &is_editing in &[true, false] {
            for line in &lines {
                seen_math_blocks.clear();
                let h = line_height_for::<iced::Renderer>(
                    line,
                    &image_cache,
                    &math_cache,
                    width,
                    is_editing,
                    None,
                    &mut seen_math_blocks,
                );

                assert!(h >= 0.0);

                if line.is_table_row {
                    if is_editing {
                        assert!(h >= body_row());
                    } else {
                        assert_eq!(h, TABLE_ROW_HEIGHT);
                    }
                } else if line.is_math_block && is_editing {
                    // Source being edited wraps like any other text.
                    assert!(h >= body_row());
                    if width >= 400.0 {
                        assert_eq!(h, body_row());
                    }
                } else if line.is_blockquote {
                    assert!(h > 0.0);
                }
            }
        }
    }
}

#[test]
fn test_renderer_total_height_accumulation() {
    let mut lines = Vec::new();
    for i in 1..=200 {
        lines.push(make_line(
            i,
            vec![StyledSpan::plain("Hello accumulated document")],
        ));
    }

    let image_cache = HashMap::new();
    let math_cache = HashMap::new();
    let height = |lines: &[StyledLine], width: f32| {
        total_height::<iced::Renderer>(lines, &image_cache, &math_cache, width, None, None, false)
    };

    // 1. Verify adding lines monotonically increases total height
    let h1 = height(&lines[0..50], 800.0);
    let h2 = height(&lines[0..100], 800.0);
    let h3 = height(&lines[0..200], 800.0);

    assert!(h2 > h1);
    assert!(h3 > h2);

    // 2. Verify width decreases wrapping space and monotonically increases total height
    let h_wide = height(&lines, 1000.0);
    let h_narrow = height(&lines, 200.0);

    assert!(h_narrow >= h_wide);
}

#[test]
fn test_bug_finder_renderer_extreme_dimensions() {
    let line = make_line(
        1,
        vec![StyledSpan::plain(
            "Wrap this extremely long sentence with extreme layout boundary dimensions to find bugs.",
        )],
    );
    let image_cache = HashMap::new();
    let math_cache = HashMap::new();
    let mut seen_math_blocks = HashSet::new();

    // Extreme layout widths (0, negative, infinite, sub-pixel)
    let extreme_widths = vec![
        0.0,
        -100.0,
        -0.0001,
        0.0001,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    for &width in &extreme_widths {
        seen_math_blocks.clear();
        let h = line_height_for::<iced::Renderer>(
            &line,
            &image_cache,
            &math_cache,
            width,
            false,
            None,
            &mut seen_math_blocks,
        );
        assert!(h >= 0.0);
    }
}
