use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct HistoryStore(Mutex<Connection>);

/// Row written the moment recording stops, before any network call — audio is
/// on disk but there's no transcript yet.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_DONE: &str = "done";
/// Transcription gave up (rate limit, network, no key). Audio is kept until
/// the user deletes the row — retention never touches these.
pub const STATUS_FAILED: &str = "failed";

const COLUMNS: &str = "id, ts, raw, clean, language, duration_ms, status, audio_path, error";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dictation {
    pub id: i64,
    pub ts: i64,
    pub raw: String,
    pub clean: String,
    pub language: Option<String>,
    pub duration_ms: i64,
    pub status: String,
    /// File name (not a full path) inside the app's `audio/` directory.
    pub audio_path: Option<String>,
    pub error: Option<String>,
}

fn row_to_dictation(row: &rusqlite::Row) -> rusqlite::Result<Dictation> {
    Ok(Dictation {
        id: row.get(0)?,
        ts: row.get(1)?,
        raw: row.get(2)?,
        clean: row.get(3)?,
        language: row.get(4)?,
        duration_ms: row.get(5)?,
        status: row.get(6)?,
        audio_path: row.get(7)?,
        error: row.get(8)?,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

impl HistoryStore {
    /// Opens (creating if absent) the SQLite DB at `path` and ensures the
    /// `dictations` table exists and is up to date.
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
                duration_ms INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'done',
                audio_path TEXT,
                error TEXT
            )",
            [],
        )?;
        Self::migrate(conn)
    }

    /// Adds columns a pre-audio database is missing. Existing rows predate
    /// audio capture, so they default to `done` with no audio file.
    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        let existing: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(dictations)")?;
            let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
            names.collect::<rusqlite::Result<_>>()?
        };
        for (col, ddl) in [
            ("status", "ALTER TABLE dictations ADD COLUMN status TEXT NOT NULL DEFAULT 'done'"),
            ("audio_path", "ALTER TABLE dictations ADD COLUMN audio_path TEXT"),
            ("error", "ALTER TABLE dictations ADD COLUMN error TEXT"),
        ] {
            if !existing.contains(col) {
                conn.execute(ddl, [])?;
            }
        }
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

    /// Claims a history row for a take whose audio is already on disk, before
    /// transcription runs. Everything after this only ever updates the row, so
    /// a crash or a failed API call can't lose the recording.
    pub fn insert_pending(
        &self,
        audio_path: Option<&str>,
        duration_ms: i64,
    ) -> rusqlite::Result<i64> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO dictations (ts, raw, clean, language, duration_ms, status, audio_path)
             VALUES (?1, '', '', NULL, ?2, ?3, ?4)",
            params![now_ms(), duration_ms, STATUS_PENDING, audio_path],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn mark_done(
        &self,
        id: i64,
        raw: &str,
        clean: &str,
        language: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute(
            "UPDATE dictations SET raw = ?2, clean = ?3, language = ?4, status = ?5, error = NULL
             WHERE id = ?1",
            params![id, raw, clean, language, STATUS_DONE],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, id: i64, error: &str) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute(
            "UPDATE dictations SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, STATUS_FAILED, error],
        )?;
        Ok(())
    }

    pub fn insert(
        &self,
        raw: &str,
        clean: &str,
        language: Option<&str>,
        duration_ms: i64,
    ) -> rusqlite::Result<i64> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO dictations (ts, raw, clean, language, duration_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now_ms(), raw, clean, language, duration_ms, STATUS_DONE],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Marks rows still `pending` as failed. Called at startup: a pending row
    /// means the app died mid-transcription, and it would otherwise sit there
    /// forever with no way to retry it.
    pub fn fail_stale_pending(&self, error: &str) -> rusqlite::Result<usize> {
        self.0.lock().unwrap().execute(
            "UPDATE dictations SET status = ?1, error = ?2 WHERE status = ?3",
            params![STATUS_FAILED, error, STATUS_PENDING],
        )
    }

    pub fn get(&self, id: i64) -> rusqlite::Result<Option<Dictation>> {
        let conn = self.0.lock().unwrap();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLUMNS} FROM dictations WHERE id = ?1"))?;
        let mut rows = stmt.query_map(params![id], row_to_dictation)?;
        rows.next().transpose()
    }

    /// Newest first. `search` (if any) matches case-insensitively against
    /// either `raw` or `clean`.
    pub fn list(&self, search: Option<&str>, limit: u32) -> rusqlite::Result<Vec<Dictation>> {
        let conn = self.0.lock().unwrap();
        match search {
            Some(q) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLUMNS} FROM dictations
                     WHERE LOWER(raw) LIKE '%'||LOWER(?1)||'%' OR LOWER(clean) LIKE '%'||LOWER(?1)||'%'
                     ORDER BY id DESC LIMIT ?2"
                ))?;
                let rows = stmt.query_map(params![q, limit], row_to_dictation)?.collect();
                rows
            }
            None => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLUMNS} FROM dictations ORDER BY id DESC LIMIT ?1"
                ))?;
                let rows = stmt.query_map(params![limit], row_to_dictation)?.collect();
                rows
            }
        }
    }

    /// Deletes the row and returns its audio file name, so the caller can
    /// unlink the WAV the row was the only reference to.
    pub fn delete(&self, id: i64) -> rusqlite::Result<Option<String>> {
        let conn = self.0.lock().unwrap();
        let audio = conn
            .query_row("SELECT audio_path FROM dictations WHERE id = ?1", params![id], |r| {
                r.get::<_, Option<String>>(0)
            })
            .unwrap_or(None);
        conn.execute("DELETE FROM dictations WHERE id = ?1", params![id])?;
        Ok(audio)
    }

    pub fn clear(&self) -> rusqlite::Result<Vec<String>> {
        let conn = self.0.lock().unwrap();
        let audio = Self::collect_audio(&conn, "SELECT audio_path FROM dictations", params![])?;
        conn.execute("DELETE FROM dictations", [])?;
        Ok(audio)
    }

    /// Frees the audio of completed dictations older than `cutoff_ms` while
    /// keeping their text. Used when retention is "forever" for text but the
    /// WAVs would otherwise pile up.
    pub fn drop_audio_done_before(&self, cutoff_ms: i64) -> rusqlite::Result<Vec<String>> {
        let conn = self.0.lock().unwrap();
        let audio = Self::collect_audio(
            &conn,
            "SELECT audio_path FROM dictations WHERE status = ?1 AND ts < ?2",
            params![STATUS_DONE, cutoff_ms],
        )?;
        conn.execute(
            "UPDATE dictations SET audio_path = NULL WHERE status = ?1 AND ts < ?2",
            params![STATUS_DONE, cutoff_ms],
        )?;
        Ok(audio)
    }

    /// Every audio file name still referenced by a row — the keep-set for
    /// sweeping orphaned WAVs off disk.
    pub fn all_audio_paths(&self) -> rusqlite::Result<Vec<String>> {
        let conn = self.0.lock().unwrap();
        Self::collect_audio(&conn, "SELECT audio_path FROM dictations", params![])
    }

    /// Ages a row so retention tests don't have to wait days.
    #[cfg(test)]
    pub fn set_ts_for_test(&self, id: i64, ts: i64) -> rusqlite::Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("UPDATE dictations SET ts = ?2 WHERE id = ?1", params![id, ts])?;
        Ok(())
    }

    fn collect_audio(
        conn: &Connection,
        sql: &str,
        args: impl rusqlite::Params,
    ) -> rusqlite::Result<Vec<String>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(args, |r| r.get::<_, Option<String>>(0))?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
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
        assert_eq!(items[0].status, STATUS_DONE);
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
    fn delete_removes_only_that_row_and_reports_its_audio() {
        let (_dir, store) = store();
        let id1 = store.insert_pending(Some("a.wav"), 100).unwrap();
        let id2 = store.insert("two", "two.", None, 100).unwrap();
        assert_eq!(store.delete(id1).unwrap().as_deref(), Some("a.wav"));
        let items = store.list(None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id2);
    }

    #[test]
    fn clear_empties_the_table_and_returns_audio_paths() {
        let (_dir, store) = store();
        store.insert_pending(Some("a.wav"), 100).unwrap();
        store.insert("two", "two.", None, 100).unwrap();
        let audio = store.clear().unwrap();
        assert_eq!(audio, vec!["a.wav".to_string()]);
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

    // ---- audio-first flow ----

    #[test]
    fn pending_row_is_listed_before_any_transcript_exists() {
        let (_dir, store) = store();
        let id = store.insert_pending(Some("take-1.wav"), 4200).unwrap();
        let items = store.list(None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].status, STATUS_PENDING);
        assert_eq!(items[0].audio_path.as_deref(), Some("take-1.wav"));
        assert_eq!(items[0].duration_ms, 4200);
        assert_eq!(items[0].raw, "");
        assert_eq!(items[0].error, None);
    }

    #[test]
    fn mark_done_fills_the_transcript_and_keeps_the_audio() {
        let (_dir, store) = store();
        let id = store.insert_pending(Some("take-1.wav"), 4200).unwrap();
        store.mark_done(id, "ahoj svet", "Ahoj, svet.", Some("sk")).unwrap();
        let d = store.get(id).unwrap().unwrap();
        assert_eq!(d.status, STATUS_DONE);
        assert_eq!(d.raw, "ahoj svet");
        assert_eq!(d.clean, "Ahoj, svet.");
        assert_eq!(d.language.as_deref(), Some("sk"));
        assert_eq!(d.audio_path.as_deref(), Some("take-1.wav"));
    }

    #[test]
    fn mark_failed_records_the_error_and_keeps_the_audio() {
        let (_dir, store) = store();
        let id = store.insert_pending(Some("take-1.wav"), 4200).unwrap();
        store.mark_failed(id, "groq api 429: rate limit").unwrap();
        let d = store.get(id).unwrap().unwrap();
        assert_eq!(d.status, STATUS_FAILED);
        assert_eq!(d.error.as_deref(), Some("groq api 429: rate limit"));
        assert_eq!(d.audio_path.as_deref(), Some("take-1.wav"));
    }

    #[test]
    fn mark_done_after_a_failure_clears_the_error() {
        let (_dir, store) = store();
        let id = store.insert_pending(Some("take-1.wav"), 100).unwrap();
        store.mark_failed(id, "groq api 429").unwrap();
        store.mark_done(id, "raw", "Clean.", None).unwrap();
        let d = store.get(id).unwrap().unwrap();
        assert_eq!(d.status, STATUS_DONE);
        assert_eq!(d.error, None);
    }

    #[test]
    fn stale_pending_rows_become_retryable_failures() {
        let (_dir, store) = store();
        let crashed = store.insert_pending(Some("crashed.wav"), 100).unwrap();
        let ok = store.insert_pending(Some("ok.wav"), 100).unwrap();
        store.mark_done(ok, "raw", "Clean.", None).unwrap();

        assert_eq!(store.fail_stale_pending("appka sa ukončila").unwrap(), 1);
        let d = store.get(crashed).unwrap().unwrap();
        assert_eq!(d.status, STATUS_FAILED);
        assert_eq!(d.error.as_deref(), Some("appka sa ukončila"));
        assert_eq!(d.audio_path.as_deref(), Some("crashed.wav"));
        assert_eq!(store.get(ok).unwrap().unwrap().status, STATUS_DONE);
    }

    #[test]
    fn get_returns_none_for_a_missing_id() {
        let (_dir, store) = store();
        assert!(store.get(999).unwrap().is_none());
    }

    // ---- retention ----

    #[test]
    fn drop_audio_keeps_the_row_but_frees_the_file() {
        let (_dir, store) = store();
        let id = store.insert_pending(Some("old.wav"), 100).unwrap();
        store.mark_done(id, "old", "Old.", None).unwrap();
        backdate(&store, id, 0);

        let freed = store.drop_audio_done_before(now_ms()).unwrap();
        assert_eq!(freed, vec!["old.wav".to_string()]);
        let d = store.get(id).unwrap().unwrap();
        assert_eq!(d.clean, "Old.");
        assert_eq!(d.audio_path, None);
    }

    #[test]
    fn all_audio_paths_skips_rows_without_audio() {
        let (_dir, store) = store();
        store.insert_pending(Some("a.wav"), 100).unwrap();
        store.insert("text only", "Text only.", None, 100).unwrap();
        assert_eq!(store.all_audio_paths().unwrap(), vec!["a.wav".to_string()]);
    }

    // ---- migration ----

    #[test]
    fn pre_audio_database_gains_the_new_columns_and_keeps_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        {
            // Exactly the Plan-1 schema, before status/audio_path/error existed.
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "CREATE TABLE dictations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    ts INTEGER NOT NULL,
                    raw TEXT NOT NULL,
                    clean TEXT NOT NULL,
                    language TEXT,
                    duration_ms INTEGER NOT NULL
                )",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO dictations (ts, raw, clean, language, duration_ms)
                 VALUES (1000, 'stary prepis', 'Stary prepis.', 'sk', 500)",
                [],
            )
            .unwrap();
        }

        let store = HistoryStore::open(&path).unwrap();
        let items = store.list(None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].raw, "stary prepis");
        assert_eq!(items[0].status, STATUS_DONE);
        assert_eq!(items[0].audio_path, None);
        assert_eq!(items[0].error, None);

        // And the migrated DB still accepts the new flow.
        let id = store.insert_pending(Some("new.wav"), 100).unwrap();
        store.mark_failed(id, "boom").unwrap();
        assert_eq!(store.get(id).unwrap().unwrap().error.as_deref(), Some("boom"));
    }

    #[test]
    fn migration_is_idempotent_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        for _ in 0..3 {
            HistoryStore::open(&path).unwrap();
        }
        let store = HistoryStore::open(&path).unwrap();
        assert!(store.list(None, 10).unwrap().is_empty());
    }

    fn backdate(store: &HistoryStore, id: i64, ts: i64) {
        store.set_ts_for_test(id, ts).unwrap();
    }
}
