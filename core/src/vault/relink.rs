//! Link rewriting for a rename or move: wikilinks and Markdown links/images whose target moved, or whose relative path broke because the note itself moved.

use std::sync::LazyLock;

use regex::Regex;

use crate::file_index::{code_ranges, in_code, wikilink_ranges};

#[cfg(test)]
mod tests;

/// `[text](target)` / `![alt](target)`: the target is `<...>` or a run without
/// spaces or `)`, optionally followed by a quoted title.
static MD_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\]\(\s*(<[^>\n]*>|[^)\s]+)(\s+"[^"\n]*")?\s*\)"#).expect("valid regex")
});

/// One rename, as vault-relative `/`-separated paths.
#[derive(Debug, Clone, Copy)]
pub(super) struct Move<'a> {
    pub old: &'a str,
    pub new: &'a str,
}

impl Move<'_> {
    /// Where `rel` lives after the move, or `None` when the move leaves it alone.
    pub fn map(&self, rel: &str) -> Option<String> {
        if rel == self.old {
            return Some(self.new.to_string());
        }
        rel.strip_prefix(self.old)
            .filter(|rest| rest.starts_with('/'))
            .map(|rest| format!("{}{rest}", self.new))
    }
}

/// What a note's links can be checked against: the vault as it was before the move.
pub(super) struct OldVault<'a> {
    /// Whether a vault-relative path existed.
    pub exists: &'a dyn Fn(&str) -> bool,
    /// Where a wikilink target, written in the note at the given vault-relative
    /// path, pointed (the index's resolution: exact path, then basename).
    pub resolve_wikilink: &'a dyn Fn(&str, &str) -> Option<String>,
}

