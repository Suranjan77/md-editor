//! SQLite FTS5 full-text index (plan §3.4): incremental by `(mtime, size)`
//! diff — a cold start over an unchanged vault re-reads *nothing* — and
//! targeted re-sync for watcher batches. Paths are stored relative to the
//! vault root so the sidecar survives a vault move.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::Connection;

use crate::error::VaultError;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
    path     TEXT PRIMARY KEY,
    mtime_ns INTEGER NOT NULL,
    size     INTEGER NOT NULL
) STRICT;
CREATE VIRTUAL TABLE IF NOT EXISTS notes USING fts5(path UNINDEXED, body);
";

/// What a sync pass did — the cheap-cold-start guarantee is testable:
/// a second pass over an unchanged vault must report all-unchanged.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub indexed: usize,
    pub removed: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Path relative to the vault root.
    pub path: PathBuf,
    /// Match context with `[` `]` around matched terms.
    pub snippet: String,
}

pub struct SearchIndex {
    conn: Connection,
}

impl SearchIndex {
    /// Open (or create) the index at `db_path` — typically the vault's
    /// sidecar database.
    pub fn open(db_path: &Path) -> Result<SearchIndex, VaultError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(SearchIndex { conn })
    }

    pub fn open_in_memory() -> Result<SearchIndex, VaultError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(SearchIndex { conn })
    }

    /// Full incremental sync: walk `*.md` under `root`, (re)index files
    /// whose `(mtime, size)` changed, drop rows for deleted files.
    pub fn sync(&mut self, root: &Path) -> Result<SyncReport, VaultError> {
        let mut on_disk = Vec::new();
        walk_markdown(root, root, &mut on_disk)?;
        let mut report = SyncReport::default();

        let mut known: HashMap<String, (i64, i64)> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT path, mtime_ns, size FROM files")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?;
            for row in rows {
                let (path, meta) = row?;
                known.insert(path, meta);
            }
        }

        let tx = self.conn.transaction()?;
        for (rel, mtime_ns, size) in &on_disk {
            match known.remove(rel) {
                Some(meta) if meta == (*mtime_ns, *size) => report.unchanged += 1,
                _ => {
                    let body = std::fs::read_to_string(root.join(rel))
                        .map_err(|e| VaultError::io(root.join(rel), e))?;
                    upsert(&tx, rel, *mtime_ns, *size, &body)?;
                    report.indexed += 1;
                }
            }
        }
        // Anything left in `known` no longer exists on disk.
        for rel in known.keys() {
            remove(&tx, rel)?;
            report.removed += 1;
        }
        tx.commit()?;
        Ok(report)
    }

    /// Targeted sync for a watcher batch: each path is re-read if present,
    /// de-indexed if gone. Paths may be absolute (under `root`) or relative.
    pub fn sync_paths(&mut self, root: &Path, paths: &[PathBuf]) -> Result<SyncReport, VaultError> {
        let mut report = SyncReport::default();
        let tx = self.conn.transaction()?;
        for path in paths {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            };
            let Ok(rel) = abs.strip_prefix(root) else {
                continue; // outside the vault — not ours
            };
            if !is_markdown(&abs) {
                continue;
            }
            let rel_str = rel.to_string_lossy().to_string();
            match std::fs::metadata(&abs) {
                Ok(meta) => {
                    let body =
                        std::fs::read_to_string(&abs).map_err(|e| VaultError::io(&abs, e))?;
                    upsert(&tx, &rel_str, mtime_ns(&meta), meta.len() as i64, &body)?;
                    report.indexed += 1;
                }
                Err(_) => {
                    remove(&tx, &rel_str)?;
                    report.removed += 1;
                }
            }
        }
        tx.commit()?;
        Ok(report)
    }

    /// FTS5 search. The user query is tokenized and quoted (`"tok"*`), so
    /// FTS syntax characters can't produce errors or injections. Every
    /// token is a literal prefix term and all must match (implicit AND);
    /// `AND`/`OR`/`NEAR` are ordinary words, never operators.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, VaultError> {
        let fts_query: Vec<String> = query
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
            .collect();
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT path, snippet(notes, 1, '[', ']', '…', 10)
             FROM notes WHERE notes MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![fts_query.join(" "), limit as i64], |r| {
            Ok(Hit {
                path: PathBuf::from(r.get::<_, String>(0)?),
                snippet: r.get(1)?,
            })
        })?;
        let mut hits = Vec::new();
        for row in rows {
            hits.push(row?);
        }
        Ok(hits)
    }

    pub fn indexed_count(&self) -> Result<usize, VaultError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}

