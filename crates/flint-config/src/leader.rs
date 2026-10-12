//! `[leader]`: what `SPC` sequences do in Normal mode.
//!
//! ```toml
//! [leader]
//! "p"   = "scope:fenix.projects"
//! "f r" = "scope:fenix.recent"
//! "j"   = "scope:quicklink.j"
//! "t"   = { action = "fenix:agenda.toggle_clock", label = "Clock in/out" }
//! "g"   = { label = "Git" }          # a group's name in the which-key popup
//! ```
//!
//! Keys are single characters separated by spaces, as Fenix's which-key
//! shows them (`SPC f r`). An action is `scope:<provider>` (limit the
//! query to it), `flint:<command>`, `fenix:<command>` or `query:<text>`
//! (replace the query line). Which providers and commands exist is the
//! daemon's to check, since only it knows them; this module checks the
//! shape, and that no key both runs something and starts a longer
//! sequence (Fenix's trie can't tell those apart, so neither can Flint's).
//!
//! Without a `[leader]` table Flint uses [`defaults`]; with one, the table
//! is the whole map, so a user can remove a default by leaving it out.

use toml::Value;

/// The kinds of action a binding may name.
pub const ACTION_KINDS: [&str; 4] = ["scope", "flint", "fenix", "query"];

/// One `[leader]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderBinding {
    /// The keys after `SPC`.
    pub keys: Vec<char>,
    /// `kind:argument`; `None` for an entry that only names a group.
    pub action: Option<String>,
    /// What the which-key popup says; the daemon fills in a default from
    /// the action when absent.
    pub label: Option<String>,
}

/// The map used when `settings.toml` has no `[leader]`.
pub fn defaults() -> Vec<LeaderBinding> {
    let bind = |keys: &str, action: &str| LeaderBinding {
        keys: keys.chars().filter(|c| *c != ' ').collect(),
        action: Some(action.to_owned()),
        label: None,
    };
    vec![
        bind("a", "scope:apps"),
        bind("p", "scope:fenix.projects"),
        bind("f", "scope:fenix.recent"),
        bind("w", "scope:windows"),
        bind("l", "scope:flint"),
        bind("r", "flint:reload"),
        bind("s", "flint:settings"),
    ]
}

/// Reads a `[leader]` table, keeping good entries and explaining the rest.
pub fn parse(value: &Value) -> Result<(Vec<LeaderBinding>, Vec<String>), String> {
    let Value::Table(table) = value else {
        return Err(format!("expected a table, found {}", value.type_str()));
    };
    let mut bindings: Vec<LeaderBinding> = Vec::new();
    let mut problems = Vec::new();
    for (sequence, entry) in table {
        match binding(sequence, entry) {
            Ok(b) => bindings.push(b),
            Err(why) => problems.push(format!("\"{sequence}\": {why}")),
        }
    }
    // A key that runs something can't also start a longer sequence.
    let mut kept: Vec<LeaderBinding> = Vec::new();
    for b in bindings {
        let clash = kept.iter().find(|k| {
            (k.action.is_some() && b.keys.starts_with(&k.keys) && b.keys.len() > k.keys.len())
                || (b.action.is_some()
                    && k.keys.starts_with(&b.keys)
                    && k.keys.len() > b.keys.len())
                || (k.keys == b.keys && k.action.is_some() && b.action.is_some())
        });
        match clash {
            Some(k) => problems.push(format!(
                "\"{}\" clashes with \"{}\": a key either runs something or starts a longer sequence",
                spaced(&b.keys),
                spaced(&k.keys)
            )),
            None => kept.push(b),
        }
    }
    Ok((kept, problems))
}

