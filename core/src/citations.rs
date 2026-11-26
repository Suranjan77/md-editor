//! BibTeX and vault-PDF citation index for `@` inline autocomplete.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::vault::{
    VaultError, VaultResult, list_all_bib_files, list_all_pdf_files, read_file_capped,
};

#[cfg(test)]
mod tests;

const MAX_BIB_BYTES: u64 = 2 * 1024 * 1024;

/// One citable source from a `.bib` file and/or a vault PDF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationEntry {
    pub key: String,
    pub display: String,
    pub title: String,
    pub author: String,
    pub year: Option<String>,
    pub pdf_path: Option<String>,
}

/// Collect citations from all `.bib` files and `.pdf` files in the active vault.
pub fn collect_vault_citations(state: &AppState) -> VaultResult<Vec<CitationEntry>> {
    let root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or(VaultError::NoVault)?.clone()
    };

    let bib_files = list_all_bib_files(&root)?;
    let mut bibs = Vec::with_capacity(bib_files.len());
    for abs in bib_files {
        let rel = rel_path(&abs, &root);
        if let Ok(text) = read_file_capped(&abs, MAX_BIB_BYTES) {
            bibs.push((rel, text));
        }
    }

    let pdfs: Vec<String> = list_all_pdf_files(&root)?
        .into_iter()
        .map(|abs| rel_path(&abs, &root))
        .collect();

    Ok(collect_citations_from_vault(&bibs, &pdfs))
}

fn rel_path(abs: &Path, root: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Combine parsed BibTeX entries with vault PDF paths, pairing matches and
/// synthesizing entries for standalone PDFs.
pub fn collect_citations_from_vault(
    bib_texts: &[(String, String)],
    pdf_rel_paths: &[String],
) -> Vec<CitationEntry> {
    let mut entries_by_key: BTreeMap<String, CitationEntry> = BTreeMap::new();
    for (_, text) in bib_texts {
        for entry in parse_bibtex(text) {
            entries_by_key
                .entry(entry.key.to_ascii_lowercase())
                .or_insert(entry);
        }
    }

    let mut matched_pdfs: HashSet<usize> = HashSet::new();
    let mut entries: Vec<CitationEntry> = entries_by_key.into_values().collect();
    for entry in &mut entries {
        if let Some((idx, pdf)) = find_matching_pdf(entry, pdf_rel_paths, &matched_pdfs) {
            entry.pdf_path = Some(pdf.to_string());
            matched_pdfs.insert(idx);
        }
    }

    for (idx, pdf) in pdf_rel_paths.iter().enumerate() {
        if !matched_pdfs.contains(&idx) {
            entries.push(synthesize_pdf_citation(pdf));
        }
    }

    entries.sort_by(|a, b| {
        a.display
            .to_ascii_lowercase()
            .cmp(&b.display.to_ascii_lowercase())
            .then_with(|| a.key.to_ascii_lowercase().cmp(&b.key.to_ascii_lowercase()))
    });
    entries
}

fn find_matching_pdf<'a>(
    entry: &CitationEntry,
    pdf_paths: &'a [String],
    already_matched: &HashSet<usize>,
) -> Option<(usize, &'a str)> {
    let key_lower = entry.key.to_ascii_lowercase();
    for (idx, path) in pdf_paths.iter().enumerate() {
        if already_matched.contains(&idx) {
            continue;
        }
        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if stem == key_lower {
            return Some((idx, path.as_str()));
        }
    }

    let surname = first_author_surname(&entry.author).to_ascii_lowercase();
    let year = entry.year.as_deref()?;
    if surname.len() < 2 {
        return None;
    }
    for (idx, path) in pdf_paths.iter().enumerate() {
        if already_matched.contains(&idx) {
            continue;
        }
        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if stem.contains(&surname) && stem.contains(year) {
            return Some((idx, path.as_str()));
        }
    }
    None
}