fn upsert(
    conn: &Connection,
    rel: &str,
    mtime_ns: i64,
    size: i64,
    body: &str,
) -> Result<(), VaultError> {
    conn.execute("DELETE FROM notes WHERE path = ?1", [rel])?;
    conn.execute(
        "INSERT INTO notes (path, body) VALUES (?1, ?2)",
        rusqlite::params![rel, body],
    )?;
    conn.execute(
        "INSERT INTO files (path, mtime_ns, size) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET mtime_ns = ?2, size = ?3",
        rusqlite::params![rel, mtime_ns, size],
    )?;
    Ok(())
}

fn remove(conn: &Connection, rel: &str) -> Result<(), VaultError> {
    conn.execute("DELETE FROM notes WHERE path = ?1", [rel])?;
    conn.execute("DELETE FROM files WHERE path = ?1", [rel])?;
    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md")
}

fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

fn walk_markdown(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, i64, i64)>,
) -> Result<(), VaultError> {
    let entries = std::fs::read_dir(dir).map_err(|e| VaultError::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| VaultError::io(dir, e))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue; // .git, .trash, sidecars
        }
        let meta = entry.metadata().map_err(|e| VaultError::io(&path, e))?;
        if meta.is_dir() {
            walk_markdown(root, &path, out)?;
        } else if is_markdown(&path)
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push((
                rel.to_string_lossy().to_string(),
                mtime_ns(&meta),
                meta.len() as i64,
            ));
        }
    }
    Ok(())
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

    fn vault() -> tempfile::TempDir {
        match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        }
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, body) {
            panic!("write {rel}: {e}");
        }
    }

    #[test]
    fn index_search_and_incremental_resync() {
        let dir = vault();
        let root = dir.path();
        write(root, "alpha.md", "the quick brown fox");
        write(root, "sub/beta.md", "lazy dogs sleep deeply");
        write(root, "ignored.txt", "quick but not markdown");

        let mut index = ok(SearchIndex::open_in_memory());
        let first = ok(index.sync(root));
        assert_eq!(first.indexed, 2);
        assert_eq!(ok(index.indexed_count()), 2);

        let hits = ok(index.search("quick", 10));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, PathBuf::from("alpha.md"));
        assert!(hits[0].snippet.contains("[quick]"), "{}", hits[0].snippet);

        // Cold-start guarantee: nothing changed → nothing re-read.
        let second = ok(index.sync(root));
        assert_eq!(
            second,
            SyncReport {
                indexed: 0,
                removed: 0,
                unchanged: 2
            }
        );

        // Deletion is noticed.
        let _ = std::fs::remove_file(root.join("alpha.md"));
        let third = ok(index.sync(root));
        assert_eq!(third.removed, 1);
        assert!(ok(index.search("quick", 10)).is_empty());
    }

    #[test]
    fn prefix_search_and_fts_syntax_is_inert() {
        let dir = vault();
        let root = dir.path();
        write(root, "n.md", "incremental parsers converge");
        write(root, "ops.md", "parsers and operators");
        let mut index = ok(SearchIndex::open_in_memory());
        ok(index.sync(root));
        assert_eq!(ok(index.search("increm", 10)).len(), 1, "prefix matches");
        // FTS operators in user input must not error — they are literal
        // words. `AND` therefore *requires* the word "and" in the body.
        assert!(ok(index.search("NEAR(", 10)).is_empty());
        assert!(ok(index.search("\"unbalanced", 10)).is_empty());
        let hits = ok(index.search("parsers AND", 10));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, PathBuf::from("ops.md"));
    }

    #[test]
    fn targeted_sync_handles_change_and_removal() {
        let dir = vault();
        let root = dir.path();
        write(root, "a.md", "original text");
        let mut index = ok(SearchIndex::open_in_memory());
        ok(index.sync(root));

        write(root, "a.md", "replacement words");
        let report = ok(index.sync_paths(root, &[root.join("a.md")]));
        assert_eq!(report.indexed, 1);
        assert!(ok(index.search("original", 10)).is_empty());
        assert_eq!(ok(index.search("replacement", 10)).len(), 1);

        let _ = std::fs::remove_file(root.join("a.md"));
        let report = ok(index.sync_paths(root, &[root.join("a.md")]));
        assert_eq!(report.removed, 1);
        assert_eq!(ok(index.indexed_count()), 0);
    }
}
