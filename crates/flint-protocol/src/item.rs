//! Items and actions (PROTOCOL.md, "Item" and "Action"): what every
//! provider gives Flint, built in or external.
//!
//! Built-in providers produce exactly this shape, so the index, ranking
//! and action handling treat their items like any external provider's
//! (and a built-in could move out of process without its callers
//! noticing). Declarative action kinds are carried out by Flint itself, so
//! a static provider needs no round trip and one policy applies to all.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Longest item id, in bytes.
pub const MAX_ID: usize = 512;
/// Longest title, in characters.
pub const MAX_TITLE: usize = 300;

/// Something Flint can show and act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Stable across sessions and unique within the provider; prefixed
    /// with the provider id. Frecency is keyed on it.
    pub id: String,
    /// Matched and highlighted.
    pub title: String,
    /// Shown dimmed; matched at a lower weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// A name from Flint's icon set, or an absolute path to an image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Extra words to match, not shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// At least one; the first is the `Enter` action.
    pub actions: Vec<Action>,
}

/// Something an item can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// Unique within the item.
    pub id: String,
    /// Shown in the action menu.
    pub title: String,
    /// A one-character accelerator in the action menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// What it does.
    #[serde(flatten)]
    pub kind: ActionKind,
}

/// What an action does, by its `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ActionKind {
    /// Opens a path or URL with the OS's default handler.
    Open {
        /// The path or URL.
        target: String,
    },
    /// Shows a path in the file manager.
    Reveal {
        /// The path.
        target: String,
    },
    /// Puts text on the clipboard.
    Copy {
        /// The text.
        text: String,
    },
    /// Spawns a program directly, never through a shell.
    Run {
        /// The program and its arguments.
        argv: Vec<String>,
        /// Where it runs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// In the user's terminal.
        #[serde(default)]
        terminal: bool,
    },
    /// Calls Fenix's control protocol (M7).
    Fenix {
        /// The method.
        method: String,
        /// Its params.
        #[serde(default)]
        params: Value,
    },
    /// Replaces the query line and keeps Flint open (drill-down).
    Query {
        /// The new query.
        text: String,
    },
    /// Sends `execute` to the provider.
    Provider,
    /// A kind this Flint doesn't know: shown disabled, not dropped, so a
    /// newer provider on an older Flint fails visibly.
    #[serde(other)]
    Unknown,
}

/// The accelerator Flint's built-in providers give an action, by its id,
/// so the same key does the same thing on every item: `o` opens, `l`
/// launches, `e` edits in Fenix, `f` shows in its folder, `t` opens a
/// terminal, `c` copies. External providers choose their own keys.
pub fn conventional_key(action_id: &str) -> Option<String> {
    let key = match action_id {
        "open" | "open-folder" => 'o',
        "launch" => 'l',
        "open-in-fenix" => 'e',
        "reveal" => 'f',
        "terminal" => 't',
        "copy" | "copy-path" => 'c',
        _ => return None,
    };
    Some(key.to_string())
}

impl Item {
    /// Checks the limits PROTOCOL.md sets. The error names the item.
    pub fn check(&self) -> Result<(), String> {
        if self.id.is_empty() || self.id.len() > MAX_ID {
            return Err(format!("item id must be 1–{MAX_ID} bytes: {:?}", self.id));
        }
        if self.title.chars().count() > MAX_TITLE {
            return Err(format!(
                "item `{}`: title over {MAX_TITLE} characters",
                self.id
            ));
        }
        if self.actions.is_empty() {
            return Err(format!("item `{}` has no actions", self.id));
        }
        Ok(())
    }

    /// The `Enter` action.
    pub fn primary(&self) -> Option<&Action> {
        self.actions.first()
    }

    /// What `y` copies: the first copy action's text, else the title.
    pub fn primary_text(&self) -> &str {
        self.actions
            .iter()
            .find_map(|a| match &a.kind {
                ActionKind::Copy { text } => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or(&self.title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_documented_examples_parse() {
        let item: Item = serde_json::from_value(json!({
            "id": "jira:ABC-123",
            "title": "ABC-123 Telemetry export drops the last frame",
            "subtitle": "In Progress · assigned to you",
            "icon": "ticket",
            "keywords": ["telemetry", "export"],
            "actions": [
                { "id": "open", "title": "Open in browser", "key": "o", "kind": "open",
                  "target": "https://jira.example.com/browse/ABC-123" },
                { "id": "copy", "title": "Copy key", "kind": "copy", "text": "ABC-123" },
                { "id": "start-timer", "title": "Start timer", "kind": "provider" }
            ]
        }))
        .unwrap();
        assert!(item.check().is_ok());
        assert_eq!(item.primary().unwrap().key.as_deref(), Some("o"));
        assert_eq!(item.primary_text(), "ABC-123");
        assert_eq!(item.actions[2].kind, ActionKind::Provider);
    }

    #[test]
    fn run_actions_take_an_argv_and_round_trip() {
        let action = Action {
            id: "run".into(),
            title: "Run".into(),
            key: None,
            kind: ActionKind::Run {
                argv: vec!["cargo".into(), "test".into()],
                cwd: Some("/src".into()),
                terminal: true,
            },
        };
        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "run");
        assert_eq!(value["argv"], json!(["cargo", "test"]));
        assert_eq!(serde_json::from_value::<Action>(value).unwrap(), action);
    }

    #[test]
    fn unknown_kinds_are_kept_not_dropped() {
        let action: Action = serde_json::from_value(
            json!({"id": "x", "title": "Teleport", "kind": "teleport", "where": "mars"}),
        )
        .unwrap();
        assert_eq!(action.kind, ActionKind::Unknown);
    }

    #[test]
    fn limits_are_checked() {
        let mut item = Item {
            id: "p:x".into(),
            title: "t".repeat(MAX_TITLE + 1),
            subtitle: None,
            icon: None,
            keywords: vec![],
            actions: vec![],
        };
        assert!(item.check().unwrap_err().contains("title"));
        item.title = "ok".into();
        assert!(item.check().unwrap_err().contains("no actions"));
        item.id = "x".repeat(MAX_ID + 1);
        assert!(item.check().unwrap_err().contains("id"));
        item.id = "p:x".into();
        assert_eq!(item.primary_text(), "ok");
    }
}
