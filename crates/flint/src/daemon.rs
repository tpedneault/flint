//! The daemon role: the resident process that owns the window, the hotkey
//! and the control endpoint.
//!
//! Three threads matter. The event-loop thread owns the window and the
//! hotkey (Windows delivers a hotkey to the thread that registered it) and
//! does all the work. The control server's threads (`flint-host`) read
//! client requests and hand each to the event loop as an [`Event`], then
//! wait for its answer, at most [`ANSWER_WITHIN`]. Nothing else runs:
//! while hidden the daemon sleeps until an event arrives.
//!
//! Settings are read at start, on `flint reload`, and on show when the
//! file is newer than what was read, which keeps the daemon free of a
//! watcher thread while still picking up an edit the next time it's used.
//!
//! The logic that doesn't need a window ([`Core`]: applying settings,
//! re-registering the hotkey when its chord changes, recording timings)
//! is generic over [`Hotkey`] and tested with a fake.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use flint_config::{Chord, Folders, Loaded};
use flint_gui::{Controller, Gui, Proxy, ShowRequest, Shown, Ui};
use flint_host::ClientHandler;
use flint_platform::{console, process, Hotkey, HotkeyError, Listener, SystemHotkey};
use flint_protocol::client::{
    Adapter, HotkeyState, HotkeyStatus, ReloadResult, ShowParams, Status, ToggleResult,
};
use flint_protocol::{code, ClientRequest, HostInfo, RpcError};
use serde_json::{json, Value};

use crate::cli::Start;
use crate::log::Log;
use crate::report;
use crate::timings::Recorder;

/// How long a control request may wait for the event loop.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// What reaches the event loop from other threads.
pub enum Event {
    /// The hotkey was pressed, at this instant.
    Hotkey(Instant),
    /// A client asked for something; answer on `reply`.
    Request {
        /// What was asked.
        request: ClientRequest,
        /// When it arrived, for the timing log.
        at: Instant,
        /// Where the answer goes.
        reply: mpsc::Sender<Result<Value, RpcError>>,
    },
    /// `flint quit` was answered; stop.
    Exit,
}

