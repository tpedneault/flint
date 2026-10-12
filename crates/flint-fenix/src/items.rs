//! Fenix's projects and recent files as items, the shape every provider
//! gives Flint.
//!
//! - **Projects** (`p`): pinned ones first, then Fenix's order (most
//!   recently used first), the group as part of the subtitle. `Enter`
//!   opens the project in Fenix (`fenix/openProject`, as `SPC p p` would);
//!   the folder, a terminal and the path are in the action menu.
//! - **Recent files** (`f`): most recent first. `Enter` opens the file in
//!   Fenix (`fenix/open`).
//! - **A typed path** (unscoped, absolute, naming a file that exists),
//!   optionally with `:line[:column]` as compilers and grep print it:
//!   one result that opens it in Fenix there. Windows' drive letter is
//!   part of the path (`C:\x.rs:12:3`).
//!
//! These are `fenix` actions: the daemon carries them out over Fenix's
//! control channel ([`crate::control`]), or by starting Fenix with them
//! when none is running ([`crate::handoff`]).

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
/// The provider of a typed path's result.
pub const GOTO: &str = "fenix.goto";
/// Its badge.
pub const GOTO_TITLE: &str = "Fenix";

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
        key: flint_protocol::item::conventional_key(id),
        kind,
    }
}

/// A path's last component and the folder before it, for display.
///
/// Split on either separator rather than with `Path`, which only knows
/// the host's: Fenix's state can hold Windows paths read on Linux (a
/// copied profile, these tests' fixtures), and a title must be the file's
/// name either way. A Unix file name with a backslash in it is shown
/// split; that is the cheaper surprise.
fn split_display(path: &Path) -> (String, Option<String>) {
    let text = path.display().to_string();
    let trimmed = text.trim_end_matches(['/', '\\']);
    match trimmed.rfind(['/', '\\']) {
        Some(i) if i + 1 < trimmed.len() => {
            let folder = &trimmed[..i];
            let folder = if folder.is_empty() || folder.ends_with(':') {
                // `/x` or `C:\x`: the folder is the root itself.
                trimmed[..=i].to_owned()
            } else {
                folder.to_owned()
            };
            (trimmed[i + 1..].to_owned(), Some(folder))
        }
        _ if trimmed.is_empty() => (text, None),
        _ => (trimmed.to_owned(), None),
    }
}