fn binding(sequence: &str, entry: &Value) -> Result<LeaderBinding, String> {
    let keys = keys(sequence)?;
    let (action, label) = match entry {
        Value::String(action) => (Some(action.trim().to_owned()), None),
        Value::Table(t) => {
            let text = |k: &str| {
                t.get(k)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            if let Some(unknown) = t.keys().find(|k| *k != "action" && *k != "label") {
                return Err(format!("`{unknown}` isn't `action` or `label`"));
            }
            (
                text("action").map(str::to_owned),
                text("label").map(str::to_owned),
            )
        }
        other => {
            return Err(format!(
                "expected an action or a table, found {}",
                other.type_str()
            ))
        }
    };
    if action.is_none() && label.is_none() {
        return Err("names neither an action nor a label".into());
    }
    if let Some(action) = &action {
        check_action(action)?;
    }
    Ok(LeaderBinding {
        keys,
        action,
        label,
    })
}

/// `"f r"` → `['f', 'r']`.
fn keys(sequence: &str) -> Result<Vec<char>, String> {
    let mut keys = Vec::new();
    for token in sequence.split_whitespace() {
        let mut chars = token.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => keys.push(c),
            _ => {
                return Err(format!(
                    "`{token}` isn't a single key; separate keys with spaces"
                ))
            }
        }
    }
    if keys.is_empty() {
        return Err("no keys".into());
    }
    Ok(keys)
}

fn check_action(action: &str) -> Result<(), String> {
    let (kind, argument) = action
        .split_once(':')
        .ok_or_else(|| format!("`{action}` should look like `scope:apps` or `flint:reload`"))?;
    if !ACTION_KINDS.contains(&kind) {
        return Err(format!(
            "`{kind}:` isn't one of scope:, flint:, fenix:, query:"
        ));
    }
    if argument.trim().is_empty() && kind != "query" {
        return Err(format!("`{kind}:` needs what to act on"));
    }
    Ok(())
}

/// `['f', 'r']` → `"f r"`.
pub fn spaced(keys: &[char]) -> String {
    keys.iter()
        .map(char::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Vec<LeaderBinding>, Vec<String>) {
        let table: toml::Table = text.parse().unwrap();
        parse(&table["leader"]).unwrap()
    }

    #[test]
    fn the_designs_example_reads() {
        let (b, problems) = parsed(
            r#"[leader]
            "p"   = "scope:fenix.projects"
            "f r" = "scope:fenix.recent"
            "j"   = "scope:quicklink.j"
            "t"   = { action = "fenix:agenda.toggle_clock", label = "Clock in/out" }
            "f"   = { label = "Fenix" }
            "#,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(b.len(), 5);
        let fr = b.iter().find(|b| b.keys == ['f', 'r']).unwrap();
        assert_eq!(fr.action.as_deref(), Some("scope:fenix.recent"));
        let t = b.iter().find(|b| b.keys == ['t']).unwrap();
        assert_eq!(t.label.as_deref(), Some("Clock in/out"));
        let f = b.iter().find(|b| b.keys == ['f']).unwrap();
        assert_eq!(
            (f.action.as_deref(), f.label.as_deref()),
            (None, Some("Fenix"))
        );
    }

    #[test]
    fn bad_entries_are_explained_and_the_rest_kept() {
        let (b, problems) = parsed(
            r#"[leader]
            "ab"  = "scope:apps"
            "x"   = "launch:rockets"
            "y"   = "flint:"
            "z"   = 3
            "w"   = { act = "flint:reload" }
            "q"   = "flint:quit"
            "#,
        );
        assert_eq!(b.len(), 1, "{problems:?}");
        assert_eq!(problems.len(), 5);
        assert!(problems
            .iter()
            .any(|p| p.starts_with("\"ab\": `ab` isn't a single key")));
        assert!(problems
            .iter()
            .any(|p| p.contains("`launch:` isn't one of")));
        assert!(problems.iter().any(|p| p.contains("needs what to act on")));
        assert!(problems.iter().any(|p| p.contains("`act` isn't")));
    }

    #[test]
    fn a_key_cannot_both_run_and_continue() {
        let (b, problems) = parsed(
            r#"[leader]
            "f"   = "scope:fenix.recent"
            "f r" = "flint:reload"
            "#,
        );
        assert_eq!(b.len(), 1);
        assert!(problems[0].contains("clashes"), "{problems:?}");
    }

    #[test]
    fn the_defaults_are_valid() {
        for b in defaults() {
            check_action(b.action.as_deref().unwrap()).unwrap();
        }
        assert_eq!(spaced(&['f', 'r']), "f r");
    }
}
