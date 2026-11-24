//! Tests for `platform`.

use super::*;

#[test]
fn percent_decoding() {
    assert_eq!(percent_decode("a%20b"), "a b");
    assert_eq!(percent_decode("caf%C3%A9"), "café");
    assert_eq!(percent_decode("100%"), "100%");
    assert_eq!(percent_decode("%zz"), "%zz");
    assert_eq!(percent_decode("é%20x"), "é x");
}

#[test]
fn text_naming_existing_files_is_a_file_list() {
    let dir = std::env::temp_dir().join(format!("md_editor_paste_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a b.png");
    std::fs::write(&a, b"x").unwrap();
    let uri = format!("file://{}", a.to_string_lossy().replace(' ', "%20"));

    assert_eq!(files_named_by(&uri), Some(vec![a.clone()]));
    assert_eq!(
        files_named_by(&format!("# comment\n{}\n", a.display())),
        Some(vec![a.clone()])
    );
    // Ordinary text, a missing file, or a mix: not a file list.
    assert_eq!(files_named_by("hello world"), None);
    assert_eq!(files_named_by("/definitely/not/here.png"), None);
    assert_eq!(files_named_by(&format!("{}\nhello", a.display())), None);
    assert_eq!(files_named_by(""), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clipboard_payload_debug_never_prints_pixels() {
    let payload = ClipboardPayload::Image {
        width: 2,
        height: 1,
        rgba: vec![7; 8],
    };
    assert_eq!(format!("{payload:?}"), "Image(2x1)");
}
