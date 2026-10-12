//! The host's side of one channel, as a state machine: what to answer to
//! each message, given whether `hello` has happened and who the peer said
//! it was.
//!
//! It does no I/O. A transport (`flint-host`) feeds it each message body
//! and does what it says: send a reply, close the channel, or hand a
//! checked request to the daemon. That keeps every handshake rule in
//! PROTOCOL.md testable from byte strings.

use serde_json::json;

use crate::client::ClientRequest;
use crate::hello::{Hello, HelloResult, HostInfo, Role, PROTOCOL_VERSION};
use crate::message::{code, params, Id, Message, RpcError};
use crate::provider::ProviderMessage;

/// What the transport should do with one incoming message.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Send this, then keep reading.
    Reply(Message),
    /// Send this, then close the channel.
    ReplyAndClose(Message),
    /// A client asked for something; answer it with `id`.
    Client {
        /// The request's id, for the response.
        id: Id,
        /// What was asked.
        request: ClientRequest,
    },
    /// A provider said something; for `Register`, answer its id.
    Provider(ProviderMessage),
    /// A provider answered one of the host's requests (`execute`).
    Response {
        /// The request's id.
        id: Option<Id>,
        /// Its result or error.
        outcome: Result<serde_json::Value, RpcError>,
    },
    /// A provider sent something unusable that can't be answered (a bad
    /// notification); worth a line in its log.
    Problem(String),
    /// Nothing to do (a notification, a stray response).
    Nothing,
}

/// The host's view of one channel.
#[derive(Debug)]
pub struct HostSession {
    host: HostInfo,
    /// Providers that may connect in (`[[providers]] connect = true`).
    declared: Vec<String>,
    peer: Option<Hello>,
}

impl HostSession {
    /// A channel that hasn't said `hello` yet.
    pub fn new(host: HostInfo, declared: Vec<String>) -> Self {
        Self {
            host,
            declared,
            peer: None,
        }
    }

    /// Who the peer said it was, once `hello` succeeded.
    pub fn peer(&self) -> Option<&Hello> {
        self.peer.as_ref()
    }

    /// Decides what to do with one message body.
    pub fn handle(&mut self, body: &[u8]) -> Action {
        let message = match Message::parse(body) {
            Ok(message) => message,
            Err(error) => return Action::Reply(Message::error(None, error)),
        };
        let is_provider = self.peer.as_ref().is_some_and(|p| p.role == Role::Provider);
        let (id, method, raw) = match message {
            Message::Request { id, method, params } => (id, method, params),
            Message::Notification { method, params } if is_provider => {
                return match ProviderMessage::notification(&method, params) {
                    Ok(Some(message)) => Action::Provider(message),
                    Ok(None) => Action::Nothing,
                    Err(why) => Action::Problem(why),
                };
            }
            Message::Response { id, outcome } if is_provider => {
                return Action::Response { id, outcome }
            }
            // A client's notifications have no meaning, and nothing is
            // asked of a client; both are ignored as the protocol's
            // versioning rule asks. So is anything before `hello`.
            Message::Notification { .. } | Message::Response { .. } => return Action::Nothing,
        };
        let Some(peer) = &self.peer else {
            return self.hello(id, &method, raw);
        };
        if method == "hello" {
            return Action::Reply(Message::error(
                Some(id),
                RpcError::new(code::INVALID_REQUEST, "`hello` was already exchanged"),
            ));
        }
        match peer.role {
            Role::Client => match ClientRequest::parse(&method, raw) {
                Ok(request) => Action::Client { id, request },
                Err(error) => Action::Reply(Message::error(Some(id), error)),
            },
            Role::Provider => match ProviderMessage::request(id.clone(), &method, raw) {
                Ok(ProviderMessage::Register { register, .. }) if register.id != peer.name => {
                    Action::Reply(Message::error(
                        Some(id),
                        RpcError::new(
                            code::INVALID_PARAMS,
                            format!(
                                "`provider/register` id `{}` must be the `hello` name `{}`",
                                register.id, peer.name
                            ),
                        ),
                    ))
                }
                Ok(message) => Action::Provider(message),
                Err(error) => Action::Reply(Message::error(Some(id), error)),
            },
        }
    }

