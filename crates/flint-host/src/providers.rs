//! The provider host: runs the providers `settings.toml` declares, talks
//! to them over PROTOCOL.md, and keeps any one of them from hurting Flint.
//!
//! A provider is a **peer**: either a process Flint spawns (its stdin and
//! stdout are the channel, its stderr goes to the provider's log) or a
//! running program that connects to Flint's control endpoint
//! (`connect = true`). Either way the same threads serve it:
//!
//! - a reader, feeding each frame to a [`HostSession`] (which holds the
//!   handshake rules) and turning what it allows into [`HostEvent`]s;
//! - a writer, the only thing that writes to the peer, which also
//!   debounces queries (`debounceMs`: a newer query replaces a waiting
//!   one);
//! - a coalescer for `provider/setItems`, so a chatty provider re-indexes
//!   at most every 250 ms;
//! - for a spawned provider, a stderr reader.
//!
//! Isolation, as DESIGN.md asks. A provider that exits or breaks the
//! protocol is restarted after 1 s, 5 s, then 30 s, and disabled after
//! three crashes within a minute, with the reason in its status. Results
//! are dropped unless they answer the latest query and arrive within 2 s
//! of it; batches over 200 items are refused; an `execute` unanswered
//! after 10 s fails with "<name> didn't answer". Nothing a provider does
//! blocks the caller: every method here only queues work and returns, and
//! everything that comes back arrives through the sink, on these threads.

use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flint_protocol::frame::{read_frame, write_frame};
use flint_protocol::provider::{
    Cancel, Context, Execute, ExecuteResult, LogLevel, ProviderMessage, Query,
};
use flint_protocol::{Action, HostInfo, HostSession, Id, Item, Message, Role};
use serde_json::json;

/// What a provider is told to do, from `[[providers]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Its id.
    pub name: String,
    /// The program and its arguments; empty when it connects instead.
    pub command: Vec<String>,
    /// It connects rather than being started.
    pub connect: bool,
}

/// Something the daemon should know.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// A provider registered (again, after a restart).
    Registered {
        /// Its id.
        name: String,
        /// Its badge and chip.
        title: String,
        /// Its own scope keyword.
        prefix: Option<String>,
        /// It answers queries.
        dynamic: bool,
    },
    /// A provider's static items changed.
    Items {
        /// Its id.
        name: String,
        /// All of them.
        items: Vec<Item>,
    },
    /// Results for the latest query.
    Results {
        /// Its id.
        name: String,
        /// The query.
        query_id: u64,
        /// This batch.
        items: Vec<Item>,
        /// The last batch.
        done: bool,
    },
    /// An `execute` finished, or failed or timed out.
    Executed {
        /// The token [`ProviderHost::execute`] returned.
        token: u64,
        /// Its id.
        name: String,
        /// The result, or one line saying why not.
        outcome: Result<ExecuteResult, String>,
    },
    /// A provider's state changed; its items are gone if it stopped.
    Changed {
        /// Its id.
        name: String,
    },
}

/// Where a provider is in its life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Started, not registered yet.
    Starting,
    /// Registered and answering.
    Running,
    /// Waiting for a program to connect (`connect = true`).
    Waiting,
    /// Crashed; starting again shortly.
    Restarting(String),
    /// Given up on, with why.
    Disabled(String),
}

impl State {
    /// One word for `flint status`.
    pub fn word(&self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Running => "running",
            State::Waiting => "waiting",
            State::Restarting(_) => "restarting",
            State::Disabled(_) => "disabled",
        }
    }

    /// Why, when it isn't running.
    pub fn reason(&self) -> Option<&str> {
        match self {
            State::Restarting(why) | State::Disabled(why) => Some(why),
            State::Waiting => Some("waiting for it to connect"),
            _ => None,
        }
    }
}

