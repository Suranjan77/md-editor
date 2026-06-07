use std::error::Error;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Path relative to a vault root, with traversal outside that root rejected.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VaultPath(PathBuf);

impl VaultPath {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, VaultPathError> {
        let path = path.into();
        validate_vault_path(&path)?;
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for VaultPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl fmt::Display for VaultPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
    }
}

impl TryFrom<PathBuf> for VaultPath {
    type Error = VaultPathError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl TryFrom<&Path> for VaultPath {
    type Error = VaultPathError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl TryFrom<&str> for VaultPath {
    type Error = VaultPathError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultPathError {
    Empty,
    Absolute,
    Traversal,
}

impl fmt::Display for VaultPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "vault path must not be empty",
            Self::Absolute => "vault path must be relative",
            Self::Traversal => "vault path must not contain parent traversal",
        };
        formatter.write_str(message)
    }
}

impl Error for VaultPathError {}

fn validate_vault_path(path: &Path) -> Result<(), VaultPathError> {
    if path.as_os_str().is_empty() {
        return Err(VaultPathError::Empty);
    }
    if path.is_absolute() {
        return Err(VaultPathError::Absolute);
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(VaultPathError::Traversal);
    }
    Ok(())
}

/// Absolute filesystem path. Existence is deliberately not required.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AbsPath(PathBuf);

impl AbsPath {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, AbsPathError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(AbsPathError::Relative);
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for AbsPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl fmt::Display for AbsPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
    }
}

impl TryFrom<PathBuf> for AbsPath {
    type Error = AbsPathError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl TryFrom<&Path> for AbsPath {
    type Error = AbsPathError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl TryFrom<&str> for AbsPath {
    type Error = AbsPathError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbsPathError {
    Relative,
}

impl fmt::Display for AbsPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("absolute path must not be relative")
    }
}

impl Error for AbsPathError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_path_accepts_nested_relative_path() {
        let vault_path = VaultPath::new("notes/projects/plan.md").unwrap();

        assert_eq!(vault_path.as_ref(), Path::new("notes/projects/plan.md"));
        assert_eq!(vault_path.to_string(), "notes/projects/plan.md");
    }

    #[test]
    fn vault_path_rejects_parent_traversal() {
        assert_eq!(
            VaultPath::new("../outside.md"),
            Err(VaultPathError::Traversal)
        );
        assert_eq!(
            VaultPath::new("notes/../../outside.md"),
            Err(VaultPathError::Traversal)
        );
    }

    #[test]
    fn vault_path_rejects_absolute_path() {
        let abs_path = std::env::current_dir().unwrap().join("note.md");

        assert_eq!(VaultPath::new(abs_path), Err(VaultPathError::Absolute));
    }

    #[test]
    fn vault_path_rejects_empty_path() {
        assert_eq!(VaultPath::new(""), Err(VaultPathError::Empty));
    }

    #[test]
    fn abs_path_accepts_absolute_nonexistent_path() {
        let path = std::env::current_dir()
            .unwrap()
            .join("does-not-need-to-exist");
        let abs_path = AbsPath::new(path.clone()).unwrap();

        assert_eq!(abs_path.as_ref(), path.as_path());
        assert_eq!(abs_path.to_string(), path.display().to_string());
    }

    #[test]
    fn abs_path_rejects_relative_path() {
        assert_eq!(AbsPath::new("notes/a.md"), Err(AbsPathError::Relative));
    }
}