/// Starts the daemon on `listener` and runs until `flint quit`.
pub fn run(
    listener: Listener,
    start: Start,
    process_start: Instant,
    console_wanted: bool,
) -> ExitCode {
    if console_wanted {
        console::open();
    }
    let Some(folders) = Folders::current() else {
        eprintln!("flint: no home folder to keep settings and logs in");
        return ExitCode::FAILURE;
    };
    let log = Arc::new(Log::open(&folders.logs()));
    log.line(&format!(
        "Flint {} starting, pid {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    ));
    let settings_path = folders.settings_file();
    let loaded = flint_config::load(&settings_path);
    let gui = match Gui::<Event>::new() {
        Ok(gui) => gui,
        Err(e) => {
            log.line(&format!("can't start: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let failure = Arc::new(Mutex::new(None));
    let daemon = Daemon {
        proxy: Arc::new(Mutex::new(gui.proxy())),
        log: Arc::clone(&log),
        listener: Some(listener),
        start,
        process_start,
        cold_start: None,
        settings_path,
        first_load: Some(loaded.clone()),
        core: None,
        failure: Arc::clone(&failure),
    };
    let outcome = gui.run(daemon, loaded.settings);
    let failed = failure.lock().unwrap_or_else(|e| e.into_inner()).take();
    match (outcome, failed) {
        (Err(e), _) | (Ok(()), Some(e)) => {
            log.line(&format!("can't run: {e}"));
            ExitCode::FAILURE
        }
        (Ok(()), None) => {
            log.line("stopped");
            ExitCode::SUCCESS
        }
    }
}

struct Daemon {
    proxy: Arc<Mutex<Proxy<Event>>>,
    log: Arc<Log>,
    listener: Option<Listener>,
    start: Start,
    process_start: Instant,
    cold_start: Option<Duration>,
    settings_path: PathBuf,
    first_load: Option<Loaded>,
    core: Option<Core<SystemHotkey>>,
    /// Why the daemon stopped early, for [`run`] to report after the loop.
    failure: Arc<Mutex<Option<String>>>,
}

impl Daemon {
    fn status(&self, ui: &Ui<'_>, timings: bool) -> Status {
        let core = self.core.as_ref();
        Status {
            version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            uptime_ms: self.process_start.elapsed().as_millis() as u64,
            cold_start_ms: self.cold_start.map(|d| d.as_millis() as u64),
            visible: ui.visible(),
            adapter: ui.adapter().map(|a| Adapter {
                name: a.name,
                backend: a.backend,
                fallback: a.fallback,
            }),
            hotkey: core.map_or(
                HotkeyStatus {
                    chord: String::new(),
                    state: HotkeyState::Unsupported,
                    message: Some("not ready".into()),
                },
                |c| c.hotkey_status.clone(),
            ),
            providers: vec![],
            memory_bytes: process::resident_bytes(),
            settings_errors: core.map(|c| c.loaded.errors.clone()).unwrap_or_default(),
            settings_file: Some(self.settings_path.display().to_string()),
            log_file: Some(self.log.path().display().to_string()),
            timings: timings.then(|| core.map(|c| c.timings.report())).flatten(),
        }
    }

    /// Updates the list and the footer from the current status.
    fn refresh(&self, ui: &mut Ui<'_>) {
        let status = self.status(ui, false);
        ui.set_rows(report::rows(&status));
        ui.set_notice(report::notice(&status));
    }

    fn reload(&mut self, ui: &mut Ui<'_>) -> Vec<String> {
        let loaded = flint_config::load(&self.settings_path);
        let Some(core) = &mut self.core else {
            return loaded.errors;
        };
        for line in core.apply(loaded) {
            self.log.line(&line);
        }
        ui.apply_settings(core.loaded.settings.clone());
        let errors = core.loaded.errors.clone();
        self.refresh(ui);
        errors
    }

    /// Reads settings again when the file changed since it was last read.
    fn reload_if_changed(&mut self, ui: &mut Ui<'_>) {
        if self
            .core
            .as_ref()
            .is_some_and(|c| c.stale(&self.settings_path))
        {
            self.log.line("settings.toml changed; reading it again");
            self.reload(ui);
        }
    }

    fn show_request(params: ShowParams, source: &'static str, at: Instant) -> ShowRequest {
        ShowRequest {
            query: params.query,
            activation_token: params.activation_token,
            source,
            requested_at: at,
        }
    }

    fn answer(
        &mut self,
        request: ClientRequest,
        at: Instant,
        ui: &mut Ui<'_>,
    ) -> Result<Value, RpcError> {
        Ok(match request {
            ClientRequest::Show(params) => {
                self.reload_if_changed(ui);
                ui.show(Self::show_request(params, "show", at));
                json!({})
            }
            ClientRequest::Hide => {
                ui.hide();
                json!({})
            }
            ClientRequest::Toggle(params) => {
                if !ui.visible() {
                    self.reload_if_changed(ui);
                }
                let show = ShowParams {
                    query: None,
                    activation_token: params.activation_token,
                };
                let visible = ui.toggle(Self::show_request(show, "toggle", at));
                json!(ToggleResult { visible })
            }
            ClientRequest::Reload => json!(ReloadResult {
                errors: self.reload(ui)
            }),
            ClientRequest::Status(params) => json!(self.status(ui, params.timings)),
            // Answered by the bridge itself, so the answer is sent before
            // the daemon goes.
            ClientRequest::Quit => json!({}),
        })
    }
}

impl Daemon {
    fn serve(&self, mut listener: Listener) {
        let bridge: Arc<dyn ClientHandler> = Arc::new(Bridge {
            proxy: Arc::clone(&self.proxy),
            log: Arc::clone(&self.log),
        });
        let host = HostInfo {
            name: "flint".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
        };
        let spawned = std::thread::Builder::new()
            .name("flint-control-accept".into())
            .spawn(move || {
                flint_host::serve(
                    move || {
                        let stream = listener.accept()?;
                        Ok((stream.try_clone()?, stream))
                    },
                    host,
                    vec![],
                    bridge,
                )
            });
        if let Err(e) = spawned {
            self.log
                .line(&format!("control endpoint: no thread to serve it: {e}"));
        }
    }
}

impl Controller for Daemon {
    type Event = Event;

    fn ready(&mut self, ui: &mut Ui<'_>, result: Result<(), String>) {
        if let Err(e) = result {
            self.log.line(&format!("can't start: {e}"));
            *self.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
            ui.exit();
            return;
        }
        // On this thread: Windows delivers the hotkey to the thread that
        // registered it.
        let proxy = Arc::clone(&self.proxy);
        let hotkey = SystemHotkey::new(move || {
            let proxy = proxy.lock().unwrap_or_else(|e| e.into_inner());
            proxy.send(Event::Hotkey(Instant::now()));
        });
        let mut core = Core::new(hotkey);
        if let Some(loaded) = self.first_load.take() {
            for line in core.apply(loaded) {
                self.log.line(&line);
            }
        }
        self.core = Some(core);
        if let Some(adapter) = ui.adapter() {
            self.log.line(&format!(
                "GPU: {} ({}{})",
                adapter.name,
                adapter.backend,
                if adapter.fallback {
                    ", software fallback"
                } else {
                    ""
                }
            ));
        }

        if let Some(listener) = self.listener.take() {
            self.serve(listener);
        }
        let cold = self.process_start.elapsed();
        self.cold_start = Some(cold);
        self.log.line(&format!("ready in {} ms", cold.as_millis()));
        self.refresh(ui);
        if let Some(params) = self.start.show.take() {
            ui.show(Self::show_request(params, "start", Instant::now()));
        }
    }

    fn event(&mut self, event: Event, ui: &mut Ui<'_>) {
        match event {
            Event::Hotkey(at) => {
                if !ui.visible() {
                    self.reload_if_changed(ui);
                }
                ui.toggle(Self::show_request(ShowParams::default(), "hotkey", at));
            }
            Event::Request { request, at, reply } => {
                let answer = self.answer(request, at, ui);
                // The client may have given up; nothing to do then.
                let _ = reply.send(answer);
            }
            Event::Exit => {
                self.log.line("quitting at a client's request");
                ui.exit();
            }
        }
    }

    fn shown(&mut self, shown: Shown, _ui: &mut Ui<'_>) {
        if let Some(core) = &mut self.core {
            let line = core.timings.record(shown.source, shown.elapsed);
            self.log.line(&line);
        }
    }
}

/// Carries client requests from the control server's threads to the
/// event loop, and the answers back.
struct Bridge {
    proxy: Arc<Mutex<Proxy<Event>>>,
    log: Arc<Log>,
}

impl ClientHandler for Bridge {
    fn handle(&self, request: ClientRequest) -> Result<Value, RpcError> {
        if request == ClientRequest::Quit {
            return Ok(json!({}));
        }
        let (reply, answer) = mpsc::channel();
        let sent = {
            let proxy = self.proxy.lock().unwrap_or_else(|e| e.into_inner());
            proxy.send(Event::Request {
                request,
                at: Instant::now(),
                reply,
            })
        };
        if !sent {
            return Err(RpcError::new(code::UNAVAILABLE, "Flint is shutting down"));
        }
        answer.recv_timeout(ANSWER_WITHIN).unwrap_or_else(|_| {
            Err(RpcError::new(
                code::INTERNAL_ERROR,
                format!(
                    "Flint's window didn't answer within {} s",
                    ANSWER_WITHIN.as_secs()
                ),
            ))
        })
    }

    fn replied(&self, request: &ClientRequest) {
        if *request == ClientRequest::Quit {
            let proxy = self.proxy.lock().unwrap_or_else(|e| e.into_inner());
            proxy.send(Event::Exit);
        }
    }

    fn log(&self, line: &str) {
        self.log.line(line);
    }
}

/// The daemon's state that needs no window: settings, the hotkey and its
/// status, and the timings.
pub struct Core<H: Hotkey> {
    hotkey: H,
    /// What `flint status` says about the hotkey.
    pub hotkey_status: HotkeyStatus,
    /// The chord last handed to the OS, successfully or not.
    registered: Option<Chord>,
    /// The settings in force and their problems.
    pub loaded: Loaded,
    /// The recent shows.
    pub timings: Recorder,
}

impl<H: Hotkey> Core<H> {
    /// A core with default settings and no hotkey registered yet.
    pub fn new(hotkey: H) -> Self {
        Core {
            hotkey,
            hotkey_status: HotkeyStatus {
                chord: String::new(),
                state: HotkeyState::Unsupported,
                message: Some("not registered yet".into()),
            },
            registered: None,
            loaded: flint_config::parse(""),
            timings: Recorder::default(),
        }
    }

    /// Puts `loaded` in force: registers the hotkey when its chord is new
    /// (or the last attempt failed), and returns lines for the log.
    pub fn apply(&mut self, loaded: Loaded) -> Vec<String> {
        let mut log: Vec<String> = loaded
            .errors
            .iter()
            .map(|e| format!("settings: {e}"))
            .collect();
        let chord = loaded.settings.hotkey;
        let retry = self.hotkey_status.state == HotkeyState::Failed;
        if self.registered != Some(chord) || retry {
            self.registered = Some(chord);
            self.hotkey_status = match self.hotkey.set(chord) {
                Ok(()) => {
                    log.push(format!("hotkey {chord} registered"));
                    HotkeyStatus {
                        chord: chord.to_string(),
                        state: HotkeyState::Registered,
                        message: None,
                    }
                }
                Err(e) => {
                    let state = match e {
                        HotkeyError::Unsupported(_) => HotkeyState::Unsupported,
                        HotkeyError::Failed(_) => HotkeyState::Failed,
                    };
                    log.push(format!("hotkey {chord}: {e}"));
                    HotkeyStatus {
                        chord: chord.to_string(),
                        state,
                        message: Some(e.to_string()),
                    }
                }
            };
        }
        self.loaded = loaded;
        log
    }

    /// Whether `path` changed since it was read: newer, appeared or gone.
    pub fn stale(&self, path: &std::path::Path) -> bool {
        let now: Option<SystemTime> = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        now != self.loaded.modified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hotkey that refuses the chords it's told to.
    #[derive(Default)]
    struct FakeHotkey {
        set: Vec<String>,
        refuse: Vec<String>,
        unsupported: bool,
    }

    impl Hotkey for FakeHotkey {
        fn set(&mut self, chord: Chord) -> Result<(), HotkeyError> {
            self.set.push(chord.to_string());
            if self.unsupported {
                Err(HotkeyError::Unsupported("Wayland".into()))
            } else if self.refuse.contains(&chord.to_string()) {
                Err(HotkeyError::Failed(format!("{chord} is taken")))
            } else {
                Ok(())
            }
        }
    }

    fn settings(text: &str) -> Loaded {
        flint_config::parse(text)
    }

    #[test]
    fn the_hotkey_is_registered_once_and_again_only_when_it_changes() {
        let mut core = Core::new(FakeHotkey::default());
        core.apply(settings(""));
        core.apply(settings("appearance.rows = 5"));
        assert_eq!(core.hotkey.set, ["Alt+Space"]);
        assert_eq!(core.hotkey_status.state, HotkeyState::Registered);
        core.apply(settings("hotkey.toggle = \"Ctrl+Shift+Space\""));
        assert_eq!(core.hotkey.set, ["Alt+Space", "Ctrl+Shift+Space"]);
        assert_eq!(core.hotkey_status.chord, "Ctrl+Shift+Space");
    }

    #[test]
    fn a_refused_hotkey_is_reported_and_retried_on_reload() {
        let mut core = Core::new(FakeHotkey {
            refuse: vec!["Alt+Space".into()],
            ..Default::default()
        });
        let log = core.apply(settings(""));
        assert_eq!(core.hotkey_status.state, HotkeyState::Failed);
        assert!(core
            .hotkey_status
            .message
            .as_deref()
            .unwrap()
            .contains("taken"));
        assert!(log.iter().any(|l| l.contains("taken")));
        // The other app may have let go of it since.
        core.hotkey.refuse.clear();
        core.apply(settings(""));
        assert_eq!(core.hotkey_status.state, HotkeyState::Registered);
        assert_eq!(core.hotkey.set.len(), 2);
    }

    #[test]
    fn unsupported_hotkeys_are_not_retried() {
        let mut core = Core::new(FakeHotkey {
            unsupported: true,
            ..Default::default()
        });
        core.apply(settings(""));
        core.apply(settings(""));
        assert_eq!(core.hotkey_status.state, HotkeyState::Unsupported);
        assert_eq!(core.hotkey.set.len(), 1);
    }

    #[test]
    fn settings_problems_are_logged_and_kept_for_status() {
        let mut core = Core::new(FakeHotkey::default());
        let log = core.apply(settings("appearance.width = 9"));
        assert!(log
            .iter()
            .any(|l| l.starts_with("settings: `appearance.width`")));
        assert_eq!(core.loaded.errors.len(), 1);
        assert_eq!(core.loaded.settings.width, 720);
    }

    #[test]
    fn a_missing_file_that_stays_missing_is_not_stale() {
        let core = Core::new(FakeHotkey::default());
        assert!(!core.stale(std::path::Path::new("nowhere/settings.toml")));
    }
}
