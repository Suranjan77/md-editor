use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// In-memory graph of wikilink references between files.
pub struct FileIndex {
    /// file path → set of files it links to
    pub outgoing: HashMap<PathBuf, HashSet<PathBuf>>,
    /// file path → set of files that link to it
    pub incoming: HashMap<PathBuf, HashSet<PathBuf>>,
    /// The vault root directory
    vault_root: PathBuf,
}

impl FileIndex {
    pub fn new(vault_root: PathBuf) -> Self {
        FileIndex {
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            vault_root,
        }
    }

    /// Extract wikilinks from document text and update the index for a given file.
    pub fn update_file(&mut self, file_path: &Path, content: &str) {
        // Remove old outgoing links for this file
        if let Some(old_targets) = self.outgoing.remove(file_path) {
            for target in &old_targets {
                if let Some(incoming_set) = self.incoming.get_mut(target) {
                    incoming_set.remove(file_path);
                }
            }
        }

        // Extract new wikilinks
        let targets = extract_wikilinks(content, &self.vault_root);
        let target_set: HashSet<PathBuf> = targets.into_iter().collect();

        // Update incoming links
        for target in &target_set {
            self.incoming
                .entry(target.clone())
                .or_default()
                .insert(file_path.to_path_buf());
        }

        self.outgoing.insert(file_path.to_path_buf(), target_set);
    }

    /// Remove a file from the index entirely.
    pub fn remove_file(&mut self, file_path: &Path) {
        if let Some(old_targets) = self.outgoing.remove(file_path) {
            for target in &old_targets {
                if let Some(incoming_set) = self.incoming.get_mut(target) {
                    incoming_set.remove(file_path);
                }
            }
        }

        // Also remove it from incoming if present
        self.incoming.remove(file_path);
    }

    /// Get all files that link to the given file (backlinks).
    pub fn get_backlinks(&self, file_path: &Path) -> Vec<PathBuf> {
        self.incoming
            .get(file_path)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all files that the given file links to.
    pub fn get_outgoing_links(&self, file_path: &Path) -> Vec<PathBuf> {
        self.outgoing
            .get(file_path)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Extract `[[target]]` and `[[target|alias]]` patterns from text.
/// Resolves targets relative to the vault root.
fn extract_wikilinks(content: &str, vault_root: &Path) -> Vec<PathBuf> {
    let re = Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap();
    let mut links = Vec::new();

    for cap in re.captures_iter(content) {
        if let Some(target) = cap.get(1) {
            let target_str = target.as_str().trim();
            let mut target_path = vault_root.join(target_str);
            // Assume .md extension if not specified
            if target_path.extension().is_none() {
                target_path.set_extension("md");
            }
            links.push(target_path);
        }
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_wikilinks() {
        let content = "See [[other-note]] and [[folder/page|My Page]].";
        let root = PathBuf::from("/vault");
        let links = extract_wikilinks(content, &root);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], PathBuf::from("/vault/other-note.md"));
        assert_eq!(links[1], PathBuf::from("/vault/folder/page.md"));
    }

    #[test]
    fn test_backlinks() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root);

        let file_a = PathBuf::from("/vault/a.md");
        let content_a = "Link to [[b]].";
        index.update_file(&file_a, content_a);

        let target_b = PathBuf::from("/vault/b.md");
        let backlinks = index.get_backlinks(&target_b);
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0], file_a);
    }
}
