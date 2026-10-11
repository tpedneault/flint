//! Frecency on disk: `state/frecency.json` in Flint's local folder, key
//! `entries`, through `fenix_storage::state`, so it is written the way
//! Fenix writes its state (re-read, replace one key, atomic write).
//!
//! Saving happens on a thread of its own: a pick hides the window and
//! launches something, and neither may wait for a disk. Snapshots queue
//! up and only the newest is written, so a burst of picks costs one write.
//! A file that can't be read is reported once and treated as empty, the
//! posture Fenix takes with its own state; it is replaced on the next save.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

use flint_index::frecency::Entry;
use flint_index::Frecency;

use crate::log::Log;

const KEY: &str = "entries";

/// Reads the stored frecency; a missing file is an empty table.
pub fn load(path: &Path, log: &Log) -> Frecency {
    match fenix_storage::state::read::<Vec<Entry>>(path, KEY) {
        Ok(entries) => Frecency::from_entries(entries.unwrap_or_default()),
        Err(e) => {
            log.line(&format!("frecency: {e}; starting afresh"));
            Frecency::default()
        }
    }
}

/// Writes snapshots in the background.
pub struct Saver {
    sender: Sender<Vec<Entry>>,
}

impl Saver {
    /// Starts the writer for `path`.
    pub fn start(path: PathBuf, log: Arc<Log>) -> Saver {
        let (sender, receiver) = mpsc::channel::<Vec<Entry>>();
        let spawned = std::thread::Builder::new()
            .name("flint-frecency".into())
            .spawn(move || {
                while let Ok(mut entries) = receiver.recv() {
                    // Only the newest snapshot matters.
                    while let Ok(newer) = receiver.try_recv() {
                        entries = newer;
                    }
                    if let Err(e) = fenix_storage::state::write(&path, KEY, &entries) {
                        log.line(&format!("frecency: couldn't save {}: {e}", path.display()));
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("flint: no thread to save frecency: {e}");
        }
        Saver { sender }
    }

    /// Queues `frecency` to be written.
    pub fn save(&self, frecency: &Frecency) {
        let _ = self.sender.send(frecency.entries());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_saved_is_loaded() {
        let dir = std::env::temp_dir().join(format!("flint-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = Arc::new(Log::open(&dir));
        let path = dir.join("state").join("frecency.json");
        assert!(load(&path, &log).is_empty());

        let mut f = Frecency::default();
        f.record("apps:firefox", "fi", 100);
        fenix_storage::state::write(&path, KEY, &f.entries()).unwrap();
        assert_eq!(load(&path, &log).entries(), f.entries());

        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path, &log).is_empty(), "a damaged file starts afresh");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
