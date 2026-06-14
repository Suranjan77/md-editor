//! Last-known Markdown asset dimensions. Cached geometry lets document
//! layout start at stable heights before image/math rendering completes.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::VaultError;
use crate::migrations::{self, now};

const COMPONENT: &str = "asset_sizes";
const MIGRATIONS: &[&str] = &["CREATE TABLE asset_sizes (
        document  TEXT NOT NULL,
        asset_key TEXT NOT NULL,
        width     REAL NOT NULL CHECK (width > 0),
        height    REAL NOT NULL CHECK (height > 0),
        saved_at  INTEGER NOT NULL,
        PRIMARY KEY (document, asset_key)
    ) STRICT;"];

pub struct AssetSizeStore {
    conn: Connection,
}

impl AssetSizeStore {
    pub fn open(db_path: &Path) -> Result<Self, VaultError> {
        let mut conn = Connection::open(db_path)?;
        migrations::run(&mut conn, COMPONENT, MIGRATIONS)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self, VaultError> {
        let mut conn = Connection::open_in_memory()?;
        migrations::run(&mut conn, COMPONENT, MIGRATIONS)?;
        Ok(Self { conn })
    }

    pub fn get(&self, document: &str, asset_key: &str) -> Result<Option<(f32, f32)>, VaultError> {
        self.conn
            .query_row(
                "SELECT width, height FROM asset_sizes
                 WHERE document = ?1 AND asset_key = ?2",
                rusqlite::params![document, asset_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn put(
        &mut self,
        document: &str,
        asset_key: &str,
        width: f32,
        height: f32,
    ) -> Result<(), VaultError> {
        self.conn.execute(
            "INSERT INTO asset_sizes (document, asset_key, width, height, saved_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(document, asset_key) DO UPDATE SET
                 width = excluded.width,
                 height = excluded.height,
                 saved_at = excluded.saved_at",
            rusqlite::params![document, asset_key, width, height, now()],
        )?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32, VaultError> {
        migrations::version(&self.conn, COMPONENT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(result: Result<T, VaultError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn dimensions_survive_reopen_and_update() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = dir.path().join("sidecar.db");
        {
            let mut store = ok(AssetSizeStore::open(&path));
            ok(store.put("note.md", "image:plot.png", 640.0, 480.0));
        }
        let mut store = ok(AssetSizeStore::open(&path));
        assert_eq!(
            ok(store.get("note.md", "image:plot.png")),
            Some((640.0, 480.0))
        );
        ok(store.put("note.md", "image:plot.png", 800.0, 600.0));
        assert_eq!(
            ok(store.get("note.md", "image:plot.png")),
            Some((800.0, 600.0))
        );
        assert_eq!(ok(store.schema_version()), 1);
    }
}
