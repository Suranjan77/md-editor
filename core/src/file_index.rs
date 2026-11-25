//! The wikilink index: parses `[[target|alias]]`, resolves targets, and keeps outgoing and incoming links.

use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod code_spans;

pub(crate) use code_spans::{code_ranges, in_code};

/// `[[target]]` / `[[target|alias]]` wikilink matcher. Compiled once and
/// reused — `update_file` runs this for every file during indexing.
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").expect("valid regex"));

/// Wikilinks in `content` outside code (fenced blocks and inline code spans),
/// so `[[nodiscard]]` in C++ or `[[ -f x ]]` in a shell snippet is not a link.
fn wikilinks(content: &str) -> impl Iterator<Item = regex::Captures<'_>> {
    let code = code_spans::code_ranges(content);
    WIKILINK_RE
        .captures_iter(content)
        .filter(move |cap| !code_spans::in_code(&code, cap.get(0).map_or(0, |m| m.start())))
}

/// Byte ranges of every `[[...]]` in `content`, code included.
pub(crate) fn wikilink_ranges(content: &str) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    WIKILINK_RE.find_iter(content).map(|m| m.range())
}

/// In-memory graph of wikilink references between files.
#[derive(Debug)]
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
    /// file path → the wikilink targets written in it, as written. Kept so a
    /// file appearing or disappearing can re-resolve exactly the links that
    /// name it, without re-reading their files.
    links: HashMap<PathBuf, Vec<String>>,
}

