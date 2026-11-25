//! Tests for `pptx`.

use super::*;
use std::io::{Cursor, Write};

const FIXTURE: &str = "../tests-fixtures/pptx/fixture.pptx";

fn fixture() -> Presentation {
    load_presentation(Path::new(FIXTURE)).expect("fixture parses")
}

fn shapes(slide: &Slide) -> Vec<(&Element, &Shape)> {
    slide
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementKind::Shape(shape) => Some((element, shape)),
            _ => None,
        })
        .collect()
}

fn text_of(body: &TextBody) -> Vec<String> {
    body.paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect())
        .collect()
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "expected {expected}, got {actual}"
    );
}

/// A package from `(part name, content)` pairs, stored uncompressed.
fn package(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, content) in parts {
        writer.start_file(*name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;

/// A one-slide package with `shapes` as the slide's shape tree.
fn single_slide(shapes: &str) -> Vec<u8> {
    let root_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#
    );
    let presentation_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let slide = format!(r#"<p:sld {NS}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#);
    package(&[
        ("_rels/.rels", root_rels),
        ("ppt/presentation.xml", &presentation),
        ("ppt/_rels/presentation.xml.rels", presentation_rels),
        ("ppt/slides/slide1.xml", &slide),
    ])
}

fn rect_shape(id: u32, extra: &str, xfrm: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="s{id}" {extra}/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm {xfrm}><a:off x="1270000" y="1270000"/><a:ext cx="1270000" cy="635000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:spPr></p:sp>"#
    )
}

#[test]
fn reads_slide_size_and_count() {
    let deck = fixture();
    assert_eq!(deck.slides.len(), 5);
    assert_close(deck.width, 720.0);
    assert_close(deck.height, 540.0);
    let numbers: Vec<usize> = deck.slides.iter().map(|s| s.number).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5]);
}

#[test]
fn title_placeholder_inherits_position_and_style() {
    let deck = fixture();
    let slide = &deck.slides[0];
    assert_eq!(slide.title.as_deref(), Some("Fixture Deck"));
    let (title_element, title) = shapes(slide)[0];
    // Geometry comes from the title layout's placeholder.
    assert_close(title_element.bounds.x, 54.0);
    assert_close(title_element.bounds.y, 167.75);
    assert_close(title_element.bounds.width, 612.0);
    let body = title.text.as_ref().unwrap();
    let paragraph = &body.paragraphs[0];
    // Alignment and size come from the master's title style, the font
    // from the theme's major font.
    assert_eq!(paragraph.align, Align::Center);
    assert_close(paragraph.runs[0].style.size, 44.0);
    assert_eq!(paragraph.runs[0].style.font, "Calibri");
    assert_eq!(body.anchor, Anchor::Middle);

    let (_, subtitle) = shapes(slide)[1];
    let run = &subtitle.text.as_ref().unwrap().paragraphs[0];
    // The layout switches the master's bullet off and tints the text.
    assert!(run.bullet.is_none());
    assert_eq!(run.runs[0].style.color, Rgba::rgb(64, 64, 64));
}

#[test]
fn body_placeholder_levels_bullets_and_runs() {
    let deck = fixture();
    let (element, body) = shapes(&deck.slides[1])[1];
    // No geometry on the slide or its layout: the master's body frame.
    assert_close(element.bounds.x, 36.0);
    assert_close(element.bounds.y, 126.0);
    let body = body.text.as_ref().unwrap();
    let first = &body.paragraphs[0];
    assert_eq!(first.bullet.as_ref().unwrap().text, "•");
    assert_close(first.runs[0].style.size, 32.0);
    let second = &body.paragraphs[1];
    assert_eq!(second.bullet.as_ref().unwrap().text, "–");
    assert_close(second.margin_left, 58.5);
    assert_close(second.runs[0].style.size, 28.0);
    let mixed = &body.paragraphs[2].runs;
    assert_eq!(mixed.len(), 3);
    assert!(!mixed[0].style.bold);
    assert!(mixed[1].style.bold);
    assert_eq!(mixed[2].style.color, Rgba::rgb(0xC0, 0, 0));
}

