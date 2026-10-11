//! The Apps provider: installed applications as items (prefix `a`).
//!
//! - **Windows**: every `.lnk` under the user's and all users' Start Menu
//!   `Programs` folders, which is what the Start menu itself lists (Scoop's
//!   "Scoop Apps" folder lives there too). The shortcut is launched as it
//!   is, through the shell, so its arguments, working directory and "run
//!   as" settings are honoured; its target is never resolved, which keeps a
//!   scan of a few hundred shortcuts to a few milliseconds.
//! - **Linux**: every `.desktop` file under the XDG data directories'
//!   `applications` folders, earlier directories winning for the same
//!   desktop-file id, as the spec says; see [`crate::desktop`].
//!
//! Discovery is file-system walking, not OS calls, so it lives here and is
//! tested on any OS against a temporary folder. Scanning is the caller's
//! to schedule (the daemon runs it on a background thread at start and
//! again when Flint is shown after a while), since a cold disk can make it
//! slow and the window must never wait for it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;

use crate::desktop;

/// The provider's id.
pub const ID: &str = "apps";
/// Its badge and chip.
pub const TITLE: &str = "Apps";
/// Its scope keyword.
pub const PREFIX: &str = "a";

/// How deep under a root a shortcut may be: Start Menu folders nest one
/// or two levels; anything deeper is a vendor's mistake, not a menu.
const MAX_DEPTH: usize = 4;

/// Lists this machine's applications.
pub fn scan() -> Vec<Item> {
    if cfg!(windows) {
        scan_start_menus(&start_menus())
    } else {
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var("LANG").ok());
        let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        scan_desktop_dirs(&application_dirs(), locale.as_deref(), &desktops, on_path)
    }
}

/// The user's and all users' Start Menu `Programs` folders.
fn start_menus() -> Vec<PathBuf> {
    ["APPDATA", "PROGRAMDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs"))
        .collect()
}

/// `$XDG_DATA_HOME/applications` then each `$XDG_DATA_DIRS/applications`,
/// with the spec's defaults.
fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .map(|d| d.join("applications"))
        .collect()
}

fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Every `.lnk` under `roots` as an item, the first root winning when two
/// have the same shortcut at the same place (the user's over all users').
pub fn scan_start_menus(roots: &[PathBuf]) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for root in roots {
        for path in walk(root, "lnk") {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let key = relative.to_string_lossy().replace('\\', "/").to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let folder = relative
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', " › "))
                .filter(|p| !p.is_empty());
            let target = path.display().to_string();
            items.push(Item {
                id: format!("{ID}:{key}"),
                title: name,
                subtitle: folder,
                icon: Some("app".into()),
                keywords: vec![],
                actions: vec![
                    action(
                        "launch",
                        "Launch",
                        ActionKind::Open {
                            target: target.clone(),
                        },
                    ),
                    action(
                        "reveal",
                        "Open containing folder",
                        ActionKind::Reveal {
                            target: target.clone(),
                        },
                    ),
                    action("copy-path", "Copy path", ActionKind::Copy { text: target }),
                ],
            });
        }
    }
    items
}

/// Every listable `.desktop` file under `dirs` as an item.
pub fn scan_desktop_dirs(
    dirs: &[PathBuf],
    locale: Option<&str>,
    desktops: &[String],
    installed: impl Fn(&str) -> bool,
) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for dir in dirs {
        for path in walk(dir, "desktop") {
            let Ok(relative) = path.strip_prefix(dir) else {
                continue;
            };
            // The desktop-file id: the relative path with `/` as `-`.
            let id = relative.to_string_lossy().replace(['/', '\\'], "-");
            // An earlier directory's file of the same id hides this one,
            // even when that one isn't listable (that's how a user hides a
            // system entry).
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(entry) = desktop::parse(&text, &path, locale, desktops, &installed) else {
                continue;
            };
            let file = path.display().to_string();
            items.push(Item {
                id: format!("{ID}:{id}"),
                title: entry.name,
                subtitle: entry.description,
                icon: Some("app".into()),
                keywords: entry.keywords,
                actions: vec![
                    action(
                        "launch",
                        "Launch",
                        ActionKind::Run {
                            argv: entry.argv,
                            cwd: entry.cwd,
                            terminal: entry.terminal,
                        },
                    ),
                    action(
                        "reveal",
                        "Open containing folder",
                        ActionKind::Reveal {
                            target: file.clone(),
                        },
                    ),
                    action("copy-path", "Copy path", ActionKind::Copy { text: file }),
                ],
            });
        }
    }
    items
}

fn action(id: &str, title: &str, kind: ActionKind) -> Action {
    Action {
        id: id.into(),
        title: title.into(),
        key: None,
        kind,
    }
}

/// Files with `extension` under `root`, at most [`MAX_DEPTH`] deep, in a
/// stable order. Unreadable folders are skipped: one locked folder must
/// not hide every other app.
fn walk(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-apps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn start_menu_shortcuts_become_launchable_items() {
        let user = temp("user");
        let common = temp("common");
        fs::create_dir_all(user.join("Accessibility")).unwrap();
        fs::write(user.join("Firefox.lnk"), b"").unwrap();
        fs::write(user.join("Accessibility").join("Magnifier.lnk"), b"").unwrap();
        fs::write(user.join("readme.txt"), b"").unwrap();
        fs::write(common.join("FIREFOX.LNK"), b"").unwrap();
        fs::write(common.join("Paint.lnk"), b"").unwrap();

        let items = scan_start_menus(&[user.clone(), common.clone()]);
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Magnifier", "Firefox", "Paint"]);
        let magnifier = &items[0];
        assert_eq!(magnifier.id, "apps:accessibility/magnifier.lnk");
        assert_eq!(magnifier.subtitle.as_deref(), Some("Accessibility"));
        assert!(
            matches!(&magnifier.actions[0].kind, ActionKind::Open { target } if target.ends_with("Magnifier.lnk"))
        );
        assert!(items.iter().all(|i| i.check().is_ok()));
        let firefox = &items[1];
        assert!(
            firefox.primary_text().starts_with(user.to_str().unwrap()),
            "the user's shortcut wins"
        );
    }

    #[test]
    fn desktop_files_become_run_actions_and_earlier_dirs_win() {
        let home = temp("home");
        let system = temp("system");
        fs::create_dir_all(system.join("kde")).unwrap();
        let app = |name: &str, exec: &str| {
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n")
        };
        fs::write(
            home.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nHidden=true\n",
        )
        .unwrap();
        fs::write(system.join("hidden.desktop"), app("Shown?", "y")).unwrap();
        fs::write(
            system.join("kde").join("dolphin.desktop"),
            app("Dolphin", "dolphin %u"),
        )
        .unwrap();
        fs::write(
            system.join("term.desktop"),
            format!("{}Terminal=true\n", app("Htop", "htop")),
        )
        .unwrap();

        let items = scan_desktop_dirs(&[home, system], None, &[], |_| true);
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Dolphin", "Htop"],
            "the user's Hidden entry hides the system one"
        );
        assert_eq!(items[0].id, "apps:kde-dolphin.desktop");
        assert!(
            matches!(&items[0].actions[0].kind, ActionKind::Run { argv, terminal: false, .. } if argv == &["dolphin"])
        );
        assert!(matches!(
            &items[1].actions[0].kind,
            ActionKind::Run { terminal: true, .. }
        ));
    }

    #[test]
    fn a_missing_root_is_simply_empty() {
        assert!(scan_start_menus(&[PathBuf::from("/definitely/not/here")]).is_empty());
    }
}
