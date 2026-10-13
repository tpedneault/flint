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
//! - **Fenix's commands** (`:`): those Fenix says may be run from
//!   outside, by description, with the keys that run them in Fenix
//!   (`SPC a a`) in the subtitle, so Flint teaches Fenix's keys too.
//! - **The agenda** (`t`): the tasks that aren't done, the one being
//!   clocked first; `Enter` clocks in, or out on the running one.
//! - **Capture** (`n <text>`): one result that adds the text to the
//!   notebook's Inbox (`Enter`) or today's journal; Fenix writes it, and
//!   doesn't take the focus.
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
/// Fenix's commands' provider.
pub const COMMANDS: &str = "fenix.commands";
/// Its badge and chip.
pub const COMMANDS_TITLE: &str = "Fenix";
/// Its scope keyword, as Fenix's own command line is reached.
pub const COMMANDS_PREFIX: &str = ":";
/// The agenda's provider.
pub const AGENDA: &str = "fenix.agenda";
/// Its badge and chip.
pub const AGENDA_TITLE: &str = "Agenda";
/// Its scope keyword.
pub const AGENDA_PREFIX: &str = "t";
/// Capture's provider.
pub const CAPTURE: &str = "fenix.capture";
/// Its badge and chip.
pub const CAPTURE_TITLE: &str = "Notebook";
/// Its scope keyword.
pub const CAPTURE_PREFIX: &str = "n";
/// How far the task being clocked rises above the others.
pub const CLOCKED_BOOST: f64 = 20.0;
/// How far a task in progress rises above one still to do.
pub const IN_PROGRESS_BOOST: f64 = 5.0;

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

/// Fenix's commands (the external ones, as `control::Session::commands`
/// gives them) as items.
pub fn commands(commands: &[crate::control::CommandInfo]) -> Vec<Item> {
    commands
        .iter()
        .map(|c| Item {
            id: format!("{COMMANDS}:{}", c.id),
            title: c.description.chars().take(300).collect(),
            subtitle: Some(match &c.keys {
                Some(keys) => format!("{keys} · {}", c.id),
                None => c.id.clone(),
            }),
            icon: Some("command".into()),
            keywords: vec![c.id.clone()],
            actions: vec![action(
                "run",
                "Run in Fenix",
                ActionKind::Fenix {
                    method: "fenix/runCommand".into(),
                    params: json!({ "id": c.id }),
                },
            )],
        })
        .collect()
}