fn synthesize_pdf_citation(pdf_path: &str) -> CitationEntry {
    let stem = Path::new(pdf_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(pdf_path)
        .trim();

    // Handle "Author - 2017 - Title" convention first.
    let dash_parts: Vec<&str> = stem.split(" - ").map(str::trim).collect();
    if dash_parts.len() >= 2
        && let Some(year) = extract_year(dash_parts[1])
    {
        let author = dash_parts[0].to_string();
        let title = if dash_parts.len() >= 3 {
            dash_parts[2..].join(" - ")
        } else {
            String::new()
        };
        let display = if author.is_empty() {
            stem.to_string()
        } else {
            format!("{author} ({year})")
        };
        return CitationEntry {
            key: stem.to_string(),
            display,
            title,
            author,
            year: Some(year),
            pdf_path: Some(pdf_path.to_string()),
        };
    }

    let year = extract_year(stem);
    let author = if let Some(ref y) = year
        && let Some(pos) = stem.find(y.as_str())
    {
        let prefix = stem[..pos].trim_matches(|c: char| !c.is_alphabetic());
        if prefix.len() >= 2 && prefix.chars().all(|c| c.is_alphabetic()) {
            capitalize_first(prefix)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let display = match (author.is_empty(), &year) {
        (false, Some(y)) => format!("{author} ({y})"),
        _ => stem.to_string(),
    };

    CitationEntry {
        key: stem.to_string(),
        display,
        title: stem.to_string(),
        author,
        year,
        pdf_path: Some(pdf_path.to_string()),
    }
}

fn capitalize_first(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = first.to_uppercase().to_string();
    out.push_str(&chars.as_str().to_ascii_lowercase());
    out
}

/// Parse BibTeX entries (`@article{key, ...}`, etc.), ignoring `@comment`, `@string`, `@preamble`.
pub fn parse_bibtex(input: &str) -> Vec<CitationEntry> {
    let mut entries = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        let type_start = i + 1;
        let mut t_end = type_start;
        while t_end < bytes.len()
            && (bytes[t_end].is_ascii_alphanumeric() || matches!(bytes[t_end], b'_' | b'-'))
        {
            t_end += 1;
        }
        let entry_type = &input[type_start..t_end];
        let mut cursor = skip_ws(input, t_end);
        let Some(open) = bytes
            .get(cursor)
            .copied()
            .filter(|&b| b == b'{' || b == b'(')
        else {
            i = t_end.max(i + 1);
            continue;
        };
        let close = if open == b'{' { b'}' } else { b')' };
        cursor += 1;

        let Some((body, next_idx)) = read_balanced_body(input, cursor, open, close) else {
            break;
        };
        i = next_idx;

        if entry_type.eq_ignore_ascii_case("comment")
            || entry_type.eq_ignore_ascii_case("string")
            || entry_type.eq_ignore_ascii_case("preamble")
        {
            continue;
        }

        if let Some(entry) = parse_bibtex_body(body) {
            entries.push(entry);
        }
    }

    entries
}

fn read_balanced_body(input: &str, start: usize, open: u8, close: u8) -> Option<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut depth = 1usize;
    let mut i = start;
    let mut in_quotes = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && in_quotes && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if b == b'"' && depth == 1 {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    return Some((&input[start..i], i + 1));
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_bibtex_body(body: &str) -> Option<CitationEntry> {
    let comma = body.find(',')?;
    let key = body[..comma].trim().to_string();
    if key.is_empty() {
        return None;
    }
    let fields = parse_bibtex_fields(&body[comma + 1..]);
    let title = fields
        .get("title")
        .map(|raw| clean_bibtex_text(raw))
        .unwrap_or_default();
    let author = fields
        .get("author")
        .map(|raw| format_authors(raw))
        .unwrap_or_default();
    let year = fields
        .get("year")
        .or_else(|| fields.get("date"))
        .and_then(|raw| extract_year(raw));

    let display = match (author.is_empty(), &year) {
        (false, Some(y)) => format!("{author} ({y})"),
        (false, None) => author.clone(),
        (true, Some(y)) => format!("{key} ({y})"),
        (true, None) => key.clone(),
    };

    Some(CitationEntry {
        key,
        display,
        title,
        author,
        year,
        pdf_path: None,
    })
}

fn parse_bibtex_fields(raw: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let bytes = raw.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        i = skip_ws(raw, i);
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b',' {
            i += 1;
            continue;
        }
        let name_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-'))
        {
            i += 1;
        }
        let field_name = raw[name_start..i].to_ascii_lowercase();
        i = skip_ws(raw, i);
        if bytes.get(i).copied() != Some(b'=') {
            while i < bytes.len() && bytes[i] != b',' {
                i += 1;
            }
            continue;
        }
        i = skip_ws(raw, i + 1);
        let (value, next) = read_field_value(raw, i);
        i = next;
        if !field_name.is_empty() {
            map.insert(field_name, value);
        }
    }

    map
}

fn read_field_value(raw: &str, mut i: usize) -> (String, usize) {
    let bytes = raw.as_bytes();
    let mut parts = Vec::new();
    loop {
        i = skip_ws(raw, i);
        if i >= bytes.len() {
            break;
        }
        match bytes[i] {
            b'{' => {
                if let Some((inner, next)) = read_balanced_body(raw, i + 1, b'{', b'}') {
                    parts.push(inner.to_string());
                    i = next;
                } else {
                    parts.push(raw[i + 1..].to_string());
                    i = bytes.len();
                    break;
                }
            }
            b'"' => {
                let (inner, next) = read_quoted_value(raw, i + 1);
                parts.push(inner);
                i = next;
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b',' | b'#') {
                    i += 1;
                }
                parts.push(raw[start..i].trim().to_string());
            }
        }
        i = skip_ws(raw, i);
        if bytes.get(i).copied() == Some(b'#') {
            i += 1;
            continue;
        }
        if bytes.get(i).copied() == Some(b',') {
            i += 1;
        }
        break;
    }
    (parts.concat(), i)
}

fn read_quoted_value(raw: &str, start: usize) -> (String, usize) {
    let bytes = raw.as_bytes();
    let mut i = start;
    let mut depth = 0usize;
    let mut out = String::new();
    while i < bytes.len() {
        let ch = raw[i..].chars().next().unwrap_or_default();
        if ch == '\\' && i + 1 < bytes.len() {
            let next_ch = raw[i + 1..].chars().next().unwrap_or_default();
            out.push(next_ch);
            i += 1 + next_ch.len_utf8();
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.saturating_sub(1);
        } else if ch == '"' && depth == 0 {
            return (out, i + 1);
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, bytes.len())
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn clean_bibtex_text(raw: &str) -> String {
    let without_braces: String = raw.chars().filter(|&c| c != '{' && c != '}').collect();
    without_braces
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_authors(raw: &str) -> String {
    let authors = split_bibtex_authors(raw);
    let surnames: Vec<String> = authors
        .into_iter()
        .map(|a| extract_surname(&a))
        .filter(|s| !s.is_empty())
        .collect();
    match surnames.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} & {second}"),
        [first, ..] => format!("{first} et al."),
    }
}

fn first_author_surname(formatted: &str) -> &str {
    formatted
        .strip_suffix(" et al.")
        .or_else(|| formatted.split(" & ").next())
        .unwrap_or(formatted)
        .trim()
}

fn split_bibtex_authors(raw: &str) -> Vec<String> {
    let mut authors = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let bytes = raw.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if depth == 0
            && i + 5 <= bytes.len()
            && bytes[i].is_ascii_whitespace()
            && bytes[i + 1..i + 4].eq_ignore_ascii_case(b"and")
            && bytes[i + 4].is_ascii_whitespace()
        {
            authors.push(current.trim().to_string());
            current.clear();
            i += 4;
            continue;
        }
        let ch = raw[i..].chars().next().unwrap_or_default();
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.saturating_sub(1);
        }
        current.push(ch);
        i += ch.len_utf8();
    }
    if !current.trim().is_empty() {
        authors.push(current.trim().to_string());
    }
    authors
}

fn extract_surname(author: &str) -> String {
    let trimmed = author.trim();
    if let Some((surname, _)) = trimmed.split_once(',') {
        return clean_bibtex_text(surname);
    }
    let clean = clean_bibtex_text(trimmed);
    clean.split_whitespace().last().unwrap_or("").to_string()
}

fn extract_year(raw: &str) -> Option<String> {
    let clean = clean_bibtex_text(raw);
    let chars: Vec<char> = clean.chars().collect();
    for window in chars.windows(4) {
        if window.iter().all(|c| c.is_ascii_digit()) {
            let year: String = window.iter().collect();
            if let Ok(num) = year.parse::<u16>()
                && (1000..=2999).contains(&num)
            {
                return Some(year);
            }
        }
    }
    None
}