    fn hello(&mut self, id: Id, method: &str, raw: serde_json::Value) -> Action {
        if method != "hello" {
            return Action::Reply(Message::error(
                Some(id),
                RpcError::new(
                    code::NOT_READY,
                    format!("`{method}` before `hello`; send `hello` first"),
                ),
            ));
        }
        let hello: Hello = match params("hello", raw) {
            Ok(hello) => hello,
            Err(error) => return Action::ReplyAndClose(Message::error(Some(id), error)),
        };
        if hello.protocol != PROTOCOL_VERSION {
            let error = RpcError::new(
                code::UNSUPPORTED_PROTOCOL,
                format!(
                    "protocol {} isn't supported; this Flint speaks {PROTOCOL_VERSION}",
                    hello.protocol
                ),
            )
            .with_data(json!({ "supported": [PROTOCOL_VERSION] }));
            return Action::ReplyAndClose(Message::error(Some(id), error));
        }
        if hello.role == Role::Provider && !self.declared.contains(&hello.name) {
            let error = RpcError::new(
                code::NOT_DECLARED,
                format!(
                    "provider `{}` isn't declared in settings.toml with `connect = true`",
                    hello.name
                ),
            );
            return Action::ReplyAndClose(Message::error(Some(id), error));
        }
        self.peer = Some(hello);
        let result = HelloResult {
            protocol: PROTOCOL_VERSION,
            host: self.host.clone(),
        };
        Action::Reply(Message::result(id, json!(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn session(declared: &[&str]) -> HostSession {
        let host = HostInfo {
            name: "flint".into(),
            version: "0.1.0".into(),
            pid: 42,
        };
        HostSession::new(host, declared.iter().map(|s| s.to_string()).collect())
    }

    fn request(id: i64, method: &str, params: Value) -> Vec<u8> {
        Message::request(Id::Number(id), method, params).to_bytes()
    }

    fn hello(role: &str, name: &str, protocol: u32) -> Vec<u8> {
        request(
            1,
            "hello",
            json!({"protocol": protocol, "role": role, "name": name, "version": "1", "pid": 7}),
        )
    }

    fn error_code(action: &Action) -> i64 {
        match action {
            Action::Reply(Message::Response {
                outcome: Err(e), ..
            })
            | Action::ReplyAndClose(Message::Response {
                outcome: Err(e), ..
            }) => e.code,
            other => panic!("expected an error reply, got {other:?}"),
        }
    }

    #[test]
    fn a_client_says_hello_then_asks() {
        let mut s = session(&[]);
        let Action::Reply(Message::Response {
            outcome: Ok(result),
            ..
        }) = s.handle(&hello("client", "flint", 1))
        else {
            panic!("hello should succeed");
        };
        let result: HelloResult = serde_json::from_value(result).unwrap();
        assert_eq!(result.host.pid, 42);
        assert_eq!(s.peer().unwrap().pid, 7);
        assert_eq!(
            s.handle(&request(2, "hide", Value::Null)),
            Action::Client {
                id: Id::Number(2),
                request: ClientRequest::Hide
            }
        );
    }

    #[test]
    fn anything_before_hello_is_not_ready_and_the_channel_stays_open() {
        let mut s = session(&[]);
        let action = s.handle(&request(1, "toggle", Value::Null));
        assert!(matches!(action, Action::Reply(_)));
        assert_eq!(error_code(&action), code::NOT_READY);
        assert!(matches!(
            s.handle(&hello("client", "flint", 1)),
            Action::Reply(_)
        ));
    }

    #[test]
    fn another_protocol_version_is_refused_with_the_supported_list() {
        let mut s = session(&[]);
        let action = s.handle(&hello("client", "flint", 2));
        let Action::ReplyAndClose(Message::Response {
            outcome: Err(e), ..
        }) = &action
        else {
            panic!("expected a closing error, got {action:?}");
        };
        assert_eq!(e.code, code::UNSUPPORTED_PROTOCOL);
        assert_eq!(e.data, Some(json!({"supported": [1]})));
    }

    #[test]
    fn an_undeclared_provider_is_refused_and_closed() {
        let mut s = session(&["fenix"]);
        let action = s.handle(&hello("provider", "jira", 1));
        assert!(matches!(action, Action::ReplyAndClose(_)));
        assert_eq!(error_code(&action), code::NOT_DECLARED);

        let mut s = session(&["fenix"]);
        assert!(matches!(
            s.handle(&hello("provider", "fenix", 1)),
            Action::Reply(_)
        ));
    }

    #[test]
    fn a_malformed_hello_closes_the_channel() {
        let mut s = session(&[]);
        let action = s.handle(&request(1, "hello", json!({"protocol": 1})));
        assert!(matches!(action, Action::ReplyAndClose(_)));
        assert_eq!(error_code(&action), code::INVALID_PARAMS);
    }

    #[test]
    fn a_second_hello_is_an_error() {
        let mut s = session(&[]);
        s.handle(&hello("client", "flint", 1));
        assert_eq!(
            error_code(&s.handle(&hello("client", "flint", 1))),
            code::INVALID_REQUEST
        );
    }

    #[test]
    fn garbage_gets_a_parse_error_without_an_id() {
        let mut s = session(&[]);
        let action = s.handle(b"{not json");
        let Action::Reply(Message::Response {
            id,
            outcome: Err(e),
        }) = action
        else {
            panic!("expected an error reply");
        };
        assert_eq!(id, None);
        assert_eq!(e.code, code::PARSE_ERROR);
    }

    #[test]
    fn a_provider_registers_reports_and_answers() {
        let mut s = session(&["jira"]);
        s.handle(&hello("provider", "jira", 1));
        let register = request(
            2,
            "provider/register",
            json!({"id": "jira", "title": "Jira"}),
        );
        assert!(matches!(
            s.handle(&register),
            Action::Provider(ProviderMessage::Register { .. })
        ));
        let wrong = request(3, "provider/register", json!({"id": "other", "title": "X"}));
        assert_eq!(error_code(&s.handle(&wrong)), code::INVALID_PARAMS);
        let results = Message::Notification {
            method: "results".into(),
            params: json!({"queryId": 1, "items": []}),
        };
        assert!(matches!(
            s.handle(&results.to_bytes()),
            Action::Provider(ProviderMessage::Results(_))
        ));
        let bad = Message::Notification {
            method: "log".into(),
            params: json!({"level": 3}),
        };
        assert!(matches!(s.handle(&bad.to_bytes()), Action::Problem(_)));
        let answer = Message::result(Id::Number(1), json!({"close": false}));
        assert!(matches!(
            s.handle(&answer.to_bytes()),
            Action::Response {
                id: Some(Id::Number(1)),
                outcome: Ok(_)
            }
        ));
        assert_eq!(
            error_code(&s.handle(&request(4, "toggle", Value::Null))),
            code::METHOD_NOT_FOUND
        );
    }

    #[test]
    fn notifications_and_unknown_client_methods() {
        let mut s = session(&[]);
        s.handle(&hello("client", "flint", 1));
        let note = Message::Notification {
            method: "whatever".into(),
            params: Value::Null,
        };
        assert_eq!(s.handle(&note.to_bytes()), Action::Nothing);
        assert_eq!(
            error_code(&s.handle(&request(3, "frobnicate", Value::Null))),
            code::METHOD_NOT_FOUND
        );
    }
}
