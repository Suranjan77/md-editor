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
        if let Some(old_targets) = self.outgoing.remove(file_path) {
            for target in &old_targets {
                if let Some(incoming_set) = self.incoming.get_mut(target) {
                    incoming_set.remove(file_path);
                }
            }
        }

        let targets = extract_wikilinks(content, &self.vault_root, file_path);
        let target_set: HashSet<PathBuf> = targets.into_iter().collect();

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

fn extract_wikilinks(content: &str, vault_root: &Path, file_path: &Path) -> Vec<PathBuf> {
    let re = Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap();
    let mut links = Vec::new();

    for cap in re.captures_iter(content) {
        if let Some(target) = cap.get(1) {
            let target_str = target.as_str().trim();
            if target_str.starts_with('#') {
                continue;
            }
            let path_part = if let Some(idx) = target_str.find('#') {
                let anchor = &target_str[idx + 1..];
                if anchor
                    .chars()
                    .any(|c| matches!(c, '%' | '^' | '&' | '*' | '!' | '@' | '(' | ')'))
                {
                    target_str
                } else {
                    target_str[..idx].trim()
                }
            } else {
                target_str
            };
            if path_part.is_empty() {
                continue;
            }

            let resolved = if path_part.starts_with('.') {
                if let Some(parent) = file_path.parent() {
                    parent.join(path_part)
                } else {
                    vault_root.join(path_part)
                }
            } else {
                vault_root.join(path_part)
            };

            // Normalize path components
            let mut components = Vec::new();
            for component in resolved.components() {
                match component {
                    std::path::Component::ParentDir => {
                        components.pop();
                    }
                    std::path::Component::Normal(c) => {
                        components.push(c);
                    }
                    std::path::Component::CurDir => {}
                    _ => {
                        components.push(component.as_os_str());
                    }
                }
            }
            let mut normalized: PathBuf = components.into_iter().collect();

            if normalized.extension().is_none() {
                normalized.set_extension("md");
            }
            links.push(normalized);
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
        let file_path = PathBuf::from("/vault/note.md");
        let links = extract_wikilinks(content, &root, &file_path);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], PathBuf::from("/vault/other-note.md"));
        assert_eq!(links[1], PathBuf::from("/vault/folder/page.md"));

        // Relative path test
        let content_rel = "See [[../other-note]] and [[./subfolder/page|My Page]].";
        let file_path_rel = PathBuf::from("/vault/nested/note.md");
        let links_rel = extract_wikilinks(content_rel, &root, &file_path_rel);
        assert_eq!(links_rel.len(), 2);
        assert_eq!(links_rel[0], PathBuf::from("/vault/other-note.md"));
        assert_eq!(
            links_rel[1],
            PathBuf::from("/vault/nested/subfolder/page.md")
        );
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
