//! Link graph service: wikilink extraction, backlinks/outlinks, broken-link
//! queries, and rename repair. Implemented without a regex dependency, with
//! rename repair as a first-class, pure operation (the caller applies the
//! rewritten contents via [`crate::atomic_save`]).
//!
//! All paths are **relative to the vault root**.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One `[[target]]` / `[[target|alias]]` / `[[target#anchor]]` occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// Raw target text between the brackets (before `|`/`#`).
    pub target: String,
    /// Char range of the whole `[[…]]` in the source.
    pub start: usize,
    pub end: usize,
}

/// Scan `[[…]]` occurrences. Char-offset based; ignores empty targets and
/// pure anchors (`[[#heading]]`).
pub fn extract_wikilinks(content: &str) -> Vec<WikiLink> {
    let chars: Vec<char> = content.chars().collect();
    let mut links = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '[' && chars[i + 1] == '[' {
            let body_start = i + 2;
            let mut j = body_start;
            while j + 1 < chars.len() && !(chars[j] == ']' && chars[j + 1] == ']') {
                j += 1;
            }
            if j + 1 < chars.len() {
                let body: String = chars[body_start..j].iter().collect();
                let target = body
                    .split(['|', '#'])
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !target.is_empty() {
                    links.push(WikiLink {
                        target,
                        start: i,
                        end: j + 2,
                    });
                }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    links
}

/// Resolve a wikilink target to a vault-relative path: `note` → `note.md`,
/// subfolders allowed, extension added when missing.
pub fn resolve_target(target: &str) -> PathBuf {
    let mut path = PathBuf::from(target.trim());
    if path.extension().is_none() {
        path.set_extension("md");
    }
    path
}

/// In-memory bidirectional wikilink graph.
#[derive(Debug, Default)]
pub struct LinkGraph {
    outgoing: HashMap<PathBuf, HashSet<PathBuf>>,
    incoming: HashMap<PathBuf, HashSet<PathBuf>>,
    /// Files known to exist (for broken-link queries).
    files: HashSet<PathBuf>,
}

impl LinkGraph {
    pub fn new() -> LinkGraph {
        LinkGraph::default()
    }

    /// (Re)index one file's links from its content.
    pub fn update_file(&mut self, file: &Path, content: &str) {
        self.files.insert(file.to_path_buf());
        let targets: HashSet<PathBuf> = extract_wikilinks(content)
            .iter()
            .map(|l| resolve_target(&l.target))
            .collect();
        self.set_outgoing(file, targets);
    }

    pub fn remove_file(&mut self, file: &Path) {
        self.set_outgoing(file, HashSet::new());
        self.outgoing.remove(file);
        self.files.remove(file);
    }

    /// A file was renamed: move its graph entries and report which files
    /// link to it (their text needs repair).
    pub fn rename_file(&mut self, old: &Path, new: &Path) -> Vec<PathBuf> {
        let referrers = self.backlinks(old);
        if let Some(targets) = self.outgoing.remove(old) {
            self.set_outgoing(new, targets);
        }
        self.files.remove(old);
        self.files.insert(new.to_path_buf());
        if let Some(set) = self.incoming.remove(old) {
            for referrer in &set {
                if let Some(out) = self.outgoing.get_mut(referrer) {
                    out.remove(old);
                    out.insert(new.to_path_buf());
                }
            }
            self.incoming.insert(new.to_path_buf(), set);
        }
        referrers
    }

    pub fn backlinks(&self, file: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self
            .incoming
            .get(file)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    pub fn outlinks(&self, file: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self
            .outgoing
            .get(file)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Link targets that do not correspond to a known file.
    pub fn broken_links(&self) -> Vec<(PathBuf, PathBuf)> {
        let mut broken = Vec::new();
        for (file, targets) in &self.outgoing {
            for target in targets {
                if !self.files.contains(target) {
                    broken.push((file.clone(), target.clone()));
                }
            }
        }
        broken.sort();
        broken
    }

    fn set_outgoing(&mut self, file: &Path, targets: HashSet<PathBuf>) {
        if let Some(old) = self.outgoing.remove(file) {
            for t in &old {
                if let Some(inc) = self.incoming.get_mut(t) {
                    inc.remove(file);
                }
            }
        }
        for t in &targets {
            self.incoming
                .entry(t.clone())
                .or_default()
                .insert(file.to_path_buf());
        }
        self.outgoing.insert(file.to_path_buf(), targets);
    }
}

/// Rewrite every wikilink in `content` that resolves to `old` so it points
/// at `new`, preserving aliases and anchors. Returns `None` when nothing
/// needed repair — rename repair is a pure text transaction; the caller
/// persists it atomically.
pub fn rewrite_links(content: &str, old: &Path, new: &Path) -> Option<String> {
    let links = extract_wikilinks(content);
    if links.is_empty() {
        return None;
    }
    let new_target = new.to_string_lossy().trim_end_matches(".md").to_string();
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::new();
    let mut cursor = 0;
    let mut touched = false;
    for link in links {
        if resolve_target(&link.target) != old {
            continue;
        }
        touched = true;
        out.extend(&chars[cursor..link.start]);
        // Rebuild `[[new(…suffix)]]` keeping everything after the target.
        let body: String = chars[link.start + 2..link.end - 2].iter().collect();
        let suffix = body
            .find(['|', '#'])
            .map(|i| body[i..].to_string())
            .unwrap_or_default();
        out.push_str("[[");
        out.push_str(&new_target);
        out.push_str(&suffix);
        out.push_str("]]");
        cursor = link.end;
    }
    if !touched {
        return None;
    }
    out.extend(&chars[cursor..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_handles_aliases_anchors_and_garbage() {
        let links =
            extract_wikilinks("a [[note]] b [[dir/other|alias]] [[x#sec]] [[]] [[#only]] [[open");
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, vec!["note", "dir/other", "x"]);
    }

    #[test]
    fn graph_backlinks_and_broken_links() {
        let mut g = LinkGraph::new();
        g.update_file(Path::new("a.md"), "see [[b]] and [[missing]]");
        g.update_file(Path::new("b.md"), "back to [[a]]");
        assert_eq!(g.backlinks(Path::new("b.md")), vec![PathBuf::from("a.md")]);
        assert_eq!(g.outlinks(Path::new("a.md")).len(), 2);
        assert_eq!(
            g.broken_links(),
            vec![(PathBuf::from("a.md"), PathBuf::from("missing.md"))]
        );
        // Re-indexing replaces, not accumulates.
        g.update_file(Path::new("a.md"), "now only [[b]]");
        assert!(g.broken_links().is_empty());
    }

    #[test]
    fn rename_reports_referrers_and_moves_edges() {
        let mut g = LinkGraph::new();
        g.update_file(Path::new("a.md"), "[[target]]");
        g.update_file(Path::new("b.md"), "[[target|t]] and [[other]]");
        g.update_file(Path::new("target.md"), "");
        g.update_file(Path::new("other.md"), "");

        let referrers = g.rename_file(Path::new("target.md"), Path::new("moved/target2.md"));
        assert_eq!(
            referrers,
            vec![PathBuf::from("a.md"), PathBuf::from("b.md")]
        );
        assert_eq!(
            g.backlinks(Path::new("moved/target2.md")),
            vec![PathBuf::from("a.md"), PathBuf::from("b.md")]
        );
        assert!(g.backlinks(Path::new("target.md")).is_empty());
        assert!(
            g.broken_links().is_empty(),
            "outgoing edges followed the rename"
        );
    }

    #[test]
    fn rewrite_preserves_alias_and_anchor() {
        let content = "see [[target|Nice]] then [[target#sec]] and [[other]]";
        let out = rewrite_links(content, Path::new("target.md"), Path::new("moved/new.md"));
        assert_eq!(
            out.as_deref(),
            Some("see [[moved/new|Nice]] then [[moved/new#sec]] and [[other]]")
        );
        // Untouched content reports None (no pointless rewrite).
        assert_eq!(
            rewrite_links(
                "no links to it [[other]]",
                Path::new("t.md"),
                Path::new("u.md")
            ),
            None
        );
    }

    #[test]
    fn rewrite_handles_unicode_content() {
        let content = "한글 👨‍👩‍👧‍👦 [[target]] 字";
        let out = rewrite_links(content, Path::new("target.md"), Path::new("née.md"));
        assert_eq!(out.as_deref(), Some("한글 👨‍👩‍👧‍👦 [[née]] 字"));
    }
}