#[test]
fn shapes_resolve_theme_styles_groups_and_pictures() {
    let deck = fixture();
    let slide = &deck.slides[2];
    assert_eq!(slide.background, Fill::Solid(Rgba::rgb(0xF5, 0xF2, 0xEC)));

    let all = shapes(slide);
    let (_, styled) = all[0];
    // fillRef 3 is the theme's gradient in accent 1; fontRef makes the
    // text lt1, lnRef 1 gives the theme's thinnest line.
    assert!(matches!(&styled.fill, Fill::Gradient(g) if g.stops.len() == 2));
    assert_close(styled.line.as_ref().unwrap().width, 0.75);
    let text = styled.text.as_ref().unwrap();
    assert_eq!(text.anchor, Anchor::Middle);
    assert_eq!(text.paragraphs[0].runs[0].style.color, Rgba::WHITE);

    let (oval_element, oval) = all[1];
    assert_eq!(oval.fill, Fill::Solid(Rgba::rgb(0xC0, 0x50, 0x4D)));
    assert_close(oval_element.rotation, 30.0);

    // The group is drawn at half size from its own origin.
    let (square, _) = all[2];
    assert_close(square.bounds.x, 72.0);
    assert_close(square.bounds.y, 216.0);
    assert_close(square.bounds.width, 36.0);
    let (triangle, _) = all[3];
    assert_close(triangle.bounds.x, 126.0);

    let (_, connector) = all[4];
    let line = connector.line.as_ref().unwrap();
    assert_close(line.width, 3.0);
    assert_eq!(line.color, Rgba::rgb(0x2C, 0x6E, 0x72));
    assert!(connector.paths.iter().all(|path| !path.filled));

    let picture = slide
        .elements
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::Picture(p) => Some(p),
            _ => None,
        })
        .unwrap();
    assert_eq!(picture.media, "ppt/media/image1.png");
    assert!(deck.media[&picture.media].starts_with(b"\x89PNG"));
}

#[test]
fn tables_merge_cells_and_apply_the_default_style() {
    let deck = fixture();
    let slide = &deck.slides[3];
    let table = slide
        .elements
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.columns, vec![120.0, 120.0, 120.0]);
    assert_eq!(table.rows.len(), 3);
    let header = &table.rows[0].cells;
    assert_eq!(header[0].fill, Fill::Solid(Rgba::rgb(0x4F, 0x81, 0xBD)));
    let header_run = &header[0].text.as_ref().unwrap().paragraphs[0].runs[0];
    assert!(header_run.style.bold);
    assert_eq!(header_run.style.color, Rgba::WHITE);
    assert_eq!(header[1].column_span, 2);
    assert!(header[2].merged);
    assert_eq!(table.rows[1].cells[0].row_span, 2);
    assert!(table.rows[2].cells[0].merged);
    // Banded body rows alternate between two tints of the accent.
    assert_ne!(table.rows[1].cells[1].fill, table.rows[2].cells[1].fill);

    assert!(
        slide
            .elements
            .iter()
            .any(|e| matches!(&e.kind, ElementKind::Unsupported { label } if label == "Chart"))
    );
}

#[test]
fn text_features_numbering_fields_and_gradients() {
    let deck = fixture();
    let slide = &deck.slides[4];
    let Fill::Gradient(gradient) = &slide.background else {
        panic!("expected a gradient background, got {:?}", slide.background);
    };
    assert_eq!(gradient.stops.len(), 2);
    assert_close(gradient.angle, 270.0);

    let all = shapes(slide);
    let body = all[0].1.text.as_ref().unwrap();
    assert_eq!(body.anchor, Anchor::Bottom);
    assert_eq!(body.paragraphs[0].align, Align::Center);
    assert_eq!(text_of(body)[0], "Centered first line\nafter a break");
    let numbers: Vec<&str> = body.paragraphs[1..]
        .iter()
        .map(|p| p.bullet.as_ref().unwrap().text.as_str())
        .collect();
    assert_eq!(numbers, ["1.", "2.", "3."]);
    assert_close(body.paragraphs[1].runs[0].style.size, 14.0);

    let formula = all[1].1.text.as_ref().unwrap();
    assert!(!formula.wrap);
    let runs = &formula.paragraphs[0].runs;
    assert_close(runs[1].style.baseline, 0.3);
    assert_eq!(runs.last().unwrap().text, "5");
}

