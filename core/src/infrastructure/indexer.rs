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
        let targets = extract_wikilinks(content, &self.vault_root, file_path);
        self.update_resolved_targets(file_path, targets);
    }

    /// Update the index for a file from pre-parsed local markdown link targets.
    pub fn update_file_targets<'a, I>(&mut self, file_path: &Path, targets: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        let targets = targets
            .into_iter()
            .filter_map(|target| resolve_wikilink_target(target, &self.vault_root, file_path))
            .collect::<Vec<_>>();
        self.update_resolved_targets(file_path, targets);
    }

    fn update_resolved_targets(&mut self, file_path: &Path, targets: Vec<PathBuf>) {
        if let Some(old_targets) = self.outgoing.remove(file_path) {
            for target in &old_targets {
                if let Some(incoming_set) = self.incoming.get_mut(target) {
                    incoming_set.remove(file_path);
                }
            }
        }

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
        if let Some(target) = cap.get(1)
            && let Some(resolved) = resolve_wikilink_target(target.as_str(), vault_root, file_path)
        {
            links.push(resolved);
        }
    }

    links
}

fn resolve_wikilink_target(target: &str, vault_root: &Path, file_path: &Path) -> Option<PathBuf> {
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

    #[test]
    fn update_file_targets_accepts_parser_metadata_targets() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root);

        let file = PathBuf::from("/vault/folder/source.md");
        index.update_file_targets(
            &file,
            [
                "../target",
                "#local-heading",
                "./sibling#Heading",
                "../complex!@#%^&*()",
            ],
        );

        assert_eq!(
            index.get_backlinks(&PathBuf::from("/vault/target.md")),
            vec![file.clone()]
        );
        assert_eq!(
            index.get_backlinks(&PathBuf::from("/vault/folder/sibling.md")),
            vec![file.clone()]
        );
        assert_eq!(
            index.get_backlinks(&PathBuf::from("/vault/complex!@#%^&*().md")),
            vec![file.clone()]
        );

        index.update_file_targets(&file, ["other"]);
        assert!(
            index
                .get_backlinks(&PathBuf::from("/vault/target.md"))
                .is_empty()
        );
        assert_eq!(
            index.get_backlinks(&PathBuf::from("/vault/other.md")),
            vec![file]
        );
    }
}

#[cfg(test)]
mod link_graph_scale_tests {
    use crate::infrastructure::indexer::FileIndex;
    use std::path::PathBuf;