/// How long things may take; [`Timing::default`] is DESIGN.md's, tests
/// shrink them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Waits before each restart: 1 s, 5 s, then 30 s.
    pub backoff: [Duration; 3],
    /// Crashes within this window that disable a provider...
    pub crash_window: Duration,
    /// ...when there are this many.
    pub crash_limit: usize,
    /// How long an `execute` may take.
    pub execute_timeout: Duration,
    /// Results later than this after their query are dropped.
    pub result_deadline: Duration,
    /// `provider/setItems` is applied at most this often.
    pub reindex_every: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            backoff: [
                Duration::from_secs(1),
                Duration::from_secs(5),
                Duration::from_secs(30),
            ],
            crash_window: Duration::from_secs(60),
            crash_limit: 3,
            execute_timeout: Duration::from_secs(10),
            result_deadline: Duration::from_secs(2),
            reindex_every: Duration::from_millis(250),
        }
    }
}

/// Where events go: called on the host's threads.
pub type Sink = Arc<dyn Fn(HostEvent) + Send + Sync>;

/// The provider host.
#[derive(Clone)]
pub struct ProviderHost {
    shared: Arc<Shared>,
}

struct Shared {
    sink: Sink,
    host: HostInfo,
    logs: PathBuf,
    cwd: PathBuf,
    timing: Timing,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    slots: Vec<Slot>,
    query: Option<(u64, Instant)>,
    next_query: u64,
    next_token: u64,
}

struct Slot {
    config: ProviderConfig,
    generation: u64,
    state: State,
    crashes: VecDeque<Instant>,
    peer: Option<Peer>,
    dynamic: bool,
    min_query_length: usize,
    /// The query this provider was last sent, to cancel it.
    asked: Option<u64>,
    /// `execute` requests in flight: request id → token.
    pending: HashMap<i64, u64>,
    next_request: i64,
}

struct Peer {
    out: Sender<Outgoing>,
    child: Option<Arc<Mutex<Child>>>,
}

enum Outgoing {
    Message(Message),
    Query(Query),
}

/// Statuses for `flint status`, in `[[providers]]` order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Its id.
    pub name: String,
    /// Where it is.
    pub state: State,
}

impl ProviderHost {
    /// A host with no providers. Spawned providers run in `cwd` (Flint's
    /// settings folder, so a relative script path in `command` works) and
    /// log to `logs/providers/<name>.log`.
    pub fn new(host: HostInfo, logs: PathBuf, cwd: PathBuf, timing: Timing, sink: Sink) -> Self {
        ProviderHost {
            shared: Arc::new(Shared {
                sink,
                host,
                logs: logs.join("providers"),
                cwd,
                timing,
                inner: Mutex::new(Inner::default()),
            }),
        }
    }

    /// Runs exactly these providers: starts new ones, restarts changed
    /// ones, stops removed ones.
    pub fn configure(&self, configs: Vec<ProviderConfig>) {
        let mut start = Vec::new();
        {
            let mut inner = self.shared.lock();
            let mut kept = Vec::new();
            for config in configs {
                match inner
                    .slots
                    .iter()
                    .position(|s| s.config.name == config.name)
                {
                    Some(i) if inner.slots[i].config == config => {
                        // Unchanged: keep it running; a disabled one gets
                        // another chance, as after `flint reload`.
                        let mut slot = inner.slots.remove(i);
                        if matches!(slot.state, State::Disabled(_)) && !config.connect {
                            slot.crashes.clear();
                            start.push(config.name.clone());
                        }
                        kept.push(slot);
                    }
                    Some(i) => {
                        let mut slot = inner.slots.remove(i);
                        stop(&mut slot);
                        slot.config = config;
                        start.push(slot.config.name.clone());
                        kept.push(slot);
                    }
                    None => {
                        start.push(config.name.clone());
                        kept.push(Slot::new(config));
                    }
                }
            }
            for mut gone in std::mem::replace(&mut inner.slots, kept) {
                stop(&mut gone);
                (self.shared.sink)(HostEvent::Changed {
                    name: gone.config.name,
                });
            }
        }
        for name in start {
            self.shared.start(&name);
        }
    }