#[test]
fn rejects_files_that_are_not_presentations() {
    let err = parse_presentation(Cursor::new(b"just some text".to_vec())).unwrap_err();
    assert!(err.to_string().contains("not a zip"), "{err}");

    let mut compound = COMPOUND_FILE_MAGIC.to_vec();
    compound.extend_from_slice(&[0; 512]);
    let err = parse_presentation(Cursor::new(compound)).unwrap_err();
    assert!(err.to_string().contains("password-protected"), "{err}");

    let word = package(&[
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/document.xml", "<w:document xmlns:w=\"urn:w\"/>"),
    ]);
    let err = parse_presentation(Cursor::new(word)).unwrap_err();
    assert!(
        err.to_string().contains("Not a PowerPoint presentation"),
        "{err}"
    );
}

#[test]
fn hidden_shapes_are_skipped_and_fallback_content_is_used() {
    let tree = format!(
        "{}{}<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice Requires=\"p14\">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>",
        rect_shape(2, "", ""),
        rect_shape(3, r#"hidden="1""#, ""),
        rect_shape(4, "", r#"rot="5400000""#),
        rect_shape(5, "", r#"flipH="1""#),
    );
    let deck = parse_presentation(Cursor::new(single_slide(&tree))).unwrap();
    let elements = &deck.slides[0].elements;
    assert_eq!(elements.len(), 2);
    assert_close(elements[0].bounds.x, 100.0);
    assert_close(elements[0].bounds.width, 100.0);
    assert_close(elements[0].bounds.height, 50.0);
    // The fallback's flipped rectangle, not the choice's rotated one.
    assert!(elements[1].flip_h);
    assert_close(elements[1].rotation, 0.0);
}

#[test]
fn rotated_groups_place_children_around_the_group_centre() {
    // A 200×100pt group at the origin, rotated 90°, holding a child in
    // its top-left quarter. Rotating about the group centre (100, 50)
    // moves that child's centre from (50, 25) to (125, 0).
    let tree = r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="2" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm rot="5400000"><a:off x="0" y="0"/><a:ext cx="2540000" cy="1270000"/><a:chOff x="0" y="0"/><a:chExt cx="2540000" cy="1270000"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="3" name="c"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></p:spPr></p:sp></p:grpSp>"#;
    let deck = parse_presentation(Cursor::new(single_slide(tree))).unwrap();
    let element = &deck.slides[0].elements[0];
    assert_close(element.bounds.x + element.bounds.width / 2.0, 125.0);
    assert_close(element.bounds.y + element.bounds.height / 2.0, 0.0);
    assert_close(element.rotation, 90.0);
}

#[test]
fn relationship_targets_resolve_against_their_part() {
    assert_eq!(
        resolve_target("ppt/slides/slide1.xml", "../media/image%201.png"),
        "ppt/media/image 1.png"
    );
    assert_eq!(
        resolve_target("ppt/slides/slide1.xml", "/ppt/x.xml"),
        "ppt/x.xml"
    );
    assert_eq!(
        resolve_target("", "ppt/presentation.xml"),
        "ppt/presentation.xml"
    );
    assert_eq!(
        rels_part("ppt/slides/slide1.xml"),
        "ppt/slides/_rels/slide1.xml.rels"
    );
    assert_eq!(rels_part(""), "_rels/.rels");
}

#[test]
fn colour_modifiers_follow_drawingml() {
    let accent = Rgba::rgb(0x44, 0x72, 0xC4);
    // "Lighter 40%" as PowerPoint writes it.
    let lighter = apply_mods(accent, &[ColorMod::LumMod(0.6), ColorMod::LumOff(0.4)]);
    assert!(lighter.r > accent.r && lighter.g > accent.g && lighter.b > accent.b);
    assert_eq!(
        apply_mods(Rgba::BLACK, &[ColorMod::Tint(0.75)]),
        Rgba::rgb(64, 64, 64)
    );
    assert_eq!(
        apply_mods(Rgba::WHITE, &[ColorMod::Shade(0.5)]),
        Rgba::rgb(128, 128, 128)
    );
    assert_eq!(apply_mods(Rgba::WHITE, &[ColorMod::Alpha(0.5)]).a, 128);
}

#[test]
fn autonumber_labels() {
    assert_eq!(autonumber_label("arabicPeriod", 3), "3.");
    assert_eq!(autonumber_label("arabicParenR", 3), "3)");
    assert_eq!(autonumber_label("romanUcPeriod", 14), "XIV.");
    assert_eq!(autonumber_label("romanLcParenBoth", 4), "(iv)");
    assert_eq!(autonumber_label("alphaLcParenR", 28), "bb)");
    assert_eq!(autonumber_label("arabicPlain", 7), "7");
}

#[test]
fn pictures_parse_srcrect_crop_and_degrade_unsupported_vector_formats() {
    let root_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#
    );
    let presentation_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let slide_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/photo.png"/>
        <Relationship Id="rSvg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/icon.svg"/>
        <Relationship Id="rEmf" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/diagram.EMF"/>
    </Relationships>"#;
    let pic_xml = |id: u32, rid: &str, crop: &str| {
        format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="p{id}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rid}"/>{crop}</p:blipFill><p:spPr><a:xfrm><a:off x="635000" y="635000"/><a:ext cx="1270000" cy="1270000"/></a:xfrm><a:ln w="25400"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:ln></p:spPr></p:pic>"#
        )
    };
    let slide = format!(
        r#"<p:sld {NS}><p:cSld><p:spTree>{}{}{}</p:spTree></p:cSld></p:sld>"#,
        pic_xml(
            2,
            "rImg",
            r#"<a:srcRect l="10000" t="20000" r="15000" b="5000"/>"#
        ),
        pic_xml(3, "rSvg", ""),
        pic_xml(4, "rEmf", ""),
    );
    let raw = package(&[
        ("_rels/.rels", root_rels),
        ("ppt/presentation.xml", &presentation),
        ("ppt/_rels/presentation.xml.rels", presentation_rels),
        ("ppt/slides/slide1.xml", &slide),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels),
        ("ppt/media/photo.png", "PNGDATA"),
        ("ppt/media/icon.svg", "<svg/>"),
        ("ppt/media/diagram.EMF", "EMFDATA"),
    ]);

    let deck = parse_presentation(Cursor::new(raw)).unwrap();
    let elems = &deck.slides[0].elements;
    assert_eq!(elems.len(), 3);

    let ElementKind::Picture(pic) = &elems[0].kind else {
        panic!("expected Picture, got {:?}", elems[0].kind);
    };
    assert_eq!(pic.media, "ppt/media/photo.png");
    assert_close(pic.crop.left, 0.10);
    assert_close(pic.crop.top, 0.20);
    assert_close(pic.crop.right, 0.15);
    assert_close(pic.crop.bottom, 0.05);
    assert_close(pic.line.as_ref().unwrap().width, 2.0);
    assert_eq!(
        pic.line.as_ref().unwrap().color,
        Rgba::rgb(0x11, 0x22, 0x33)
    );
    assert_eq!(deck.media.get("ppt/media/photo.png").unwrap(), b"PNGDATA");
    // Unsupported vector formats are not loaded into raster `media`.
    assert!(!deck.media.contains_key("ppt/media/icon.svg"));

    assert!(matches!(
        &elems[1].kind,
        ElementKind::Unsupported { label } if label == "SVG picture"
    ));
    assert!(matches!(
        &elems[2].kind,
        ElementKind::Unsupported { label } if label == "EMF picture"
    ));
}

