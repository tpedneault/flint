//! The provider messages (PROTOCOL.md, "Provider messages"): what a
//! provider says to the host after `hello`, and what the host says back.
//!
//! Provider → host: `provider/register` (request), `provider/setItems`,
//! `results` and `log` (notifications). Host → provider: `query` and
//! `cancel` (notifications) and `execute` (request). Every struct reads
//! what a newer provider might add without complaint (unknown fields are
//! ignored) and fills absent optional fields with the documented defaults.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::item::Item;
use crate::message::{code, params, Id, RpcError};

/// The environment variable a provider started with a `credential`
/// finds its secret in (docs/PROVIDERS.md). Only the process Flint starts
/// gets it; it is never written anywhere.
pub const SECRET_ENV: &str = "FLINT_PROVIDER_SECRET";

/// Most items in one `results` or `provider/setItems` batch.
pub const MAX_BATCH: usize = 200;
/// Largest `debounceMs` honoured.
pub const MAX_DEBOUNCE_MS: u64 = 1000;

/// `provider/register`'s params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Register {
    /// Must equal `hello.name`.
    pub id: String,
    /// The badge and scope chip.
    pub title: String,
    /// The default scope keyword; settings may override it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Wants `query` notifications as the user types.
    #[serde(default)]
    pub dynamic: bool,
    /// Least time between `query` notifications, capped at
    /// [`MAX_DEBOUNCE_MS`].
    #[serde(default)]
    pub debounce_ms: u64,
    /// No `query` for shorter text.
    #[serde(default)]
    pub min_query_length: usize,
    /// Static items, indexed and matched by Flint itself.
    #[serde(default)]
    pub items: Vec<Item>,
}

/// `provider/setItems`'s params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetItems {
    /// The provider's static items, replacing the previous ones.
    pub items: Vec<Item>,
}

/// Who is in front when Flint is summoned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Foreground {
    /// Its executable's file name (`fenix.exe`).
    pub process: String,
    /// Its window title.
    pub title: String,
    /// Its process id.
    pub pid: u32,
}

/// Fenix's state when it is in front (M7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FenixContext {
    /// The current project's root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The current file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// What a query is made in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    /// The window in front before Flint showed itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<Foreground>,
    /// Fenix's state, when Fenix is in front and answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fenix: Option<FenixContext>,
}

/// `query`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    /// Increases with every query the host sends.
    pub query_id: u64,
    /// The query text, without the scope keyword.
    pub text: String,
    /// The user typed this provider's prefix.
    pub scoped: bool,
    /// Where the query was made.
    #[serde(default)]
    pub context: Context,
}

/// `results`'s params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    /// The query these answer.
    pub query_id: u64,
    /// At most [`MAX_BATCH`] items.
    pub items: Vec<Item>,
    /// The last batch for this query.
    #[serde(default)]
    pub done: bool,
}

/// `cancel`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cancel {
    /// The query to stop working on.
    pub query_id: u64,
}

/// `execute`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execute {
    /// The item's id.
    pub item_id: String,
    /// The action's id.
    pub action_id: String,
    /// The query the item was found for.
    pub query: String,
}

/// `execute`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteResult {
    /// Hide Flint (default) or keep it open.
    #[serde(default = "yes")]
    pub close: bool,
    /// Shown in the footer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

fn yes() -> bool {
    true
}

/// How serious a `log` line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Routine.
    Info,
    /// Worth a look.
    Warn,
    /// Something failed.
    Error,
}

/// `log`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogParams {
    /// How serious.
    pub level: LogLevel,
    /// The line.
    pub message: String,
}

/// A message a provider sent, checked.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderMessage {
    /// `provider/register`; answer `id` with `{}`.
    Register {
        /// The request's id.
        id: Id,
        /// What the provider is.
        register: Register,
    },
    /// `provider/setItems`.
    SetItems(SetItems),
    /// `results`.
    Results(Results),
    /// `log`.
    Log(LogParams),
}

