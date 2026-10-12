//! The windows provider (prefix `w`): the open top-level windows, to
//! switch to (`Enter`) or close.
//!
//! The list itself is the OS's (`flint_platform::windows`, read with
//! `EnumWindows` just before Flint shows); this turns it into items. A
//! window has no identity that outlives it, so an item's id is its
//! program (`windows:firefox.exe`), numbered for the second and later
//! windows of one program in front-to-back order. Frecency then learns
//! which *program* you switch to, which is what survives a restart, and
//! the window in front ranks first among a program's windows.
//!
//! Windows only: Wayland gives ordinary clients no way to list or focus
//! other windows (KWin scripting over D-Bus is a later step), so on Linux
//! there are no items and the prefix says why.

use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;

/// The provider's id.
pub const ID: &str = "windows";
/// Its badge and chip.
pub const TITLE: &str = "Windows";
/// Its scope keyword.
pub const PREFIX: &str = "w";
/// The action that brings a window to the front (`Enter`).
pub const FOCUS: &str = "focus";
/// The action that asks a window to close, as its close button would.
pub const CLOSE: &str = "close";

/// A window, as the platform lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// The OS's handle, to act on it while it is open.
    pub handle: u64,
    /// Its title.
    pub title: String,
    /// Its program's file name (`firefox.exe`); empty when unknown.
    pub process: String,
}

/// The windows as items, in the order given (front to back), each with
/// the handle its actions act on.
pub fn items(windows: &[Window]) -> Vec<(Item, u64)> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    windows
        .iter()
        .map(|w| {
            let program = if w.process.is_empty() {
                "unknown"
            } else {
                w.process.as_str()
            };
            let n = match seen
                .iter_mut()
                .find(|(p, _)| p.eq_ignore_ascii_case(program))
            {
                Some((_, count)) => {
                    *count += 1;
                    *count
                }
                None => {
                    seen.push((program, 1));
                    1
                }
            };
            let key = program.to_ascii_lowercase();
            let id = if n == 1 {
                format!("{ID}:{key}")
            } else {
                format!("{ID}:{key}#{n}")
            };
            let action = |id: &str, title: &str, key: &str| Action {
                id: id.into(),
                title: title.into(),
                key: Some(key.into()),
                kind: ActionKind::Provider,
            };
            let item = Item {
                id,
                // Titles can be longer than an item's 300 characters.
                title: w.title.chars().take(300).collect(),
                subtitle: Some(program.trim_end_matches(".exe").to_owned()),
                icon: Some("window".into()),
                // The program's name matches too: `w code` finds every
                // VS Code window whatever its title.
                keywords: vec![program.trim_end_matches(".exe").to_owned()],
                actions: vec![
                    action(FOCUS, "Switch to", "s"),
                    action(CLOSE, "Close window", "x"),
                ],
            };
            (item, w.handle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(handle: u64, title: &str, process: &str) -> Window {
        Window {
            handle,
            title: title.into(),
            process: process.into(),
        }
    }

    #[test]
    fn ids_name_the_program_and_number_its_later_windows() {
        let list = [
            window(10, "Inbox - Mail", "firefox.exe"),
            window(20, "main.rs - fenix", "fenix.exe"),
            window(30, "Docs", "Firefox.exe"),
            window(40, "Untitled", ""),
        ];
        let items = items(&list);
        let ids: Vec<&str> = items.iter().map(|(i, _)| i.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "windows:firefox.exe",
                "windows:fenix.exe",
                "windows:firefox.exe#2",
                "windows:unknown"
            ]
        );
        assert_eq!(items[2].1, 30, "each item keeps its window's handle");
        let (fenix, _) = &items[1];
        assert!(fenix.check().is_ok());
        assert_eq!(fenix.subtitle.as_deref(), Some("fenix"));
        assert_eq!(fenix.keywords, ["fenix"]);
        assert_eq!(fenix.actions[0].id, FOCUS, "Enter switches to it");
        assert_eq!(fenix.actions[1].id, CLOSE);
    }
}
