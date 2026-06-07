use std::path::{Path, PathBuf};

pub fn cargo_target_root(profile_dir: &Path, target: &str) -> PathBuf {
    let Some(parent) = profile_dir.parent() else {
        return profile_dir.to_path_buf();
    };

    if parent.file_name().is_some_and(|name| name == target) {
        parent
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| parent.to_path_buf())
    } else {
        parent.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::cargo_target_root;
    use std::path::Path;

    #[test]
    fn finds_native_custom_target_root() {
        assert_eq!(
            cargo_target_root(
                Path::new("/workspace/build-output/release"),
                "x86_64-unknown-linux-gnu"
            ),
            Path::new("/workspace/build-output")
        );
    }

    #[test]
    fn finds_cross_target_root_above_target_triple() {
        assert_eq!(
            cargo_target_root(
                Path::new("/workspace/build-output/aarch64-apple-darwin/release"),
                "aarch64-apple-darwin"
            ),
            Path::new("/workspace/build-output")
        );
    }
}
