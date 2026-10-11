//! Fenix's projects and recent files as items, the shape every provider
//! gives Flint.
//!
//! - **Projects** (`p`): pinned ones first, then Fenix's order (most
//!   recently used first), the group as part of the subtitle. `Enter` opens the folder. "Open in Fenix" is listed but
//!   says it needs a newer Fenix: `fenix <folder>` opens a folder as a
//!   file today, not as a project, so offering it would do the wrong thing
//!   quietly (FENIX-INTEGRATION.md; F1 brings `fenix/openProject`).
//! - **Recent files** (`f`): most recent first. `Enter` opens the file in
//!   Fenix through its existing hand-off, `fenix <file>`.
//!
//! The Fenix actions are `fenix` actions with the methods F1 will provide
//! (`fenix/open`, `fenix/openProject`); until Fenix speaks that protocol
//! the daemon carries them out with [`crate::handoff`], so items won't
//! change when it does.

use std::path::Path;

use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;
use serde_json::json;

use crate::state::FenixState;

/// The projects provider's id.
pub const PROJECTS: &str = "fenix.projects";
/// Its badge and chip.
pub const PROJECTS_TITLE: &str = "Projects";
/// Its scope keyword.
pub const PROJECTS_PREFIX: &str = "p";
/// The recent files provider's id.
pub const RECENT: &str = "fenix.recent";
/// Its badge and chip.
pub const RECENT_TITLE: &str = "Recent files";
/// Its scope keyword.
pub const RECENT_PREFIX: &str = "f";

/// How far a pinned project rises: above an unpinned one of the same match.
pub const PINNED_BOOST: f64 = 15.0;
/// The most recent file's lift; older ones get proportionally less, so
/// among equal matches the recent order holds.
pub const RECENT_BOOST: f64 = 10.0;
/// The same for projects, in Fenix's most-recently-used order: smaller
/// than a pin, so a pinned project stays above every unpinned one.
pub const PROJECT_ORDER_BOOST: f64 = 5.0;

/// A lift falling from `top` for the first of `count` to near zero for
/// the last, keeping a list's order among equal matches.
fn in_order(top: f64, n: usize, count: usize) -> f64 {
    top * (1.0 - n as f64 / count.max(1) as f64)
}

fn action(id: &str, title: &str, kind: ActionKind) -> Action {
    Action {
        id: id.into(),
        title: title.into(),
        key: None,
        kind,
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A shell opened in the user's terminal, in a folder.
fn terminal_here(root: &Path) -> ActionKind {
    let shell = if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".into())
    };
    ActionKind::Run {
        argv: vec![shell],
        cwd: Some(root.display().to_string()),
        terminal: true,
    }
}

/// The project items, each with its ranking boost.
pub fn projects(state: &FenixState) -> Vec<(Item, f64)> {
    let count = state.projects.len();
    state
        .projects
        .iter()
        .enumerate()
        .map(|(n, project)| {
            let root = project.root.display().to_string();
            let subtitle = match &project.group {
                Some(group) => format!("{group} · {root}"),
                None => root.clone(),
            };
            let item = Item {
                id: format!("{PROJECTS}:{root}"),
                title: name_of(&project.root),
                subtitle: Some(subtitle),
                icon: Some("project".into()),
                keywords: project.group.iter().cloned().collect(),
                actions: vec![
                    action(
                        "open-folder",
                        "Open folder",
                        ActionKind::Open {
                            target: root.clone(),
                        },
                    ),
                    action(
                        "open-in-fenix",
                        "Open in Fenix",
                        ActionKind::Fenix {
                            method: "fenix/openProject".into(),
                            params: json!({ "root": root }),
                        },
                    ),
                    action("terminal", "Terminal here", terminal_here(&project.root)),
                    action("copy-path", "Copy path", ActionKind::Copy { text: root }),
                ],
            };
            let pin = if project.pinned { PINNED_BOOST } else { 0.0 };
            (item, pin + in_order(PROJECT_ORDER_BOOST, n, count))
        })
        .collect()
}

/// The recent-file items, each with its ranking boost.
pub fn recent(state: &FenixState) -> Vec<(Item, f64)> {
    let count = state.recent.len();
    state
        .recent
        .iter()
        .enumerate()
        .map(|(n, path)| {
            let file = path.display().to_string();
            let item = Item {
                id: format!("{RECENT}:{file}"),
                title: name_of(path),
                subtitle: path.parent().map(|p| p.display().to_string()),
                icon: Some("file".into()),
                keywords: vec![],
                actions: vec![
                    action(
                        "open-in-fenix",
                        "Open in Fenix",
                        ActionKind::Fenix {
                            method: "fenix/open".into(),
                            params: json!({ "paths": [{ "path": file }] }),
                        },
                    ),
                    action(
                        "reveal",
                        "Show in folder",
                        ActionKind::Reveal {
                            target: file.clone(),
                        },
                    ),
                    action("copy-path", "Copy path", ActionKind::Copy { text: file }),
                ],
            };
            (item, in_order(RECENT_BOOST, n, count))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{self, Project};
    use std::path::PathBuf;

    fn fixture_state() -> FenixState {
        let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fenix-home");
        state::read(&fenix_storage::paths::Roots::portable(&home)).0
    }

    #[test]
    fn projects_open_their_folder_and_pinned_ones_rise() {
        let items = projects(&fixture_state());
        assert_eq!(items.len(), 4);
        let (fenix, boost) = &items[0];
        assert_eq!(fenix.title, "fenix");
        assert_eq!(*boost, PINNED_BOOST + PROJECT_ORDER_BOOST);
        let boosts: Vec<f64> = items.iter().map(|(_, b)| *b).collect();
        assert!(
            boosts[1] > boosts[2] && boosts[2] > boosts[3],
            "Fenix's order: {boosts:?}"
        );
        assert!(
            matches!(&fenix.actions[0].kind, ActionKind::Open { target } if target == r"C:\dev\fenix")
        );
        assert!(
            matches!(&fenix.actions[1].kind, ActionKind::Fenix { method, .. } if method == "fenix/openProject")
        );
        let (ops, boost) = &items[2];
        assert_eq!(
            ops.subtitle.as_deref(),
            Some(r"Mission ops · \\server\share\ops")
        );
        assert_eq!(ops.keywords, ["Mission ops"]);
        assert!(*boost < PINNED_BOOST);
        assert!(items.iter().all(|(i, _)| i.check().is_ok()));
    }

    #[test]
    fn recent_files_open_in_fenix_most_recent_highest() {
        let items = recent(&fixture_state());
        let titles: Vec<&str> = items.iter().map(|(i, _)| i.title.as_str()).collect();
        assert_eq!(titles, ["main.rs", "Cargo.toml", "DESIGN.md"]);
        assert!(items[0].1 > items[1].1 && items[1].1 > items[2].1);
        let (main, _) = &items[0];
        assert_eq!(main.subtitle.as_deref(), Some(r"C:\dev\fenix\src"));
        assert_eq!(
            main.actions[0].kind,
            ActionKind::Fenix {
                method: "fenix/open".into(),
                params: json!({ "paths": [{ "path": r"C:\dev\fenix\src\main.rs" }] }),
            }
        );
        assert_eq!(main.primary_text(), r"C:\dev\fenix\src\main.rs");
    }

    #[test]
    fn a_root_without_a_name_is_shown_whole() {
        let state = FenixState {
            projects: vec![Project {
                root: PathBuf::from("/"),
                pinned: false,
                group: None,
            }],
            recent: vec![],
        };
        assert_eq!(projects(&state)[0].0.title, "/");
    }
}
