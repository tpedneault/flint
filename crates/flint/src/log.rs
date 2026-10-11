//! The daemon's log: `flint.log` in Flint's log folder, one timestamped
//! line per event (start, shows with their timings, settings problems,
//! hotkey failures, control connections that went wrong).
//!
//! Deliberately small: one file, appended, started afresh past 1 MiB with
//! the previous one kept as `flint.log.old`, the same policy as Fenix's
//! hang log. Lines are also written to stderr, so `flint --console` (or
//! running the daemon from a terminal on Linux) shows them live. Writing
//! never fails loudly: a launcher that stops because its log folder is
//! read-only would be worse than one that logs nowhere.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Past this size the log starts afresh.
pub const MAX_LOG: u64 = 1024 * 1024;

/// The log file, shared between threads.
pub struct Log {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl Log {
    /// Opens (creating the folder if needed) `flint.log` in `dir`.
    pub fn open(dir: &Path) -> Log {
        let path = dir.join("flint.log");
        let _ = fs::create_dir_all(dir);
        rotate_if_large(&path);
        Log {
            file: Mutex::new(open_append(&path)),
            path,
        }
    }

    /// Where the log is written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes one line.
    pub fn line(&self, text: &str) {
        let line = format!("{} {text}\n", timestamp(SystemTime::now()));
        eprint!("{line}");
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() > MAX_LOG)
        {
            *file = None;
            rotate_if_large(&self.path);
            *file = open_append(&self.path);
        }
        if let Some(f) = file.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn open_append(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn rotate_if_large(path: &Path) {
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG) {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        let _ = fs::rename(path, old);
    }
}

/// `2026-10-11T15:04:05.123Z`, UTC, without a date-time dependency.
fn timestamp(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_iso_8601() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let t = UNIX_EPOCH + Duration::from_millis(1_791_731_045_123);
        assert_eq!(timestamp(t), "2026-10-11T15:04:05.123Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert!(timestamp(leap).starts_with("2000-02-29T00:00:00"));
    }

    #[test]
    fn lines_are_appended_and_a_large_log_is_rotated() {
        let dir = std::env::temp_dir().join(format!("flint-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let log = Log::open(&dir);
        log.line("first");
        log.line("second");
        let text = fs::read_to_string(log.path()).unwrap();
        assert!(text.lines().next().unwrap().ends_with(" first"));
        assert_eq!(text.lines().count(), 2);
        drop(log);

        fs::write(dir.join("flint.log"), vec![b'x'; MAX_LOG as usize + 1]).unwrap();
        let log = Log::open(&dir);
        log.line("fresh");
        assert!(dir.join("flint.log.old").exists());
        assert_eq!(fs::read_to_string(log.path()).unwrap().lines().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
