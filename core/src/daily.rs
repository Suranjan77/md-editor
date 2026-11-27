//! Daily Desk note creation: rolls over unfinished tasks and builds a Spaced Synthesis Brief from vault callouts.

use std::fs;
use std::path::Path;

use crate::file_index::{code_ranges, in_code};
use crate::state::AppState;
use crate::vault::{
    VaultError, VaultResult, is_markdown_path, list_vault_entries, resolve_vault_path_checked,
    write_file,
};

#[cfg(test)]
mod tests;

const MAX_SYNTHESIS_ITEMS: usize = 5;
const MAX_SUMMARY_CHARS: usize = 140;

/// Result of opening or synthesizing `daily/<date>.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyNoteOutcome {
    pub path: String,
    pub created: bool,
}

/// Open `daily/<date>.md` if it exists, or synthesize it from the latest prior
/// daily note's unchecked tasks and up to 5 `[!question]` / `[!claim]` callouts.
pub fn open_or_create_daily_note(state: &AppState, date: &str) -> VaultResult<DailyNoteOutcome> {
    if !is_valid_iso_date(date) {
        return Err(VaultError::from(format!("Invalid daily note date: {date}")));
    }

    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or(VaultError::NoVault)?.clone()
    };
    let rel_path = format!("daily/{date}.md");
    let abs_path = resolve_vault_path_checked(&vault_root, &rel_path)?;

    if abs_path.is_file() {
        return Ok(DailyNoteOutcome {
            path: rel_path,
            created: false,
        });
    }
    if abs_path.exists() {
        return Err(VaultError::from(format!(
            "Path is not a file: {}",
            abs_path.display()
        )));
    }

    let content = build_daily_note_content(state, &vault_root, date)?;

    let _mutation = state.vault_mutation_guard()?;
    if abs_path.is_file() {
        return Ok(DailyNoteOutcome {
            path: rel_path,
            created: false,
        });
    }
    write_file(&abs_path, &content)?;

    {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, &content);
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let _ = db.execute(
        "DELETE FROM file_search WHERE path = ?1",
        rusqlite::params![rel_path],
    );
    if let Err(e) = db.execute(
        "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
        rusqlite::params![rel_path, content],
    ) {
        log::warn!("Failed to update search index for {rel_path}: {e}");
    }

    Ok(DailyNoteOutcome {
        path: rel_path,
        created: true,
    })
}

fn build_daily_note_content(
    state: &AppState,
    vault_root: &Path,
    date: &str,
) -> VaultResult<String> {
    let mut entries = list_vault_entries(vault_root)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let prior_tasks = latest_prior_daily_path(&entries, date)
        .and_then(|rel| fs::read_to_string(vault_root.join(rel)).ok())
        .map(|text| extract_unchecked_tasks(&text))
        .unwrap_or_default();

    let mut synthesis = collect_synthesis_callouts(vault_root, &entries);
    if let Ok(sessions) = crate::tracker::get_sessions(state)
        && let Some(line) = last_study_day_line(&sessions, date)
    {
        synthesis.push(line);
    }

    let mut out = format!("# {date}\n\n## Tasks\n");
    if prior_tasks.is_empty() {
        out.push_str("- [ ] \n");
    } else {
        for task in &prior_tasks {
            out.push_str(task);
            out.push('\n');
        }
    }

    if !synthesis.is_empty() {
        out.push_str("\n## Synthesis Brief\n");
        for item in &synthesis {
            out.push_str(item);
            out.push('\n');
        }
    }

    out.push_str("\n## Notes\n\n");
    Ok(out)
}

/// The brief's study line: the most recent day before `date` with logged
/// sessions, and how much was logged that day. Session dates are
/// `YYYY-MM-DD`, optionally followed by a time.
fn last_study_day_line(sessions: &[crate::tracker::StudySession], date: &str) -> Option<String> {
    let day_of = |s: &crate::tracker::StudySession| s.date.get(..10).map(str::to_string);
    let last_day = sessions
        .iter()
        .filter_map(day_of)
        .filter(|day| is_valid_iso_date(day) && day.as_str() < date)
        .max()?;
    let on_day: Vec<_> = sessions
        .iter()
        .filter(|s| day_of(s).as_deref() == Some(last_day.as_str()))
        .collect();
    let hours: f32 = on_day.iter().map(|s| s.hours).sum();
    let count = on_day.len();
    let plural = if count == 1 { "" } else { "s" };
    Some(format!(
        "- **Study Log**: {last_day}: {count} session{plural}, {hours:.1}h"
    ))
}

