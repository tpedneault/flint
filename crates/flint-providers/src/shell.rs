//! The shell prefix (`>`): the query as a command for the user's shell,
//! run in a terminal or hidden with its exit status in the footer.
//!
//! This is the one place Flint hands typed text to a shell, because that
//! is what the prefix asks for: the command is passed to the shell as a
//! single argument (`pwsh -Command <text>`, `sh -c <text>`), never pasted
//! into a command line of Flint's own, so nothing else is interpreted.
//! Only `> ` does it; an unscoped query is never run.
//!
//! Which shell: `shell.program` in `settings.toml`, else on Windows `pwsh`
//! when it is on the `PATH` and Windows PowerShell otherwise, and on Linux
//! `$SHELL`, else `/bin/sh`. The arguments that run one command (and, in
//! a terminal, keep the window open afterwards) depend on the shell, so
//! they are chosen from its name: PowerShell's `-Command`/`-NoExit`,
//! `cmd`'s `/c`/`/k`, and `-c` for the POSIX family, where the terminal
//! keeps an interactive shell by `exec`ing it after the command.
//!
//! On Windows the argument reaches the shell through the usual
//! command-line quoting, which PowerShell follows and `cmd` doesn't: with
//! `cmd`, a command containing double quotes may split differently than
//! at its prompt. PowerShell is the default; CONFIGURATION.md says so.
//!
//! It doesn't keep a history of commands: frecency learns the commands
//! you run (each is its own item id), which is history enough for now.

use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;

/// The provider's id.
pub const ID: &str = "shell";
/// Its badge and chip.
pub const TITLE: &str = "Shell";
/// Its scope keyword.
pub const PREFIX: &str = ">";
/// The action that runs the command in a terminal (`Enter`).
pub const TERMINAL: &str = "terminal";
/// The action that runs it hidden and reports its exit status.
pub const HIDDEN: &str = "hidden";

/// The command typed after `> ` as an item, or `None` while it is empty.
pub fn item(text: &str, shell: &str) -> Option<Item> {
    let command = text.trim();
    if command.is_empty() {
        return None;
    }
    let action = |id: &str, title: &str, key: &str| Action {
        id: id.into(),
        title: title.into(),
        key: Some(key.into()),
        kind: ActionKind::Provider,
    };
    Some(Item {
        id: format!("{ID}:{command}"),
        title: command.to_owned(),
        subtitle: Some(format!("run with {}", program_name(shell))),
        icon: Some("terminal".into()),
        keywords: Vec::new(),
        actions: vec![
            action(TERMINAL, "Run in terminal", "t"),
            action(HIDDEN, "Run hidden", "h"),
            Action {
                id: "copy".into(),
                title: "Copy command".into(),
                key: Some("c".into()),
                kind: ActionKind::Copy {
                    text: command.to_owned(),
                },
            },
        ],
    })
}

/// The command an item of this provider runs: its id without the prefix.
pub fn command_of(item_id: &str) -> Option<&str> {
    item_id.strip_prefix(ID)?.strip_prefix(':')
}

/// The shell to use: `configured` (`shell.program`) when set, else the
/// platform's default. `on_path` says whether a program can be found, and
/// `env_shell` is `$SHELL`; both are parameters so this is tested on any
/// OS.
pub fn resolve(
    configured: &str,
    on_path: impl Fn(&str) -> bool,
    env_shell: Option<&str>,
) -> String {
    if !configured.trim().is_empty() {
        return configured.trim().to_owned();
    }
    if cfg!(windows) {
        if on_path("pwsh.exe") {
            "pwsh".into()
        } else {
            "powershell".into()
        }
    } else {
        env_shell
            .filter(|s| !s.is_empty())
            .unwrap_or("/bin/sh")
            .to_owned()
    }
}

/// The argument vector that runs `command` with `shell`; in a terminal
/// when `terminal`, keeping it open once the command is done.
pub fn argv(shell: &str, command: &str, terminal: bool) -> Vec<String> {
    let mut argv = vec![shell.to_owned()];
    let args: Vec<String> = match Family::of(shell) {
        Family::PowerShell if terminal => vec![
            "-NoLogo".into(),
            "-NoExit".into(),
            "-Command".into(),
            command.into(),
        ],
        Family::PowerShell => vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            command.into(),
        ],
        Family::Cmd if terminal => vec!["/k".into(), command.into()],
        Family::Cmd => vec!["/d".into(), "/c".into(), command.into()],
        Family::Posix if terminal => vec!["-c".into(), format!("{command}; exec {shell}")],
        Family::Posix => vec!["-c".into(), command.into()],
    };
    argv.extend(args);
    argv
}

/// A shell's name as the subtitle shows it: `pwsh`, `bash`.
fn program_name(shell: &str) -> &str {
    let name = shell.rsplit(['/', '\\']).next().unwrap_or(shell);
    name.strip_suffix(".exe").unwrap_or(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    PowerShell,
    Cmd,
    Posix,
}

impl Family {
    fn of(shell: &str) -> Family {
        match program_name(shell).to_ascii_lowercase().as_str() {
            "pwsh" | "powershell" => Family::PowerShell,
            "cmd" => Family::Cmd,
            _ => Family::Posix,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_one_argument_to_the_shell() {
        assert_eq!(
            argv("pwsh", "git status; ls", false),
            [
                "pwsh",
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "git status; ls"
            ]
        );
        assert_eq!(
            argv(
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                "dir",
                true
            ),
            [
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                "-NoLogo",
                "-NoExit",
                "-Command",
                "dir"
            ]
        );
        assert_eq!(
            argv("cmd.exe", "dir & echo hi", false),
            ["cmd.exe", "/d", "/c", "dir & echo hi"]
        );
        assert_eq!(argv("CMD", "dir", true), ["CMD", "/k", "dir"]);
        assert_eq!(
            argv("/bin/bash", "make -j8", false),
            ["/bin/bash", "-c", "make -j8"]
        );
        assert_eq!(
            argv("/usr/bin/fish", "htop", true),
            ["/usr/bin/fish", "-c", "htop; exec /usr/bin/fish"]
        );
    }

    #[test]
    fn the_shell_is_the_setting_else_the_platforms() {
        assert_eq!(resolve(" nu ", |_| false, Some("/bin/zsh")), "nu");
        let default = resolve("", |p| p == "pwsh.exe", Some("/bin/zsh"));
        if cfg!(windows) {
            assert_eq!(default, "pwsh");
            assert_eq!(resolve("", |_| false, None), "powershell");
        } else {
            assert_eq!(default, "/bin/zsh");
            assert_eq!(resolve("", |_| false, None), "/bin/sh");
            assert_eq!(resolve("", |_| false, Some("")), "/bin/sh");
        }
    }

    #[test]
    fn a_typed_command_is_an_item_that_names_its_shell() {
        assert!(item("   ", "pwsh").is_none());
        let item = item(" cargo test ", "/bin/bash").unwrap();
        assert!(item.check().is_ok());
        assert_eq!(item.id, "shell:cargo test");
        assert_eq!(item.title, "cargo test");
        assert_eq!(item.subtitle.as_deref(), Some("run with bash"));
        assert_eq!(item.actions[0].id, TERMINAL, "Enter runs it in a terminal");
        assert_eq!(item.actions[1].id, HIDDEN);
        assert_eq!(command_of(&item.id), Some("cargo test"));
        assert_eq!(command_of("apps:x"), None);
    }
}
