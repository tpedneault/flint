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
//!   and (when Fenix is the window in front) what it is showing, for the
//!   context sent with queries.
//! - A *run*: one Fenix action. With a Fenix answering, Flint grants it
//!   the foreground (Windows: Flint is the foreground process while the
//!   user picks a row, so it may) and sends the request; with none, it
//!   starts `fenix` with the request as flags.

use std::sync::mpsc::{self, Sender};

use flint_fenix::control::{self, Link, StateResult};
use flint_fenix::handoff;
use serde_json::Value;

/// Work for the Fenix thread.
pub enum Job {
    /// Find out whether a Fenix answers and what it can do; with `state`,
    /// also what it shows.
    Look {
        /// Ask `fenix/state` too.
        state: bool,
    },
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
    /// A look: the Fenix answering (`None`: none), what it shows when
    /// asked, and what went wrong talking to one that is there.
    Looked {
        /// Its hello.
        link: Option<Link>,
        /// `fenix/state`, when asked and answered.
        state: Option<StateResult>,
        /// A Fenix was there but the exchange failed.
        problem: Option<String>,
    },
    /// A run finished.
    Ran {
        /// The request's token.
        token: u64,
        /// What happened, as one footer line on failure.
        result: Result<(), String>,
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
                    Job::Look { state } => look(state),
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

fn look(want_state: bool) -> Done {
    let looked = |link, state, problem| Done::Looked {
        link,
        state,
        problem,
    };
    let (reader, writer) = match flint_platform::fenix::connect() {
        Ok(Some(connection)) => connection,
        Ok(None) => return looked(None, None, None),
        Err(e) => return looked(None, None, Some(format!("couldn't reach Fenix: {e}"))),
    };
    let mut session = match control::hello(reader, writer) {
        Ok(session) => session,
        Err(why) => return looked(None, None, Some(why)),
    };
    if !want_state {
        return looked(Some(session.link), None, None);
    }
    match session.state() {
        Ok(state) => looked(Some(session.link), Some(state), None),
        Err(why) => looked(Some(session.link), None, Some(why)),
    }
}

fn run(method: &str, params: &Value, executable: &str) -> (Result<(), String>, Option<Link>) {
    let (reader, writer) = match flint_platform::fenix::connect() {
        Ok(Some(connection)) => connection,
        // No Fenix answering: start one with the request.
        Ok(None) => return (handoff::start(method, params, executable), None),
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
