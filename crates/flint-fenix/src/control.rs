//! Fenix level 1: talking to the running Fenix over its control channel
//! (Fenix's `docs/CONTROL.md`, F1 in FENIX-INTEGRATION.md).
//!
//! Every exchange starts with `fenix/hello`, which says what this Fenix
//! can do (`capabilities`, method names) and its pid. Flint goes by the
//! capabilities, never the version: an action whose method this Fenix
//! doesn't list is shown disabled with that reason, so a Fenix older or
//! newer than Flint expects costs individual actions, not Flint. Since
//! Fenix #58, `fenix/open` (with line and column), `fenix/openProject` and
//! `fenix/state`; since #59, its commands with their keys
//! (`fenix/commands`, `fenix/runCommand`), notebook capture and the
//! agenda's tasks and clock (F5, F6).
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
pub use fenix_control::protocol::{AgendaTask, AgendaTimer, CommandInfo, StateResult, TaskStatus};
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
            "`{method}` needs Fenix running; start Fenix and try again"
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

    /// Fenix's commands that may be run from outside (`external`), with
    /// their keys; none from a Fenix that doesn't list them.
    pub fn commands(&mut self) -> Result<Vec<CommandInfo>, String> {
        if !self.link.can(method::COMMANDS) {
            return Ok(Vec::new());
        }
        let commands = self
            .client
            .commands()
            .map_err(|e| why("couldn't list its commands", &e))?;
        Ok(commands.into_iter().filter(|c| c.external).collect())
    }

    /// The agenda's tasks that aren't done; none from a Fenix without an
    /// agenda to offer.
    pub fn open_tasks(&mut self) -> Result<Vec<AgendaTask>, String> {
        if !self.link.can(method::AGENDA_TASKS) {
            return Ok(Vec::new());
        }
        let open = vec![
            TaskStatus::InProgress,
            TaskStatus::Todo,
            TaskStatus::Blocked,
        ];
        self.client
            .agenda_tasks(Some(open))
            .map(|r| r.tasks)
            .map_err(|e| why("couldn't list the agenda", &e))
    }

    /// Carries out the Fenix action `method` with `params`, as an item's
    /// action gave them; Fenix's result (`{ "entry": ... }` for a capture).
    pub fn execute(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        available(method, Some(&self.link))?;
        self.client
            .request::<Value>(method, params.clone())
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
    use fenix_control::protocol::{Call, CommandsResult, OpenParams, RpcError, TasksResult};
    use fenix_control::ServerInfo;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    type Calls = Arc<Mutex<Vec<String>>>;

    /// A Fenix's server, as Fenix runs it (`fenix-control`'s own), with a
    /// fake editor behind it recording what it was asked. `capabilities`
    /// is what this Fenix offers: `None` for all of today's, or fewer,
    /// as an older Fenix would, which the server then refuses itself.
    fn fenix(
        state: StateResult,
        capabilities: Option<&[&str]>,
    ) -> (Session<std::io::PipeReader, std::io::PipeWriter>, Calls) {
        let (client_read, server_write) = std::io::pipe().unwrap();
        let (server_read, client_write) = std::io::pipe().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let mut info = ServerInfo::new("1.1.0", 4242);
        if let Some(only) = capabilities {
            info.capabilities = only.iter().map(|c| c.to_string()).collect();
        }
        std::thread::spawn(move || {
            let _ = fenix_control::serve(server_read, server_write, &info, |call| {
                let (line, answer) = match &call {
                    Call::Open(OpenParams { paths, .. }) => (
                        format!(
                            "open {}",
                            paths
                                .iter()
                                .map(|p| format!("{}:{:?}:{:?}", p.path, p.line, p.column))
                                .collect::<Vec<_>>()
                                .join(" ")
                        ),
                        json!({}),
                    ),
                    Call::OpenProject(p) if p.root == "missing" => {
                        return Err(RpcError::invalid_params("missing isn't a folder"));
                    }
                    Call::OpenProject(p) => (format!("project {}", p.root), json!({})),
                    Call::State => ("state".into(), serde_json::to_value(&state).unwrap()),
                    Call::Commands => (
                        "commands".into(),
                        serde_json::to_value(CommandsResult {
                            commands: vec![
                                CommandInfo {
                                    id: "agenda.open".into(),
                                    description: "Open the agenda".into(),
                                    keys: Some("SPC a a".into()),
                                    external: true,
                                },
                                CommandInfo {
                                    id: "file.save".into(),
                                    description: "Save the file".into(),
                                    keys: None,
                                    external: false,
                                },
                            ],
                        })
                        .unwrap(),
                    ),
                    Call::AgendaTasks(p) => (
                        format!("tasks {:?}", p.status),
                        serde_json::to_value(TasksResult {
                            tasks: vec![AgendaTask {
                                id: 7,
                                title: "Review telemetry".into(),
                                status: TaskStatus::InProgress,
                                jira_key: Some("OPS-12".into()),
                                elapsed_minutes: 95,
                            }],
                        })
                        .unwrap(),
                    ),
                    Call::NotebookCapture(p) => (
                        format!("capture {}", p.text),
                        json!({"entry": "C:\\notes\\Inbox.md"}),
                    ),
                    Call::AgendaClockIn(p) => (format!("clock in {}", p.task_id), json!({})),
                    Call::AgendaClockOut => ("clock out".into(), json!({})),
                    Call::RunCommand(p) => (format!("run {}", p.id), json!({})),
                };
                seen.lock().unwrap().push(line);
                Ok(answer)
            });
        });
        (hello(client_read, client_write).unwrap(), calls)
    }

    #[test]
    fn hello_reports_version_pid_and_capabilities() {
        let (session, _) = fenix(StateResult::default(), None);
        assert_eq!(session.link.version, "1.1.0");
        assert_eq!(session.link.pid, 4242);
        for method in [
            "fenix/open",
            "fenix/openProject",
            "fenix/commands",
            "fenix/agenda.clockIn",
        ] {
            assert!(session.link.can(method), "{method}");
        }
    }

    #[test]
    fn opening_files_at_a_position_and_projects_reach_the_editor() {
        let (mut session, calls) = fenix(StateResult::default(), None);
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
    fn state_says_what_fenix_shows_and_the_running_clock() {
        let shown = StateResult {
            project: Some("C:\\src\\fenix".into()),
            file: Some("C:\\src\\fenix\\Cargo.toml".into()),
            line: Some(3),
            agenda_timer: Some(AgendaTimer {
                task_id: 7,
                title: "Review telemetry".into(),
                since: "2026-10-13T09:00:00Z".into(),
            }),
        };
        let (mut session, _) = fenix(shown.clone(), None);
        assert_eq!(session.state().unwrap(), shown);
    }

    #[test]
    fn commands_are_the_external_ones_and_tasks_the_open_ones() {
        let (mut session, calls) = fenix(StateResult::default(), None);
        let commands = session.commands().unwrap();
        assert_eq!(commands.len(), 1, "file.save can't be run from outside");
        assert_eq!(commands[0].keys.as_deref(), Some("SPC a a"));
        let tasks = session.open_tasks().unwrap();
        assert_eq!(tasks[0].id, 7);
        assert_eq!(
            calls.lock().unwrap()[1],
            "tasks Some([InProgress, Todo, Blocked])",
            "done tasks aren't asked for"
        );
    }

    #[test]
    fn capture_clock_and_commands_go_through_execute() {
        let (mut session, calls) = fenix(StateResult::default(), None);
        let captured = session
            .execute("fenix/notebook.capture", &json!({"text": "buy film"}))
            .unwrap();
        assert_eq!(captured["entry"], "C:\\notes\\Inbox.md");
        session
            .execute("fenix/agenda.clockIn", &json!({"taskId": 7}))
            .unwrap();
        session
            .execute("fenix/agenda.clockOut", &json!({}))
            .unwrap();
        session
            .execute("fenix/runCommand", &json!({"id": "agenda.open"}))
            .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "capture buy film",
                "clock in 7",
                "clock out",
                "run agenda.open"
            ]
        );
    }

    #[test]
    fn an_older_fenix_lists_and_does_less_and_flint_says_why() {
        let only: &[&str] = &["fenix/open", "fenix/openProject", "fenix/state"];
        let (mut session, calls) = fenix(StateResult::default(), Some(only));
        assert!(session.commands().unwrap().is_empty());
        assert!(session.open_tasks().unwrap().is_empty());
        let err = session
            .execute("fenix/agenda.clockIn", &json!({"taskId": 7}))
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
        let err = available("fenix/notebook.capture", None).unwrap_err();
        assert!(err.contains("needs Fenix running"), "{err}");
    }
}