#[test]
fn table_styles_plain_grid_medium2_and_custom_cell_borders() {
    let make_table = |id: u32, style_id: &str, extra_tc_pr: &str| {
        format!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="t{id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="2540000" cy="1270000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{style_id}</a:tableStyleId></a:tblPr><a:tblGrid><a:gridCol w="1270000"/></a:tblGrid><a:tr h="381000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Header</a:t></a:r></a:p></a:txBody><a:tcPr>{extra_tc_pr}</a:tcPr></a:tc></a:tr><a:tr h="381000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Band0</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr><a:tr h="381000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Band1</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#
        )
    };
    let custom_borders = r#"
        <a:lnL w="38100"><a:solidFill><a:srgbClr val="AA0000"/></a:solidFill><a:prstDash val="dash"/></a:lnL>
        <a:lnR><a:noFill/></a:lnR>
    "#;
    let shapes = format!(
        "{}{}{}",
        make_table(2, "{2D5ABB26-0587-4C30-8999-92F81FD0307C}", custom_borders),
        make_table(3, "{5940675A-B579-460E-94D1-54222C63F5DA}", ""),
        make_table(4, "{21E4AEA4-8DFA-4A89-87EB-49C32662AFE8}", custom_borders),
    );

    let deck = parse_presentation(Cursor::new(single_slide(&shapes))).unwrap();
    let tables: Vec<&Table> = deck.slides[0]
        .elements
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 3);

    // 1. Plain table: no default fill or default borders, custom left border honored.
    let plain_hdr = &tables[0].rows[0].cells[0];
    assert_eq!(plain_hdr.fill, Fill::None);
    assert!(plain_hdr.borders.top.is_none());
    let left = plain_hdr.borders.left.as_ref().unwrap();
    assert_close(left.width, 3.0);
    assert_eq!(left.color, Rgba::rgb(0xAA, 0, 0));
    assert_eq!(left.dash, Dash::Dash);

    // 2. Grid table: no fill, solid 1pt dark borders on all four sides.
    let grid_hdr = &tables[1].rows[0].cells[0];
    assert_eq!(grid_hdr.fill, Fill::None);
    assert_close(grid_hdr.borders.top.as_ref().unwrap().width, 1.0);
    assert_close(grid_hdr.borders.bottom.as_ref().unwrap().width, 1.0);

    // 3. Medium2 (accent2): solid accent2 header with bold white text, alternating
    // banded rows, custom left border overrides default, `<a:noFill/>` clears right border.
    let med_hdr = &tables[2].rows[0].cells[0];
    let fallback_accent2 = Theme::fallback().colors["accent2"];
    assert_eq!(med_hdr.fill, Fill::Solid(fallback_accent2));
    assert!(
        med_hdr.text.as_ref().unwrap().paragraphs[0].runs[0]
            .style
            .bold
    );
    assert_eq!(
        med_hdr.borders.left.as_ref().unwrap().color,
        Rgba::rgb(0xAA, 0, 0)
    );
    assert!(med_hdr.borders.right.is_none());
    assert!(med_hdr.borders.top.is_some());
    assert_ne!(
        tables[2].rows[1].cells[0].fill,
        tables[2].rows[2].cells[0].fill
    );
}

