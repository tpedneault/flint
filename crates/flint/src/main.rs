//! The `flint` executable: parses the command line and decides whether
//! this process is the daemon (no daemon is running for this user) or a
//! client that asks the running daemon to `toggle`, `show`, `hide`,
//! `status`, `reload` or `quit`, then exits. `flint autostart` needs no
//! daemon: it changes (or reports) this user's login entry itself.
//!
//! Deciding is "connect; if nobody answers, bind; if binding finds the
//! endpoint taken, another daemon is starting this instant, so connect
//! again". `flint` alone and `flint toggle`/`show` may become the daemon;
//! the other commands only ever talk to one.
//!
//! On Windows this is a GUI-subsystem program, as Fenix is, so starting
//! the daemon (at login, from a shortcut) opens no console window. Client
//! commands attach to the terminal they were typed in to print there, and
//! `--console` gives the daemon a console for its log
//! (`flint_platform::console` explains the tradeoff).

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod catalog;
mod cli;
mod client;
mod daemon;
mod fenix_link;
mod leader;
mod log;
mod report;
mod store;
mod timings;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use flint_platform::autostart::{self, State};
use flint_platform::credentials;
use flint_platform::{console, BindError, Endpoint};

use crate::cli::Command;

/// `flint credential ...`. After a change, a running Flint is asked to
/// reload, so the provider that wanted the secret starts (or restarts)
/// with it.
fn credential(what: &cli::Credential) -> ExitCode {
    let (cli::Credential::Set(name) | cli::Credential::Delete(name) | cli::Credential::Check(name)) =
        what;
    if let Err(why) = flint_config::check_credential_name(name) {
        eprintln!("flint: {why}");
        return ExitCode::from(2);
    }
    let done = match what {
        cli::Credential::Check(_) => {
            return match credentials::get(name) {
                Ok(Some(_)) => {
                    println!("{name} is stored");
                    ExitCode::SUCCESS
                }
                Ok(None) => {
                    println!("{name} isn't stored");
                    ExitCode::FAILURE
                }
                Err(why) => {
                    eprintln!("flint: {why}");
                    ExitCode::FAILURE
                }
            };
        }
        cli::Credential::Set(_) => console::read_secret(&format!("Secret for {name}: "))
            .map_err(|e| format!("couldn't read the secret: {e}"))
            .and_then(|secret| match secret.trim() {
                "" => Err("nothing was entered; nothing stored".into()),
                secret => credentials::set(name, secret),
            })
            .map(|()| format!("{name} stored")),
        cli::Credential::Delete(_) => credentials::delete(name).map(|()| format!("{name} removed")),
    };
    match done {
        Ok(said) => {
            let endpoint = Endpoint::for_user(std::env::var_os("FLINT_HOME").as_deref());
            let reloaded = endpoint
                .connect()
                .ok()
                .flatten()
                .is_some_and(client::reload_quietly);
            if reloaded {
                println!("{said}; Flint has reloaded its providers");
            } else {
                println!("{said}");
            }
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("flint: {why}");
            ExitCode::FAILURE
        }
    }
}

/// `flint autostart [on|off]`.
fn autostart(turn: Option<bool>) -> ExitCode {
    let done = match turn {
        Some(true) => std::env::current_exe()
            .map_err(|e| format!("couldn't tell where this flint is: {e}"))
            .and_then(|exe| autostart::enable(&exe)),
        Some(false) => autostart::disable(),
        None => Ok(()),
    };
    match done.and_then(|()| autostart::state()) {
        Ok(State::On(command)) => {
            println!("Autostart is on: {command} runs when you log in");
            ExitCode::SUCCESS
        }
        Ok(State::Off) => {
            println!("Autostart is off");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("flint: {why}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let process_start = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match cli::parse(&args) {
        Ok(cli) => cli,
        Err(reason) => {
            console::attach_parent();
            eprintln!("flint: {reason}");
            return ExitCode::from(2);
        }
    };
    let request = match &cli.command {
        Command::Version => {
            console::attach_parent();
            println!("flint {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Command::Help => {
            console::attach_parent();
            println!("flint {}\n\n{}", env!("CARGO_PKG_VERSION"), cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Command::Autostart(turn) => {
            console::attach_parent();
            return autostart(*turn);
        }
        Command::Credential(what) => {
            console::attach_parent();
            return credential(what);
        }
        command => command.request(),
    };
    let token = std::env::var("XDG_ACTIVATION_TOKEN").ok();
    let request = request.map(|r| cli::with_activation_token(r, token.clone()));
    let endpoint = Endpoint::for_user(std::env::var_os("FLINT_HOME").as_deref());

    // A daemon starting at this moment holds the endpoint before it
    // listens; a short wait covers that.
    for _ in 0..40 {
        match endpoint.connect() {
            Ok(Some(stream)) => {
                console::attach_parent();
                return match request {
                    Some(request) => client::run(stream, request),
                    None => ExitCode::SUCCESS,
                };
            }
            Ok(None) => {}
            Err(e) => {
                console::attach_parent();
                eprintln!("flint: couldn't reach Flint's control endpoint: {e}");
                return ExitCode::FAILURE;
            }
        }
        let Some(mut start) = cli.command.starts_daemon() else {
            console::attach_parent();
            eprintln!("flint: Flint isn't running; start it with `flint`");
            return ExitCode::FAILURE;
        };
        match endpoint.bind() {
            Ok(listener) => {
                if let Some(show) = &mut start.show {
                    show.activation_token = show.activation_token.take().or(token);
                }
                return daemon::run(listener, start, process_start, cli.console);
            }
            Err(BindError::InUse) => std::thread::sleep(Duration::from_millis(50)),
            Err(BindError::Io(e)) => {
                console::attach_parent();
                eprintln!("flint: couldn't open Flint's control endpoint: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    console::attach_parent();
    eprintln!("flint: another Flint is starting but didn't answer within 2 s");
    ExitCode::FAILURE
}
