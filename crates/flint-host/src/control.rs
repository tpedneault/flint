//! The control server: accepts connections on the control endpoint, runs
//! the `hello` handshake on each, and hands every checked client request
//! to the daemon through [`ClientHandler`].
//!
//! One thread per connection, because connections are few (a CLI
//! invocation each, and later a handful of long-running providers) and a
//! blocking thread is the simplest thing that keeps one slow peer from
//! holding up another. A cap on simultaneous connections keeps a runaway
//! script from exhausting threads; beyond it, new connections are closed
//! at once and the client reports that Flint is busy.
//!
//! Nothing here knows about sockets or pipes: [`serve`] takes a function
//! returning the next connection's two halves, so tests drive it over
//! in-memory pipes and the daemon over `flint-platform`'s endpoint.

use std::io::{self, BufReader, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use flint_protocol::frame::{read_frame, write_frame};
use flint_protocol::{Action, ClientRequest, HostInfo, HostSession, Message, RpcError};
use serde_json::Value;

/// How many connections may be open at once.
pub const MAX_CONNECTIONS: usize = 16;

/// What the daemon does with client requests.
pub trait ClientHandler: Send + Sync + 'static {
    /// Carries out `request` and returns the result to send, or the error
    /// (whose message the CLI prints).
    fn handle(&self, request: ClientRequest) -> Result<Value, RpcError>;

    /// Called once the answer to `request` has reached the client, for
    /// what must happen only after: `quit` exits here, so the client
    /// isn't left without an answer.
    fn replied(&self, _request: &ClientRequest) {}

    /// A line for the daemon's log: a connection that failed.
    fn log(&self, _line: &str) {}
}