#[test]
fn nested_group_transforms_compose_scale_offset_and_rotation() {
    // Outer group: maps child space [0..100, 0..100]pt onto slide [50..250, 100..300]pt (2x scale).
    // Inner group inside outer child space: at (10, 20) size (40, 40), mapping its own
    // child space [0..20, 0..20]pt (another 2x scale -> total 4x scale) with 45° rotation.
    // Child rect in inner space: at (0, 0) size (20, 10) -> scaled 4x => width 80, height 40.
    let tree = r#"
    <p:grpSp>
      <p:nvGrpSpPr><p:cNvPr id="2" name="outer"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr>
        <a:xfrm>
          <a:off x="635000" y="1270000"/>
          <a:ext cx="2540000" cy="2540000"/>
          <a:chOff x="0" y="0"/>
          <a:chExt cx="1270000" cy="1270000"/>
        </a:xfrm>
      </p:grpSpPr>
      <p:grpSp>
        <p:nvGrpSpPr><p:cNvPr id="3" name="inner"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
        <p:grpSpPr>
          <a:xfrm rot="2700000">
            <a:off x="127000" y="254000"/>
            <a:ext cx="508000" cy="508000"/>
            <a:chOff x="0" y="0"/>
            <a:chExt cx="254000" cy="254000"/>
          </a:xfrm>
        </p:grpSpPr>
        <p:sp>
          <p:nvSpPr><p:cNvPr id="4" name="leaf"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
          <p:spPr>
            <a:xfrm>
              <a:off x="0" y="0"/>
              <a:ext cx="254000" cy="127000"/>
            </a:xfrm>
            <a:solidFill><a:srgbClr val="0088FF"/></a:solidFill>
          </p:spPr>
        </p:sp>
      </p:grpSp>
    </p:grpSp>"#;

    let deck = parse_presentation(Cursor::new(single_slide(tree))).unwrap();
    assert_eq!(deck.slides[0].elements.len(), 1);
    let leaf = &deck.slides[0].elements[0];
    assert_close(leaf.bounds.width, 80.0);
    assert_close(leaf.bounds.height, 40.0);
    assert_close(leaf.rotation, 45.0);
    // Inner group sits at slide (50 + 2*10, 100 + 2*20) = (70, 140) with size 80x80,
    // so its centre is (110, 180). Leaf is the top half of inner (unrotated centre (110, 160));
    // rotating (0, -20) by +45° around (110, 180) puts leaf centre at (110 + 14.142, 180 - 14.142).
    assert_close(leaf.bounds.x + leaf.bounds.width / 2.0, 124.142);
    assert_close(leaf.bounds.y + leaf.bounds.height / 2.0, 165.858);
}

