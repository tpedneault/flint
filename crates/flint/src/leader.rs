//! Turning `[leader]` into what the window walks and what the daemon
//! runs.
//!
//! `flint-config` checks each binding's shape; only the daemon knows which
//! providers, quicklinks and commands exist, so this is where
//! `scope:fenix.recent` becomes the Recent files chip and an unknown
//! `scope:nope` becomes one line of explanation. A binding without a label
//! gets one from what it does (the chip's name, the command's title), so
//! the which-key popup always says something useful.

use flint_config::leader::spaced;
use flint_config::LeaderBinding;
use flint_gui::{Chip, LeaderEntry};
use flint_providers::commands;

use crate::catalog::Catalog;

/// What a leader binding does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaderAction {
    /// Scope the query and go to Insert mode.
    Scope(Chip),
    /// Run one of Flint's commands, by its action id (`reload`).
    Flint(String),
    /// Run a Fenix command (needs Fenix's control protocol).
    Fenix(String),
    /// Replace the query line and go to Insert mode.
    Query(String),
}

/// The bindings, resolved: the entries the window builds its trie from
/// (each `target` an index into the returned actions) and lines for the
/// bindings that name something that doesn't exist.
pub fn resolve(
    bindings: &[LeaderBinding],
    catalog: &Catalog,
) -> (Vec<LeaderEntry>, Vec<LeaderAction>, Vec<String>) {
    let mut entries = Vec::new();
    let mut actions = Vec::new();
    let mut problems = Vec::new();
    for binding in bindings {
        let Some(text) = &binding.action else {
            entries.push(LeaderEntry {
                keys: binding.keys.clone(),
                label: binding.label.clone().unwrap_or_default(),
                target: None,
            });
            continue;
        };
        let (kind, argument) = text.split_once(':').unwrap_or((text.as_str(), ""));
        let argument = argument.trim();
        let resolved = match kind {
            "scope" => catalog
                .chip_for(argument)
                .map(|chip| (chip.label.clone(), LeaderAction::Scope(chip)))
                .ok_or_else(|| format!("there's no provider or quicklink `{argument}`")),
            "flint" => commands::command(argument)
                .map(|c| {
                    (
                        commands::title(c).to_owned(),
                        LeaderAction::Flint(argument.to_owned()),
                    )
                })
                .ok_or_else(|| format!("Flint has no command `{argument}`")),
            "fenix" => Ok((
                format!("Fenix: {argument}"),
                LeaderAction::Fenix(argument.to_owned()),
            )),
            "query" => Ok((
                format!("Search {argument}"),
                LeaderAction::Query(argument.to_owned()),
            )),
            other => Err(format!("`{other}:` isn't an action Flint knows")),
        };
        match resolved {
            Ok((default_label, action)) => {
                entries.push(LeaderEntry {
                    keys: binding.keys.clone(),
                    label: binding.label.clone().unwrap_or(default_label),
                    target: Some(actions.len()),
                });
                actions.push(action);
            }
            Err(why) => problems.push(format!(
                "`leader` \"{}\": {why}; it is ignored",
                spaced(&binding.keys)
            )),
        }
    }
    (entries, actions, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_index::Frecency;

    fn binding(keys: &str, action: Option<&str>, label: Option<&str>) -> LeaderBinding {
        LeaderBinding {
            keys: keys.chars().filter(|c| *c != ' ').collect(),
            action: action.map(str::to_owned),
            label: label.map(str::to_owned),
        }
    }

    #[test]
    fn the_defaults_all_resolve() {
        let catalog = Catalog::new(Frecency::default());
        let (entries, actions, problems) = resolve(&flint_config::leader::defaults(), &catalog);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(entries.len(), actions.len());
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Apps",
                "Projects",
                "Recent files",
                "Windows",
                "Flint",
                "Reload settings",
                "Open settings"
            ]
        );
    }

    #[test]
    fn labels_default_from_the_action_and_unknown_targets_are_explained() {
        let catalog = Catalog::new(Frecency::default());
        let (entries, actions, problems) = resolve(
            &[
                binding("f r", Some("scope:fenix.recent"), None),
                binding("x", Some("scope:nope"), None),
                binding("q", Some("flint:frobnicate"), None),
                binding("t", Some("fenix:agenda.toggle_clock"), Some("Clock in/out")),
                binding("w", Some("query:w "), None),
                binding("f", None, Some("Fenix")),
            ],
            &catalog,
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("`leader` \"x\": there's no provider or quicklink `nope`"));
        assert!(problems[1].contains("no command `frobnicate`"));
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Recent files", "Clock in/out", "Search w", "Fenix"]
        );
        assert_eq!(entries[3].target, None);
        assert_eq!(
            actions[1],
            LeaderAction::Fenix("agenda.toggle_clock".into())
        );
        assert_eq!(entries[1].target, Some(1));
    }
}
