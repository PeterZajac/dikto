use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Notes live in `history.sqlite` next to the dictations, sharing one
/// connection and one mutex (see `HistoryStore::connection`). They are their
/// own table, so History's "delete all" and the retention sweep — both scoped
/// to `dictations` — can never touch them.
///
/// Accepted risk: `HistoryStore::open_or_recover` renames the whole file to
/// `.corrupt` when it is beyond repair, which takes notes with it. Same
/// exposure as today's dictations, and the file is preserved, not destroyed.
pub struct NotesStore(Arc<Mutex<Connection>>);

const COLUMNS: &str = "id, title, body, created_at, updated_at";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Note {
    pub id: i64,
    /// May be empty — the UI renders a localised "Untitled" placeholder rather
    /// than storing one.
    pub title: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

/// `%` and `_` are LIKE wildcards; a user typing "100%" into the search box
/// means the characters, not "match everything".
fn escape_like(q: &str) -> String {
    q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

impl NotesStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> rusqlite::Result<Self> {
        Self::init_schema(&conn.lock().unwrap())?;
        Ok(Self(conn))
    }

    /// Notes must never be the reason the app won't start. If the table can't
    /// be created on the shared connection, fall back to an in-memory store:
    /// notes work for the session and are not persisted, exactly as
    /// `HistoryStore::open_or_recover` does for dictations.
    pub fn open_or_recover(conn: Arc<Mutex<Connection>>) -> Self {
        match Self::new(conn) {
            Ok(store) => store,
            Err(e) => {
                eprintln!("could not create the notes table ({e}), falling back to in-memory (not persisted)");
                let mem = Connection::open_in_memory().expect("open in-memory sqlite connection");
                Self::init_schema(&mem).expect("init in-memory notes schema");
                Self(Arc::new(Mutex::new(mem)))
            }
        }
    }

    /// `CREATE TABLE IF NOT EXISTS` *is* the v1 migration. When the first
    /// column is added, follow `HistoryStore::migrate` — PRAGMA table_info,
    /// then ALTER for whatever is missing.
    fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS notes (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                title      TEXT    NOT NULL DEFAULT '',
                body       TEXT    NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS notes_updated_at ON notes (updated_at DESC, id DESC)",
            [],
        )?;
        Ok(())
    }

    pub fn create(&self, title: Option<&str>) -> rusqlite::Result<Note> {
        let conn = self.0.lock().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO notes (title, body, created_at, updated_at) VALUES (?1, '', ?2, ?2)",
            params![title.unwrap_or(""), now],
        )?;
        let id = conn.last_insert_rowid();
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM notes WHERE id = ?1"),
            params![id],
            row_to_note,
        )
    }

    pub fn get(&self, id: i64) -> rusqlite::Result<Option<Note>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM notes WHERE id = ?1"))?;
        let mut rows = stmt.query_map(params![id], row_to_note)?;
        rows.next().transpose()
    }

    /// Most recently edited first. `search` (if any) matches against either
    /// `title` or `body`, case-insensitively for ASCII — SQLite's `LOWER` does
    /// not fold `Č` to `č`, so a Slovak query only matches the same casing.
    pub fn list(&self, search: Option<&str>, limit: u32) -> rusqlite::Result<Vec<Note>> {
        let conn = self.0.lock().unwrap();
        match search {
            Some(q) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLUMNS} FROM notes
                     WHERE LOWER(title) LIKE '%'||LOWER(?1)||'%' ESCAPE '\\'
                        OR LOWER(body)  LIKE '%'||LOWER(?1)||'%' ESCAPE '\\'
                     ORDER BY updated_at DESC, id DESC LIMIT ?2"
                ))?;
                let rows = stmt.query_map(params![escape_like(q), limit], row_to_note)?.collect();
                rows
            }
            None => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLUMNS} FROM notes ORDER BY updated_at DESC, id DESC LIMIT ?1"
                ))?;
                let rows = stmt.query_map(params![limit], row_to_note)?.collect();
                rows
            }
        }
    }

    /// Partial patch: `None` leaves the field alone. Returns false when there
    /// is no such row — the note was deleted while the editor still had it
    /// open, and a pending autosave must not resurrect it.
    pub fn update(
        &self,
        id: i64,
        title: Option<&str>,
        body: Option<&str>,
    ) -> rusqlite::Result<bool> {
        let conn = self.0.lock().unwrap();
        let changed = conn.execute(
            "UPDATE notes
             SET title = COALESCE(?2, title), body = COALESCE(?3, body), updated_at = ?4
             WHERE id = ?1",
            params![id, title, body, now_ms()],
        )?;
        Ok(changed > 0)
    }

    pub fn delete(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self
            .0
            .lock()
            .unwrap()
            .execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Ages a note so ordering tests don't depend on millisecond timing.
    #[cfg(test)]
    fn set_updated_at_for_test(&self, id: i64, ts: i64) -> rusqlite::Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("UPDATE notes SET updated_at = ?2 WHERE id = ?1", params![id, ts])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryStore;
    use std::path::PathBuf;

    fn store() -> (tempfile::TempDir, PathBuf, HistoryStore, NotesStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        let history = HistoryStore::open(&path).unwrap();
        let notes = NotesStore::new(history.connection()).unwrap();
        (dir, path, history, notes)
    }

    #[test]
    fn create_then_list_roundtrip() {
        let (_dir, _path, _history, notes) = store();
        let note = notes.create(Some("Shopping")).unwrap();
        let all = notes.list(None, 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, note.id);
        assert_eq!(all[0].title, "Shopping");
        assert_eq!(all[0].body, "");
        assert!(all[0].created_at > 0);
        assert_eq!(all[0].created_at, all[0].updated_at);
    }

    #[test]
    fn create_without_a_title_stores_an_empty_string() {
        let (_dir, _path, _history, notes) = store();
        let note = notes.create(None).unwrap();
        assert_eq!(note.title, "");
        assert_eq!(notes.get(note.id).unwrap().unwrap(), note);
    }

    #[test]
    fn list_is_ordered_by_most_recently_edited() {
        let (_dir, _path, _history, notes) = store();
        let older = notes.create(Some("older")).unwrap();
        let newer = notes.create(Some("newer")).unwrap();
        notes.set_updated_at_for_test(older.id, 1_000).unwrap();
        notes.set_updated_at_for_test(newer.id, 2_000).unwrap();

        let all = notes.list(None, 10).unwrap();
        assert_eq!(all.iter().map(|n| n.id).collect::<Vec<_>>(), vec![newer.id, older.id]);

        notes.update(older.id, None, Some("touched")).unwrap();
        let all = notes.list(None, 10).unwrap();
        assert_eq!(all[0].id, older.id, "editing a note floats it to the top");
    }

    #[test]
    fn search_matches_title_and_body_case_insensitively() {
        let (_dir, _path, _history, notes) = store();
        let by_title = notes.create(Some("Groceries")).unwrap();
        let by_body = notes.create(Some("Standup")).unwrap();
        notes.update(by_body.id, None, Some("Buy MILK on the way")).unwrap();
        notes.create(Some("Unrelated")).unwrap();

        assert_eq!(
            notes.list(Some("grocer"), 10).unwrap().iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![by_title.id]
        );
        assert_eq!(
            notes.list(Some("milk"), 10).unwrap().iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![by_body.id]
        );
        assert!(notes.list(Some("nothing here"), 10).unwrap().is_empty());
    }

    #[test]
    fn update_patches_one_field_and_bumps_updated_at_only() {
        let (_dir, _path, _history, notes) = store();
        let note = notes.create(Some("Title")).unwrap();
        notes.set_updated_at_for_test(note.id, 1_000).unwrap();

        assert!(notes.update(note.id, None, Some("body text")).unwrap());
        let patched = notes.get(note.id).unwrap().unwrap();
        assert_eq!(patched.title, "Title", "an untouched field stays put");
        assert_eq!(patched.body, "body text");
        assert_eq!(patched.created_at, note.created_at);
        assert!(patched.updated_at > 1_000);

        assert!(notes.update(note.id, Some("Renamed"), None).unwrap());
        let patched = notes.get(note.id).unwrap().unwrap();
        assert_eq!(patched.title, "Renamed");
        assert_eq!(patched.body, "body text");
    }

    #[test]
    fn a_title_can_be_cleared_back_to_empty() {
        // The partial patch hinges on "" and NULL meaning different things:
        // Some("") clears the title, None leaves it alone.
        let (_dir, _path, _history, notes) = store();
        let note = notes.create(Some("Named")).unwrap();
        assert!(notes.update(note.id, Some(""), None).unwrap());
        assert_eq!(notes.get(note.id).unwrap().unwrap().title, "");
    }

    #[test]
    fn search_treats_like_wildcards_as_plain_characters() {
        let (_dir, _path, _history, notes) = store();
        let literal = notes.create(Some("100% done")).unwrap();
        notes.create(Some("nothing special")).unwrap();

        let hits = notes.list(Some("%"), 10).unwrap();
        assert_eq!(hits.iter().map(|n| n.id).collect::<Vec<_>>(), vec![literal.id]);
        assert!(notes.list(Some("_"), 10).unwrap().is_empty());
    }

    #[test]
    fn list_honours_the_limit_with_and_without_a_search() {
        let (_dir, _path, _history, notes) = store();
        notes.create(Some("alpha one")).unwrap();
        notes.create(Some("alpha two")).unwrap();
        assert_eq!(notes.list(None, 1).unwrap().len(), 1);
        assert_eq!(notes.list(Some("alpha"), 1).unwrap().len(), 1);
    }

    #[test]
    fn update_of_a_missing_note_reports_false() {
        let (_dir, _path, _history, notes) = store();
        assert!(!notes.update(404, Some("ghost"), None).unwrap());
    }

    #[test]
    fn delete_removes_only_that_note() {
        let (_dir, _path, _history, notes) = store();
        let doomed = notes.create(Some("doomed")).unwrap();
        let kept = notes.create(Some("kept")).unwrap();

        assert!(notes.delete(doomed.id).unwrap());
        assert!(!notes.delete(doomed.id).unwrap(), "deleting twice is not an error");
        let all = notes.list(None, 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, kept.id);
    }

    #[test]
    fn schema_init_survives_reopening_the_file() {
        let (_dir, path, history, notes) = store();
        let note = notes.create(Some("persisted")).unwrap();
        drop(notes);
        drop(history);

        let history = HistoryStore::open(&path).unwrap();
        let notes = NotesStore::new(history.connection()).unwrap();
        let all = notes.list(None, 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, note.id);
        assert_eq!(all[0].title, "persisted");
    }

    // ---- the two that guard the shared-database decision ----

    #[test]
    fn history_clear_leaves_notes_alone() {
        let (_dir, _path, history, notes) = store();
        let note = notes.create(Some("survivor")).unwrap();
        history.insert("raw", "Clean.", None, 100).unwrap();

        history.clear().unwrap();

        assert!(history.list(None, 10).unwrap().is_empty());
        assert_eq!(notes.get(note.id).unwrap().unwrap().title, "survivor");
    }

    #[test]
    fn retention_never_touches_notes() {
        let (_dir, _path, history, notes) = store();
        let note = notes.create(Some("survivor")).unwrap();
        let id = history.insert("raw", "Clean.", None, 100).unwrap();
        history.set_ts_for_test(id, 0).unwrap();

        let (deleted, _audio) = history.delete_done_before(now_ms()).unwrap();
        assert_eq!(deleted, 1);

        assert_eq!(notes.list(None, 10).unwrap().len(), 1);
        assert_eq!(notes.get(note.id).unwrap().unwrap().title, "survivor");
    }
}
