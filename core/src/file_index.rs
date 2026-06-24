use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// `[[target]]` / `[[target|alias]]` wikilink matcher. Compiled once and
/// reused — `update_file` runs this for every file during indexing.
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap());

/// In-memory graph of wikilink references between files.
pub struct FileIndex {
    /// file path → set of files it links to
    pub outgoing: HashMap<PathBuf, HashSet<PathBuf>>,
    /// file path → set of files that link to it
    pub incoming: HashMap<PathBuf, HashSet<PathBuf>>,
    /// The vault root directory
    vault_root: PathBuf,
    /// Lowercased file stem → the known vault files with that stem. Lets the
    /// index resolve bare `[[Name]]` links to files in subfolders the same way
    /// the navigator does (basename match, shortest path wins), so backlinks
    /// agree with where clicking the link actually navigates.
    names: HashMap<String, BTreeSet<PathBuf>>,
}

impl FileIndex {
    pub fn new(vault_root: PathBuf) -> Self {
        FileIndex {
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            vault_root,
            names: HashMap::new(),
        }
    }

    /// Rebuild the whole index from a complete file listing in two passes:
    /// first register every file's name, then resolve links. The two passes
    /// are required so a bare `[[Name]]` link resolves to a file that appears
    /// later in the list (or in another subfolder).
    pub fn rebuild(&mut self, files: &[(PathBuf, String)]) {
        self.outgoing.clear();
        self.incoming.clear();
        self.names.clear();
        for (path, _) in files {
            self.add_name(path);
        }
        for (path, content) in files {
            let targets = self.extract_targets(content, path);
            for target in &targets {
                self.incoming
                    .entry(target.clone())
                    .or_default()
                    .insert(path.clone());
            }
            self.outgoing.insert(path.clone(), targets);
        }
    }

    /// Extract wikilinks from document text and update the index for a given
    /// file. Used for incremental single-file edits; for a full vault build
    /// prefer [`FileIndex::rebuild`], which resolves bare links against the
    /// complete file set.
    pub fn update_file(&mut self, file_path: &Path, content: &str) {
        if let Some(old_targets) = self.outgoing.remove(file_path) {
            for target in &old_targets {
                if let Some(incoming_set) = self.incoming.get_mut(target) {
                    incoming_set.remove(file_path);
                }
            }
        }

        self.add_name(file_path);
        let target_set = self.extract_targets(content, file_path);

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
        self.remove_name(file_path);
    }

    /// Resolve every wikilink in `content` to the file it actually points at,
    /// using path-based resolution first and falling back to basename matching
    /// across subfolders (mirroring the navigator).
    fn extract_targets(&self, content: &str, file_path: &Path) -> HashSet<PathBuf> {
        let mut set = HashSet::new();
        for cap in WIKILINK_RE.captures_iter(content) {
            if let Some(target) = cap.get(1) {
                if let Some(resolved) = self.resolve_link(target.as_str(), file_path) {
                    set.insert(resolved);
                }
            }
        }
        set
    }

    /// Resolve a single wikilink target to the file it points at.
    ///
    /// Resolution order: an exact path match (handles `[[sub/Name]]` and
    /// root-level `[[Name]]`); failing that, the shortest known file sharing
    /// the link's basename, so a bare `[[Name]]` resolves into a subfolder the
    /// same way the navigator does. When no file with that basename is known —
    /// a forward reference during incremental indexing, or a dangling link —
    /// the path-based candidate is kept so the link still records a backlink
    /// that a later-created file at that location inherits.
    fn resolve_link(&self, target: &str, file_path: &Path) -> Option<PathBuf> {
        let candidate = resolve_wikilink_target(target, &self.vault_root, file_path)?;
        let Some(stem) = candidate.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else {
            return Some(candidate);
        };
        match self.names.get(&stem) {
            // Exact path is a known file — use it directly.
            Some(known) if known.contains(&candidate) => Some(candidate),
            // Basename match: shortest path wins (BTreeSet iterates in path
            // order, giving a deterministic tie-break).
            Some(known) => known.iter().min_by_key(|p| p.as_os_str().len()).cloned(),
            // Unknown basename: keep the optimistic path-based candidate.
            None => Some(candidate),
        }
    }

    fn add_name(&mut self, path: &Path) {
        if let Some(stem) = stem_lower(path) {
            self.names.entry(stem).or_default().insert(path.to_path_buf());
        }
    }

