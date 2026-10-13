//! Starting Fenix with a request, for when none is running to ask over
//! the control channel ([`crate::control`]).
//!
//! Fenix's command line says what its protocol says (Fenix's
//! `docs/CONTROL.md`, "From the command line"):
//!
//! - `fenix/open` → `fenix [--new-window] [--goto path:line[:col]]... [-- paths...]`:
//!   a path with a position goes through `--goto`, which always reads one
//!   (a file named `x.rs:12` exists nowhere in practice, but `--goto`
//!   makes it certain); the others come after `--`, so a file whose name
//!   starts with `-` is still a file.
//! - `fenix/openProject` → `fenix --project <root>`.
//! - anything else needs Fenix running: there is no flag for it.
//!
//! If a Fenix starts between Flint's look and this launch, the `fenix`
//! started here hands the request to it over the channel, granting it
//! the foreground itself (F3), and exits.

use serde_json::Value;

use crate::locate;

/// The arguments after the program that ask a new Fenix for `method`
/// with `params`.
pub fn arguments(method: &str, params: &Value) -> Result<Vec<String>, String> {
    match method {
        "fenix/open" => {
            let entries = params
                .get("paths")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut args = Vec::new();
            if params.get("newWindow").and_then(Value::as_bool) == Some(true) {
                args.push("--new-window".to_owned());
            }
            let mut plain = Vec::new();
            for entry in entries {
                let Some(path) = entry.get("path").and_then(Value::as_str) else {
                    continue;
                };
                let line = entry.get("line").and_then(Value::as_u64);
                let column = entry.get("column").and_then(Value::as_u64);
                match (line, column) {
                    (Some(l), Some(c)) => args.extend(["--goto".into(), format!("{path}:{l}:{c}")]),
                    (Some(l), None) => args.extend(["--goto".into(), format!("{path}:{l}")]),
                    _ => plain.push(path.to_owned()),
                }
            }
            if args.iter().all(|a| a != "--goto") && plain.is_empty() {
                return Err("nothing to open".into());
            }
            if !plain.is_empty() {
                args.push("--".into());
                args.extend(plain);
            }
            Ok(args)
        }
        "fenix/openProject" => match params.get("root").and_then(Value::as_str) {
            Some(root) if !root.is_empty() => Ok(vec!["--project".into(), root.to_owned()]),
            _ => Err("no project folder to open".into()),
        },
        other => Err(format!(
            "`{other}` needs Fenix running; start Fenix and try again"
        )),
    }
}

/// Starts `fenix` (from `executable`, the `fenix.executable` setting, may
/// be empty) asking for `method` with `params`. The new process is
/// granted the foreground as it starts (`flint_platform::launch::run`).
pub fn start(method: &str, params: &Value, executable: &str) -> Result<(), String> {
    let args = arguments(method, params)?;
    let fenix = locate::find(executable)?;
    let mut argv = vec![fenix.display().to_string()];
    argv.extend(args);
    flint_platform::launch::run(&argv, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_becomes_goto_for_positions_and_paths_after_the_separator() {
        let params = json!({"paths": [
            {"path": "C:\\src\\x.rs", "line": 12, "column": 3},
            {"path": "notes.md"},
            {"path": "-odd.txt"},
            {"path": "/a/b.rs", "line": 7}
        ]});
        assert_eq!(
            arguments("fenix/open", &params).unwrap(),
            [
                "--goto",
                "C:\\src\\x.rs:12:3",
                "--goto",
                "/a/b.rs:7",
                "--",
                "notes.md",
                "-odd.txt"
            ]
        );
        assert_eq!(
            arguments(
                "fenix/open",
                &json!({"paths": [{"path": "a"}], "newWindow": true})
            )
            .unwrap(),
            ["--new-window", "--", "a"]
        );
        assert_eq!(
            arguments("fenix/open", &json!({})).unwrap_err(),
            "nothing to open"
        );
    }

    #[test]
    fn a_project_is_a_flag_and_the_rest_needs_fenix_running() {
        assert_eq!(
            arguments("fenix/openProject", &json!({"root": "C:\\dev"})).unwrap(),
            ["--project", "C:\\dev"]
        );
        let err = arguments("fenix/agenda.clockIn", &json!({})).unwrap_err();
        assert!(err.contains("needs Fenix running"), "{err}");
    }

    #[test]
    fn a_missing_fenix_is_reported_not_spawned() {
        let err = start(
            "fenix/open",
            &json!({"paths": [{"path": "a.rs"}]}),
            "/no/such/fenix",
        )
        .unwrap_err();
        assert!(err.contains("doesn't exist"), "{err}");
    }
}