fn latest_prior_daily_path<'a>(
    entries: &'a [crate::types::FileEntry],
    target_date: &str,
) -> Option<&'a str> {
    entries
        .iter()
        .filter(|e| !e.is_dir && e.path.starts_with("daily/"))
        .filter_map(|e| {
            let p = Path::new(&e.path);
            if !is_markdown_path(p) {
                return None;
            }
            let stem = p.file_stem()?.to_str()?;
            (is_valid_iso_date(stem) && stem < target_date).then_some((stem, e.path.as_str()))
        })
        .max_by(|a, b| a.0.cmp(b.0))
        .map(|(_, path)| path)
}

fn extract_unchecked_tasks(text: &str) -> Vec<String> {
    let code = code_ranges(text);
    let mut tasks = Vec::new();
    let mut offset = 0;
    for raw_line in text.split_inclusive('\n') {
        let start = offset;
        offset += raw_line.len();
        if in_code(&code, start) {
            continue;
        }
        let line = raw_line.trim_end_matches(['\r', '\n']).trim_start();
        let rest = line
            .strip_prefix("- [ ]")
            .or_else(|| line.strip_prefix("* [ ]"));
        if let Some(after) = rest
            && (after.is_empty() || after.starts_with(' ') || after.starts_with('\t'))
        {
            let body = after.trim();
            if !body.is_empty() {
                tasks.push(format!("- [ ] {body}"));
            }
        }
    }
    tasks
}

fn collect_synthesis_callouts(
    vault_root: &Path,
    entries: &[crate::types::FileEntry],
) -> Vec<String> {
    let mut items = Vec::new();
    for entry in entries {
        if entry.is_dir || entry.path.starts_with("daily/") {
            continue;
        }
        let abs = vault_root.join(&entry.path);
        if !is_markdown_path(&abs) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&abs) else {
            continue;
        };
        let target = wikilink_target(&entry.path);
        let stem = abs
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| entry.path.clone());
        for (kind, summary) in extract_callouts(&content) {
            items.push(format!("- **{kind}** in [[{target}|{stem}]]: {summary}"));
            if items.len() >= MAX_SYNTHESIS_ITEMS {
                return items;
            }
        }
    }
    items
}

fn extract_callouts(content: &str) -> Vec<(&'static str, String)> {
    let code = code_ranges(content);
    let lines: Vec<(usize, &str)> = {
        let mut out = Vec::new();
        let mut offset = 0;
        for raw in content.split_inclusive('\n') {
            out.push((offset, raw.trim_end_matches(['\r', '\n'])));
            offset += raw.len();
        }
        out
    };

    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (start, line) = lines[i];
        if in_code(&code, start) {
            i += 1;
            continue;
        }
        let Some((label, inline_summary)) = parse_callout_header(line) else {
            i += 1;
            continue;
        };
        let mut summary = inline_summary;
        if summary.is_empty() {
            let mut j = i + 1;
            while j < lines.len() {
                let (next_start, next_line) = lines[j];
                if in_code(&code, next_start) {
                    break;
                }
                let Some(quoted) = next_line.trim_start().strip_prefix('>') else {
                    break;
                };
                let trimmed = quoted.trim();
                if !trimmed.is_empty() {
                    summary = trimmed.to_string();
                    break;
                }
                j += 1;
            }
        }
        if !summary.is_empty() {
            found.push((label, truncate_chars(&summary, MAX_SUMMARY_CHARS)));
        }
        i += 1;
    }
    found
}

fn parse_callout_header(line: &str) -> Option<(&'static str, String)> {
    let inner = line.trim_start().strip_prefix('>')?.trim_start();
    let after_open = inner.strip_prefix("[!")?;
    let close = after_open.find(']')?;
    let kind = after_open[..close].trim();
    let label = if kind.eq_ignore_ascii_case("question") {
        "Question"
    } else if kind.eq_ignore_ascii_case("claim") {
        "Claim"
    } else {
        return None;
    };
    let rest = after_open[close + 1..].trim();
    Some((label, rest.to_string()))
}

fn wikilink_target(rel_path: &str) -> String {
    rel_path
        .strip_suffix(".md")
        .map(str::to_string)
        .unwrap_or_else(|| rel_path.to_string())
}

fn truncate_chars(text: &str, max: usize) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max {
        normalized
    } else {
        let kept: String = normalized.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}

fn is_valid_iso_date(date: &str) -> bool {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    if !digits(0..4) || !digits(5..7) || !digits(8..10) {
        return false;
    }
    let month: u8 = date[5..7].parse().unwrap_or(0);
    let day: u8 = date[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}
