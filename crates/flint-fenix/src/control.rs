//! Fenix level 1: talking to the running Fenix over its control channel
//! (Fenix's `docs/CONTROL.md`, F1 in FENIX-INTEGRATION.md).
//!
//! Every exchange starts with `fenix/hello`, which says what this Fenix
//! can do (`capabilities`, method names) and its pid. Flint goes by the
//! capabilities, never the version: an action whose method this Fenix
//! doesn't list is shown disabled with that reason, so a Fenix older or
//! newer than Flint expects costs individual actions, not Flint. Today's
//! Fenix offers `fenix/open` (with line and column), `fenix/openProject`
//! and `fenix/state`; its commands, notebook capture and agenda (F5, F6)
//! come later and light up here when Fenix lists them.
//!
//! The protocol is Fenix's own client (`fenix-control`), driven over any
//! reader and writer: connecting is `flint-platform`'s, and these
//! functions are tested against Fenix's own server, in memory. They
//! block until Fenix answers (Fenix answers on its UI thread, within 30
//! seconds or with an error), so the daemon calls them from a worker
//! thread, never the event loop.
//!
//! When no Fenix is serving the endpoint, [`crate::handoff`] starts one
//! with the same request as command-line flags.

use std::io::{Read, Write};

use fenix_control::protocol::method;
pub use fenix_control::protocol::StateResult;
use fenix_control::{Client, ClientError};
use serde_json::Value;

/// What a running Fenix said about itself in `fenix/hello`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Its version, for `flint status`.
    pub version: String,
    /// Its process id: on Windows, Flint grants it the foreground before
    /// asking it to show something (F3).
    pub pid: u32,
    /// The methods it implements besides `fenix/hello`.
    pub capabilities: Vec<String>,
}

impl Link {
    /// Whether this Fenix implements `method`.
    pub fn can(&self, method: &str) -> bool {
        self.capabilities.iter().any(|c| c == method)
    }
}

/// Methods Flint carries out even when no Fenix is running, by starting
/// one with the same request on its command line.
pub const STARTABLE: [&str; 2] = [method::OPEN, method::OPEN_PROJECT];

/// Why `method` can't be carried out now, or `Ok` when it can. `link` is
/// what the running Fenix said at the last look, `None` when none was
/// running. The action menu shows the reason on a disabled entry, and
/// running the action anyway gives the same words.
pub fn available(method: &str, link: Option<&Link>) -> Result<(), String> {
    match link {
        Some(link) if link.can(method) => Ok(()),
        Some(link) => Err(format!(
            "this Fenix ({}) can't `{method}` from Flint; it needs a newer Fenix",
            link.version
        )),
        None if STARTABLE.contains(&method) => Ok(()),
        None => Err(format!(
            "`{method}` needs a running Fenix that offers it, and Fenix doesn't yet"
        )),
    }
}

/// A conversation with a running Fenix, after `fenix/hello`.
pub struct Session<R: Read, W: Write> {
    client: Client<R, W>,
    /// What Fenix said about itself.
    pub link: Link,
}

/// Says `fenix/hello` over `reader` and `writer`.
pub fn hello<R: Read, W: Write>(reader: R, writer: W) -> Result<Session<R, W>, String> {
    let mut client = Client::new(reader, writer);
    let hello = client
        .hello("flint", env!("CARGO_PKG_VERSION"))
        .map_err(|e| why("didn't answer Flint's hello", &e))?;
    Ok(Session {
        client,
        link: Link {
            version: if hello.version.is_empty() {
                "unknown version".into()
            } else {
                hello.version
            },
            pid: hello.pid,
            capabilities: hello.capabilities,
        },
    })
}

impl<R: Read, W: Write> Session<R, W> {
    /// `fenix/state`: the project, file and line Fenix is showing.
    pub fn state(&mut self) -> Result<StateResult, String> {
        if !self.link.can(method::STATE) {
            return Ok(StateResult::default());
        }
        self.client
            .state()
            .map_err(|e| why("couldn't say what it shows", &e))
    }

    /// Carries out the Fenix action `method` with `params`, as an item's
    /// action gave them.
    pub fn execute(&mut self, method: &str, params: &Value) -> Result<(), String> {
        available(method, Some(&self.link))?;
        self.client
            .request::<Value>(method, params.clone())
            .map(drop)
            .map_err(|e| why(&format!("refused `{method}`"), &e))
    }
}