/// `content` of the note at `note_old` (moving to `note_new`) with every link
/// that `mv` would break pointed at the right place again. `None` when no
/// link needed changing. Links in code are left alone.
pub(super) fn relink_note(
    content: &str,
    note_old: &str,
    note_new: &str,
    mv: Move<'_>,
    old: &OldVault<'_>,
) -> Option<String> {
    let code = code_ranges(content);
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    for link in wikilink_ranges(content) {
        if in_code(&code, link.start) {
            continue;
        }
        let whole = &content[link.clone()];
        if let Some(new) = relink_wikilink(whole, note_old, note_new, mv, old) {
            edits.push((link, new));
        }
    }
    for cap in MD_LINK_RE.captures_iter(content) {
        let Some(target) = cap.get(1) else { continue };
        if in_code(&code, target.start()) {
            continue;
        }
        if let Some(new) = relink_md_target(target.as_str(), note_old, note_new, mv, old) {
            edits.push((target.range(), new));
        }
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(content.len());
    let mut at = 0;
    for (range, new) in edits {
        if range.start < at {
            continue; // overlapping matches: keep the first
        }
        out.push_str(&content[at..range.start]);
        out.push_str(&new);
        at = range.end;
    }
    out.push_str(&content[at..]);
    Some(out)
}

/// `[[target#anchor|alias]]` rewritten, or `None` when it still resolves.
fn relink_wikilink(
    whole: &str,
    note_old: &str,
    note_new: &str,
    mv: Move<'_>,
    old: &OldVault<'_>,
) -> Option<String> {
    let inner = &whole[2..whole.len() - 2];
    let (target, alias) = match inner.find('|') {
        Some(i) => (&inner[..i], &inner[i..]),
        None => (inner, ""),
    };
    let target = target.trim();
    let (path, anchor) = match target.find('#') {
        Some(i) => (target[..i].trim(), &target[i..]),
        None => (target, ""),
    };
    if path.is_empty() {
        return None;
    }
    let was = (old.resolve_wikilink)(path, note_old)?;
    let was = existing(&was, old)?;
    let now = mv.map(&was).unwrap_or_else(|| was.clone());
    if wikilink_still_resolves(path, note_new, &now) {
        return None;
    }
    let link = if crate::vault::is_markdown_path(std::path::Path::new(&now)) {
        now.rsplit_once('.').map_or(now.as_str(), |(stem, _)| stem)
    } else {
        now.as_str()
    };
    Some(format!("[[{link}{anchor}{alias}]]"))
}

/// The path itself, or its `.markdown` twin (a wikilink's implicit extension
/// is `.md`), when it existed before the move.
fn existing(rel: &str, old: &OldVault<'_>) -> Option<String> {
    if (old.exists)(rel) {
        return Some(rel.to_string());
    }
    let twin = format!("{}.markdown", rel.strip_suffix(".md")?);
    (old.exists)(&twin).then_some(twin)
}

/// Whether the unchanged text `path`, written in `note`, reaches `target`
/// after the move: a bare name of a note by basename (the index knows notes by
/// name), anything else by its path.
fn wikilink_still_resolves(path: &str, note: &str, target: &str) -> bool {
    let bare = !path.contains('/')
        && !path.starts_with('.')
        && crate::vault::is_markdown_path(std::path::Path::new(target));
    if bare {
        let stem = |p: &str| {
            let name = p.rsplit('/').next().unwrap_or(p);
            let name = if crate::vault::is_markdown_path(std::path::Path::new(name)) {
                name.rsplit_once('.').map_or(name, |(s, _)| s)
            } else {
                name
            };
            name.to_lowercase()
        };
        return stem(path) == stem(target);
    }
    let base = if path.starts_with('.') {
        parent(note)
    } else {
        ""
    };
    let Some(joined) = normalize(&join(base, path)) else {
        return false;
    };
    joined == target
        || (!crate::vault::is_supported_vault_path(std::path::Path::new(&joined))
            && (format!("{joined}.md") == target || format!("{joined}.markdown") == target))
}

/// A Markdown link target rewritten, or `None` when it still resolves. Paths
/// are tried relative to the note first (how images load), then to the vault.
fn relink_md_target(
    raw: &str,
    note_old: &str,
    note_new: &str,
    mv: Move<'_>,
    old: &OldVault<'_>,
) -> Option<String> {
    let bracketed = raw.starts_with('<') && raw.ends_with('>');
    let text = if bracketed {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };
    if text.is_empty() || text.starts_with('#') || text.contains("://") || text.contains(':') {
        return None;
    }
    let split = text.find(['#', '?']).unwrap_or(text.len());
    let (path, suffix) = text.split_at(split);
    let decoded = percent_decode(path);
    let rooted = decoded.starts_with('/');

    let note_relative = if rooted {
        None
    } else {
        normalize(&join(parent(note_old), &decoded)).filter(|rel| (old.exists)(rel))
    };
    let (was, relative_to_note) = match note_relative {
        Some(rel) => (rel, true),
        None => {
            let rel = normalize(decoded.trim_start_matches('/'))?;
            if !(old.exists)(&rel) {
                return None;
            }
            (rel, false)
        }
    };
    let now = mv.map(&was).unwrap_or_else(|| was.clone());
    let still_resolves = if relative_to_note {
        normalize(&join(parent(note_new), &decoded)).as_deref() == Some(now.as_str())
    } else {
        now == was
    };
    if still_resolves {
        return None;
    }
    let new_path = if relative_to_note {
        relative_path(parent(note_new), &now)
    } else if rooted {
        format!("/{now}")
    } else {
        now
    };
    let encoded = if bracketed {
        new_path
    } else if path.contains('%') || new_path.contains(' ') {
        new_path.replace('%', "%25").replace(' ', "%20")
    } else if new_path.contains(['(', ')', '<', '>']) {
        return Some(format!("<{new_path}{suffix}>"));
    } else {
        new_path
    };
    Some(if bracketed {
        format!("<{encoded}{suffix}>")
    } else {
        format!("{encoded}{suffix}")
    })
}

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

/// Resolve `.` and `..`; `None` when the path climbs out of the vault.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The path from directory `from` to `to`, both vault-relative.
fn relative_path(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|p| !p.is_empty()).collect();
    let common = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&to_parts[common..]);
    out.join("/")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}
