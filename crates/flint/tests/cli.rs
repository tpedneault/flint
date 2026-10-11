//! The built `flint` executable, run as a user would run it: what it prints
//! and the exit code it returns. Argument parsing itself is unit-tested in
//! `main.rs`; this checks the wiring to stdout, stderr and the exit code.

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
