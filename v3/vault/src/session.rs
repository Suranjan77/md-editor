//! Workspace session persistence (plan §3.4 "sessions" / §5 M2 "session
//! restore"): one row in the sidecar holding the shell's serialized session
//! snapshot. The blob is *opaque* here — the shell owns the snapshot shape
//! (pane tree, view state) exactly like it owns keymap-file parsing; the
//! vault owns durability, migrations, and the single-sidecar guarantee
//! (plan pillar 5: one SQLite sidecar, not a sprawl of dotfiles).

use std::path::Path;

use rusqlite::Connection;

use crate::error::VaultError;
use crate::migrations::{self, now};

const COMPONENT: &str = "session";
const MIGRATIONS: &[&str] = &[
    // 1: a single-row table — a workspace has one current session.
    "CREATE TABLE session_state (
        id       INTEGER PRIMARY KEY CHECK (id = 1),
        json     TEXT NOT NULL,
        saved_at INTEGER NOT NULL
    ) STRICT;",
];

pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Open (or create) on `db_path` — the vault sidecar, shared with the
    /// FTS index and annotation store (disjoint tables, shared ladder).
    pub fn open(db_path: &Path) -> Result<SessionStore, VaultError> {
        let mut conn = Connection::open(db_path)?;
        migrations::run(&mut conn, COMPONENT, MIGRATIONS)?;
        Ok(SessionStore { conn })
    }

    pub fn open_in_memory() -> Result<SessionStore, VaultError> {
        let mut conn = Connection::open_in_memory()?;
        migrations::run(&mut conn, COMPONENT, MIGRATIONS)?;
        Ok(SessionStore { conn })
    }

    pub fn schema_version(&self) -> Result<u32, VaultError> {
        migrations::version(&self.conn, COMPONENT)
    }

    /// Replace the saved session with `json`.
    pub fn save(&mut self, json: &str) -> Result<(), VaultError> {
        self.conn.execute(
            "INSERT INTO session_state (id, json, saved_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET json = ?1, saved_at = ?2",
            rusqlite::params![json, now()],
        )?;
        Ok(())
    }

    /// The saved session, if any.
    pub fn load(&self) -> Result<Option<String>, VaultError> {
        let mut stmt = self
            .conn
            .prepare("SELECT json FROM session_state WHERE id = 1")?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Forget the saved session (e.g. after a restore fails irrecoverably).
    pub fn clear(&mut self) -> Result<(), VaultError> {
        self.conn.execute("DELETE FROM session_state", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(r: Result<T, VaultError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn save_load_round_trip_and_overwrite() {
        let mut store = ok(SessionStore::open_in_memory());
        assert_eq!(ok(store.load()), None, "fresh store has no session");
        ok(store.save(r#"{"layout":1}"#));
        assert_eq!(ok(store.load()).as_deref(), Some(r#"{"layout":1}"#));
        ok(store.save(r#"{"layout":2}"#));
        assert_eq!(
            ok(store.load()).as_deref(),
            Some(r#"{"layout":2}"#),
            "save replaces — one current session per workspace"
        );
        ok(store.clear());
        assert_eq!(ok(store.load()), None);
    }

    #[test]
    fn session_persists_across_reopen_and_shares_the_sidecar() {
        let dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        };
        let db = dir.path().join("sidecar.db");
        {
            let mut store = ok(SessionStore::open(&db));
            ok(store.save("persisted"));
        }
        // Cohabitation: the annotation store opens the same file and runs
        // its own ladder without disturbing the session component.
        let annotations = ok(crate::AnnotationStore::open(&db));
        assert!(ok(annotations.schema_version()) >= 1);

        let store = ok(SessionStore::open(&db));
        assert_eq!(ok(store.load()).as_deref(), Some("persisted"));
        assert_eq!(ok(store.schema_version()), MIGRATIONS.len() as u32);
    }

    #[test]
    fn migration_ladder_is_append_only() {
        // Same discipline as annotations: the shipped prefix is frozen.
        let fingerprint: usize = MIGRATIONS
            .iter()
            .take(1)
            .map(|m| m.split_whitespace().collect::<String>().len())
            .sum();
        assert_eq!(fingerprint, 102, "shipped migrations must not change");
    }
}
