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

/// Register the bundled fonts with the renderer's font system. The real app
/// does this from `program`'s settings, but `iced_test::Emulator` ignores
/// `settings.fonts`, so without this every headless screenshot falls back to
/// system faces (serif for semibold and bold). Idempotent.
pub(crate) fn load_bundled_fonts() {
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(|| {
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system lock");
        for bytes in crate::design::fonts::BUNDLED_FONTS {
            fonts.load_font(std::borrow::Cow::Borrowed(bytes));
        }
    });
}

#[test]
fn building_the_app_never_uses_the_real_data_dir() {
    let _ = crate::app::MdEditor::new_with_startup_file(None);
    let temp = std::env::temp_dir();
    assert!(md_editor_core::state::data_dir().starts_with(&temp));
    assert!(md_editor_core::state::settings_db_path().starts_with(&temp));
    assert!(md_editor_core::state::log_dir().starts_with(&temp));
}
