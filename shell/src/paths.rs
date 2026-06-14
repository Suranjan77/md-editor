//! Runtime path resolution for installed and portable builds.

use std::path::{Path, PathBuf};

const PORTABLE_MARKER: &str = "portable.flag";

pub fn config_file(name: &str) -> PathBuf {
    let exe_path = std::env::current_exe().ok();
    let appimage_path = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let platform_dir = directories::ProjectDirs::from("com", "Suranjan77", "md-editor")
        .map(|dirs| dirs.config_dir().to_path_buf());
    config_file_for(
        name,
        exe_path.as_deref(),
        appimage_path.as_deref(),
        platform_dir,
    )
}

pub fn pdfium_dirs() -> Vec<PathBuf> {
    let exe_path = std::env::current_exe().ok();
    let env_dir = std::env::var_os("PDFIUM_LIB_DIR").map(PathBuf::from);
    pdfium_dirs_for(exe_path.as_deref(), env_dir.as_deref())
}

fn config_file_for(
    name: &str,
    exe_path: Option<&Path>,
    appimage_path: Option<&Path>,
    platform_dir: Option<PathBuf>,
) -> PathBuf {
    if let Some(dir) = portable_dir(exe_path, appimage_path) {
        return dir.join(name);
    }
    platform_dir
        .or_else(|| exe_path.and_then(Path::parent).map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(name)
}

fn portable_dir(exe_path: Option<&Path>, appimage_path: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(appimage_dir) = appimage_path.and_then(Path::parent) {
        return Some(appimage_dir.to_path_buf());
    }
    if let Some(exe_dir) = exe_path.and_then(Path::parent) {
        candidates.push(exe_dir.to_path_buf());
        if is_macos_bundle_executable(exe_dir)
            && let Some(bundle_parent) = exe_dir
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
        {
            candidates.push(bundle_parent.to_path_buf());
        }
    }
    candidates
        .into_iter()
        .find(|dir| dir.join(PORTABLE_MARKER).is_file())
}

fn pdfium_dirs_for(exe_path: Option<&Path>, env_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(env_dir) = env_dir {
        dirs.push(env_dir.to_path_buf());
    }
    if let Some(exe_dir) = exe_path.and_then(Path::parent) {
        dirs.push(exe_dir.join("resources"));
        dirs.push(exe_dir.to_path_buf());
        dirs.push(exe_dir.join("../lib"));
        if is_macos_bundle_executable(exe_dir)
            && let Some(contents_dir) = exe_dir.parent()
        {
            dirs.push(contents_dir.join("Resources"));
        }
    }
    dirs
}

fn is_macos_bundle_executable(exe_dir: &Path) -> bool {
    exe_dir.file_name().is_some_and(|name| name == "MacOS")
        && exe_dir
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Contents")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn executable_marker_enables_portable_config() {
        let temp = tempfile::tempdir().expect("temp directory");
        std::fs::write(temp.path().join(PORTABLE_MARKER), []).expect("portable marker");
        let exe = temp.path().join("md-editor");
        assert_eq!(
            config_file_for(
                "tracker.db",
                Some(&exe),
                None,
                Some(PathBuf::from("/platform"))
            ),
            temp.path().join("tracker.db")
        );
    }

    #[test]
    fn macos_marker_lives_beside_app_bundle() {
        let temp = tempfile::tempdir().expect("temp directory");
        std::fs::write(temp.path().join(PORTABLE_MARKER), []).expect("portable marker");
        let exe = temp.path().join("MD Editor.app/Contents/MacOS/md-editor");
        assert_eq!(
            config_file_for("recent.json", Some(&exe), None, None),
            temp.path().join("recent.json")
        );
    }

    #[test]
    fn appimage_marker_uses_image_directory() {
        let temp = tempfile::tempdir().expect("temp directory");
        let appimage = temp.path().join("MD-Editor.AppImage");
        let mounted_exe = Path::new("/tmp/.mount-md/usr/bin/md-editor");
        assert_eq!(
            config_file_for("recent.json", Some(mounted_exe), Some(&appimage), None),
            temp.path().join("recent.json")
        );
    }

    #[test]
    fn installed_build_uses_platform_config() {
        assert_eq!(
            config_file_for(
                "tracker.db",
                Some(Path::new("/opt/md-editor")),
                None,
                Some(PathBuf::from("/config/md-editor"))
            ),
            PathBuf::from("/config/md-editor/tracker.db")
        );
    }

    #[test]
    fn packaged_pdfium_directories_cover_all_layouts() {
        assert_eq!(
            pdfium_dirs_for(
                Some(Path::new(
                    "/Applications/MD Editor.app/Contents/MacOS/md-editor"
                )),
                None
            ),
            [
                "/Applications/MD Editor.app/Contents/MacOS/resources",
                "/Applications/MD Editor.app/Contents/MacOS",
                "/Applications/MD Editor.app/Contents/MacOS/../lib",
                "/Applications/MD Editor.app/Contents/Resources",
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn configured_pdfium_directory_has_priority() {
        assert_eq!(
            pdfium_dirs_for(
                Some(Path::new("/workspace/target/debug/md-editor")),
                Some(Path::new("/opt/pdfium")),
            ),
            [
                "/opt/pdfium",
                "/workspace/target/debug/resources",
                "/workspace/target/debug",
                "/workspace/target/debug/../lib",
            ]
            .map(PathBuf::from)
        );
    }
}