    /// Sends a query to the dynamic providers in scope (only `scope` when
    /// scoped, every dynamic one otherwise) and returns its id. Results
    /// for any earlier query are dropped from now on.
    pub fn query(&self, text: &str, scope: Option<&str>, context: &Context) -> u64 {
        let mut inner = self.shared.lock();
        inner.next_query += 1;
        let id = inner.next_query;
        inner.query = Some((id, Instant::now()));
        for slot in &mut inner.slots {
            let Some(peer) = &slot.peer else { continue };
            let wanted = scope.is_none_or(|s| s == slot.config.name);
            if !slot.dynamic || !wanted || text.chars().count() < slot.min_query_length {
                continue;
            }
            if let Some(previous) = slot.asked.replace(id) {
                let cancel = Message::Notification {
                    method: "cancel".into(),
                    params: json!(Cancel { query_id: previous }),
                };
                let _ = peer.out.send(Outgoing::Message(cancel));
            }
            let _ = peer.out.send(Outgoing::Query(Query {
                query_id: id,
                text: text.to_owned(),
                scoped: scope.is_some(),
                context: context.clone(),
            }));
        }
        id
    }

    /// Asks `name` to carry out an action; the answer arrives as
    /// [`HostEvent::Executed`] with the returned token.
    pub fn execute(&self, name: &str, request: Execute) -> Result<u64, String> {
        let timeout = self.shared.timing.execute_timeout;
        let (token, id, generation) = {
            let mut inner = self.shared.lock();
            inner.next_token += 1;
            let token = inner.next_token;
            let slot = inner
                .slots
                .iter_mut()
                .find(|s| s.config.name == name)
                .ok_or_else(|| format!("there's no provider `{name}`"))?;
            let Some(peer) = &slot.peer else {
                return Err(match slot.state.reason() {
                    Some(why) => format!("{name} isn't running: {why}"),
                    None => format!("{name} isn't running"),
                });
            };
            slot.next_request += 1;
            let id = slot.next_request;
            let request = Message::request(Id::Number(id), "execute", json!(request));
            peer.out
                .send(Outgoing::Message(request))
                .map_err(|_| format!("{name} isn't running"))?;
            slot.pending.insert(id, token);
            (token, id, slot.generation)
        };
        let shared = Arc::clone(&self.shared);
        let name = name.to_owned();
        let _ = std::thread::Builder::new()
            .name("flint-execute-timeout".into())
            .spawn(move || {
                std::thread::sleep(timeout);
                let expired = {
                    let mut inner = shared.lock();
                    inner
                        .slots
                        .iter_mut()
                        .find(|s| s.config.name == name && s.generation == generation)
                        .and_then(|s| s.pending.remove(&id))
                };
                if let Some(token) = expired {
                    (shared.sink)(HostEvent::Executed {
                        token,
                        outcome: Err(format!("{name} didn't answer")),
                        name,
                    });
                }
            });
        Ok(token)
    }

    /// Each provider's state.
    pub fn statuses(&self) -> Vec<Status> {
        self.shared
            .lock()
            .slots
            .iter()
            .map(|s| Status {
                name: s.config.name.clone(),
                state: s.state.clone(),
            })
            .collect()
    }

    /// Serves a provider that connected to the control endpoint, on the
    /// calling thread until it disconnects. `session` has done `hello`.
    pub fn connected(
        &self,
        session: HostSession,
        reader: Box<dyn BufRead + Send>,
        writer: Box<dyn Write + Send>,
    ) {
        let Some(name) = session
            .peer()
            .filter(|p| p.role == Role::Provider)
            .map(|p| p.name.clone())
        else {
            return;
        };
        let (generation, out, debounce) = {
            let mut inner = self.shared.lock();
            let Some(slot) = inner
                .slots
                .iter_mut()
                .find(|s| s.config.name == name && s.config.connect)
            else {
                return;
            };
            stop(slot);
            slot.state = State::Starting;
            let (out, debounce) = self.shared.writer(&name, writer);
            slot.peer = Some(Peer {
                out: out.clone(),
                child: None,
            });
            (slot.generation, out, debounce)
        };
        (self.shared.sink)(HostEvent::Changed { name: name.clone() });
        let log = ProviderLog::open(&self.shared.logs, &name);
        log.line("connected");
        let why = self
            .shared
            .serve(&name, generation, session, reader, &out, &debounce, &log);
        self.shared.ended(&name, generation, why, &log);
    }

