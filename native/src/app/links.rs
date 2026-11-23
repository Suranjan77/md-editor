//! Resolving wikilinks, relative links and heading anchors to vault paths and lines.


/// Lexically normalize a vault-relative path and render it with forward
/// slashes. Vault-relative paths are forward-slash separated everywhere else
/// (see `vault::path_to_relative_string`), so on Windows the `PathBuf`'s native
/// backslashes have to be converted back or the result stops matching indexed
/// entries, the active path, and the paths stored in the settings database.
pub(super) fn normalize_path(path: &std::path::Path) -> String {
    let mut components = Vec::new();
    for component in path.components() {
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
    let normalized: std::path::PathBuf = components.into_iter().collect();
    // Vault-relative strings use forward slashes everywhere (see core's
    // path_to_relative_string); PathBuf would yield `\` on Windows.
    normalized.to_string_lossy().replace('\\', "/")
}

pub(super) fn resolve_relative_link_path(
    vault_root: Option<&str>,
    active_path: Option<&str>,
    link_path: &str,
) -> String {
    if link_path.starts_with('.')
        && let Some(active_file) = active_path
    {
        let active_path_buf = std::path::Path::new(active_file);
        if let Some(parent) = active_path_buf.parent() {
            let resolved = parent.join(link_path);
            return normalize_path(&resolved);
        }
    }
    // If it doesn't start with '.', check if there is an existing file relative to the active path's parent.
    if let (Some(vault), Some(active_file)) = (vault_root, active_path) {
        let active_path_buf = std::path::Path::new(active_file);
        if let Some(parent) = active_path_buf.parent() {
            let relative_candidate = parent.join(link_path);
            let abs_relative = std::path::Path::new(vault).join(&relative_candidate);
            if abs_relative.exists()
                || abs_relative.with_extension("md").exists()
                || abs_relative.with_extension("markdown").exists()
            {
                return normalize_path(&relative_candidate);
            }
        }
    }
    link_path.to_string()
}

/// Resolve a bare wikilink name (e.g. `[[NoteName]]`) to a vault file path by
/// matching the link's final component against every file's basename, case-
/// insensitively, with or without a `.md`/`.markdown` extension. When several
/// files match, the shortest path wins (Obsidian-style "closest" resolution);
/// `None` is returned for no match or an ambiguous tie at the same depth.
pub(super) fn resolve_vault_note_by_name(
    entries: &[md_editor_core::types::FileEntry],
    link_path: &str,
) -> Option<String> {
    // Use only the final path component as the target name.
    let target = link_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(link_path)
        .trim();
    if target.is_empty() {
        return None;
    }
    let target_lower = target.to_lowercase();

    let mut matches: Vec<&str> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let name_lower = entry.name.to_lowercase();
        let stem_lower = name_lower
            .strip_suffix(".md")
            .or_else(|| name_lower.strip_suffix(".markdown"))
            .unwrap_or(&name_lower);
        if name_lower == target_lower || stem_lower == target_lower {
            matches.push(entry.path.as_str());
        }
    }

    matches.sort_by_key(|p| p.len());
    match matches.as_slice() {
        [] => None,
        [only] => Some((*only).to_string()),
        // Prefer a strictly shortest path; bail if the two shortest tie.
        [a, b, ..] if a.len() != b.len() => Some((*a).to_string()),
        _ => None,
    }
}

pub(super) fn slugify(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '_' {
            result.push(c);
            last_was_hyphen = false;
        } else if (c.is_whitespace() || c == '-') && !last_was_hyphen {
            result.push('-');
            last_was_hyphen = true;
        }
    }
    result.trim_matches('-').to_string()
}

pub(super) fn find_heading_line(text: &str, target_slug: &str) -> Option<usize> {
    let mut in_code_block = false;
    for (line_idx, line_content) in text.split('\n').enumerate() {
        let trimmed = line_content.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if trimmed.starts_with('#') {
            let mut level = 0;
            for c in trimmed.chars() {
                if c == '#' {
                    level += 1;
                } else {
                    break;
                }
            }
            if level > 0 && level <= 6 {
                let heading_text = trimmed[level..].trim();
                if slugify(heading_text) == target_slug {
                    return Some(line_idx);
                }
            }
        }
    }
    None
}

pub(super) fn find_heading_or_widget_line(
    text: &str,
    highlighted_lines: &[crate::editor::highlight::StyledLine],
    target_slug: &str,
) -> Option<usize> {
    // If target_slug is "listing-N", we also want to look for "code-N", and vice-versa
    let alternative_slug = if let Some(num_str) = target_slug.strip_prefix("listing-") {
        Some(format!("code-{}", num_str))
    } else {
        target_slug
            .strip_prefix("code-")
            .map(|num_str| format!("listing-{}", num_str))
    };

    for (line_idx, line) in highlighted_lines.iter().enumerate() {
        for span in &line.spans {
            if let Some(ref span_id) = span.id {
                if span_id.eq_ignore_ascii_case(target_slug) {
                    return Some(line_idx);
                }
                if let Some(ref alt) = alternative_slug
                    && span_id.eq_ignore_ascii_case(alt)
                {
                    return Some(line_idx);
                }
            }
        }
    }

    if let Some(line_idx) = find_heading_line(text, target_slug) {
        return Some(line_idx);
    }
    let target_slug_underscored = target_slug.replace('-', "_");

    let re_slug_str = format!(
        r#"(?i)id\s*=\s*["']{}["']|name\s*=\s*["']{}["']|\\label\s*\{{\s*{}\s*\}}|\{{\s*#\s*{}\s*\}}"#,
        regex::escape(target_slug),
        regex::escape(target_slug),
        regex::escape(target_slug),
        regex::escape(target_slug)
    );
    let re_slug = regex::Regex::new(&re_slug_str).ok()?;

    let re_under_str = format!(
        r#"(?i)id\s*=\s*["']{}["']|name\s*=\s*["']{}["']|\\label\s*\{{\s*{}\s*\}}|\{{\s*#\s*{}\s*\}}"#,
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored)
    );
    let re_under = regex::Regex::new(&re_under_str).ok()?;

    for (line_idx, line_content) in text.split('\n').enumerate() {
        if re_slug.is_match(line_content) || re_under.is_match(line_content) {
            return Some(line_idx);
        }
    }
    None
}
