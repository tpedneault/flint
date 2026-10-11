//! The JSON-RPC 2.0 envelope: requests, notifications, responses, and
//! errors with the codes PROTOCOL.md assigns.
//!
//! Messages are parsed by hand from a `serde_json::Value` rather than with
//! a derived `#[serde(untagged)]` enum, because what matters most here is
//! the error: a malformed message from a peer has to become the right
//! JSON-RPC error code with a message a person can act on, and untagged
//! enums only say "did not match any variant".

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Error codes (PROTOCOL.md, "Error codes").
pub mod code {
    /// The body is not JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// JSON, but not a JSON-RPC message.
    pub const INVALID_REQUEST: i64 = -32600;
    /// The method doesn't exist (for this role).
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The method exists but its params don't fit.
    pub const INVALID_PARAMS: i64 = -32602;
    /// Something went wrong on the answering side.
    pub const INTERNAL_ERROR: i64 = -32603;
    /// `hello.protocol` isn't one the host speaks.
    pub const UNSUPPORTED_PROTOCOL: i64 = -32001;
    /// A message before the `hello` exchange completed.
    pub const NOT_READY: i64 = -32002;
    /// A connecting provider that config doesn't declare.
    pub const NOT_DECLARED: i64 = -32003;
    /// The action needs something that's absent.
    pub const UNAVAILABLE: i64 = -32004;
    /// The provider's own failure.
    pub const PROVIDER_FAILED: i64 = -32010;
}

/// A request id. JSON-RPC allows numbers and strings; Flint sends numbers
/// and echoes whatever a peer sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// A numeric id.
    Number(i64),
    /// A string id.
    String(String),
}

/// A JSON-RPC error object. Its `message` is what the user sees in the
/// footer or on the command line, so it names what failed and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// One of [`code`]'s values, or a peer's own.
    pub code: i64,
    /// One line, for a person.
    pub message: String,
    /// Anything structured that goes with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// An error with no `data`.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// The same error carrying `data`.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpcError {}

/// One message, either direction.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Expects a response with the same `id`.
    Request {
        /// Echoed in the response.
        id: Id,
        /// The method name.
        method: String,
        /// `null` when the sender left them out.
        params: Value,
    },
    /// Expects nothing back.
    Notification {
        /// The method name.
        method: String,
        /// `null` when the sender left them out.
        params: Value,
    },
    /// The answer to a request. `id` is `None` only for an error about a
    /// message so broken its id couldn't be read.
    Response {
        /// The request's id.
        id: Option<Id>,
        /// `result` or `error`.
        outcome: Result<Value, RpcError>,
    },
}

impl Message {
    /// A request.
    pub fn request(id: Id, method: &str, params: Value) -> Self {
        Message::Request {
            id,
            method: method.to_owned(),
            params,
        }
    }

    /// A successful response.
    pub fn result(id: Id, result: Value) -> Self {
        Message::Response {
            id: Some(id),
            outcome: Ok(result),
        }
    }

    /// An error response.
    pub fn error(id: Option<Id>, error: RpcError) -> Self {
        Message::Response {
            id,
            outcome: Err(error),
        }
    }