    /// Stops every provider.
    pub fn shutdown(&self) {
        let mut inner = self.shared.lock();
        for slot in &mut inner.slots {
            stop(slot);
        }
    }
}

impl Slot {
    fn new(config: ProviderConfig) -> Slot {
        Slot {
            state: if config.connect {
                State::Waiting
            } else {
                State::Starting
            },
            config,
            generation: 0,
            crashes: VecDeque::new(),
            peer: None,
            dynamic: false,
            min_query_length: 0,
            asked: None,
            pending: HashMap::new(),
            next_request: 0,
        }
    }
}

/// Stops a slot's peer: the writer ends when its channel closes, a child
/// is killed (and reaped in the background).
fn stop(slot: &mut Slot) {
    slot.generation += 1;
    slot.dynamic = false;
    slot.asked = None;
    slot.pending.clear();
    if let Some(peer) = slot.peer.take() {
        if let Some(child) = peer.child {
            let _ = std::thread::Builder::new()
                .name("flint-provider-stop".into())
                .spawn(move || {
                    let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = child.kill();
                    let _ = child.wait();
                });
        }
    }
    if slot.config.connect {
        slot.state = State::Waiting;
    }
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Spawns a configured provider, unless it connects by itself.
    fn start(self: &Arc<Self>, name: &str) {
        let log = ProviderLog::open(&self.logs, name);
        let (command, generation) = {
            let mut inner = self.lock();
            let Some(slot) = inner.slots.iter_mut().find(|s| s.config.name == name) else {
                return;
            };
            if slot.config.connect {
                slot.state = State::Waiting;
                drop(inner);
                (self.sink)(HostEvent::Changed {
                    name: name.to_owned(),
                });
                return;
            }
            slot.generation += 1;
            slot.state = State::Starting;
            (slot.config.command.clone(), slot.generation)
        };
        let spawned = spawn(&command, &self.cwd);
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                let why = format!("couldn't start {}: {e}", command.join(" "));
                log.line(&why);
                if let Some(slot) = self
                    .lock()
                    .slots
                    .iter_mut()
                    .find(|s| s.config.name == name && s.generation == generation)
                {
                    // A program that isn't there won't appear by retrying.
                    slot.state = State::Disabled(why);
                }
                (self.sink)(HostEvent::Changed {
                    name: name.to_owned(),
                });
                return;
            }
        };
        log.line(&format!("started: {}", command.join(" ")));
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return;
        };
        let child = Arc::new(Mutex::new(child));
        let (out, debounce) = self.writer(name, Box::new(stdin));
        {
            let mut inner = self.lock();
            let Some(slot) = inner
                .slots
                .iter_mut()
                .find(|s| s.config.name == name && s.generation == generation)
            else {
                return;
            };
            slot.peer = Some(Peer {
                out: out.clone(),
                child: Some(Arc::clone(&child)),
            });
        }
        (self.sink)(HostEvent::Changed {
            name: name.to_owned(),
        });

        // The last thing it said on stderr, kept for the reason it stopped:
        // a program that exits at once (a script that isn't there, a bad
        // argument) explains itself there, and "exit code: 2" alone would
        // send the user to the log to find out.
        let stderr_log = log.clone();
        let (said, last_said) = mpsc::channel::<Option<String>>();
        let _ = std::thread::Builder::new()
            .name(format!("flint-{name}-stderr"))
            .spawn(move || {
                let mut last = None;
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(line) => {
                            stderr_log.line(&format!("stderr: {line}"));
                            if !line.trim().is_empty() {
                                last = Some(line.trim().to_owned());
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = said.send(last);
            });

        let shared = Arc::clone(self);
        let name = name.to_owned();
        let _ = std::thread::Builder::new()
            .name(format!("flint-{name}-read"))
            .spawn(move || {
                let session = HostSession::new(shared.host.clone(), vec![name.clone()]);
                let why = shared.serve(
                    &name,
                    generation,
                    session,
                    Box::new(BufReader::new(stdout)),
                    &out,
                    &debounce,
                    &log,
                );
                // Whatever ended the conversation, the process is done for:
                // one that broke the protocol may still be running, blocked
                // reading a stdin it gets no answer on. A moment's grace
                // lets one that is exiting anyway report its own status (a
                // crash's code); then it is killed.
                let status = {
                    let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
                    if !matches!(child.try_wait(), Ok(Some(_))) {
                        std::thread::sleep(Duration::from_millis(50));
                        if !matches!(child.try_wait(), Ok(Some(_))) {
                            let _ = child.kill();
                        }
                    }
                    child.wait()
                };
                let why = match status {
                    Ok(status) if status.success() => format!("{why} (it exited)"),
                    Ok(status) => format!("{why} ({status})"),
                    Err(_) => why,
                };
                // Its stderr ends with it, unless a child of its own holds
                // the pipe open; don't wait on that.
                let why = match last_said.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(line)) => format!("{why}: {}", shorten(&line)),
                    _ => why,
                };
                shared.ended(&name, generation, why, &log);
            });
    }

    /// The writer thread for a peer: the channel to it, and the debounce
    /// it applies to queries (set when the provider registers).
    fn writer(
        &self,
        name: &str,
        mut writer: Box<dyn Write + Send>,
    ) -> (Sender<Outgoing>, Arc<Mutex<Duration>>) {
        let (out, queue): (Sender<Outgoing>, Receiver<Outgoing>) = mpsc::channel();
        let debounce = Arc::new(Mutex::new(Duration::ZERO));
        let wait = Arc::clone(&debounce);
        let _ = std::thread::Builder::new()
            .name(format!("flint-{name}-write"))
            .spawn(move || {
                let mut last_query: Option<Instant> = None;
                let write = |writer: &mut Box<dyn Write + Send>, message: &Message| {
                    write_frame(writer, &message.to_bytes()).is_ok()
                };
                while let Ok(next) = queue.recv() {
                    let mut query = match next {
                        Outgoing::Message(message) => {
                            if !write(&mut writer, &message) {
                                return;
                            }
                            continue;
                        }
                        Outgoing::Query(query) => query,
                    };
                    let debounce = *wait.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(last) = last_query {
                        let ready = last + debounce;
                        loop {
                            let now = Instant::now();
                            if now >= ready {
                                break;
                            }
                            match queue.recv_timeout(ready - now) {
                                Ok(Outgoing::Query(newer)) => query = newer,
                                Ok(Outgoing::Message(message)) => {
                                    if !write(&mut writer, &message) {
                                        return;
                                    }
                                }
                                Err(RecvTimeoutError::Timeout) => break,
                                Err(RecvTimeoutError::Disconnected) => return,
                            }
                        }
                    }
                    last_query = Some(Instant::now());
                    let message = Message::Notification {
                        method: "query".into(),
                        params: json!(query),
                    };
                    if !write(&mut writer, &message) {
                        return;
                    }
                }
            });
        (out, debounce)
    }

    /// Reads a peer until it ends; returns why it ended.
    #[allow(clippy::too_many_arguments)]
    fn serve(
        self: &Arc<Self>,
        name: &str,
        generation: u64,
        mut session: HostSession,
        mut reader: Box<dyn BufRead + Send>,
        out: &Sender<Outgoing>,
        debounce: &Arc<Mutex<Duration>>,
        log: &ProviderLog,
    ) -> String {
        let items = self.coalescer(name, generation);
        loop {
            let body = match read_frame(&mut reader) {
                Ok(Some(body)) => body,
                Ok(None) => return "it closed its output".into(),
                Err(e) => return format!("it broke the protocol: {e}"),
            };
            match session.handle(&body) {
                Action::Reply(message) => {
                    let _ = out.send(Outgoing::Message(message));
                }
                Action::ReplyAndClose(message) => {
                    let _ = out.send(Outgoing::Message(message));
                    return "its hello was refused".into();
                }
                Action::Provider(ProviderMessage::Register { id, register }) => {
                    *debounce.lock().unwrap_or_else(|e| e.into_inner()) =
                        Duration::from_millis(register.debounce_ms);
                    {
                        let mut inner = self.lock();
                        let Some(slot) = inner
                            .slots
                            .iter_mut()
                            .find(|s| s.config.name == name && s.generation == generation)
                        else {
                            return "it was stopped".into();
                        };
                        slot.dynamic = register.dynamic;
                        slot.min_query_length = register.min_query_length;
                        slot.state = State::Running;
                    }
                    let _ = out.send(Outgoing::Message(Message::result(id, json!({}))));
                    log.line(&format!(
                        "registered as {} ({} items, {})",
                        register.title,
                        register.items.len(),
                        if register.dynamic {
                            "dynamic"
                        } else {
                            "static"
                        }
                    ));
                    (self.sink)(HostEvent::Registered {
                        name: name.to_owned(),
                        title: register.title,
                        prefix: register.prefix,
                        dynamic: register.dynamic,
                    });
                    let _ = items.send(register.items);
                }
                Action::Provider(ProviderMessage::SetItems(set)) => {
                    let _ = items.send(set.items);
                }
                Action::Provider(ProviderMessage::Results(results)) => {
                    let fresh = self.lock().query.is_some_and(|(id, sent)| {
                        id == results.query_id && sent.elapsed() <= self.timing.result_deadline
                    });
                    if fresh {
                        (self.sink)(HostEvent::Results {
                            name: name.to_owned(),
                            query_id: results.query_id,
                            items: results.items,
                            done: results.done,
                        });
                    }
                }
                Action::Provider(ProviderMessage::Log(line)) => {
                    let level = match line.level {
                        LogLevel::Info => "info",
                        LogLevel::Warn => "warn",
                        LogLevel::Error => "error",
                    };
                    log.line(&format!("{level}: {}", line.message));
                }
                Action::Response {
                    id: Some(Id::Number(id)),
                    outcome,
                } => {
                    let token = self
                        .lock()
                        .slots
                        .iter_mut()
                        .find(|s| s.config.name == name && s.generation == generation)
                        .and_then(|s| s.pending.remove(&id));
                    if let Some(token) = token {
                        let outcome = match outcome {
                            Ok(value) => serde_json::from_value::<ExecuteResult>(value)
                                .map_err(|e| format!("{name} answered `execute` oddly: {e}")),
                            Err(error) => Err(error.message),
                        };
                        (self.sink)(HostEvent::Executed {
                            token,
                            name: name.to_owned(),
                            outcome,
                        });
                    }
                }
                Action::Problem(why) => log.line(&format!("ignored a message: {why}")),
                Action::Response { .. } | Action::Client { .. } | Action::Nothing => {}
            }
        }
    }

    /// Applies `provider/setItems` at most every `reindex_every`, always
    /// the newest.
    fn coalescer(self: &Arc<Self>, name: &str, generation: u64) -> Sender<Vec<Item>> {
        let (send, receive): (Sender<Vec<Item>>, Receiver<Vec<Item>>) = mpsc::channel();
        let shared = Arc::clone(self);
        let name = name.to_owned();
        let _ = std::thread::Builder::new()
            .name(format!("flint-{name}-items"))
            .spawn(move || {
                let mut pending: Option<Vec<Item>> = None;
                loop {
                    let items = match pending.take() {
                        Some(items) => items,
                        None => match receive.recv() {
                            Ok(items) => items,
                            Err(_) => return,
                        },
                    };
                    let current = shared
                        .lock()
                        .slots
                        .iter()
                        .any(|s| s.config.name == name && s.generation == generation);
                    if !current {
                        return;
                    }
                    (shared.sink)(HostEvent::Items {
                        name: name.clone(),
                        items,
                    });
                    std::thread::sleep(shared.timing.reindex_every);
                    while let Ok(newer) = receive.try_recv() {
                        pending = Some(newer);
                    }
                }
            });
        send
    }

    /// A peer ended: fail what it owed, then restart it, wait for it to
    /// connect again, or disable it.
    fn ended(self: &Arc<Self>, name: &str, generation: u64, why: String, log: &ProviderLog) {
        let (owed, restart) = {
            let mut inner = self.lock();
            let Some(slot) = inner
                .slots
                .iter_mut()
                .find(|s| s.config.name == name && s.generation == generation)
            else {
                return; // stopped or replaced on purpose
            };
            slot.peer = None;
            slot.dynamic = false;
            slot.asked = None;
            let owed: Vec<u64> = slot.pending.drain().map(|(_, token)| token).collect();
            log.line(&format!("stopped: {why}"));
            let restart = if slot.config.connect {
                slot.state = State::Waiting;
                None
            } else {
                let now = Instant::now();
                slot.crashes.push_back(now);
                while slot
                    .crashes
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > self.timing.crash_window)
                {
                    slot.crashes.pop_front();
                }
                let recent = slot.crashes.len();
                if recent >= self.timing.crash_limit {
                    slot.state = State::Disabled(format!(
                        "it stopped {recent} times within {} s; last: {why}",
                        self.timing.crash_window.as_secs()
                    ));
                    None
                } else {
                    let wait = self.timing.backoff[(recent - 1).min(self.timing.backoff.len() - 1)];
                    slot.state = State::Restarting(format!(
                        "{why}; restarting in {} s",
                        wait.as_secs_f32().ceil()
                    ));
                    Some(wait)
                }
            };
            (owed, restart)
        };
        for token in owed {
            (self.sink)(HostEvent::Executed {
                token,
                name: name.to_owned(),
                outcome: Err(format!("{name} stopped before answering")),
            });
        }
        (self.sink)(HostEvent::Changed {
            name: name.to_owned(),
        });
        if let Some(wait) = restart {
            let shared = Arc::clone(self);
            let name = name.to_owned();
            let _ = std::thread::Builder::new()
                .name("flint-provider-restart".into())
                .spawn(move || {
                    std::thread::sleep(wait);
                    let still = shared.lock().slots.iter().any(|s| {
                        s.config.name == name && s.generation == generation && s.peer.is_none()
                    });
                    if still {
                        shared.start(&name);
                    }
                });
        }
    }
}