impl FileIndex {
    pub fn new(vault_root: PathBuf) -> Self {
        FileIndex {
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            vault_root,
            names: HashMap::new(),
            links: HashMap::new(),
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
        self.links.clear();
        for (path, _) in files {
            self.add_name(path);
        }
        for (path, content) in files {
            self.links.insert(path.clone(), raw_targets(content));
            self.resolve_outgoing(path);
        }
    }

    /// [`FileIndex::rebuild`] from each file's link targets as written (see
    /// [`raw_targets`]), for files that were not re-read.
    pub fn rebuild_from_links(&mut self, files: Vec<(PathBuf, Vec<String>)>) {
        self.outgoing.clear();
        self.incoming.clear();
        self.names.clear();
        self.links.clear();
        for (path, _) in &files {
            self.add_name(path);
        }
        let paths: Vec<PathBuf> = files.iter().map(|(path, _)| path.clone()).collect();
        self.links.extend(files);
        for path in paths {
            self.resolve_outgoing(&path);
        }
    }

    /// Extract wikilinks from document text and update the index for a given
    /// file. Used for incremental single-file edits; for a full vault build
    /// prefer [`FileIndex::rebuild`], which resolves bare links against the
    /// complete file set.
    pub fn update_file(&mut self, file_path: &Path, content: &str) {
        let is_new = self.add_name(file_path);
        self.links
            .insert(file_path.to_path_buf(), raw_targets(content));
        self.resolve_outgoing(file_path);
        if is_new {
            // A new name can claim links that dangled, or that resolved by
            // basename to a longer path, anywhere in the vault.
            self.rebind(file_path);
        }
    }

    /// Remove a file from the index entirely. Links to it from other files
    /// are re-resolved: to another file with its name, or left dangling.
    pub fn remove_file(&mut self, file_path: &Path) {
        self.clear_outgoing(file_path);
        self.links.remove(file_path);
        self.incoming.remove(file_path);
        if self.remove_name(file_path) {
            self.rebind(file_path);
        }
    }

    /// Drop `source`'s outgoing edges and their `incoming` mirror.
    fn clear_outgoing(&mut self, source: &Path) {
        if let Some(old_targets) = self.outgoing.remove(source) {
            for target in &old_targets {
                if let Some(incoming_set) = self.incoming.get_mut(target) {
                    incoming_set.remove(source);
                    if incoming_set.is_empty() {
                        self.incoming.remove(target);
                    }
                }
            }
        }
    }

    /// (Re)compute `source`'s outgoing edges from its recorded link targets.
    fn resolve_outgoing(&mut self, source: &Path) {
        self.clear_outgoing(source);
        let targets: HashSet<PathBuf> = self
            .links
            .get(source)
            .into_iter()
            .flatten()
            .filter_map(|target| self.resolve_link(target, source))
            .collect();
        for target in &targets {
            self.incoming
                .entry(target.clone())
                .or_default()
                .insert(source.to_path_buf());
        }
        self.outgoing.insert(source.to_path_buf(), targets);
    }

    /// Re-resolve every file with a link whose basename is `changed`'s.
    fn rebind(&mut self, changed: &Path) {
        let Some(stem) = stem_lower(changed) else {
            return;
        };
        let sources: Vec<PathBuf> = self
            .links
            .iter()
            .filter(|(source, targets)| {
                targets.iter().any(|target| {
                    resolve_wikilink_target(target, &self.vault_root, source)
                        .and_then(|candidate| stem_lower(&candidate))
                        .is_some_and(|s| s == stem)
                })
            })
            .map(|(source, _)| source.clone())
            .collect();
        for source in sources {
            self.resolve_outgoing(&source);
        }
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
    pub(crate) fn resolve_link(&self, target: &str, file_path: &Path) -> Option<PathBuf> {
        let candidate = resolve_wikilink_target(target, &self.vault_root, file_path)?;
        let Some(stem) = candidate
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
        else {
            return Some(candidate);
        };
        let (path_part, _) = target.trim().split_once('#').unwrap_or((target.trim(), ""));
        let qualified = path_part.contains('/') || path_part.starts_with('.');
        match self.names.get(&stem) {
            // Exact path is a known file — use it directly.
            Some(known) if known.contains(&candidate) => Some(candidate),
            // `[[sub/Name]]` names a place: only `sub/Name.markdown` may stand
            // in for `sub/Name.md`, never a same-named file elsewhere.
            Some(known) if qualified => {
                let twin = candidate.with_extension("markdown");
                Some(if known.contains(&twin) {
                    twin
                } else {
                    candidate
                })
            }
            // Basename match: shortest path wins (BTreeSet iterates in path
            // order, giving a deterministic tie-break).
            Some(known) => known.iter().min_by_key(|p| p.as_os_str().len()).cloned(),
            // Unknown basename: keep the optimistic path-based candidate.
            None => Some(candidate),
        }
    }

    /// Register `path` by name; whether it was not known before.
    fn add_name(&mut self, path: &Path) -> bool {
        stem_lower(path).is_some_and(|stem| {
            self.names
                .entry(stem)
                .or_default()
                .insert(path.to_path_buf())
        })
    }

    /// Forget `path`'s name; whether it was known.
    fn remove_name(&mut self, path: &Path) -> bool {
        let Some(stem) = stem_lower(path) else {
            return false;
        };
        let Some(set) = self.names.get_mut(&stem) else {
            return false;
        };
        let removed = set.remove(path);
        if set.is_empty() {
            self.names.remove(&stem);
        }
        removed
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

/// The wikilink targets in `content` (outside code), as written.
pub(crate) fn raw_targets(content: &str) -> Vec<String> {
    wikilinks(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// Lowercased file stem (filename without extension), used as the basename key
/// for bare-link resolution.
fn stem_lower(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_lowercase())
}

#[cfg(test)]
fn extract_wikilinks(content: &str, vault_root: &Path, file_path: &Path) -> Vec<PathBuf> {
    let mut links = Vec::new();
    for cap in wikilinks(content) {
        if let Some(target) = cap.get(1)
            && let Some(resolved) = resolve_wikilink_target(target.as_str(), vault_root, file_path)
        {
            links.push(resolved);
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
        target_str[..idx].trim()
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
    // `..` that climbs out of the vault points at nothing in it.
    if !normalized.starts_with(vault_root) {
        return None;
    }
    if !crate::vault::is_supported_vault_path(&normalized) {
        let mut name = normalized.into_os_string();
        name.push(".md");
        normalized = PathBuf::from(name);
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
    rewrite_links_resolved(content, old_abs, new_rel, |target| {
        resolve_wikilink_target(target, vault_root, file_path)
    })
}

impl FileIndex {
    /// [`rewrite_links_to`], resolving each link the way the index does
    /// (exact path, then shortest matching basename) — so a bare `[[B]]` that
    /// the index counts as a backlink to `notes/B.md` is rewritten too.
    pub fn rewrite_links_to(
        &self,
        content: &str,
        file_path: &Path,
        old_abs: &Path,
        new_rel: &str,
    ) -> Option<String> {
        rewrite_links_resolved(content, old_abs, new_rel, |target| {
            self.resolve_link(target, file_path)
        })
    }
}

fn rewrite_links_resolved(
    content: &str,
    old_abs: &Path,
    new_rel: &str,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    let mut changed = false;
    let code = code_spans::code_ranges(content);
    let result = WIKILINK_RE.replace_all(content, |caps: &regex::Captures| {
        let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        let target = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if code_spans::in_code(&code, caps.get(0).map_or(0, |m| m.start())) {
            return whole.to_string();
        }
        match resolve(target) {
            Some(resolved)
                if resolved == *old_abs
                    || (crate::vault::is_markdown_path(&resolved)
                        && resolved.with_extension("markdown") == *old_abs) =>
            {
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
        assert_eq!(
            backlinks,
            vec![file_a],
            "bare link should backlink the subfolder file"
        );
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

    #[test]
    fn dotted_titles_and_punctuated_anchors_resolve() {
        let root = PathBuf::from("/vault");
        let source = root.join("source.md");
        assert_eq!(
            resolve_wikilink_target("Chapter 3.2", &root, &source),
            Some(root.join("Chapter 3.2.md"))
        );
        assert_eq!(
            resolve_wikilink_target("Note#Results (2024)", &root, &source),
            Some(root.join("Note.md"))
        );
        assert_eq!(
            resolve_wikilink_target("paper.pdf", &root, &source),
            Some(root.join("paper.pdf"))
        );
    }

    #[test]
    fn wikilinks_inside_code_are_neither_indexed_nor_rewritten() {
        let root = PathBuf::from("/vault");
        let file = root.join("note.md");
        let content =
            "Real [[likely]].\n\n```cpp\n[[likely]] if (x) {}\n```\n\nInline `[[likely]]` too.\n";
        assert_eq!(extract_wikilinks(content, &root, &file).len(), 1);

        let updated =
            rewrite_links_to(content, &root, &file, &root.join("likely.md"), "probable").unwrap();
        assert_eq!(
            updated,
            "Real [[probable]].\n\n```cpp\n[[likely]] if (x) {}\n```\n\nInline `[[likely]]` too.\n"
        );
    }

    #[test]
    fn path_qualified_links_do_not_fall_back_to_a_same_named_file_elsewhere() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root.clone());
        index.rebuild(&[
            (root.join("notes/topic.md"), String::new()),
            (root.join("a.md"), "[[archive/topic]] [[topic]]".to_string()),
        ]);
        let mut out = index.get_outgoing_links(&root.join("a.md"));
        out.sort();
        assert_eq!(
            out,
            vec![root.join("archive/topic.md"), root.join("notes/topic.md")]
        );
    }

    #[test]
    fn dotdot_out_of_the_vault_resolves_to_nothing() {
        let root = PathBuf::from("/vault");
        let note = root.join("sub/a.md");
        assert_eq!(
            resolve_wikilink_target("../../etc/passwd", &root, &note),
            None
        );
        assert_eq!(
            resolve_wikilink_target("../b", &root, &note),
            Some(root.join("b.md"))
        );
    }

    #[test]
    fn creating_and_deleting_a_note_rebinds_links_that_name_it() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root.clone());
        let a = root.join("a.md");
        index.update_file(&a, "See [[Topic]].");
        assert_eq!(index.get_outgoing_links(&a), vec![root.join("Topic.md")]);

        // `sub/Topic.md` appears later: the dangling link now finds it.
        let topic = root.join("sub/Topic.md");
        index.update_file(&topic, "");
        assert_eq!(index.get_outgoing_links(&a), vec![topic.clone()]);
        assert_eq!(index.get_backlinks(&topic), vec![a.clone()]);
        assert!(index.get_backlinks(&root.join("Topic.md")).is_empty());

        // Deleting it leaves the link dangling again, not pointing at a ghost.
        index.remove_file(&topic);
        assert_eq!(index.get_outgoing_links(&a), vec![root.join("Topic.md")]);
        assert!(index.get_backlinks(&topic).is_empty());
    }
}
