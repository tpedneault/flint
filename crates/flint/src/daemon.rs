//! The daemon role: the resident process that owns the window, the hotkey,
//! the control endpoint and the catalog of everything Flint can find.
//!
//! Three kinds of thread matter. The event-loop thread owns the window,
//! the hotkey (Windows delivers a hotkey to the thread that registered it)
//! and the catalog, and does all the work: a keystroke searches right
//! there, inside the frame. The control server's threads (`flint-host`)
//! read client requests and hand each to the event loop as an [`Event`],
//! then wait for its answer, at most [`ANSWER_WITHIN`]. Short-lived
//! threads scan for apps and save frecency, so neither a cold disk nor a
//! slow write is ever waited for. While hidden the daemon sleeps until an
//! event arrives.
//!
//! Settings are read at start, on `flint reload`, and on show when the
//! file is newer than what was read, which keeps the daemon free of a
//! watcher thread while still picking up an edit the next time it's used.
//! Apps are scanned at start and again on a show more than
//! [`RESCAN_AFTER`] after the last scan; results that arrive while the
//! list is on screen go below the selection (the stable merge).
//!
//! The logic that doesn't need a window lives in [`Core`] (settings, the
//! hotkey and its status, timings; generic over [`Hotkey`] and tested with
//! a fake) and in [`Catalog`] (searching and what's shown).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flint_config::{Chord, Folders, Loaded};
use flint_gui::{Controller, FooterLine, Gui, Proxy, ShowRequest, Shown, Ui};
use flint_host::ClientHandler;
use flint_platform::{
    console, launch, process, Clipboard, Hotkey, HotkeyError, Listener, SystemHotkey,
};
use flint_protocol::client::{
    Adapter, HotkeyState, HotkeyStatus, ProviderStatus, ReloadResult, ShowParams, Status,
    ToggleResult,
};
use flint_protocol::item::{Action, ActionKind};
use flint_protocol::{code, ClientRequest, HostInfo, Item, RpcError};
use flint_providers::{apps, commands, quicklinks};
use serde_json::{json, Value};

use crate::catalog::Catalog;
use crate::cli::Start;
use crate::log::Log;
use crate::report;
use crate::store::{self, Saver};
use crate::timings::Recorder;

/// How long a control request may wait for the event loop.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);
/// Apps are scanned again on a show this long after the last scan.
pub const RESCAN_AFTER: Duration = Duration::from_secs(5 * 60);

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
    /// An apps scan finished.
    Apps(Vec<Item>),
    /// `flint quit` was answered; stop.
    Exit,
}

/// What the window does after an action succeeded.
enum After {
    /// Hide: the action handed off to something else.
    Hide,
    /// Stay open (a message, a reload).
    Stay,
    /// Stay open and search again (the query was replaced).
    Search,
    /// Stop Flint.
    Exit,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
    let frecency_path = folders.state("frecency.json");
    let catalog = Catalog::new(store::load(&frecency_path, &log));
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
        saver: Saver::start(frecency_path, Arc::clone(&log)),
        log: Arc::clone(&log),
        listener: Some(listener),
        start,
        process_start,
        cold_start: None,
        settings_path,
        first_load: Some(loaded.clone()),
        core: None,
        catalog,
        link_problems: Vec::new(),
        clipboard: Clipboard::new(),
        apps_scanned: None,
        scanning: false,
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
    saver: Saver,
    listener: Option<Listener>,
    start: Start,
    process_start: Instant,
    cold_start: Option<Duration>,
    settings_path: PathBuf,
    first_load: Option<Loaded>,
    core: Option<Core<SystemHotkey>>,
    catalog: Catalog,
    /// Quicklinks whose keyword a built-in provider has.
    link_problems: Vec<String>,
    clipboard: Clipboard,
    apps_scanned: Option<Instant>,
    scanning: bool,
    /// Why the daemon stopped early, for [`run`] to report after the loop.
    failure: Arc<Mutex<Option<String>>>,
}

