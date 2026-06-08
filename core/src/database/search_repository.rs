use rusqlite::Connection;

pub(crate) struct SearchRepository<'a> {
    db: &'a Connection,
}

impl<'a> SearchRepository<'a> {
    pub(crate) const fn new(db: &'a Connection) -> Self {
        Self { db }
    }

    pub(crate) fn clear_markdown(&self) -> Result<(), String> {
        self.db
            .execute("DELETE FROM file_search", [])
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn upsert_markdown(&self, vault_path: &str, content: &str) -> Result<(), String> {
        let transaction = self.db.unchecked_transaction().map_err(|e| e.to_string())?;
        transaction
            .execute(
                "DELETE FROM file_search WHERE path = ?1",
                rusqlite::params![vault_path],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![vault_path, content],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn delete_markdown(&self, vault_path: &str) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM file_search WHERE path = ?1",
                rusqlite::params![vault_path],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::SearchRepository;
    use crate::database::initialize;

    #[test]
    fn markdown_upsert_replaces_previous_fts_row() {
        let db = Connection::open_in_memory().expect("memory database should open");
        initialize(&db).expect("database should initialize");
        let repository = SearchRepository::new(&db);

        repository
            .upsert_markdown("notes/a.md", "first")
            .expect("first value should save");
        repository
            .upsert_markdown("notes/a.md", "second")
            .expect("second value should replace first");

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM file_search WHERE path = 'notes/a.md'",
                [],
                |row| row.get(0),
            )
            .expect("row count should load");
        let content: String = db
            .query_row(
                "SELECT content FROM file_search WHERE path = 'notes/a.md'",
                [],
                |row| row.get(0),
            )
            .expect("content should load");

        assert_eq!(count, 1);
        assert_eq!(content, "second");
    }
}
