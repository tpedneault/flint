//! The built `flint` executable, run as a user would run it: what it prints
//! and the exit code it returns. Argument parsing itself is unit-tested in
//! `cli.rs`; this checks the wiring to stdout, stderr and the exit code,
//! and the paths that need no display.

use std::process::Command;

fn flint(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_flint"))
        .args(args)
        .output()
        .expect("the flint binary runs")
}

#[test]
fn version_prints_the_package_version() {
    let out = flint(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("flint {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn an_unknown_argument_fails_with_one_line_naming_it() {
    let out = flint(&["--frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.trim().lines().count(), 1, "{stderr}");
    assert!(stderr.contains("`--frobnicate`"), "{stderr}");
}

#[test]
fn commands_that_need_a_daemon_say_none_is_running() {
    // A FLINT_HOME of its own gets an endpoint of its own, so this never
    // reaches a Flint running on the machine.
    let home = std::env::temp_dir().join(format!("flint-cli-test-{}", std::process::id()));
    for command in ["status", "hide", "reload", "quit"] {
        let out = Command::new(env!("CARGO_BIN_EXE_flint"))
            .arg(command)
            .env("FLINT_HOME", &home)
            .output()
            .expect("the flint binary runs");
        assert_eq!(out.status.code(), Some(1), "{command}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            stderr.trim(),
            "flint: Flint isn't running; start it with `flint`",
            "{command}"
        );
    }
}

#[test]
fn help_lists_every_command() {
    let out = flint(&["--help"]);
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for command in [
        "toggle",
        "show",
        "hide",
        "status",
        "reload",
        "quit",
        "--console",
    ] {
        assert!(help.contains(command), "{command} missing from --help");
    }
}