/// One footer line from a client error: Fenix's own message when it
/// answered with one.
fn why(what: &str, error: &ClientError) -> String {
    match error {
        ClientError::Rpc(e) => format!("Fenix {what}: {}", e.message),
        other => format!("Fenix {what}: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_control::protocol::{Call, OpenParams, RpcError};
    use fenix_control::ServerInfo;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    /// A Fenix's server, as Fenix runs it, with a fake editor behind it
    /// recording what it was asked; `capabilities` are the methods the
    /// editor answers (`fenix-control` advertises its own list, so a
    /// missing one is refused the way an older Fenix would).
    fn fenix(
        answer_state: StateResult,
    ) -> (
        Session<std::io::PipeReader, std::io::PipeWriter>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let (client_read, server_write) = std::io::pipe().unwrap();
        let (server_read, client_write) = std::io::pipe().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        std::thread::spawn(move || {
            let info = ServerInfo {
                version: "1.1.0".into(),
                pid: 4242,
            };
            let _ = fenix_control::serve(server_read, server_write, &info, |call| {
                let line = match &call {
                    Call::Open(OpenParams { paths, .. }) => format!(
                        "open {}",
                        paths
                            .iter()
                            .map(|p| format!("{}:{:?}:{:?}", p.path, p.line, p.column))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                    Call::OpenProject(p) if p.root == "missing" => {
                        return Err(RpcError::invalid_params("missing isn't a folder"));
                    }
                    Call::OpenProject(p) => format!("project {}", p.root),
                    Call::State => "state".into(),
                };
                seen.lock().unwrap().push(line);
                Ok(serde_json::to_value(&answer_state).unwrap())
            });
        });
        (hello(client_read, client_write).unwrap(), calls)
    }

    #[test]
    fn hello_reports_version_pid_and_capabilities() {
        let (session, _) = fenix(StateResult::default());
        assert_eq!(session.link.version, "1.1.0");
        assert_eq!(session.link.pid, 4242);
        assert!(session.link.can("fenix/open"));
        assert!(session.link.can("fenix/openProject"));
        assert!(!session.link.can("fenix/commands"));
    }

    #[test]
    fn opening_files_at_a_position_and_projects_reach_the_editor() {
        let (mut session, calls) = fenix(StateResult::default());
        session
            .execute(
                "fenix/open",
                &json!({"paths": [{"path": "C:\\src\\x.rs", "line": 12, "column": 3}]}),
            )
            .unwrap();
        session
            .execute("fenix/openProject", &json!({"root": "C:\\src"}))
            .unwrap();
        let err = session
            .execute("fenix/openProject", &json!({"root": "missing"}))
            .unwrap_err();
        assert_eq!(
            err,
            "Fenix refused `fenix/openProject`: missing isn't a folder"
        );
        assert_eq!(
            *calls.lock().unwrap(),
            ["open C:\\src\\x.rs:Some(12):Some(3)", "project C:\\src"]
        );
    }

    #[test]
    fn state_says_what_fenix_shows() {
        let shown = StateResult {
            project: Some("C:\\src\\fenix".into()),
            file: Some("C:\\src\\fenix\\Cargo.toml".into()),
            line: Some(3),
        };
        let (mut session, _) = fenix(shown.clone());
        assert_eq!(session.state().unwrap(), shown);
    }

    #[test]
    fn what_fenix_doesnt_offer_is_disabled_with_the_reason() {
        let (mut session, calls) = fenix(StateResult::default());
        let err = session
            .execute("fenix/agenda.clockIn", &json!({"taskId": "t1"}))
            .unwrap_err();
        assert_eq!(
            err,
            "this Fenix (1.1.0) can't `fenix/agenda.clockIn` from Flint; it needs a newer Fenix"
        );
        assert!(calls.lock().unwrap().is_empty(), "nothing was sent");
    }

    #[test]
    fn without_fenix_running_only_what_can_start_it_is_offered() {
        assert!(available("fenix/open", None).is_ok());
        assert!(available("fenix/openProject", None).is_ok());
        let err = available("fenix/commands", None).unwrap_err();
        assert!(err.contains("Fenix doesn't yet"), "{err}");
    }
}
