//! The daemon's line to Fenix: one worker thread that does every exchange
//! with the running Fenix, so the event loop never waits on it.
//!
//! Fenix answers its control requests on its own UI thread: usually at
//! once, but a busy Fenix may take a while (it gives up after 30 s), and
//! a frame of Flint's must never wait for that. So the daemon only queues
//! [`Job`]s here and gets [`Done`]s back as events; one thread runs them
//! in order, each on a fresh connection (connecting to a local pipe is
//! cheap, and a connection held open would need watching for Fenix
//! restarting).
//!
//! - A *look*, before each show: is a Fenix answering, what can it do,
//!   what it is showing (for the context sent with queries, and the
//!   agenda's running clock), and, when it offers them, its commands and
//!   the agenda's open tasks. A few small requests to a local pipe, off
//!   the event loop, so the lists are as fresh as the show.
//! - A *run*: one Fenix action. With a Fenix answering, Flint grants it
//!   the foreground (Windows: Flint is the foreground process while the
//!   user picks a row, so it may) and sends the request; with none, it
//!   starts `fenix` with the request as flags.

use std::sync::mpsc::{self, Sender};

use flint_fenix::control::{self, AgendaTask, CommandInfo, Link, StateResult};
use flint_fenix::handoff;
use serde_json::Value;

/// Work for the Fenix thread.
pub enum Job {
    /// Find out whether a Fenix answers, what it can do and shows, and
    /// its commands and open tasks.
    Look,
    /// Carry out one Fenix action.
    Run {
        /// To match the answer to the request.
        token: u64,
        /// The `fenix/...` method.
        method: String,
        /// Its params.
        params: Value,
        /// `fenix.executable`, for starting Fenix when none answers.
        executable: String,
    },
}

/// What came back.
pub enum Done {
    /// A look: the Fenix answering (`None`: none), what it shows, its
    /// commands and open tasks, and what went wrong talking to one that
    /// is there.
    Looked {
        /// Its hello.
        link: Option<Link>,
        /// `fenix/state`, when answered.
        state: Option<StateResult>,
        /// Its external commands (empty when it doesn't list them).
        commands: Vec<CommandInfo>,
        /// The agenda's open tasks (empty when it doesn't list them).
        tasks: Vec<AgendaTask>,
        /// A Fenix was there but an exchange failed.
        problem: Option<String>,
    },
    /// A run finished.
    Ran {
        /// The request's token.
        token: u64,
        /// Fenix's result, or one footer line on failure.
        result: Result<Value, String>,
        /// The Fenix that carried it out, if one answered.
        link: Option<Link>,
    },
}

/// Starts the thread; `send` delivers its answers to the event loop.
pub fn start(send: impl Fn(Done) + Send + 'static) -> Result<Sender<Job>, String> {
    let (jobs, queue) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("flint-fenix".into())
        .spawn(move || {
            for job in queue {
                send(match job {
                    Job::Look => look(),
                    Job::Run {
                        token,
                        method,
                        params,
                        executable,
                    } => {
                        let (result, link) = run(&method, &params, &executable);
                        Done::Ran {
                            token,
                            result,
                            link,
                        }
                    }
                });
            }
        })
        .map(|_| jobs)
        .map_err(|e| format!("no thread to talk to Fenix with: {e}"))
}

fn look() -> Done {
    let nobody = |problem| Done::Looked {
        link: None,
        state: None,
        commands: Vec::new(),
        tasks: Vec::new(),
        problem,
    };
    let (reader, writer) = match flint_platform::fenix::connect() {
        Ok(Some(connection)) => connection,
        Ok(None) => return nobody(None),
        Err(e) => return nobody(Some(format!("couldn't reach Fenix: {e}"))),
    };
    let mut session = match control::hello(reader, writer) {
        Ok(session) => session,
        Err(why) => return nobody(Some(why)),
    };
    // Each part on its own: one that fails costs only itself.
    let mut problems = Vec::new();
    let state = session.state().map_err(|e| problems.push(e)).ok();
    let commands = session.commands().unwrap_or_else(|e| {
        problems.push(e);
        Vec::new()
    });
    let tasks = session.open_tasks().unwrap_or_else(|e| {
        problems.push(e);
        Vec::new()
    });
    Done::Looked {
        link: Some(session.link),
        state,
        commands,
        tasks,
        problem: (!problems.is_empty()).then(|| problems.join("; ")),
    }
}

fn run(method: &str, params: &Value, executable: &str) -> (Result<Value, String>, Option<Link>) {
    let (reader, writer) = match flint_platform::fenix::connect() {
        Ok(Some(connection)) => connection,
        // No Fenix answering: start one with the request.
        Ok(None) => {
            let started = handoff::start(method, params, executable);
            return (started.map(|()| Value::Null), None);
        }
        Err(e) => return (Err(format!("couldn't reach Fenix: {e}")), None),
    };
    let mut session = match control::hello(reader, writer) {
        Ok(session) => session,
        Err(why) => return (Err(why), None),
    };
    // Fenix comes to the front for what it opens; Windows lets it only
    // with the foreground process's leave, which Flint still is.
    flint_platform::focus::allow_foreground(session.link.pid);
    let result = session.execute(method, params);
    (result, Some(session.link))
}
