//! Fenix's state files, read-only (FENIX-INTEGRATION.md, "What Flint
//! reads today"):
//!
//! - `state/projects.json`, key `known`: project roots, most recent first;
//!   key `meta`: `{ "pinned": [root], "groups": { root: name } }`.
//! - `state/recent.json`, key `files`: recent files, most recent first.
//!
//! Read by key through `fenix_storage::state`, not through
//! `fenix-project`, which would pull in Fenix's MIB and CCSDS crates to
//! read two JSON keys (F7 would make these keys a contract). Defensive the
//! way Fenix is: a missing file is no items, a damaged one is no items and
//! one line saying so, and nothing here ever writes them (principle 2).
//!
//! Paths come back plain: Fenix stores recent files canonicalized, so on
//! Windows they read `\\?\C:\...`, which must not reach a title or Fenix's
//! command line. The same rule as Fenix's `fenix_project::plain_path`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fenix_storage::paths::Roots;
use serde::Deserialize;

/// Fenix keeps at most this many recent files; more means a damaged file.
pub const MAX_RECENT: usize = 200;

/// A known project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Its root folder, plain.
    pub root: PathBuf,
    /// Pinned in Fenix's project list.
    pub pinned: bool,
    /// The group Fenix files it under, if any.
    pub group: Option<String>,
}

/// What Flint reads from Fenix's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FenixState {
    /// Known projects, in Fenix's order (most recent first).
    pub projects: Vec<Project>,
    /// Recent files, most recent first.
    pub recent: Vec<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Meta {
    pinned: Vec<PathBuf>,
    groups: BTreeMap<PathBuf, String>,
}

/// `path` without Windows' `\\?\` (or `\\?\UNC\`) prefix.
pub fn plain_path(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

/// The files read, for noticing changes.
pub fn files(roots: &Roots) -> [PathBuf; 2] {
    [roots.state("projects.json"), roots.state("recent.json")]
}

/// Reads Fenix's state under `roots`. The lines name files that exist but
/// couldn't be read; their items are simply absent.
pub fn read(roots: &Roots) -> (FenixState, Vec<String>) {
    let [projects_file, recent_file] = files(roots);
    let mut problems = Vec::new();
    let mut get = |path: &Path, key: &str| -> Option<serde_json::Value> {
        match fenix_storage::state::read::<serde_json::Value>(path, key) {
            Ok(value) => value,
            Err(e) => {
                problems.push(format!("Fenix's {}: {e}", path.display()));
                None
            }
        }
    };
    let known: Vec<PathBuf> = get(&projects_file, "known")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let meta: Meta = get(&projects_file, "meta")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let recent: Vec<PathBuf> = get(&recent_file, "files")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let pinned: HashSet<PathBuf> = meta.pinned.into_iter().map(plain_path).collect();
    let groups: BTreeMap<PathBuf, String> = meta
        .groups
        .into_iter()
        .map(|(root, group)| (plain_path(root), group))
        .collect();
    let mut seen = HashSet::new();
    let projects = known
        .into_iter()
        .map(plain_path)
        .filter(|root| seen.insert(root.clone()))
        .map(|root| Project {
            pinned: pinned.contains(&root),
            group: groups.get(&root).cloned().filter(|g| !g.trim().is_empty()),
            root,
        })
        .collect();
    let mut seen = HashSet::new();
    let recent = recent
        .into_iter()
        .map(plain_path)
        .filter(|path| seen.insert(path.clone()))
        .take(MAX_RECENT)
        .collect();
    (FenixState { projects, recent }, problems)
}

/// Modification times of a set of files, to read them again only when
/// one changed. A file appearing or disappearing counts as a change.
#[derive(Debug, Clone, Default)]
pub struct Stamps {
    files: Vec<PathBuf>,
    seen: Option<Vec<Option<SystemTime>>>,
}

impl Stamps {
    /// Watches `files`; the first [`Stamps::changed`] is always true.
    pub fn new(files: Vec<PathBuf>) -> Self {
        Stamps { seen: None, files }
    }

    /// Whether any file changed since the last call (or this is the first).
    pub fn changed(&mut self) -> bool {
        let now: Vec<Option<SystemTime>> = self
            .files
            .iter()
            .map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
            .collect();
        let changed = self.seen.as_ref() != Some(&now);
        self.seen = Some(now);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Roots {
        Roots::portable(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fenix-home"))
    }

    #[test]
    fn the_fixture_state_reads_as_fenix_left_it() {
        let (state, problems) = read(&fixture());
        assert!(problems.is_empty(), "{problems:?}");
        let roots: Vec<String> = state
            .projects
            .iter()
            .map(|p| p.root.display().to_string())
            .collect();
        assert_eq!(
            roots,
            [
                r"C:\dev\fenix",
                r"C:\dev\flint",
                r"\\server\share\ops",
                r"C:\dev\scratch"
            ]
        );
        assert!(
            state.projects[0].pinned,
            "pinned under its \\\\?\\ spelling, matched plain"
        );
        assert_eq!(state.projects[2].group.as_deref(), Some("Mission ops"));
        assert_eq!(state.projects[3].group, None, "a blank group is no group");
        assert_eq!(state.recent[0], PathBuf::from(r"C:\dev\fenix\src\main.rs"));
        assert_eq!(
            state.recent.len(),
            3,
            "the duplicate under another spelling is dropped"
        );
    }

    #[test]
    fn plain_paths_lose_only_the_verbatim_prefix() {
        assert_eq!(
            plain_path(r"\\?\C:\x\y.rs".into()),
            PathBuf::from(r"C:\x\y.rs")
        );
        assert_eq!(
            plain_path(r"\\?\UNC\srv\share\a".into()),
            PathBuf::from(r"\\srv\share\a")
        );
        assert_eq!(
            plain_path("/home/me/a.rs".into()),
            PathBuf::from("/home/me/a.rs")
        );
    }

    #[test]
    fn no_fenix_is_no_items_and_no_complaint() {
        let (state, problems) = read(&Roots::portable(Path::new("definitely/not/here")));
        assert_eq!(state, FenixState::default());
        assert!(problems.is_empty());
    }

    #[test]
    fn a_damaged_file_is_named_and_costs_only_its_items() {
        let dir = std::env::temp_dir().join(format!("flint-fenix-damaged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("state")).unwrap();
        std::fs::write(dir.join("state/projects.json"), "{ not json").unwrap();
        std::fs::write(dir.join("state/recent.json"), r#"{"files": ["/a.rs"]}"#).unwrap();
        let (state, problems) = read(&Roots::portable(&dir));
        assert!(state.projects.is_empty());
        assert_eq!(state.recent, [PathBuf::from("/a.rs")]);
        assert_eq!(
            problems.len(),
            2,
            "known and meta each say so: {problems:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamps_notice_changes_and_appearances() {
        let dir = std::env::temp_dir().join(format!("flint-fenix-stamps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.json");
        let mut stamps = Stamps::new(vec![file.clone()]);
        assert!(stamps.changed(), "the first look always reads");
        assert!(!stamps.changed());
        std::fs::write(&file, "{}").unwrap();
        assert!(stamps.changed(), "appeared");
        assert!(!stamps.changed());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
