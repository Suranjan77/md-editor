//! Shared sidecar migration runner: numbered, transactional, append-only
//! ladders keyed by *component* in one `migrations` table, so every sidecar
//! citizen (annotations, sessions, future FTS schema changes) can evolve its
//! tables independently in the same database file.
//!
//! Editing a shipped migration is a bug — existing sidecars recorded its
//! version, not its content. Add a new numbered entry instead (each
//! component's test pins its shipped prefix).

use std::time::UNIX_EPOCH;

use rusqlite::Connection;

use crate::error::VaultError;

/// Apply `migrations[version..]` for `component`, recording each step.
/// Idempotent: already-recorded versions are skipped.
pub fn run(conn: &mut Connection, component: &str, migrations: &[&str]) -> Result<(), VaultError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS migrations (
            component  TEXT NOT NULL,
            version    INTEGER NOT NULL,
            applied_at INTEGER NOT NULL,
            PRIMARY KEY (component, version)
        ) STRICT;",
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM migrations WHERE component = ?1",
        [component],
        |r| r.get(0),
    )?;
    for (i, sql) in migrations.iter().enumerate() {
        let version = (i + 1) as i64;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO migrations (component, version, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![component, version, now()],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// Highest applied version for `component` (0 when none).
pub fn version(conn: &Connection, component: &str) -> Result<u32, VaultError> {
    let v: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM migrations WHERE component = ?1",
        [component],
        |r| r.get(0),
    )?;
    Ok(v as u32)
}

/// Unix seconds; also used by sidecar stores for row timestamps.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
