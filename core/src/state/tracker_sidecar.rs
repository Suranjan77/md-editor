//! Portable Study Tracker sessions: `<vault>/.md-editor/study-sessions.jsonl`, three-way merged with SQLite.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::pdf_sidecar::{SIDECAR_DIR, ensure_gitattributes};
use super::{AppState, StateResult};
use crate::tracker::StudySession;

#[cfg(test)]
mod tests;

const SESSIONS_FILE: &str = "study-sessions.jsonl";

/// Portable JSONL representation of a study session (no local SQLite `id`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct SidecarSession {
    uid: String,
    date: String,
    hours: f32,
    activity_type: String,
    phase: String,
    notes: Option<String>,
    #[serde(default)]
    updated_at: i64,
}

impl From<&StudySession> for SidecarSession {
    fn from(s: &StudySession) -> Self {
        Self {
            uid: s.uid.clone(),
            date: s.date.clone(),
            hours: s.hours,
            activity_type: s.activity_type.clone(),
            phase: s.phase.clone(),
            notes: s.notes.clone(),
            updated_at: s.updated_at,
        }
    }
}

impl From<SidecarSession> for StudySession {
    fn from(s: SidecarSession) -> Self {
        Self {
            id: 0,
            uid: s.uid,
            date: s.date,
            hours: s.hours,
            activity_type: s.activity_type,
            phase: s.phase,
            notes: s.notes,
            updated_at: s.updated_at,
        }
    }
}

struct SyncRecord {
    content_hash: String,
    ids: BTreeSet<String>,
}

/// Path to the portable Study Tracker JSONL file inside `vault_root`.
pub fn sidecar_path(vault_root: &Path) -> PathBuf {
    vault_root.join(SIDECAR_DIR).join(SESSIONS_FILE)
}