impl Daemon {
    fn status(&self, ui: &Ui<'_>, timings: bool) -> Status {
        let core = self.core.as_ref();
        let built_in = |name: &str, count: usize, extra: Option<&str>| ProviderStatus {
            name: name.into(),
            state: "built in".into(),
            message: Some(match extra {
                Some(extra) => format!("{count} items, {extra}"),
                None => format!("{count} items"),
            }),
        };
        let mut settings_errors = core.map(|c| c.loaded.errors.clone()).unwrap_or_default();
        settings_errors.extend(self.link_problems.iter().cloned());
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
            providers: vec![
                built_in(
                    apps::ID,
                    self.catalog.count(apps::ID),
                    self.scanning.then_some("scanning"),
                ),
                built_in(quicklinks::ID, self.catalog.count(quicklinks::ID), None),
                built_in(commands::ID, self.catalog.count(commands::ID), None),
            ],
            memory_bytes: process::resident_bytes(),
            settings_errors,
            settings_file: Some(self.settings_path.display().to_string()),
            log_file: Some(self.log.path().display().to_string()),
            timings: timings.then(|| core.map(|c| c.timings.report())).flatten(),
        }
    }

    /// Updates the footer's standing notice from the current status.
    fn refresh(&self, ui: &mut Ui<'_>) {
        let status = self.status(ui, false);
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
        self.link_problems = self
            .catalog
            .set_quicklinks(&core.loaded.settings.quicklinks);
        for problem in &self.link_problems {
            self.log.line(&format!("settings: {problem}"));
        }
        let mut errors = core.loaded.errors.clone();
        errors.extend(self.link_problems.iter().cloned());
        self.refresh(ui);
        if ui.visible() {
            self.search(ui);
        }
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

    /// Lists apps on a thread of its own; the result arrives as
    /// [`Event::Apps`].
    fn scan_apps(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let proxy = Arc::clone(&self.proxy);
        let spawned = std::thread::Builder::new()
            .name("flint-apps".into())
            .spawn(move || {
                let items = apps::scan();
                let proxy = proxy.lock().unwrap_or_else(|e| e.into_inner());
                proxy.send(Event::Apps(items));
            });
        if let Err(e) = spawned {
            self.scanning = false;
            self.log.line(&format!("apps: no thread to scan with: {e}"));
        }
    }

    /// After any show: results for the query, and a fresh apps scan when
    /// the last one is old.
    fn shown_now(&mut self, ui: &mut Ui<'_>) {
        if self
            .apps_scanned
            .is_none_or(|at| at.elapsed() > RESCAN_AFTER)
        {
            self.scan_apps();
        }
        self.search(ui);
    }

    /// Searches for the query line as it is, turning a leading scope
    /// keyword into the chip first.
    fn search(&mut self, ui: &mut Ui<'_>) {
        let mut text = ui.query().to_owned();
        let mut scope = ui.scope().map(|c| c.keyword.clone());
        if scope.is_none() {
            if let Some((chip, rest)) = self.catalog.split_scope(&text) {
                let rest = rest.to_owned();
                scope = Some(chip.keyword.clone());
                ui.set_scope(Some(chip));
                ui.set_query(rest.clone());
                text = rest;
            }
        }
        let rows = self.catalog.search(scope.as_deref(), &text, now());
        ui.set_rows(rows);
    }

    /// New apps arrived: re-rank, keeping on-screen rows in place.
    fn apps_arrived(&mut self, items: Vec<Item>, ui: &mut Ui<'_>) {
        self.scanning = false;
        self.apps_scanned = Some(Instant::now());
        let count = items.len();
        for problem in self.catalog.set_items(apps::ID, items) {
            self.log.line(&format!("apps: {problem}"));
        }
        self.log.line(&format!("apps: {count} found"));
        if !ui.visible() {
            // Draw what the next show will start with, while nobody is
            // waiting: the first show after start otherwise rasterises
            // every row's glyphs and misses the 50 ms budget.
            self.search(ui);
            ui.prepare();
            return;
        }
        if ui.rows_presented() {
            let scope = ui.scope().map(|c| c.keyword.clone());
            let rows = self
                .catalog
                .merge(scope.as_deref(), ui.query(), ui.selected(), now());
            ui.merge_rows(rows);
        } else {
            self.search(ui);
        }
    }

    /// Runs `action` of the item on row `row`.
    fn run_action(&mut self, row: usize, action: &Action, ui: &mut Ui<'_>) {
        let Some(shown) = self.catalog.shown(row).cloned() else {
            return;
        };
        let result = match &action.kind {
            ActionKind::Open { target } => launch::open(target).map(|()| After::Hide),
            ActionKind::Run {
                argv,
                cwd,
                terminal,
            } => launch::run(argv, cwd.as_deref(), *terminal).map(|()| After::Hide),
            ActionKind::Copy { text } => self.clipboard.set(text).map(|()| After::Hide),
            ActionKind::Query { text } => {
                ui.set_scope(None);
                ui.set_query(text.clone());
                Ok(After::Search)
            }
            ActionKind::Provider if shown.provider == commands::ID => {
                self.flint_command(&action.id, ui)
            }
            ActionKind::Provider => Err(format!(
                "`{}` needs its provider, which arrives in M5",
                action.title
            )),
            ActionKind::Reveal { .. } => {
                Err("showing in the file manager arrives with the action menu in M4".into())
            }
            ActionKind::Fenix { .. } => Err("Fenix actions arrive in M7".into()),
            ActionKind::Unknown => Err(format!(
                "`{}` is a kind of action this Flint doesn't know",
                action.title
            )),
        };
        match result {
            Ok(after) => {
                self.catalog.record(&shown.item.id, now());
                self.saver.save(self.catalog.frecency());
                match after {
                    After::Hide => ui.hide(),
                    After::Stay => {}
                    After::Search => self.search(ui),
                    After::Exit => {
                        self.log.line("quitting from the window");
                        ui.exit();
                    }
                }
            }
            Err(why) => {
                self.log.line(&format!("{}: {why}", shown.item.title));
                ui.set_message(FooterLine::error(why));
            }
        }
    }

    fn flint_command(&mut self, action_id: &str, ui: &mut Ui<'_>) -> Result<After, String> {
        match commands::command(action_id) {
            Some(commands::Command::Reload) => {
                let errors = self.reload(ui);
                ui.set_message(match errors.first() {
                    None => FooterLine::info("Settings reloaded"),
                    Some(first) if errors.len() == 1 => FooterLine::error(first.clone()),
                    Some(first) => {
                        FooterLine::error(format!("{first} (and {} more)", errors.len() - 1))
                    }
                });
                Ok(After::Stay)
            }
            Some(commands::Command::Settings) => {
                create_settings_file(&self.settings_path)?;
                launch::open(&self.settings_path.display().to_string()).map(|()| After::Hide)
            }
            Some(commands::Command::Log) => {
                launch::open(&self.log.path().display().to_string()).map(|()| After::Hide)
            }
            Some(commands::Command::Quit) => Ok(After::Exit),
            None => Err(format!("Flint has no command `{action_id}`")),
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
                self.shown_now(ui);
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
                if visible {
                    self.shown_now(ui);
                }
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

/// `settings.toml` doesn't have to exist, but "Open settings" needs a
/// file to open: an empty one pointing at the docs. Never overwrites.
fn create_settings_file(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let header = "# Flint's settings. Every setting, its default and what it does:\n\
                  # https://github.com/tpedneault/flint/blob/master/docs/CONFIGURATION.md\n";
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, header.as_bytes()))
        .map_err(|e| format!("couldn't create {}: {e}", path.display()))
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
        self.link_problems = self
            .catalog
            .set_quicklinks(&core.loaded.settings.quicklinks);
        for problem in &self.link_problems {
            self.log.line(&format!("settings: {problem}"));
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
        self.scan_apps();
        let cold = self.process_start.elapsed();
        self.cold_start = Some(cold);
        self.log.line(&format!("ready in {} ms", cold.as_millis()));
        self.refresh(ui);
        if let Some(params) = self.start.show.take() {
            ui.show(Self::show_request(params, "start", Instant::now()));
            self.search(ui);
        }
    }

    fn event(&mut self, event: Event, ui: &mut Ui<'_>) {
        match event {
            Event::Hotkey(at) => {
                if !ui.visible() {
                    self.reload_if_changed(ui);
                }
                if ui.toggle(Self::show_request(ShowParams::default(), "hotkey", at)) {
                    self.shown_now(ui);
                }
            }
            Event::Request { request, at, reply } => {
                let answer = self.answer(request, at, ui);
                // The client may have given up; nothing to do then.
                let _ = reply.send(answer);
            }
            Event::Apps(items) => self.apps_arrived(items, ui),
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

    fn query_changed(&mut self, ui: &mut Ui<'_>) {
        self.search(ui);
    }

    fn activate(&mut self, row: usize, ui: &mut Ui<'_>) {
        let Some(action) = self
            .catalog
            .shown(row)
            .and_then(|s| s.item.primary().cloned())
        else {
            return;
        };
        self.run_action(row, &action, ui);
    }

    fn copy(&mut self, row: usize, ui: &mut Ui<'_>) {
        let Some(text) = self
            .catalog
            .shown(row)
            .map(|s| s.item.primary_text().to_owned())
        else {
            return;
        };
        match self.clipboard.set(&text) {
            Ok(()) => ui.set_message(FooterLine::info(format!("Copied {text}"))),
            Err(why) => ui.set_message(FooterLine::error(why)),
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
