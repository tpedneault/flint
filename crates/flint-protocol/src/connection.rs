//! The client's side of a channel: say `hello`, then make requests one at
//! a time and wait for each answer. This is all the `flint` CLI needs, so
//! it is blocking and has no threads; a long-running provider would want
//! more and gets it from `flint-host` in M5.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::client::ClientRequest;
use crate::frame::{read_frame, write_frame};
use crate::hello::{Hello, HelloResult};
use crate::message::{Id, Message, RpcError};

/// Why a call didn't produce a result.
#[derive(Debug)]
pub enum CallError {
    /// The channel failed or closed.
    Io(io::Error),
    /// The host answered with an error.
    Rpc(RpcError),
    /// The host sent something this client can't make sense of.
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Io(e) => write!(f, "{e}"),
            CallError::Rpc(e) => write!(f, "{e}"),
            CallError::Protocol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CallError {}

impl From<io::Error> for CallError {
    fn from(e: io::Error) -> Self {
        CallError::Io(e)
    }
}

/// A client channel after a successful `hello`.
pub struct ClientConnection<R, W> {
    reader: R,
    writer: W,
    next_id: i64,
    host: HelloResult,
}

impl<R: BufRead, W: Write> ClientConnection<R, W> {
    /// Says `hello` and waits for the answer.
    pub fn open(reader: R, writer: W, hello: &Hello) -> Result<Self, CallError> {
        let mut connection = Self {
            reader,
            writer,
            next_id: 1,
            host: HelloResult {
                protocol: 0,
                host: crate::hello::HostInfo {
                    name: String::new(),
                    version: String::new(),
                    pid: 0,
                },
            },
        };
        let result = connection.call_raw("hello", json!(hello))?;
        connection.host = serde_json::from_value(result)
            .map_err(|e| CallError::Protocol(format!("unexpected `hello` result: {e}")))?;
        Ok(connection)
    }

    /// Who answered `hello`.
    pub fn host(&self) -> &HelloResult {
        &self.host
    }

    /// Sends a request and waits for its result.
    pub fn call(&mut self, request: &ClientRequest) -> Result<Value, CallError> {
        self.call_raw(request.method(), request.params())
    }

    fn call_raw(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        let id = Id::Number(self.next_id);
        self.next_id += 1;
        write_frame(
            &mut self.writer,
            &Message::request(id.clone(), method, params).to_bytes(),
        )?;
        loop {
            let Some(body) = read_frame(&mut self.reader)? else {
                return Err(CallError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("Flint closed the connection before answering `{method}`"),
                )));
            };
            let message = Message::parse(&body).map_err(|e| CallError::Protocol(e.message))?;
            match message {
                Message::Response {
                    id: Some(answered),
                    outcome,
                } if answered == id => return outcome.map_err(CallError::Rpc),
                // An error about a message the host couldn't read is
                // about this one: it is the only one in flight.
                Message::Response {
                    id: None,
                    outcome: Err(e),
                } => return Err(CallError::Rpc(e)),
                // Notifications, and answers to nothing we asked.
                _ => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hello::{HostInfo, PROTOCOL_VERSION};
    use std::io::Cursor;

    fn framed(messages: &[Message]) -> Cursor<Vec<u8>> {
        let mut bytes = Vec::new();
        for m in messages {
            write_frame(&mut bytes, &m.to_bytes()).unwrap();
        }
        Cursor::new(bytes)
    }

    fn hello_result() -> Message {
        let result = HelloResult {
            protocol: PROTOCOL_VERSION,
            host: HostInfo {
                name: "flint".into(),
                version: "9".into(),
                pid: 99,
            },
        };
        Message::result(Id::Number(1), json!(result))
    }

    #[test]
    fn hello_then_a_call_skipping_notifications() {
        let replies = framed(&[
            hello_result(),
            Message::Notification {
                method: "noise".into(),
                params: Value::Null,
            },
            Message::result(Id::Number(2), json!({"visible": true})),
        ]);
        let mut sent = Vec::new();
        let mut c = ClientConnection::open(replies, &mut sent, &Hello::client("1")).unwrap();
        assert_eq!(c.host().host.pid, 99);
        let result = c.call(&ClientRequest::Toggle(Default::default())).unwrap();
        assert_eq!(result, json!({"visible": true}));

        let mut written = Cursor::new(sent);
        let first = Message::parse(&read_frame(&mut written).unwrap().unwrap()).unwrap();
        assert!(matches!(first, Message::Request { ref method, .. } if method == "hello"));
    }

    #[test]
    fn an_error_answer_is_returned_as_rpc() {
        let replies = framed(&[
            hello_result(),
            Message::error(Some(Id::Number(2)), RpcError::new(-1, "nope")),
        ]);
        let mut c = ClientConnection::open(replies, Vec::new(), &Hello::client("1")).unwrap();
        let err = c.call(&ClientRequest::Hide).unwrap_err();
        assert!(
            matches!(err, CallError::Rpc(ref e) if e.message == "nope"),
            "{err}"
        );
    }

    #[test]
    fn a_closed_channel_names_the_unanswered_method() {
        let replies = framed(&[hello_result()]);
        let mut c = ClientConnection::open(replies, Vec::new(), &Hello::client("1")).unwrap();
        let err = c.call(&ClientRequest::Quit).unwrap_err();
        assert!(err.to_string().contains("`quit`"), "{err}");
    }
}
