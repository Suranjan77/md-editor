//! Tests for `pdf_attachments`.

use super::*;

#[test]
fn runnable_attachments_are_recognised_whatever_the_case() {
    assert!(is_runnable("install.sh"));
    assert!(is_runnable("Setup.EXE"));
    assert!(is_runnable("launcher.desktop"));
    assert!(!is_runnable("data.csv"));
    assert!(!is_runnable("figure.png"));
    assert!(!is_runnable("README"));
}

#[test]
fn cached_copies_stay_in_their_own_folder() {
    let a = cache_path("/v/a.pdf", 0, "../../etc/passwd");
    let b = cache_path("/v/b.pdf", 0, "data.csv");
    let c = cache_path("/v/a.pdf", 1, "data.csv");
    let root = md_editor_core::state::cache_dir().join("pdf-attachments");
    assert!(a.starts_with(&root));
    assert_eq!(a.parent().unwrap().parent().unwrap(), root);
    // Separators become `-` and leading dots go: it cannot leave its folder.
    assert_eq!(a.file_name().unwrap(), "-..-etc-passwd");
    assert_ne!(b.parent(), c.parent());
}