/// Spawns `command` with its standard streams piped, without a console
/// window on Windows, and never through a shell.
fn spawn(command: &[String], cwd: &Path) -> io::Result<Child> {
    let program = command
        .first()
        .ok_or_else(|| io::Error::other("empty command"))?;
    // `resolve_command` finds `.cmd`/`.bat` shims on Windows' PATH the way
    // a shell would (npm-installed tools, `py`); elsewhere it's a no-op.
    let mut process = Command::new(fenix_rpc::resolve_command(program));
    process
        .args(&command[1..])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("XDG_ACTIVATION_TOKEN")
        .env_remove("DESKTOP_STARTUP_ID");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        process.creation_flags(CREATE_NO_WINDOW);
    }
    process.spawn()
}

/// A provider's log: `logs/providers/<name>.log`, a timestamp per line,
/// started afresh past 1 MiB with the previous kept as `.old`.
#[derive(Clone)]
struct ProviderLog {
    path: PathBuf,
    file: Arc<Mutex<Option<File>>>,
}

const MAX_LOG: u64 = 1024 * 1024;

impl ProviderLog {
    fn open(dir: &Path, name: &str) -> ProviderLog {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(format!("{name}.log"));
        ProviderLog {
            file: Arc::new(Mutex::new(open_append(&path))),
            path,
        }
    }

    fn line(&self, text: &str) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() > MAX_LOG)
        {
            let mut old = self.path.as_os_str().to_owned();
            old.push(".old");
            *file = None;
            let _ = std::fs::rename(&self.path, old);
            *file = open_append(&self.path);
        }
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{stamp} {text}");
        }
    }
}

/// A line short enough for the footer.
fn shorten(line: &str) -> String {
    const MAX: usize = 160;
    match line.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}

fn open_append(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}