/// `95` minutes as `1 h 35 min`.
fn duration(minutes: i64) -> String {
    match (minutes.max(0) / 60, minutes.max(0) % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The agenda's open tasks as items, each with its ranking boost: the
/// one being clocked (`timer`) first, then those in progress.
pub fn tasks(
    tasks: &[crate::control::AgendaTask],
    timer: Option<&crate::control::AgendaTimer>,
) -> Vec<(Item, f64)> {
    use crate::control::TaskStatus;
    tasks
        .iter()
        .map(|t| {
            let running = timer.is_some_and(|r| r.task_id == t.id);
            let status = match t.status {
                TaskStatus::Todo => "To do",
                TaskStatus::InProgress => "In progress",
                TaskStatus::Blocked => "Blocked",
                TaskStatus::Done => "Done",
            };
            let mut details = vec![if running {
                "clocked in".to_owned()
            } else {
                status.to_owned()
            }];
            details.extend(t.jira_key.clone());
            details.push(duration(t.elapsed_minutes));
            let clock = if running {
                action(
                    "clock-out",
                    "Clock out",
                    ActionKind::Fenix {
                        method: "fenix/agenda.clockOut".into(),
                        params: json!({}),
                    },
                )
            } else {
                action(
                    "clock-in",
                    "Clock in",
                    ActionKind::Fenix {
                        method: "fenix/agenda.clockIn".into(),
                        params: json!({ "taskId": t.id }),
                    },
                )
            };
            let mut actions = vec![clock];
            if let Some(key) = &t.jira_key {
                actions.push(action(
                    "copy-key",
                    "Copy Jira key",
                    ActionKind::Copy { text: key.clone() },
                ));
            }
            let item = Item {
                id: format!("{AGENDA}:{}", t.id),
                title: t.title.chars().take(300).collect(),
                subtitle: Some(details.join(" · ")),
                icon: Some("task".into()),
                keywords: t.jira_key.iter().cloned().collect(),
                actions,
            };
            let boost = if running {
                CLOCKED_BOOST
            } else if t.status == TaskStatus::InProgress {
                IN_PROGRESS_BOOST
            } else {
                0.0
            };
            (item, boost)
        })
        .collect()
}

/// The capture result for `text` typed after `n `: `Enter` adds it to the
/// notebook's Inbox, the menu to today's journal.
pub fn capture(text: &str) -> Option<Item> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let to = |target: &str| ActionKind::Fenix {
        method: "fenix/notebook.capture".into(),
        params: json!({ "text": text, "target": target }),
    };
    Some(Item {
        // One id: frecency learns that capture is wanted, not the words.
        id: format!("{CAPTURE}:note"),
        title: text.chars().take(300).collect(),
        subtitle: Some("capture to the notebook's Inbox".into()),
        icon: Some("note".into()),
        keywords: vec![],
        actions: vec![
            action("capture-inbox", "Capture to Inbox", to("inbox")),
            action(
                "capture-journal",
                "Capture to today's journal",
                to("journal"),
            ),
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
    fn commands_show_their_keys_and_run_in_fenix() {
        use crate::control::CommandInfo;
        let items = commands(&[
            CommandInfo {
                id: "agenda.open".into(),
                description: "Open the agenda".into(),
                keys: Some("SPC a a".into()),
                external: true,
            },
            CommandInfo {
                id: "home.open".into(),
                description: "Home".into(),
                keys: None,
                external: true,
            },
        ]);
        assert!(items.iter().all(|i| i.check().is_ok()));
        assert_eq!(items[0].title, "Open the agenda");
        assert_eq!(items[0].subtitle.as_deref(), Some("SPC a a · agenda.open"));
        assert_eq!(items[1].subtitle.as_deref(), Some("home.open"));
        assert_eq!(
            items[0].actions[0].kind,
            ActionKind::Fenix {
                method: "fenix/runCommand".into(),
                params: json!({ "id": "agenda.open" }),
            }
        );
    }

    #[test]
    fn the_clocked_task_comes_first_and_clocks_out() {
        use crate::control::{AgendaTask, AgendaTimer, TaskStatus};
        let task = |id, title: &str, status| AgendaTask {
            id,
            title: title.into(),
            status,
            jira_key: (id == 7).then(|| "OPS-12".to_string()),
            elapsed_minutes: 95,
        };
        let list = [
            task(3, "Write the report", TaskStatus::Todo),
            task(7, "Review telemetry", TaskStatus::InProgress),
            task(9, "Order parts", TaskStatus::InProgress),
        ];
        let timer = AgendaTimer {
            task_id: 7,
            title: "Review telemetry".into(),
            since: "2026-10-13T09:00:00Z".into(),
        };
        let items = tasks(&list, Some(&timer));
        assert!(items.iter().all(|(i, _)| i.check().is_ok()));
        let (review, boost) = &items[1];
        assert_eq!(*boost, CLOCKED_BOOST);
        assert_eq!(
            review.subtitle.as_deref(),
            Some("clocked in · OPS-12 · 1 h 35 min")
        );
        assert_eq!(review.actions[0].title, "Clock out");
        assert_eq!(review.actions[1].title, "Copy Jira key");
        let (report, boost) = &items[0];
        assert_eq!(*boost, 0.0);
        assert_eq!(report.subtitle.as_deref(), Some("To do · 1 h 35 min"));
        assert_eq!(
            report.actions[0].kind,
            ActionKind::Fenix {
                method: "fenix/agenda.clockIn".into(),
                params: json!({ "taskId": 3 }),
            }
        );
        assert_eq!(items[2].1, IN_PROGRESS_BOOST);
        assert_eq!(duration(0), "0 min");
        assert_eq!(duration(120), "2 h");
    }

    #[test]
    fn capture_offers_the_inbox_then_the_journal() {
        assert!(capture("   ").is_none());
        let item = capture(" buy film ").unwrap();
        assert!(item.check().is_ok());
        assert_eq!(item.title, "buy film");
        assert_eq!(
            item.actions[1].kind,
            ActionKind::Fenix {
                method: "fenix/notebook.capture".into(),
                params: json!({ "text": "buy film", "target": "journal" }),
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
