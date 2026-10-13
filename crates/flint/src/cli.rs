//! The command line, parsed. A pure function so every form is tested
//! without starting anything.

use flint_protocol::client::{ShowParams, StatusParams, ToggleParams};
use flint_protocol::ClientRequest;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    /// The command.
    pub command: Command,
    /// `--console`: give the daemon a console window to print to.
    pub console: bool,
}

/// The commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `flint --version`.
    Version,
    /// `flint --help`.
    Help,
    /// `flint`: start the daemon hidden, or show the running one.
    Default,
    /// `flint autostart [on|off]`: turn starting at login on or off, or
    /// (with neither) say whether it is on. Done by this process; no
    /// daemon is involved.
    Autostart(Option<bool>),
    /// `flint credential set|delete <name>`, or `flint credential <name>`
    /// to say whether it is stored: secrets in the OS credential store,
    /// for providers' `credential`. Done by this process.
    Credential(Credential),
    /// Anything sent to the daemon.
    Request(ClientRequest),
}

impl Command {
    /// Whether this command starts the daemon when none is running, and
    /// what it shows once started. `flint` alone starts it hidden (that's
    /// how it runs at login); `flint toggle` and `flint show` start it and
    /// show it, so a desktop shortcut bound to `flint toggle` works on the
    /// first press.
    pub fn starts_daemon(&self) -> Option<Start> {
        match self {
            Command::Default => Some(Start { show: None }),
            Command::Request(ClientRequest::Toggle(_)) => Some(Start {
                show: Some(ShowParams::default()),
            }),
            Command::Request(ClientRequest::Show(params)) => Some(Start {
                show: Some(params.clone()),
            }),
            _ => None,
        }
    }

    /// The request a running daemon is sent. `flint` alone asks it to show.
    pub fn request(&self) -> Option<ClientRequest> {
        match self {
            Command::Default => Some(ClientRequest::Show(ShowParams::default())),
            Command::Request(request) => Some(request.clone()),
            Command::Version | Command::Help | Command::Autostart(_) | Command::Credential(_) => {
                None
            }
        }
    }
}

/// What `flint credential` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// Store a secret under this name, asking for it without echo.
    Set(String),
    /// Remove it.
    Delete(String),
    /// Say whether it is stored (never what it is).
    Check(String),
}

/// How a command that starts the daemon wants it to begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// Show the window once ready, with these params.
    pub show: Option<ShowParams>,
}

/// The usage text.
pub const USAGE: &str = "\
Usage: flint [COMMAND] [--console]

With no command, starts Flint in the background if it isn't running, or
shows it if it is.