fn name_of(path: &Path) -> String {
    split_display(path).0
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
                        "open-in-fenix",
                        "Open in Fenix",
                        ActionKind::Fenix {
                            method: "fenix/openProject".into(),
                            params: json!({ "root": root }),
                        },
                    ),
                    action(
                        "open-folder",
                        "Open folder",
                        ActionKind::Open {
                            target: root.clone(),
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

/// A path typed into the query line, and where in it, if `text` is an
/// absolute path to a file (`is_file` says which exist) optionally
/// followed by `:line` or `:line:column`.
pub fn parse_goto(
    text: &str,
    is_file: impl Fn(&str) -> bool,
) -> Option<(String, Option<u64>, Option<u64>)> {
    let text = text.trim().trim_matches('"');
    if !is_absolute(text) {
        return None;
    }
    if is_file(text) {
        return Some((text.to_owned(), None, None));
    }
    let number = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let (rest, last) = text.rsplit_once(':')?;
    let last = number(last)?;
    if is_absolute(rest) && is_file(rest) {
        return Some((rest.to_owned(), Some(last), None));
    }
    let (path, line) = rest.rsplit_once(':')?;
    let line = number(line)?;
    (is_absolute(path) && is_file(path)).then(|| (path.to_owned(), Some(line), Some(last)))
}

/// `C:\x`, `C:/x`, `\\server\share`, `/x`: a path that doesn't depend
/// on a working directory (Flint's would mean nothing to the user).
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// The result for a typed path, if `text` is one: `Enter` opens it in
/// Fenix at the line and column given.
pub fn goto(text: &str, is_file: impl Fn(&str) -> bool) -> Option<Item> {
    let (path, line, column) = parse_goto(text, is_file)?;
    let (name, folder) = split_display(Path::new(&path));
    let at = match (line, column) {
        (Some(l), Some(c)) => format!(":{l}:{c}"),
        (Some(l), None) => format!(":{l}"),
        _ => String::new(),
    };
    let mut target = json!({ "path": path });
    if let Some(l) = line {
        target["line"] = json!(l);
    }
    if let Some(c) = column {
        target["column"] = json!(c);
    }
    Some(Item {
        id: format!("{GOTO}:{path}"),
        title: format!("{name}{at}"),
        subtitle: folder,
        icon: Some("file".into()),
        keywords: vec![],
        actions: vec![
            action(
                "open-in-fenix",
                "Open in Fenix",
                ActionKind::Fenix {
                    method: "fenix/open".into(),
                    params: json!({ "paths": [target] }),
                },
            ),
            action(
                "reveal",
                "Show in folder",
                ActionKind::Reveal {
                    target: path.clone(),
                },
            ),
            action("copy-path", "Copy path", ActionKind::Copy { text: path }),
        ],
    })
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
                subtitle: split_display(path).1,
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
    fn projects_open_in_fenix_and_pinned_ones_rise() {
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
            matches!(&fenix.actions[0].kind, ActionKind::Fenix { method, .. } if method == "fenix/openProject")
        );
        assert!(
            matches!(&fenix.actions[1].kind, ActionKind::Open { target } if target == r"C:\dev\fenix")
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
    fn a_typed_path_opens_at_its_position() {
        let files = [r"C:\src\x.rs", "/home/me/a.rs", r"C:\odd:12"];
        let is_file = |p: &str| files.contains(&p);
        let parse = |t: &str| parse_goto(t, is_file);
        assert_eq!(
            parse(r"C:\src\x.rs"),
            Some((r"C:\src\x.rs".into(), None, None))
        );
        assert_eq!(
            parse(r"  C:\src\x.rs:12:3 "),
            Some((r"C:\src\x.rs".into(), Some(12), Some(3)))
        );
        assert_eq!(
            parse("/home/me/a.rs:40"),
            Some(("/home/me/a.rs".into(), Some(40), None))
        );
        assert_eq!(
            parse(r#""C:\src\x.rs:7""#),
            Some((r"C:\src\x.rs".into(), Some(7), None)),
            "quoted, as copied from a terminal"
        );
        assert_eq!(
            parse(r"C:\odd:12"),
            Some((r"C:\odd:12".into(), None, None)),
            "a file of that name wins"
        );
        assert_eq!(
            parse("a.rs:12"),
            None,
            "relative paths mean nothing to Flint"
        );
        assert_eq!(parse(r"C:\src\missing.rs:1"), None);
        assert_eq!(parse(r"C:\src\x.rs:abc"), None);
        assert_eq!(parse("C:"), None);

        let item = goto(r"C:\src\x.rs:12:3", is_file).unwrap();
        assert!(item.check().is_ok());
        assert_eq!(item.title, "x.rs:12:3");
        assert_eq!(item.subtitle.as_deref(), Some(r"C:\src"));
        assert_eq!(
            item.actions[0].kind,
            ActionKind::Fenix {
                method: "fenix/open".into(),
                params: json!({ "paths": [{ "path": r"C:\src\x.rs", "line": 12, "column": 3 }] }),
            }
        );
    }

    #[test]
    fn names_and_folders_split_on_either_separator() {
        let split = |p: &str| split_display(Path::new(p));
        let pair = |name: &str, folder: Option<&str>| (name.to_owned(), folder.map(str::to_owned));
        assert_eq!(
            split(r"C:\dev\fenix\src\main.rs"),
            pair("main.rs", Some(r"C:\dev\fenix\src"))
        );
        assert_eq!(split("/home/me/a.rs"), pair("a.rs", Some("/home/me")));
        assert_eq!(
            split(r"\\server\share\ops"),
            pair("ops", Some(r"\\server\share"))
        );
        assert_eq!(split(r"C:\dev\"), pair("dev", Some(r"C:\")));
        assert_eq!(split("/etc"), pair("etc", Some("/")));
        assert_eq!(split("fenix"), pair("fenix", None));
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
