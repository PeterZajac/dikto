use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The WAV files behind history rows. `history.rs` stores only the file name;
/// this owns the directory and is the single place that turns a name back into
/// a path, so a crafted `audio_path` can't reach outside the store.
pub struct RecordingStore {
    dir: PathBuf,
}

/// Rejects anything that isn't a plain `<stem>.wav` living directly in the
/// store — no separators, no `..`, no absolute paths.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.ends_with(".wav")
        && Path::new(name).components().count() == 1
        && !name.contains(['/', '\\'])
        && !name.contains("..")
}

impl RecordingStore {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }

    #[cfg(test)]
    fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `wav` under a name unique to this take and returns it. `gen` is
    /// the pipeline's take generation — combined with the millisecond stamp it
    /// keeps concurrent and back-to-back takes apart.
    pub fn save(&self, wav: &[u8], gen: u64) -> io::Result<String> {
        std::fs::create_dir_all(&self.dir)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let name = format!("take-{stamp}-{gen}.wav");
        std::fs::write(self.dir.join(&name), wav)?;
        Ok(name)
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        is_safe_name(name).then(|| self.dir.join(name))
    }

    pub fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let path = self
            .path(name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad recording name"))?;
        std::fs::read(path)
    }

    #[cfg(test)]
    fn exists(&self, name: &str) -> bool {
        self.path(name).is_some_and(|p| p.exists())
    }

    pub fn remove(&self, name: &str) {
        if let Some(path) = self.path(name) {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn remove_all<I: IntoIterator<Item = S>, S: AsRef<str>>(&self, names: I) {
        for name in names {
            self.remove(name.as_ref());
        }
    }

    /// Deletes WAVs no history row references any more — covers files orphaned
    /// by a crash between writing the audio and committing the row.
    pub fn sweep_orphans(&self, keep: &HashSet<String>) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if is_safe_name(name) && !keep.contains(name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, RecordingStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = RecordingStore::new(dir.path().join("audio"));
        (dir, store)
    }

    #[test]
    fn save_then_read_roundtrip() {
        let (_dir, store) = store();
        let name = store.save(b"RIFFfake", 7).unwrap();
        assert!(name.ends_with(".wav"));
        assert!(name.contains("-7."));
        assert_eq!(store.read(&name).unwrap(), b"RIFFfake");
        assert!(store.exists(&name));
    }

    #[test]
    fn save_creates_the_directory_if_it_was_removed() {
        let (_dir, store) = store();
        std::fs::remove_dir_all(store.dir()).unwrap();
        let name = store.save(b"x", 1).unwrap();
        assert_eq!(store.read(&name).unwrap(), b"x");
    }

    #[test]
    fn two_takes_never_share_a_name() {
        let (_dir, store) = store();
        let a = store.save(b"a", 1).unwrap();
        let b = store.save(b"b", 2).unwrap();
        assert_ne!(a, b);
        assert_eq!(store.read(&a).unwrap(), b"a");
        assert_eq!(store.read(&b).unwrap(), b"b");
    }

    #[test]
    fn traversal_and_absolute_names_are_rejected() {
        let (_dir, store) = store();
        for bad in [
            "../escape.wav",
            "sub/take.wav",
            "..\\escape.wav",
            "/etc/passwd.wav",
            "take.txt",
            "",
        ] {
            assert!(store.path(bad).is_none(), "{bad} should be rejected");
            assert!(store.read(bad).is_err(), "{bad} should not be readable");
        }
    }

    #[test]
    fn remove_deletes_only_the_named_file() {
        let (_dir, store) = store();
        let a = store.save(b"a", 1).unwrap();
        let b = store.save(b"b", 2).unwrap();
        store.remove(&a);
        assert!(!store.exists(&a));
        assert!(store.exists(&b));
    }

    #[test]
    fn remove_ignores_unknown_and_unsafe_names() {
        let (_dir, store) = store();
        store.remove("nope.wav");
        store.remove("../../etc/passwd.wav");
    }

    #[test]
    fn sweep_deletes_orphans_and_keeps_referenced_files() {
        let (_dir, store) = store();
        let keep = store.save(b"keep", 1).unwrap();
        let orphan = store.save(b"orphan", 2).unwrap();
        store.sweep_orphans(&HashSet::from([keep.clone()]));
        assert!(store.exists(&keep));
        assert!(!store.exists(&orphan));
    }

    #[test]
    fn sweep_leaves_foreign_files_alone() {
        let (_dir, store) = store();
        std::fs::write(store.dir().join("notes.txt"), b"mine").unwrap();
        store.sweep_orphans(&HashSet::new());
        assert!(store.dir().join("notes.txt").exists());
    }

    #[test]
    fn remove_all_clears_a_batch() {
        let (_dir, store) = store();
        let a = store.save(b"a", 1).unwrap();
        let b = store.save(b"b", 2).unwrap();
        store.remove_all([a.clone(), b.clone()]);
        assert!(!store.exists(&a));
        assert!(!store.exists(&b));
    }
}
