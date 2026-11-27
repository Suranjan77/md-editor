//! Unit and deterministic property tests for vault path jailing, symlinks, and atomic writes.

use super::*;
use std::path::Component;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("md-editor-vault-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let idx = (self.next_u64() as usize) % items.len();
        &items[idx]
    }
}

#[test]
fn resolve_vault_path_jails_500_plus_seeded_adversarial_paths() {
    let tmp = TestDir::new();
    let vault_root = tmp.path().join("vault");
    fs::create_dir_all(vault_root.join("notes/sub")).unwrap();
    let canonical_root = fs::canonicalize(&vault_root).unwrap();

    let tokens = [
        "notes",
        "sub",
        "topic.md",
        "étude.md",
        "my note.md",
        ".",
        "..",
        "...",
        "",
        "C:",
        r"..\..\etc\passwd",
        r"sub\nested\file.md",
    ];
    let seps = ["/", "//", r"\"];
    let prefixes = ["", "/", "//", "../", "../../", "./", r"\", r"C:\"];

    let mut rng = Rng(0xDEAD_BEEF_CAFE_BABE);
    let mut checked_escapes = 0usize;
    let mut checked_in_bounds = 0usize;

    for _ in 0..750 {
        let seg_count = 1 + (rng.next_u64() as usize % 8);
        let mut candidate = String::from(*rng.pick(&prefixes));
        for idx in 0..seg_count {
            if idx > 0 {
                candidate.push_str(rng.pick(&seps));
            }
            candidate.push_str(rng.pick(&tokens));
        }

        // Invariant 1: `resolve_vault_path` NEVER leaves `vault_root` and leaves no `..` / `.`.
        let clamped = resolve_vault_path(&vault_root, &candidate);
        assert!(
            clamped.starts_with(&vault_root),
            "clamped path {:?} escaped {:?} for input {:?}",
            clamped,
            vault_root,
            candidate
        );
        let tail = clamped.strip_prefix(&vault_root).unwrap();
        for comp in tail.components() {
            assert!(
                matches!(comp, Component::Normal(_)),
                "unexpected non-normal component {:?} in {:?} for input {:?}",
                comp,
                clamped,
                candidate
            );
        }

        // Invariant 2: `resolve_vault_path_checked` agrees with lexical escape detection
        // and never resolves outside `canonical_root`.
        let (_, lexical_escape) = normalize_within_root(&vault_root, &candidate);
        match resolve_vault_path_checked(&vault_root, &candidate) {
            Ok(resolved) => {
                assert!(
                    !lexical_escape,
                    "checked accepted lexically escaping input {:?}",
                    candidate
                );
                assert_eq!(resolved, clamped);
                assert!(resolved.starts_with(&vault_root));
                let mut existing = resolved.as_path();
                while fs::symlink_metadata(existing).is_err() {
                    existing = existing.parent().unwrap();
                }
                assert!(
                    fs::canonicalize(existing)
                        .unwrap()
                        .starts_with(&canonical_root)
                );
                checked_in_bounds += 1;
            }
            Err(VaultError::OutsideVault(_)) => {
                assert!(
                    lexical_escape,
                    "Unexpected OutsideVault without lexical or symlink escape on {:?}",
                    candidate
                );
                checked_escapes += 1;
            }
            Err(other) => panic!("unexpected error for {:?}: {other:?}", candidate),
        }
    }

    assert!(checked_escapes > 100, "expected plenty of escape cases");
    assert!(checked_in_bounds > 100, "expected plenty of valid cases");
}

#[test]
fn resolve_vault_path_checked_explicit_edge_cases() {
    let tmp = TestDir::new();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();

    for bad in [
        "..",
        "../secret.md",
        "notes/../../secret.md",
        "/etc/passwd",
        "//network/share",
        "a/b/c/../../../../outside.md",
    ] {
        let err = resolve_vault_path_checked(&root, bad).unwrap_err();
        assert!(
            matches!(err, VaultError::OutsideVault(_)),
            "expected OutsideVault for {bad:?}, got {err:?}"
        );
        assert!(resolve_vault_path(&root, bad).starts_with(&root));
    }

    for ok in [
        "note.md",
        "./note.md",
        "a/b/../note.md",
        "a/./b/c/../../d.md",
    ] {
        let resolved = resolve_vault_path_checked(&root, ok).unwrap();
        assert!(resolved.starts_with(&root));
    }
}

#[cfg(unix)]
#[test]
fn resolve_vault_path_checked_rejects_symlinks_escaping_vault() {
    use std::os::unix::fs::symlink;

    let tmp = TestDir::new();
    let vault = tmp.path().join("vault");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(vault.join("sub")).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let secret_file = outside.join("secret.md");
    fs::write(&secret_file, "classified").unwrap();
    let inside_target = vault.join("sub/real.md");
    fs::write(&inside_target, "hello").unwrap();

    // 1. File symlink pointing outside the vault -> OutsideVault.
    symlink(&secret_file, vault.join("leak.md")).unwrap();
    assert!(matches!(
        resolve_vault_path_checked(&vault, "leak.md"),
        Err(VaultError::OutsideVault(_))
    ));

    // 2. Directory symlink pointing outside the vault (existing & nonexistent children) -> OutsideVault.
    symlink(&outside, vault.join("escape_dir")).unwrap();
    assert!(matches!(
        resolve_vault_path_checked(&vault, "escape_dir/secret.md"),
        Err(VaultError::OutsideVault(_))
    ));
    assert!(matches!(
        resolve_vault_path_checked(&vault, "escape_dir/deep/new_note.md"),
        Err(VaultError::OutsideVault(_))
    ));

    // 3. Symlink pointing to a file *inside* the vault is allowed.
    symlink(&inside_target, vault.join("alias.md")).unwrap();
    assert!(resolve_vault_path_checked(&vault, "alias.md").is_ok());
}

#[test]
fn write_file_refuses_read_only_targets_and_leaves_no_temp_files() {
    let tmp = TestDir::new();
    let note = tmp.path().join("protected.md");
    fs::write(&note, "original content").unwrap();

    let mut perms = fs::metadata(&note).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&note, perms).unwrap();

    let err = write_file(&note, "attempted overwrite").unwrap_err();
    assert!(matches!(err, VaultError::ReadOnly(_)), "got {err:?}");
    assert!(err.is_permission_denied());
    assert_eq!(fs::read_to_string(&note).unwrap(), "original content");

    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut writable = fs::metadata(&note).unwrap().permissions();
        writable.set_readonly(false);
        let _ = fs::set_permissions(&note, writable);
    }

    let entries: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries, vec!["protected.md"]);
}

#[cfg(unix)]
#[test]
fn write_file_preserves_unix_permissions_and_writes_through_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let tmp = TestDir::new();
    let private_note = tmp.path().join("private.md");
    fs::write(&private_note, "v1").unwrap();
    fs::set_permissions(&private_note, fs::Permissions::from_mode(0o600)).unwrap();

    write_file(&private_note, "v2-updated").unwrap();
    assert_eq!(fs::read_to_string(&private_note).unwrap(), "v2-updated");
    let mode = fs::metadata(&private_note).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "atomic write must preserve 0600 permissions");

    let link_path = tmp.path().join("shortcut.md");
    symlink(&private_note, &link_path).unwrap();
    write_file(&link_path, "v3-via-symlink").unwrap();
    assert!(
        fs::symlink_metadata(&link_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&private_note).unwrap(), "v3-via-symlink");
}