    fn remove_name(&mut self, path: &Path) {
        if let Some(stem) = stem_lower(path) {
            if let Some(set) = self.names.get_mut(&stem) {
                set.remove(path);
                if set.is_empty() {
                    self.names.remove(&stem);
                }
            }
        }
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

/// Lowercased file stem (filename without extension), used as the basename key
/// for bare-link resolution.
fn stem_lower(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_lowercase())
}

#[cfg(test)]
fn extract_wikilinks(content: &str, vault_root: &Path, file_path: &Path) -> Vec<PathBuf> {
    let mut links = Vec::new();
    for cap in WIKILINK_RE.captures_iter(content) {
        if let Some(target) = cap.get(1) {
            if let Some(resolved) = resolve_wikilink_target(target.as_str(), vault_root, file_path) {
                links.push(resolved);
            }
        }
    }
    links
}

/// Resolve a single wikilink target (the text inside `[[ ]]` before any
/// `|alias`) to a normalized absolute `.md` path, using the same rules as
/// indexing: relative (`./`, `../`) targets resolve against the linking file's
/// directory, everything else against the vault root, and a missing extension
/// defaults to `.md`. Returns `None` for heading-only links (`#anchor`) and
/// empty targets.
pub fn resolve_wikilink_target(
    target: &str,
    vault_root: &Path,
    file_path: &Path,
) -> Option<PathBuf> {
    let target_str = target.trim();
    if target_str.starts_with('#') {
        return None;
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
        return None;
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

    // Normalize path components.
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
    Some(normalized)
}

/// Rewrite every wikilink in `content` whose target resolves to `old_abs` so it
/// instead points at `new_rel` (a vault-relative path **without** extension),
/// preserving any `#anchor` and `|alias`. Returns `Some(new_content)` when at
/// least one link changed, or `None` when nothing matched.
///
/// Used by rename to keep backlinks intact when a note is renamed or moved.
pub fn rewrite_links_to(
    content: &str,
    vault_root: &Path,
    file_path: &Path,
    old_abs: &Path,
    new_rel: &str,
) -> Option<String> {
    let mut changed = false;
    let result = WIKILINK_RE.replace_all(content, |caps: &regex::Captures| {
        let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        let target = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        match resolve_wikilink_target(target, vault_root, file_path) {
            Some(resolved) if resolved == *old_abs => {
                changed = true;
                let trimmed = target.trim();
                // Keep the `#anchor` (from the target group) and `|alias`
                // (which lives outside group 1, in the full match).
                let anchor = trimmed.find('#').map(|i| &trimmed[i..]).unwrap_or("");
                let inner = &whole[2..whole.len().saturating_sub(2)];
                let alias = inner.find('|').map(|i| &inner[i..]).unwrap_or("");
                format!("[[{new_rel}{anchor}{alias}]]")
            }
            _ => whole.to_string(),
        }
    });
    if changed {
        Some(result.into_owned())
    } else {
        None
    }
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
    fn test_rewrite_links_to_preserves_alias_and_anchor() {
        let root = PathBuf::from("/vault");
        let file = PathBuf::from("/vault/note.md");
        let old_abs = PathBuf::from("/vault/old-name.md");
        let content = "See [[old-name]], [[old-name|Alias]], [[old-name#Section]] and [[other]].";

        let updated = rewrite_links_to(content, &root, &file, &old_abs, "new-name").unwrap();
        assert_eq!(
            updated,
            "See [[new-name]], [[new-name|Alias]], [[new-name#Section]] and [[other]]."
        );

        // Unrelated content returns None.
        assert!(rewrite_links_to("no links here", &root, &file, &old_abs, "new-name").is_none());
    }

    #[test]
    fn test_rewrite_links_to_handles_subfolder_targets() {
        let root = PathBuf::from("/vault");
        let file = PathBuf::from("/vault/note.md");
        let old_abs = PathBuf::from("/vault/sub/old.md");
        let content = "Link [[sub/old|x]].";
        let updated = rewrite_links_to(content, &root, &file, &old_abs, "sub/new").unwrap();
        assert_eq!(updated, "Link [[sub/new|x]].");
    }

    #[test]
    fn test_backlinks() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root);

        let file_a = PathBuf::from("/vault/a.md");
        let file_b = PathBuf::from("/vault/b.md");
        // Register b so the bare link resolves to a known file.
        index.rebuild(&[
            (file_a.clone(), "Link to [[b]].".to_string()),
            (file_b.clone(), String::new()),
        ]);

        let backlinks = index.get_backlinks(&file_b);
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0], file_a);
    }

    #[test]
    fn test_bare_link_resolves_into_subfolder() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root);

        let file_a = PathBuf::from("/vault/a.md");
        // Target lives in a subfolder, referenced by bare name.
        let target = PathBuf::from("/vault/sub/topic.md");
        index.rebuild(&[
            (file_a.clone(), "See [[topic]].".to_string()),
            (target.clone(), String::new()),
        ]);

        let backlinks = index.get_backlinks(&target);
        assert_eq!(backlinks, vec![file_a], "bare link should backlink the subfolder file");
    }

    #[test]
    fn test_dangling_bare_link_uses_vault_root_candidate() {
        // A link with no matching file resolves optimistically to the
        // vault-root path, so a note created there later inherits the backlink.
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root);
        let file_a = PathBuf::from("/vault/a.md");
        index.rebuild(&[(file_a.clone(), "See [[nonexistent]].".to_string())]);
        let backlinks = index.get_backlinks(&PathBuf::from("/vault/nonexistent.md"));
        assert_eq!(backlinks, vec![file_a]);
    }
}