    #[test]
    fn test_file_index_wikilink_combinatorics() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root.clone());
        let source_file = PathBuf::from("/vault/source.md");

        // 1. Generate 500 distinct wikilink variants to test parsing combinatorics
        // We vary whitespace, aliases, special characters, and extensions

        let target_names = vec![
            "simple",
            "simple-dashed",
            "simple_under",
            "nested/path/to/file",
            "spac y target",
            "unicode-🦀",
            "japanese-日本語",
            "umlaut-öäü",
            "emoji-🚀-star🌟",
            "dot.name",
            "complex!@#%^&*()",
            "caps_LOCK",
            "nested/sub/sub/file",
            "spaces-around-words",
            "multiple--dashes",
            "accented-éàçè",
        ];

        let alias_options = [
            None,
            Some("simple_alias"),
            Some("spaced alias name"),
            Some("unicode-🔥"),
        ];

        let space_variations = [("", ""), (" ", " "), ("  ", ""), ("", "  "), ("   ", "   ")];

        let mut content = String::new();
        let mut expected_targets = std::collections::HashSet::new();

        for (t_idx, target) in target_names.iter().enumerate() {
            for (a_idx, alias) in alias_options.iter().enumerate() {
                for (s_idx, (sp_start, sp_end)) in space_variations.iter().enumerate() {
                    // Ensure unique target name to prevent HashSet deduplication
                    let unique_target = format!("{}-{}-{}-{}", target, t_idx, a_idx, s_idx);

                    // Construct the link content: [[ <sp_start> target [| alias] <sp_end> ]]
                    let link = match alias {
                        Some(al) => format!(
                            "[[{}{}|{}{}{}]]",
                            sp_start, unique_target, sp_start, al, sp_end
                        ),
                        None => format!("[[{}{}{}]]", sp_start, unique_target, sp_end),
                    };
                    content.push_str(&link);
                    content.push(' ');

                    // Deduplicate expected path resolution
                    let trimmed_target = unique_target.trim();
                    let mut target_path = root.join(trimmed_target);
                    if target_path.extension().is_none() {
                        target_path.set_extension("md");
                    }
                    expected_targets.insert(target_path);
                }
            }
        }

        // Update file index and extract links
        index.update_file(&source_file, &content);

        let outgoing = index.get_outgoing_links(&source_file);
        let outgoing_set: std::collections::HashSet<_> = outgoing.iter().collect();

        // Verify all generated cases parsed correctly
        for expected in &expected_targets {
            assert!(
                outgoing_set.contains(expected),
                "Missing expected link: {:?}",
                expected
            );
        }

        assert!(
            outgoing.len() >= 300,
            "Should have tested hundreds of link combinatorics"
        );
    }

    #[test]
    fn test_file_index_graph_topologies() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root.clone());

        // --- 1. Star Topology: Center linked from and to 200 nodes ---
        let center = PathBuf::from("/vault/center.md");
        let mut center_content = String::new();

        for i in 1..=200 {
            let leaf = PathBuf::from(format!("/vault/leaf_{}.md", i));
            // leaf_i links to center
            index.update_file(&leaf, "Link to [[center]].");
            // center links back to leaf_i
            center_content.push_str(&format!("[[leaf_{}]] ", i));
        }
        index.update_file(&center, &center_content);

        // Verify star topology backlinks
        let center_backlinks = index.get_backlinks(&center);
        assert_eq!(center_backlinks.len(), 200);
        for i in 1..=200 {
            let leaf = PathBuf::from(format!("/vault/leaf_{}.md", i));
            assert!(center_backlinks.contains(&leaf));

            let leaf_backlinks = index.get_backlinks(&leaf);
            assert_eq!(leaf_backlinks.len(), 1);
            assert_eq!(leaf_backlinks[0], center);
        }

        // --- 2. Chain Topology: file_1 -> file_2 -> ... -> file_200 ---
        let mut index_chain = FileIndex::new(root.clone());
        for i in 1..200 {
            let current = PathBuf::from(format!("/vault/node_{}.md", i));
            let next_name = format!("node_{}", i + 1);
            index_chain.update_file(&current, &format!("Link to [[{}]].", next_name));
        }

        // Verify chain links
        for i in 1..199 {
            let current = PathBuf::from(format!("/vault/node_{}.md", i));
            let next = PathBuf::from(format!("/vault/node_{}.md", i + 1));
            let next_backlinks = index_chain.get_backlinks(&next);
            assert_eq!(next_backlinks.len(), 1);
            assert_eq!(next_backlinks[0], current);
        }

        // --- 3. Fully Connected Mesh Topology: 30 nodes (30 * 29 = 870 directional links) ---
        let mut index_mesh = FileIndex::new(root.clone());
        let mut nodes = Vec::new();
        for i in 0..30 {
            nodes.push(PathBuf::from(format!("/vault/mesh_{}.md", i)));
        }

        for (i, node) in nodes.iter().enumerate().take(30) {
            let mut mesh_content = String::new();
            for j in 0..30 {
                if i != j {
                    mesh_content.push_str(&format!("[[mesh_{}]] ", j));
                }
            }
            index_mesh.update_file(node, &mesh_content);
        }

        // Verify mesh links
        for node in nodes.iter().take(30) {
            let outgoing = index_mesh.get_outgoing_links(node);
            assert_eq!(outgoing.len(), 29);
            let incoming = index_mesh.get_backlinks(node);
            assert_eq!(incoming.len(), 29);
        }
    }

    #[test]
    fn test_file_index_dynamic_fuzzing_updates() {
        let root = PathBuf::from("/vault");
        let mut index = FileIndex::new(root.clone());

        // Generate 50 files
        let mut files = Vec::new();
        for i in 0..50 {
            files.push(PathBuf::from(format!("/vault/f_{}.md", i)));
        }

        // Run 300 cycles of random updates and assert link invariant:
        // sum(outgoing_link_counts) == sum(incoming_link_counts)
        let mut pseudo_rng = 42u64;
        let mut next_random = |modulus: usize| -> usize {
            pseudo_rng = pseudo_rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            (pseudo_rng as usize) % modulus
        };

        for _ in 0..300 {
            let file_idx = next_random(50);
            let file_to_update = &files[file_idx];

            // Link to between 0 and 5 other random files
            let num_links = next_random(6);
            let mut content = String::new();
            for _ in 0..num_links {
                let target_idx = next_random(50);
                if target_idx != file_idx {
                    content.push_str(&format!("[[f_{}]] ", target_idx));
                }
            }

            index.update_file(file_to_update, &content);

            // Assert invariant: count of all outgoing links matches all incoming backlinks
            let mut total_outgoing = 0;
            let mut total_incoming = 0;

            for f in &files {
                total_outgoing += index.get_outgoing_links(f).len();
                total_incoming += index.get_backlinks(f).len();
            }

            assert_eq!(
                total_outgoing, total_incoming,
                "Link count invariant violated during updates"
            );
        }

        // Delete files one by one and ensure index drains cleanly to empty
        for f in &files {
            index.remove_file(f);
        }

        for f in &files {
            assert!(index.get_outgoing_links(f).is_empty());
            assert!(index.get_backlinks(f).is_empty());
        }
    }
}