#[test]
fn master_and_layout_placeholder_inheritance_chain() {
    let root_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="6858000"/></p:presentation>"#
    );
    let presentation_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let slide_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rLayout" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#;
    let layout_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rMaster" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;
    let master_rels =
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

    // Master defines a decorative badge shape, a `body` placeholder frame at (40, 120) 500x300pt,
    // and `txStyles` setting body lvl1pPr font size to 26pt.
    let master = format!(
        r#"<p:sldMaster {NS}>
            <p:cSld><p:spTree>
                <p:sp>
                    <p:nvSpPr><p:cNvPr id="10" name="badge"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
                    <p:spPr><a:xfrm><a:off x="127000" y="127000"/><a:ext cx="635000" cy="254000"/></a:xfrm><a:solidFill><a:srgbClr val="224466"/></a:solidFill></p:spPr>
                </p:sp>
                <p:sp>
                    <p:nvSpPr><p:cNvPr id="11" name="masterBody"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
                    <p:spPr><a:xfrm><a:off x="508000" y="1524000"/><a:ext cx="6350000" cy="3810000"/></a:xfrm></p:spPr>
                </p:sp>
            </p:spTree></p:cSld>
            <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
            <p:txStyles>
                <p:bodyStyle><a:lvl1pPr algn="l"><a:defRPr sz="2600"/></a:lvl1pPr></p:bodyStyle>
            </p:txStyles>
        </p:sldMaster>"#
    );
    // Layout matches `idx="1"` without overriding `xfrm` (so geometry still comes from master),
    // but overrides paragraph alignment to right-aligned (`algn="r"`) and anchor to bottom (`b`).
    let layout = format!(
        r#"<p:sldLayout {NS}>
            <p:cSld><p:spTree>
                <p:sp>
                    <p:nvSpPr><p:cNvPr id="20" name="layoutBody"/><p:cNvSpPr/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
                    <p:spPr/>
                    <p:txBody>
                        <a:bodyPr anchor="b"/>
                        <a:lstStyle><a:lvl1pPr algn="r"/></a:lstStyle>
                        <a:p><a:r><a:t>Template prompt ignored</a:t></a:r></a:p>
                    </p:txBody>
                </p:sp>
            </p:spTree></p:cSld>
        </p:sldLayout>"#
    );
    // Slide placeholder gives only `idx="1"` and the actual run text — no `xfrm`, `algn`, or `sz`.
    let slide = format!(
        r#"<p:sld {NS}>
            <p:cSld><p:spTree>
                <p:sp>
                    <p:nvSpPr><p:cNvPr id="30" name="slideBody"/><p:cNvSpPr/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
                    <p:spPr/>
                    <p:txBody><a:bodyPr/><a:p><a:r><a:t>Inherited Content</a:t></a:r></a:p></p:txBody>
                </p:sp>
            </p:spTree></p:cSld>
        </p:sld>"#
    );

    let raw = package(&[
        ("_rels/.rels", root_rels),
        ("ppt/presentation.xml", &presentation),
        ("ppt/_rels/presentation.xml.rels", presentation_rels),
        ("ppt/slides/slide1.xml", &slide),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels),
        ("ppt/slideLayouts/slideLayout1.xml", &layout),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", layout_rels),
        ("ppt/slideMasters/slideMaster1.xml", &master),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", master_rels),
    ]);

    let deck = parse_presentation(Cursor::new(raw)).unwrap();
    let all = shapes(&deck.slides[0]);
    // Master's non-placeholder decorative badge + the slide's inherited body shape (template placeholders skipped).
    assert_eq!(all.len(), 2);
    assert_close(all[0].0.bounds.x, 10.0);

    let (body_elem, body_shape) = all[1];
    assert_close(body_elem.bounds.x, 40.0);
    assert_close(body_elem.bounds.y, 120.0);
    assert_close(body_elem.bounds.width, 500.0);
    assert_close(body_elem.bounds.height, 300.0);

    let text = body_shape.text.as_ref().unwrap();
    assert_eq!(text.anchor, Anchor::Bottom);
    assert_eq!(text.paragraphs[0].align, Align::Right);
    assert_close(text.paragraphs[0].runs[0].style.size, 26.0);
    assert_eq!(text.paragraphs[0].runs[0].text, "Inherited Content");
}