    /// Parses a message body. The error is the one to send back.
    pub fn parse(body: &[u8]) -> Result<Message, RpcError> {
        let value: Value = serde_json::from_slice(body)
            .map_err(|e| RpcError::new(code::PARSE_ERROR, format!("not JSON: {e}")))?;
        let Value::Object(mut object) = value else {
            return Err(invalid("a message must be a JSON object"));
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(invalid("`jsonrpc` must be \"2.0\""));
        }
        let id = match object.remove("id") {
            None => None,
            Some(Value::Null) => None,
            Some(value) => Some(
                serde_json::from_value::<Id>(value)
                    .map_err(|_| invalid("`id` must be a number or a string"))?,
            ),
        };
        if let Some(method) = object.remove("method") {
            let Value::String(method) = method else {
                return Err(invalid("`method` must be a string"));
            };
            let params = object.remove("params").unwrap_or(Value::Null);
            if !matches!(params, Value::Null | Value::Object(_) | Value::Array(_)) {
                return Err(invalid("`params` must be an object or an array"));
            }
            return Ok(match id {
                Some(id) => Message::Request { id, method, params },
                None => Message::Notification { method, params },
            });
        }
        let outcome = match (object.remove("result"), object.remove("error")) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(serde_json::from_value::<RpcError>(error)
                .map_err(|_| invalid("`error` must have a numeric `code` and a `message`"))?),
            _ => {
                return Err(invalid(
                    "a message needs `method`, or exactly one of `result` and `error`",
                ))
            }
        };
        Ok(Message::Response { id, outcome })
    }

    /// The JSON body, ready to frame.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut object = Map::new();
        object.insert("jsonrpc".into(), json!("2.0"));
        match self {
            Message::Request { id, method, params } => {
                object.insert("id".into(), json!(id));
                object.insert("method".into(), json!(method));
                if !params.is_null() {
                    object.insert("params".into(), params.clone());
                }
            }
            Message::Notification { method, params } => {
                object.insert("method".into(), json!(method));
                if !params.is_null() {
                    object.insert("params".into(), params.clone());
                }
            }
            Message::Response { id, outcome } => {
                object.insert("id".into(), json!(id));
                match outcome {
                    Ok(result) => object.insert("result".into(), result.clone()),
                    Err(error) => object.insert("error".into(), json!(error)),
                };
            }
        }
        // Serializing a `Value` can't fail.
        serde_json::to_vec(&Value::Object(object)).unwrap_or_default()
    }
}

fn invalid(message: &str) -> RpcError {
    RpcError::new(code::INVALID_REQUEST, message)
}

/// Reads `params` as `T`, treating absent params as `{}` so a method whose
/// fields are all optional can be called without any. The error is the
/// `-32602` to send back.
pub fn params<T: serde::de::DeserializeOwned>(method: &str, params: Value) -> Result<T, RpcError> {
    let params = if params.is_null() { json!({}) } else { params };
    serde_json::from_value(params)
        .map_err(|e| RpcError::new(code::INVALID_PARAMS, format!("`{method}`: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Message, RpcError> {
        Message::parse(text.as_bytes())
    }

    #[test]
    fn every_kind_survives_a_round_trip() {
        let messages = [
            Message::request(Id::Number(1), "hello", json!({"protocol": 1})),
            Message::request(Id::String("a".into()), "hide", Value::Null),
            Message::Notification {
                method: "log".into(),
                params: json!({"level": "info"}),
            },
            Message::result(Id::Number(7), json!({})),
            Message::error(Some(Id::Number(2)), RpcError::new(code::NOT_READY, "wait")),
            Message::error(
                None,
                RpcError::new(code::UNSUPPORTED_PROTOCOL, "no")
                    .with_data(json!({"supported": [1]})),
            ),
        ];
        for message in messages {
            assert_eq!(Message::parse(&message.to_bytes()).unwrap(), message);
        }
    }

    #[test]
    fn broken_messages_get_the_right_code() {
        let cases = [
            ("{", code::PARSE_ERROR),
            ("[1]", code::INVALID_REQUEST),
            (r#"{"id":1,"method":"x"}"#, code::INVALID_REQUEST),
            (
                r#"{"jsonrpc":"2.0","id":true,"method":"x"}"#,
                code::INVALID_REQUEST,
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"method":3}"#,
                code::INVALID_REQUEST,
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"method":"x","params":3}"#,
                code::INVALID_REQUEST,
            ),
            (r#"{"jsonrpc":"2.0","id":1}"#, code::INVALID_REQUEST),
            (
                r#"{"jsonrpc":"2.0","id":1,"result":1,"error":{}}"#,
                code::INVALID_REQUEST,
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"error":{"message":"m"}}"#,
                code::INVALID_REQUEST,
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(parse(text).unwrap_err().code, expected, "{text}");
        }
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let message = parse(r#"{"jsonrpc":"2.0","id":1,"method":"hide","extra":true}"#).unwrap();
        assert_eq!(
            message,
            Message::request(Id::Number(1), "hide", Value::Null)
        );
    }

    #[test]
    fn absent_params_read_as_an_empty_object() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct P {
            #[serde(default)]
            timings: bool,
        }
        assert_eq!(
            params::<P>("status", Value::Null).unwrap(),
            P { timings: false }
        );
        let err = params::<P>("status", json!({"timings": "yes"})).unwrap_err();
        assert_eq!(err.code, code::INVALID_PARAMS);
        assert!(err.message.starts_with("`status`"), "{}", err.message);
    }
}