impl ProviderMessage {
    /// A provider's request. The error is the one to send back.
    pub fn request(id: Id, method: &str, raw: Value) -> Result<Self, RpcError> {
        match method {
            "provider/register" => {
                let mut register: Register = params(method, raw)?;
                register.debounce_ms = register.debounce_ms.min(MAX_DEBOUNCE_MS);
                Ok(ProviderMessage::Register { id, register })
            }
            other => Err(RpcError::new(
                code::METHOD_NOT_FOUND,
                format!("unknown method `{other}`"),
            )),
        }
    }

    /// A provider's notification: `Ok(None)` for one this host doesn't
    /// know (ignored, as PROTOCOL.md's versioning rule asks); `Err` names
    /// what was wrong with one it knows.
    pub fn notification(method: &str, raw: Value) -> Result<Option<Self>, String> {
        let message = match method {
            "provider/setItems" => {
                ProviderMessage::SetItems(params(method, raw).map_err(|e| e.message)?)
            }
            "results" => {
                let results: Results = params(method, raw).map_err(|e| e.message)?;
                if results.items.len() > MAX_BATCH {
                    return Err(format!(
                        "`results` of {} items is over the {MAX_BATCH}-item batch limit",
                        results.items.len()
                    ));
                }
                ProviderMessage::Results(results)
            }
            "log" => ProviderMessage::Log(params(method, raw).map_err(|e| e.message)?),
            _ => return Ok(None),
        };
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn register_takes_the_documented_shape_and_defaults() {
        let m = ProviderMessage::request(
            Id::Number(2),
            "provider/register",
            json!({"id": "jira", "title": "Jira", "prefix": "j", "dynamic": true,
                   "debounceMs": 5000, "minQueryLength": 2, "items": []}),
        )
        .unwrap();
        let ProviderMessage::Register { register, .. } = m else {
            panic!()
        };
        assert_eq!(register.debounce_ms, MAX_DEBOUNCE_MS, "capped");
        assert_eq!(register.min_query_length, 2);
        let m = ProviderMessage::request(
            Id::Number(2),
            "provider/register",
            json!({"id": "x", "title": "X"}),
        )
        .unwrap();
        let ProviderMessage::Register { register, .. } = m else {
            panic!()
        };
        assert!(!register.dynamic && register.items.is_empty() && register.prefix.is_none());
    }

    #[test]
    fn notifications_are_checked_and_unknown_ones_ignored() {
        let results = ProviderMessage::notification(
            "results",
            json!({"queryId": 3, "items": [], "done": true}),
        );
        assert!(matches!(
            results,
            Ok(Some(ProviderMessage::Results(Results {
                query_id: 3,
                done: true,
                ..
            })))
        ));
        assert_eq!(
            ProviderMessage::notification("progress", json!({})),
            Ok(None)
        );
        assert!(
            ProviderMessage::notification("log", json!({"level": "loud", "message": "x"})).is_err()
        );
        let item = json!({"id": "p:x", "title": "x", "actions": [{"id": "c", "title": "c", "kind": "copy", "text": "x"}]});
        let flood = json!({"queryId": 1, "items": vec![item; MAX_BATCH + 1]});
        assert!(ProviderMessage::notification("results", flood)
            .unwrap_err()
            .contains("batch limit"));
    }

    #[test]
    fn execute_defaults_to_closing() {
        let r: ExecuteResult = serde_json::from_value(json!({"message": "Clocked in"})).unwrap();
        assert!(r.close);
        assert!(ProviderMessage::request(Id::Number(1), "execute", Value::Null).is_err());
    }

    #[test]
    fn a_query_round_trips_with_its_context() {
        let q = Query {
            query_id: 731,
            text: "ABC-12".into(),
            scoped: true,
            context: Context {
                foreground: Some(Foreground {
                    process: "fenix.exe".into(),
                    title: "main.rs".into(),
                    pid: 7016,
                }),
                fenix: None,
            },
        };
        let value = serde_json::to_value(&q).unwrap();
        assert_eq!(value["queryId"], 731);
        assert_eq!(value["context"]["foreground"]["process"], "fenix.exe");
        assert!(value["context"].get("fenix").is_none());
        assert_eq!(serde_json::from_value::<Query>(value).unwrap(), q);
    }
}
