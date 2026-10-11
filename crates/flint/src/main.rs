//! The `flint` executable: parses the command line and decides whether
//! this process is the daemon (no daemon is running for this user) or a
//! client that asks the running daemon to `toggle`, `show`, `hide`,
//! `status`, `reload` or `quit`, then exits.
//!
//! It holds no logic of its own beyond that choice; everything it does is
//! wiring the other crates together. Argument parsing is a pure function
//! so it is tested without spawning anything.
//!
//! In M0 there is no daemon yet, so only `--version` and `--help` do
//! anything. The Windows GUI subsystem (no console window) and `--console`
//! arrive with the daemon in M1: until then the binary must stay a console
//! program, or what it prints would go nowhere.

use std::process::ExitCode;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    Help,
    /// No arguments: start the daemon or show the running one, from M1.
    Default,
}

/// Parses the arguments after the program name. An unknown argument is an
/// error naming it, never a panic: the command line is user input.
fn parse(args: &[String]) -> Result<Command, String> {
    let Some(first) = args.first() else {
        return Ok(Command::Default);
    };
    let command = match first.as_str() {
        "--version" | "-V" => Command::Version,
        "--help" | "-h" => Command::Help,
        other => return Err(format!("unknown argument `{other}`; try `flint --help`")),
    };
    match args.get(1) {
        Some(extra) => Err(format!("unexpected argument `{extra}`; try `flint --help`")),
        None => Ok(command),
    }
}

fn version_line() -> String {
    format!("flint {}", env!("CARGO_PKG_VERSION"))
}

const HELP: &str = "\
A keyboard-first launcher for Windows and Linux.

Usage: flint [--version | --help]

Nothing else is implemented yet: the daemon, the hotkey and the window
arrive in milestone M1 (docs/DESIGN.md, \"Milestones\").";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Command::Version) => {
            println!("{}", version_line());
            ExitCode::SUCCESS
        }
        Ok(Command::Help) => {
            println!("{}\n\n{HELP}", version_line());
            ExitCode::SUCCESS
        }
        Ok(Command::Default) => {
            eprintln!(
                "{}: nothing to run yet; the daemon arrives in M1",
                version_line()
            );
            ExitCode::FAILURE
        }
        Err(reason) => {
            eprintln!("flint: {reason}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_is_the_default_role() {
        assert_eq!(parse(&args(&[])), Ok(Command::Default));
    }

    #[test]
    fn version_and_help_have_short_and_long_forms() {
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(parse(&args(&["-V"])), Ok(Command::Version));
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["-h"])), Ok(Command::Help));
    }

    #[test]
    fn unknown_arguments_are_named_in_the_error() {
        let err = parse(&args(&["toggle"])).unwrap_err();
        assert!(err.contains("`toggle`"), "{err}");
        let err = parse(&args(&["--version", "extra"])).unwrap_err();
        assert!(err.contains("`extra`"), "{err}");
    }
}
