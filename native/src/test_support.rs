//! Test-only isolation: pins the settings/log directory to a per-process temp folder so no test can read or write the user's real `~/.local/share/md-editor`.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Pin the data directory for this test process. Idempotent; called by
/// `MdEditor::new_with_startup_file` in test builds and by the e2e harness,
/// so every test that builds the app is isolated without opting in.
pub(crate) fn isolate_data_dir() {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DATA_DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("md-editor-test-data-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test data dir");
        dir
    });
    assert!(
        md_editor_core::state::override_data_dir(dir.clone()),
        "a different data directory was pinned before the tests"
    );
}

#[test]
fn building_the_app_never_uses_the_real_data_dir() {
    let _ = crate::app::MdEditor::new_with_startup_file(None);
    let temp = std::env::temp_dir();
    assert!(md_editor_core::state::data_dir().starts_with(&temp));
    assert!(md_editor_core::state::settings_db_path().starts_with(&temp));
    assert!(md_editor_core::state::log_dir().starts_with(&temp));
}