Commands:
  toggle               Show Flint, or hide it if shown (starts it if needed)
  show [--query TEXT]  Show Flint, with TEXT in the query line (starts it if needed)
  hide                 Hide Flint
  status [--timings]   Report on the running Flint; --timings adds the last
                       hotkey-to-frame times
  reload               Read settings.toml again and report problems
  quit                 Stop Flint
  autostart [on|off]   Start Flint (hidden) when you log in, or stop doing so;
                       alone, say whether it does
  credential set NAME  Store a secret (a provider's token) in the OS credential
                       store as NAME, like jira:https://jira.example.com; it is
                       read without echo, or from a pipe
  credential delete NAME
                       Remove it
  credential NAME      Say whether NAME is stored

Options:
  --console            Windows: show the daemon's log in a console window
  -h, --help           Print this help
  -V, --version        Print the version";

/// Parses the arguments after the program name. Errors are one line,
/// naming the argument; the command line is user input, never a panic.
pub fn parse(args: &[String]) -> Result<Cli, String> {
    let mut console = false;
    let mut words = Vec::new();
    for arg in args {
        if arg == "--console" {
            console = true;
        } else {
            words.push(arg.as_str());
        }
    }
    let command = match words.as_slice() {
        [] => Command::Default,
        ["--version" | "-V"] => Command::Version,
        ["--help" | "-h"] => Command::Help,
        ["toggle"] => Command::Request(ClientRequest::Toggle(ToggleParams::default())),
        ["show"] => Command::Request(ClientRequest::Show(ShowParams::default())),
        ["show", "--query", query] => Command::Request(ClientRequest::Show(ShowParams {
            query: Some((*query).to_owned()),
            activation_token: None,
        })),
        ["show", "--query"] => return Err("`--query` needs the text to show".into()),
        ["hide"] => Command::Request(ClientRequest::Hide),
        ["status"] => Command::Request(ClientRequest::Status(StatusParams { timings: false })),
        ["status", "--timings"] => {
            Command::Request(ClientRequest::Status(StatusParams { timings: true }))
        }
        ["reload"] => Command::Request(ClientRequest::Reload),
        ["quit"] => Command::Request(ClientRequest::Quit),
        ["autostart"] => Command::Autostart(None),
        ["autostart", "on"] => Command::Autostart(Some(true)),
        ["autostart", "off"] => Command::Autostart(Some(false)),
        ["credential", "set", name] => Command::Credential(Credential::Set((*name).to_owned())),
        ["credential", "delete", name] => {
            Command::Credential(Credential::Delete((*name).to_owned()))
        }
        ["credential", name] if !matches!(*name, "set" | "delete") => {
            Command::Credential(Credential::Check((*name).to_owned()))
        }
        ["credential", ..] => {
            return Err("`credential` needs a name: `flint credential set jira:https://…`".into())
        }
        [first, rest @ ..] => {
            let known = [
                "--version",
                "-V",
                "--help",
                "-h",
                "toggle",
                "show",
                "hide",
                "status",
                "reload",
                "quit",
                "autostart",
                "credential",
            ];
            let wrong = if known.contains(first) {
                rest.first().copied().unwrap_or(first)
            } else {
                first
            };
            return Err(format!("unexpected argument `{wrong}`; try `flint --help`"));
        }
    };
    Ok(Cli { command, console })
}

/// Adds Wayland's activation token from the environment to the requests
/// that show the window, so the daemon may take focus (DESIGN.md, KDE
/// notes). Elsewhere the variable is unset and this changes nothing.
pub fn with_activation_token(request: ClientRequest, token: Option<String>) -> ClientRequest {
    let token = token.filter(|t| !t.is_empty());
    match request {
        ClientRequest::Show(mut p) => {
            p.activation_token = p.activation_token.or(token);
            ClientRequest::Show(p)
        }
        ClientRequest::Toggle(mut p) => {
            p.activation_token = p.activation_token.or(token);
            ClientRequest::Toggle(p)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(list: &[&str]) -> Result<Cli, String> {
        parse(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    fn command(list: &[&str]) -> Command {
        cli(list).unwrap().command
    }

    #[test]
    fn every_command_parses() {
        assert_eq!(command(&[]), Command::Default);
        assert_eq!(command(&["-V"]), Command::Version);
        assert_eq!(command(&["--help"]), Command::Help);
        assert_eq!(command(&["hide"]), Command::Request(ClientRequest::Hide));
        assert_eq!(command(&["quit"]), Command::Request(ClientRequest::Quit));
        assert_eq!(command(&["autostart"]), Command::Autostart(None));
        assert_eq!(
            command(&["autostart", "on"]),
            Command::Autostart(Some(true))
        );
        assert_eq!(
            command(&["autostart", "off"]),
            Command::Autostart(Some(false))
        );
        assert_eq!(
            command(&["reload"]),
            Command::Request(ClientRequest::Reload)
        );
        assert_eq!(
            command(&["status", "--timings"]),
            Command::Request(ClientRequest::Status(StatusParams { timings: true }))
        );
        assert_eq!(
            command(&["show", "--query", "j "]),
            Command::Request(ClientRequest::Show(ShowParams {
                query: Some("j ".into()),
                activation_token: None
            }))
        );
    }

    #[test]
    fn credential_commands_parse() {
        assert_eq!(
            command(&["credential", "set", "jira:https://j"]),
            Command::Credential(Credential::Set("jira:https://j".into()))
        );
        assert_eq!(
            command(&["credential", "delete", "jira:https://j"]),
            Command::Credential(Credential::Delete("jira:https://j".into()))
        );
        assert_eq!(
            command(&["credential", "jira:https://j"]),
            Command::Credential(Credential::Check("jira:https://j".into()))
        );
        assert!(cli(&["credential", "set"])
            .unwrap_err()
            .contains("needs a name"));
        assert!(command(&["credential", "set", "x:y"])
            .starts_daemon()
            .is_none());
    }

    #[test]
    fn console_may_go_anywhere() {
        let c = cli(&["--console", "toggle"]).unwrap();
        assert!(c.console);
        assert!(matches!(
            c.command,
            Command::Request(ClientRequest::Toggle(_))
        ));
        assert!(cli(&["--console"]).unwrap().console);
    }

    #[test]
    fn mistakes_name_the_argument() {
        for (args, named) in [
            (&["frobnicate"][..], "`frobnicate`"),
            (&["status", "--verbose"][..], "`--verbose`"),
            (&["--version", "extra"][..], "`extra`"),
            (&["show", "--query"][..], "`--query`"),
            (&["autostart", "maybe"][..], "`maybe`"),
        ] {
            let err = cli(args).unwrap_err();
            assert!(err.contains(named), "{args:?}: {err}");
        }
    }

    #[test]
    fn only_flint_toggle_and_show_start_the_daemon() {
        assert_eq!(command(&[]).starts_daemon(), Some(Start { show: None }));
        assert!(command(&["toggle"]).starts_daemon().unwrap().show.is_some());
        let shown = command(&["show", "--query", "p "])
            .starts_daemon()
            .unwrap()
            .show;
        assert_eq!(shown.unwrap().query.as_deref(), Some("p "));
        for args in [
            &["hide"][..],
            &["status"],
            &["reload"],
            &["quit"],
            &["-V"],
            &["autostart", "on"],
        ] {
            assert!(command(args).starts_daemon().is_none(), "{args:?}");
        }
    }

    #[test]
    fn the_activation_token_rides_on_show_and_toggle_only() {
        let token = Some("abc".to_string());
        let toggled = with_activation_token(command(&["toggle"]).request().unwrap(), token.clone());
        assert_eq!(
            toggled,
            ClientRequest::Toggle(ToggleParams {
                activation_token: Some("abc".into())
            })
        );
        let hidden = with_activation_token(ClientRequest::Hide, token);
        assert_eq!(hidden, ClientRequest::Hide);
        let shown =
            with_activation_token(command(&["show"]).request().unwrap(), Some(String::new()));
        assert_eq!(shown, ClientRequest::Show(ShowParams::default()));
    }
}