impl AppState {
    /// Reconcile `<vault>/.md-editor/study-sessions.jsonl` with SQLite `tracker_sessions`.
    /// Returns `true` if SQLite's sessions changed.
    pub fn sync_tracker_sidecar(&self) -> StateResult<bool> {
        let Some(root) = self.vault_root.lock()?.clone() else {
            return Ok(false);
        };
        let scope = Self::pdf_scope_prefix_for_root(&root);
        let path = sidecar_path(&root);
        let local = crate::tracker::get_sessions(self)?;
        let local_text = serialize(&local);

        let file = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !local.is_empty() {
                    self.write_tracker_sidecar(&root, &scope, &local)?;
                }
                return Ok(false);
            }
            Err(e) => return Err(format!("Failed to read {}: {e}", path.display()).into()),
        };

        let record = self.tracker_sync_record(&scope)?;
        if file == local_text {
            if record
                .as_ref()
                .is_none_or(|r| r.content_hash != hash(&file))
            {
                self.put_tracker_sync_record(&scope, &file, &local)?;
            }
            return Ok(false);
        }
        if record
            .as_ref()
            .is_some_and(|r| r.content_hash == hash(&file))
        {
            // Only SQLite changed since last sync.
            self.write_tracker_sidecar(&root, &scope, &local)?;
            return Ok(false);
        }

        let remote = parse(&file);
        let merged = merge(&local, &remote, record.as_ref().map(|r| &r.ids));
        let changed = serialize(&merged) != local_text;
        if changed {
            self.replace_tracker_sessions(&scope, &merged)?;
            log::info!(
                "Merged study sessions from {}: {} here, {} in file, {} now",
                path.display(),
                local.len(),
                remote.len(),
                merged.len()
            );
        }
        self.write_tracker_sidecar(&root, &scope, &merged)?;
        Ok(changed)
    }

    /// Best-effort rewrite of `<vault>/.md-editor/study-sessions.jsonl` after a local save/delete.
    pub(crate) fn export_tracker_sidecar(&self) {
        let result = (|| -> StateResult<()> {
            let Some(root) = self.vault_root.lock()?.clone() else {
                return Ok(());
            };
            let scope = Self::pdf_scope_prefix_for_root(&root);
            let local = crate::tracker::get_sessions(self)?;
            let path = sidecar_path(&root);
            if local.is_empty() && !path.exists() {
                return Ok(());
            }
            self.write_tracker_sidecar(&root, &scope, &local)
        })();
        if let Err(e) = result {
            log::warn!("Could not write study-sessions.jsonl: {e}");
        }
    }

    fn write_tracker_sidecar(
        &self,
        root: &Path,
        scope: &str,
        sessions: &[StudySession],
    ) -> StateResult<()> {
        let text = serialize(sessions);
        let path = sidecar_path(root);
        let unchanged = std::fs::read_to_string(&path).is_ok_and(|current| current == text);
        if !unchanged {
            crate::vault::write_file(&path, &text)
                .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        }
        ensure_gitattributes(root);
        self.put_tracker_sync_record(scope, &text, sessions)
    }

    fn tracker_sync_record(&self, scope: &str) -> StateResult<Option<SyncRecord>> {
        let db = self.db.lock()?;
        let row: Option<(String, String)> = db
            .query_row(
                "SELECT content_hash, ids_json FROM tracker_sidecar_sync WHERE vault_scope = ?1",
                [scope],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(row.map(|(content_hash, ids_json)| SyncRecord {
            content_hash,
            ids: serde_json::from_str(&ids_json).unwrap_or_default(),
        }))
    }

    fn put_tracker_sync_record(
        &self,
        scope: &str,
        text: &str,
        sessions: &[StudySession],
    ) -> StateResult<()> {
        let ids: BTreeSet<&str> = sessions.iter().map(|s| s.uid.as_str()).collect();
        let ids_json =
            serde_json::to_string(&ids).map_err(|e| format!("Failed to serialize uids: {e}"))?;
        let db = self.db.lock()?;
        db.execute(
            "INSERT INTO tracker_sidecar_sync (vault_scope, content_hash, ids_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(vault_scope) DO UPDATE SET
                content_hash = excluded.content_hash,
                ids_json = excluded.ids_json",
            rusqlite::params![scope, hash(text), ids_json],
        )?;
        Ok(())
    }

    fn replace_tracker_sessions(&self, scope: &str, sessions: &[StudySession]) -> StateResult<()> {
        let db = self.db.lock()?;
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM tracker_sessions WHERE vault_scope = ?1",
            [scope],
        )?;
        for s in sessions {
            tx.execute(
                "INSERT INTO tracker_sessions
                    (date, hours, activity_type, phase, notes, uid, updated_at, vault_scope)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    s.date,
                    s.hours,
                    s.activity_type,
                    s.phase,
                    s.notes,
                    s.uid,
                    s.updated_at,
                    scope,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn serialize(sessions: &[StudySession]) -> String {
    let mut sorted: Vec<&StudySession> = sessions.iter().collect();
    sorted.sort_by(|a, b| (&a.date, &a.uid).cmp(&(&b.date, &b.uid)));
    let mut out = String::new();
    for s in sorted {
        let entry = SidecarSession::from(s);
        match serde_json::to_string(&entry) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            Err(e) => log::warn!("Could not serialize study session {}: {e}", s.uid),
        }
    }
    out
}

fn parse(text: &str) -> Vec<StudySession> {
    let mut by_uid: BTreeMap<String, StudySession> = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<SidecarSession>(line) {
            Ok(item) if !item.uid.trim().is_empty() => {
                keep_later(&mut by_uid, item.into());
            }
            Ok(_) => log::warn!("Skipping study-sessions.jsonl line {}: empty uid", n + 1),
            Err(e) => log::warn!(
                "Skipping malformed study-sessions.jsonl line {}: {e}",
                n + 1
            ),
        }
    }
    by_uid.into_values().collect()
}

fn keep_later(by_uid: &mut BTreeMap<String, StudySession>, session: StudySession) {
    match by_uid.get(&session.uid) {
        Some(existing) if existing.updated_at >= session.updated_at => {}
        _ => {
            by_uid.insert(session.uid.clone(), session);
        }
    }
}

/// Three-way merge of local SQLite sessions with the sidecar file against `base` uids.
/// File wins ties on `updated_at`.
fn merge(
    local: &[StudySession],
    remote: &[StudySession],
    base: Option<&BTreeSet<String>>,
) -> Vec<StudySession> {
    let in_base = |uid: &str| base.is_some_and(|b| b.contains(uid));
    let local_uids: BTreeSet<&str> = local.iter().map(|s| s.uid.as_str()).collect();
    let remote_uids: BTreeSet<&str> = remote.iter().map(|s| s.uid.as_str()).collect();
    let mut by_uid = BTreeMap::new();
    for s in remote {
        if local_uids.contains(s.uid.as_str()) || !in_base(&s.uid) {
            keep_later(&mut by_uid, s.clone());
        }
    }
    for s in local {
        if remote_uids.contains(s.uid.as_str()) || !in_base(&s.uid) {
            keep_later(&mut by_uid, s.clone());
        }
    }
    by_uid.into_values().collect()
}