/// Accepts connections from `accept` until it fails for good, serving
/// each on its own thread. `declared` lists providers allowed to connect
/// in (`connect = true`); empty until M5.
pub fn serve<R, W, A>(
    mut accept: A,
    host: HostInfo,
    declared: Vec<String>,
    handler: Arc<dyn ClientHandler>,
) where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
    A: FnMut() -> io::Result<(R, W)>,
{
    let open = Arc::new(AtomicUsize::new(0));
    let mut failures = 0;
    loop {
        let (reader, writer) = match accept() {
            Ok(halves) => {
                failures = 0;
                halves
            }
            Err(e) => {
                failures += 1;
                handler.log(&format!("control endpoint: accepting failed: {e}"));
                if failures >= 10 {
                    handler.log("control endpoint: giving up after 10 failures in a row");
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            handler.log("control endpoint: too many connections; one was refused");
            continue; // dropping the halves closes the connection
        }
        let session = HostSession::new(host.clone(), declared.clone());
        let worker = Arc::clone(&handler);
        let still_open = Arc::clone(&open);
        let spawned = std::thread::Builder::new()
            .name("flint-control".into())
            .spawn(move || {
                if let Err(e) = serve_connection(reader, writer, session, worker.as_ref()) {
                    worker.log(&format!("control connection: {e}"));
                }
                still_open.fetch_sub(1, Ordering::SeqCst);
            });
        if let Err(e) = spawned {
            open.fetch_sub(1, Ordering::SeqCst);
            handler.log(&format!(
                "control endpoint: no thread for a connection: {e}"
            ));
        }
    }
}

/// Serves one connection until the peer closes it, the handshake fails,
/// or the stream breaks.
pub fn serve_connection<R: Read, W: Write>(
    reader: R,
    mut writer: W,
    mut session: HostSession,
    handler: &dyn ClientHandler,
) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    loop {
        let body = match read_frame(&mut reader) {
            Ok(Some(body)) => body,
            Ok(None) => return Ok(()),
            Err(e) => {
                // The stream can't be resynchronised; say why and close.
                if e.kind() == io::ErrorKind::InvalidData {
                    let error = RpcError::new(flint_protocol::code::INVALID_REQUEST, e.to_string());
                    let _ = write_frame(&mut writer, &Message::error(None, error).to_bytes());
                }
                return Err(e);
            }
        };
        match session.handle(&body) {
            Action::Reply(message) => write_frame(&mut writer, &message.to_bytes())?,
            Action::ReplyAndClose(message) => {
                write_frame(&mut writer, &message.to_bytes())?;
                return Ok(());
            }
            Action::Client { id, request } => {
                let outcome = handler.handle(request.clone());
                let succeeded = outcome.is_ok();
                let message = match outcome {
                    Ok(result) => Message::result(id, result),
                    Err(error) => Message::error(Some(id), error),
                };
                write_frame(&mut writer, &message.to_bytes())?;
                if succeeded {
                    handler.replied(&request);
                }
            }
            Action::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_protocol::{code, ClientConnection, Hello, Id};
    use serde_json::json;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        handled: Mutex<Vec<String>>,
        replied: Mutex<Vec<String>>,
    }

    impl ClientHandler for Recorder {
        fn handle(&self, request: ClientRequest) -> Result<Value, RpcError> {
            self.handled.lock().unwrap().push(request.method().into());
            match request {
                ClientRequest::Toggle(_) => Ok(json!({"visible": true})),
                ClientRequest::Reload => Err(RpcError::new(code::UNAVAILABLE, "no")),
                _ => Ok(json!({})),
            }
        }
        fn replied(&self, request: &ClientRequest) {
            self.replied.lock().unwrap().push(request.method().into());
        }
    }

    fn host() -> HostInfo {
        HostInfo {
            name: "flint".into(),
            version: "0.1.0".into(),
            pid: 1234,
        }
    }

    type ClientSide = ClientConnection<BufReader<io::PipeReader>, io::PipeWriter>;

    /// A server thread on one end of two pipes, a client on the other.
    fn connect(handler: Arc<Recorder>) -> (ClientSide, std::thread::JoinHandle<io::Result<()>>) {
        let (server_reads, client_writes) = io::pipe().unwrap();
        let (client_reads, server_writes) = io::pipe().unwrap();
        let server = std::thread::spawn(move || {
            serve_connection(
                server_reads,
                server_writes,
                HostSession::new(host(), vec![]),
                handler.as_ref(),
            )
        });
        let client = ClientConnection::open(
            BufReader::new(client_reads),
            client_writes,
            &Hello::client("test"),
        )
        .unwrap();
        (client, server)
    }

    #[test]
    fn a_client_is_answered_and_told_who_the_host_is() {
        let handler = Arc::new(Recorder::default());
        let (mut client, server) = connect(Arc::clone(&handler));
        assert_eq!(client.host().host.pid, 1234);
        let result = client
            .call(&ClientRequest::Toggle(Default::default()))
            .unwrap();
        assert_eq!(result, json!({"visible": true}));
        client.call(&ClientRequest::Quit).unwrap();
        drop(client);
        server.join().unwrap().unwrap();
        assert_eq!(*handler.handled.lock().unwrap(), ["toggle", "quit"]);
        assert_eq!(*handler.replied.lock().unwrap(), ["toggle", "quit"]);
    }

    #[test]
    fn a_failed_request_is_an_error_and_not_reported_as_replied() {
        let handler = Arc::new(Recorder::default());
        let (mut client, server) = connect(Arc::clone(&handler));
        let err = client.call(&ClientRequest::Reload).unwrap_err();
        assert_eq!(err.to_string(), "no");
        drop(client);
        server.join().unwrap().unwrap();
        assert!(handler.replied.lock().unwrap().is_empty());
    }

    #[test]
    fn an_oversized_frame_is_answered_then_closed() {
        let (server_reads, mut client_writes) = io::pipe().unwrap();
        let (client_reads, server_writes) = io::pipe().unwrap();
        let handler = Recorder::default();
        let server = std::thread::spawn(move || {
            serve_connection(
                server_reads,
                server_writes,
                HostSession::new(host(), vec![]),
                &handler,
            )
        });
        client_writes
            .write_all(b"Content-Length: 999999999999\r\n\r\n")
            .unwrap();
        let mut reader = BufReader::new(client_reads);
        let reply = Message::parse(&read_frame(&mut reader).unwrap().unwrap()).unwrap();
        let Message::Response {
            id: None,
            outcome: Err(e),
        } = reply
        else {
            panic!("expected an error");
        };
        assert!(e.message.contains("limit"), "{}", e.message);
        assert!(server.join().unwrap().is_err());
        assert!(read_frame(&mut reader).unwrap().is_none(), "closed");
    }

    #[test]
    fn requests_before_hello_are_refused_but_the_connection_survives() {
        let (server_reads, mut client_writes) = io::pipe().unwrap();
        let (client_reads, server_writes) = io::pipe().unwrap();
        let handler = Arc::new(Recorder::default());
        let h = Arc::clone(&handler);
        let server = std::thread::spawn(move || {
            serve_connection(
                server_reads,
                server_writes,
                HostSession::new(host(), vec![]),
                h.as_ref(),
            )
        });
        let early = Message::request(Id::Number(9), "toggle", Value::Null);
        write_frame(&mut client_writes, &early.to_bytes()).unwrap();
        let mut reader = BufReader::new(client_reads);
        let reply = Message::parse(&read_frame(&mut reader).unwrap().unwrap()).unwrap();
        assert!(
            matches!(reply, Message::Response { outcome: Err(ref e), .. } if e.code == code::NOT_READY)
        );
        let mut client =
            ClientConnection::open(reader, client_writes, &Hello::client("t")).unwrap();
        client.call(&ClientRequest::Hide).unwrap();
        drop(client);
        server.join().unwrap().unwrap();
        assert_eq!(*handler.handled.lock().unwrap(), ["hide"]);
    }

    #[test]
    fn serve_accepts_many_connections() {
        let (tx, rx) = std::sync::mpsc::channel::<(io::PipeReader, io::PipeWriter)>();
        let handler = Arc::new(Recorder::default());
        let h: Arc<dyn ClientHandler> = handler.clone();
        std::thread::spawn(move || {
            serve(
                move || rx.recv().map_err(|_| io::Error::other("done")),
                host(),
                vec![],
                h,
            )
        });
        for _ in 0..3 {
            let (server_reads, client_writes) = io::pipe().unwrap();
            let (client_reads, server_writes) = io::pipe().unwrap();
            tx.send((server_reads, server_writes)).unwrap();
            let mut client = ClientConnection::open(
                BufReader::new(client_reads),
                client_writes,
                &Hello::client("t"),
            )
            .unwrap();
            client.call(&ClientRequest::Hide).unwrap();
        }
        assert_eq!(handler.handled.lock().unwrap().len(), 3);
    }
}
