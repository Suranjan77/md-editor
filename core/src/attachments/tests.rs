//! Tests for `attachments`.

use super::*;

fn vault() -> (PathBuf, AppState) {
    let base = std::env::temp_dir().join(format!("md_editor_att_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(base.join("notes/deep")).unwrap();
    let state = AppState::new_in_memory();
    crate::vault::set_vault_root(&state, base.to_str().unwrap()).unwrap();
    (base, state)
}

#[test]
fn folder_setting_parses() {
    assert_eq!(
        AttachmentFolder::from_setting(None),
        AttachmentFolder::VaultRoot
    );
    assert_eq!(
        AttachmentFolder::from_setting(Some("beside-note")),
        AttachmentFolder::BesideNote
    );
    assert_eq!(
        AttachmentFolder::from_setting(Some("/media/img/")),
        AttachmentFolder::Custom("media/img".to_string())
    );
}

#[test]
fn attachments_dir_follows_the_setting() {
    let note = Some("notes/deep/A.md");
    assert_eq!(
        attachments_dir(&AttachmentFolder::VaultRoot, note),
        "attachments"
    );
    assert_eq!(
        attachments_dir(&AttachmentFolder::BesideNote, note),
        "notes/deep/attachments"
    );
    assert_eq!(
        attachments_dir(&AttachmentFolder::BesideNote, Some("A.md")),
        "attachments"
    );
    assert_eq!(
        attachments_dir(&AttachmentFolder::Custom("media".into()), note),
        "media"
    );
}

#[test]
fn collisions_get_numbered_and_identical_bytes_are_reused() {
    let (base, state) = vault();
    let f = AttachmentFolder::VaultRoot;
    let a = store_bytes(&state, &f, None, "shot.png", b"one").unwrap();
    assert_eq!(
        a,
        Stored {
            rel_path: "attachments/shot.png".into(),
            reused: false
        }
    );
    let b = store_bytes(&state, &f, None, "shot.png", b"two").unwrap();
    assert_eq!(b.rel_path, "attachments/shot 2.png");
    let again = store_bytes(&state, &f, None, "other-name.png", b"two").unwrap();
    assert_eq!(
        again,
        Stored {
            rel_path: "attachments/shot 2.png".into(),
            reused: true
        }
    );
    assert_eq!(fs::read(base.join("attachments/shot.png")).unwrap(), b"one");
    let _ = fs::remove_dir_all(base);
}

#[test]
fn bitmaps_are_stored_as_png() {
    let (base, state) = vault();
    let stored = store_bitmap(
        &state,
        &AttachmentFolder::BesideNote,
        Some("notes/A.md"),
        2,
        1,
        vec![255, 0, 0, 255, 0, 255, 0, 255],
        "2026-11-24 10-00-00",
    )
    .unwrap();
    assert_eq!(
        stored.rel_path,
        "notes/attachments/Pasted image 2026-11-24 10-00-00.png"
    );
    let decoded = image::open(base.join(&stored.rel_path)).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (2, 1));
    assert!(
        store_bitmap(
            &state,
            &AttachmentFolder::VaultRoot,
            None,
            5,
            5,
            vec![0; 3],
            "x"
        )
        .is_err()
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn import_links_vault_files_in_place_and_copies_outside_ones() {
    let (base, state) = vault();
    fs::write(base.join("notes/inside.png"), b"in").unwrap();
    let inside = import_file(
        &state,
        &AttachmentFolder::VaultRoot,
        None,
        &base.join("notes/inside.png"),
    )
    .unwrap();
    assert_eq!(
        inside,
        Stored {
            rel_path: "notes/inside.png".into(),
            reused: true
        }
    );

    let outside_dir = std::env::temp_dir().join(format!("md_editor_out_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&outside_dir).unwrap();
    fs::write(outside_dir.join("photo é (1).jpg"), b"jpg").unwrap();
    let copied = import_file(
        &state,
        &AttachmentFolder::VaultRoot,
        None,
        &outside_dir.join("photo é (1).jpg"),
    )
    .unwrap();
    assert_eq!(copied.rel_path, "attachments/photo é -1-.jpg");
    assert_eq!(fs::read(base.join(&copied.rel_path)).unwrap(), b"jpg");
    let _ = fs::remove_dir_all(base);
    let _ = fs::remove_dir_all(outside_dir);
}

#[test]
fn markdown_paths_are_relative_to_the_note() {
    assert_eq!(
        image_markdown(Some("A.md"), "attachments/a.png"),
        "![](attachments/a.png)"
    );
    assert_eq!(
        image_markdown(Some("notes/deep/A.md"), "attachments/a.png"),
        "![](../../attachments/a.png)"
    );
    assert_eq!(
        image_markdown(Some("notes/A.md"), "notes/attachments/a.png"),
        "![](attachments/a.png)"
    );
    assert_eq!(
        image_markdown(Some("A.md"), "attachments/Pasted image 1.png"),
        "![](<attachments/Pasted image 1.png>)"
    );
}

#[test]
fn read_only_vault_reports_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let (base, state) = vault();
    fs::create_dir_all(base.join("attachments")).unwrap();
    fs::set_permissions(base.join("attachments"), fs::Permissions::from_mode(0o555)).unwrap();
    let result = store_bytes(&state, &AttachmentFolder::VaultRoot, None, "a.png", b"x");
    fs::set_permissions(base.join("attachments"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    let _ = fs::remove_dir_all(base);
}
