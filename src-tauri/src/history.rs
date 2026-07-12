use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct HistoryStore(Mutex<Connection>);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dictation {
    pub id: i64,
    pub ts: i64,
    pub raw: String,
    pub clean: String,
    pub language: Option<String>,
    pub duration_ms: i64,
}

fn row_to_dictation(row: &rusqlite::Row) -> rusqlite::Result<Dictation> {
    Ok(Dictation {
        id: row.get(0)?,
        ts: row.get(1)?,
        raw: row.get(2)?,
        clean: row.get(3)?,
        language: row.get(4)?,
        duration_ms: row.get(5)?,
    })
}

impl HistoryStore {
    /// Opens (creating if absent) the SQLite DB at `path` and ensures the
    /// `dictations` table exists.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dictations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                raw TEXT NOT NULL,
                clean TEXT NOT NULL,
                language TEXT,
                duration_ms INTEGER NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    /// Opens `path`, recovering from a corrupt DB instead of bricking the app:
    /// on open/schema failure, rename the bad file aside and retry once; if
    /// that also fails, fall back to a non-persistent in-memory store so
    /// dictation still works for the rest of the session.
    pub fn open_or_recover(path: &Path) -> Self {
        if let Ok(store) = Self::open(path) {
            return store;
        }
        let corrupt_path = path.with_extension("sqlite.corrupt");
        let _ = std::fs::rename(path, &corrupt_path);
        if let Ok(store) = Self::open(path) {
            eprintln!(
                "history db was corrupt, moved to {} and reopened",
                corrupt_path.display()
            );
            return store;
        }
        eprintln!(
            "history db unrecoverable at {}, falling back to in-memory (not persisted)",
            path.display()
        );
        let conn = Connection::open_in_memory().expect("open in-memory sqlite connection");
        Self::init_schema(&conn).expect("init in-memory schema");
        Self(Mutex::new(conn))
    }

    pub fn insert(
        &self,
        raw: &str,
        clean: &str,
        language: Option<&str>,
        duration_ms: i64,
    ) -> rusqlite::Result<i64> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO dictations (ts, raw, clean, language, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ts, raw, clean, language, duration_ms],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Newest first. `search` (if any) matches case-insensitively against
    /// either `raw` or `clean`.
    pub fn list(&self, search: Option<&str>, limit: u32) -> rusqlite::Result<Vec<Dictation>> {
        let conn = self.0.lock().unwrap();
        match search {
            Some(q) => {
                let mut stmt = conn.prepare(
                    "SELECT id, ts, raw, clean, language, duration_ms FROM dictations
                     WHERE LOWER(raw) LIKE '%'||LOWER(?1)||'%' OR LOWER(clean) LIKE '%'||LOWER(?1)||'%'
                     ORDER BY id DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![q, limit], row_to_dictation)?.collect();
                rows
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, ts, raw, clean, language, duration_ms FROM dictations
                     ORDER BY id DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit], row_to_dictation)?.collect();
                rows
            }
        }
    }

    pub fn delete(&self, id: i64) -> rusqlite::Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("DELETE FROM dictations WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("DELETE FROM dictations", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, HistoryStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::open(&dir.path().join("history.sqlite")).unwrap();
        (dir, store)
    }

    #[test]
    fn open_or_recover_replaces_corrupt_db_and_still_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        std::fs::write(&path, b"not a sqlite file").unwrap();

        let store = HistoryStore::open_or_recover(&path);
        store.insert("hello", "Hello.", None, 100).unwrap();
        let items = store.list(None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].raw, "hello");

        assert!(dir.path().join("history.sqlite.corrupt").exists());
        assert!(Connection::open(&path).is_ok());
    }

    #[test]
    fn insert_then_list_roundtrip() {
        let (_dir, store) = store();
        let id = store.insert("ahoj svet", "Ahoj svet.", Some("sk"), 1234).unwrap();
        let items = store.list(None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].raw, "ahoj svet");
        assert_eq!(items[0].clean, "Ahoj svet.");
        assert_eq!(items[0].language.as_deref(), Some("sk"));
        assert_eq!(items[0].duration_ms, 1234);
        assert!(items[0].ts > 0);
    }

    #[test]
    fn search_matches_raw_case_insensitively() {
        let (_dir, store) = store();
        store.insert("Hello World", "hello world.", None, 100).unwrap();
        store.insert("something else", "something else.", None, 100).unwrap();
        let items = store.list(Some("HELLO"), 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].raw, "Hello World");
    }

    #[test]
    fn search_matches_clean_case_insensitively() {
        let (_dir, store) = store();
        store.insert("raw text one", "Cleaned Text.", None, 100).unwrap();
        store.insert("raw text two", "other output.", None, 100).unwrap();
        let items = store.list(Some("cleaned"), 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].clean, "Cleaned Text.");
    }

    #[test]
    fn delete_removes_only_that_row() {
        let (_dir, store) = store();
        let id1 = store.insert("one", "one.", None, 100).unwrap();
        let id2 = store.insert("two", "two.", None, 100).unwrap();
        store.delete(id1).unwrap();
        let items = store.list(None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id2);
    }

    #[test]
    fn clear_empties_the_table() {
        let (_dir, store) = store();
        store.insert("one", "one.", None, 100).unwrap();
        store.insert("two", "two.", None, 100).unwrap();
        store.clear().unwrap();
        assert!(store.list(None, 10).unwrap().is_empty());
    }

    #[test]
    fn list_respects_limit_and_orders_newest_first() {
        let (_dir, store) = store();
        for i in 0..5 {
            store.insert(&format!("item {i}"), &format!("item {i}."), None, 100).unwrap();
        }
        let items = store.list(None, 2).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].raw, "item 4");
        assert_eq!(items[1].raw, "item 3");
    }
}
