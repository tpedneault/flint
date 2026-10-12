//! The Flint provider: Flint's own commands as items (prefix `flint`).
//!
//! Each is a `provider` action, carried out by the daemon itself, the same
//! way an external provider's `execute` would be; there is no special path
//! for built-in commands in the action handling.

use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;

/// The provider's id.
pub const ID: &str = "flint";
/// Its badge and chip.
pub const TITLE: &str = "Flint";
/// Its scope keyword.
pub const PREFIX: &str = "flint";

/// A command the daemon carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Read `settings.toml` again and report problems.
    Reload,
    /// Open `settings.toml` in the default editor.
    Settings,
    /// Open the log.
    Log,
    /// Stop Flint.
    Quit,
}

const ALL: [(Command, &str, &str, &str); 4] = [
    (
        Command::Reload,
        "reload",
        "Reload settings",
        "read settings.toml again",
    ),
    (
        Command::Settings,
        "settings",
        "Open settings",
        "settings.toml in your editor",
    ),
    (Command::Log, "log", "Show log", "flint.log"),
    (
        Command::Quit,
        "quit",
        "Quit Flint",
        "stop the background process",
    ),
];

/// The commands as items.
pub fn items() -> Vec<Item> {
    ALL.iter()
        .map(|&(_, name, title, subtitle)| Item {
            id: format!("{ID}:{name}"),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            icon: Some("command".into()),
            keywords: vec!["flint".into()],
            actions: vec![Action {
                id: name.into(),
                title: title.into(),
                key: None,
                kind: ActionKind::Provider,
            }],
        })
        .collect()
}

/// The command's title, as its item shows it.
pub fn title(command: Command) -> &'static str {
    ALL.iter()
        .find(|(c, ..)| *c == command)
        .map_or("", |&(_, _, title, _)| title)
}

/// The command an action id names.
pub fn command(action_id: &str) -> Option<Command> {
    ALL.iter()
        .find(|(_, name, _, _)| *name == action_id)
        .map(|&(command, ..)| command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_names_its_command() {
        for item in items() {
            assert!(item.check().is_ok());
            assert!(command(&item.actions[0].id).is_some(), "{}", item.id);
        }
        assert_eq!(command("quit"), Some(Command::Quit));
        assert_eq!(command("nope"), None);
    }
}
