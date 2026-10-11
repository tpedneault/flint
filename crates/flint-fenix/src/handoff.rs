//! Fenix level 0: carrying out `fenix` actions with what Fenix offers
//! today, its single-instance hand-off. `fenix a b` passes the paths to
//! the Fenix already running (which opens them and focuses its window) or
//! starts one.
//!
//! - `fenix/open` → spawn `fenix <paths...>`.
//! - `fenix/openProject` → refused with the reason: `fenix <folder>`
//!   opens the folder as a file, not as a project, today.
//! - anything else → refused: it needs Fenix's control protocol (F1).
//!
//! Focus on Windows: the `fenix` Flint starts exits once it has handed the
//! paths over, and it is the Fenix already running that must come to the
//! front, which Windows only allows a process the foreground grants. Flint
//! is the foreground when `Enter` is pressed but doesn't know that Fenix's
//! pid (F3), so it lifts the foreground lock for everyone until the next
//! input, then starts `fenix`. M7 replaces this module with the control
//! protocol, which reports the pid.

use serde_json::Value;

use crate::locate;

/// Carries out the Fenix action `method` with `params`, starting `fenix`
/// from `executable` (the `fenix.executable` setting, may be empty).
pub fn execute(method: &str, params: &Value, executable: &str) -> Result<(), String> {
    match method {
        "fenix/open" => {
            let paths = paths(params)?;
            let fenix = locate::find(executable)?;
            let mut argv = vec![fenix.display().to_string()];
            argv.extend(paths);
            flint_platform::focus::allow_any_foreground();
            flint_platform::launch::run(&argv, None, false)
        }
        "fenix/openProject" => Err(
            "opening a project in Fenix needs a newer Fenix (one that speaks Flint's control \
             protocol); Open folder works today"
                .into(),
        ),
        other => Err(format!(
            "Fenix can't `{other}` from Flint yet; it needs Fenix's control protocol"
        )),
    }
}

/// `{ "paths": [{ "path": ... }] }` → the paths, refusing anything a
/// command line would read as a flag (Fenix drops arguments starting with
/// `-` except its own).
fn paths(params: &Value) -> Result<Vec<String>, String> {
    let paths: Vec<String> = params
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    if paths.is_empty() {
        return Err("nothing to open".into());
    }
    if let Some(flag) = paths.iter().find(|p| p.starts_with('-')) {
        return Err(format!("Fenix would read {flag} as an option, not a file"));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_needs_paths_that_cant_pass_for_flags() {
        assert_eq!(
            paths(&json!({"paths": [{"path": "a.rs"}, {"path": "b.rs"}]})).unwrap(),
            ["a.rs", "b.rs"]
        );
        assert_eq!(paths(&json!({})).unwrap_err(), "nothing to open");
        assert!(paths(&json!({"paths": [{"path": "--new-window"}]}))
            .unwrap_err()
            .contains("option"));
    }

    #[test]
    fn opening_a_project_says_why_not_yet() {
        let err = execute("fenix/openProject", &json!({"root": "C:\\dev"}), "").unwrap_err();
        assert!(
            err.contains("newer Fenix") && err.contains("Open folder"),
            "{err}"
        );
        let err = execute("fenix/agenda.clockIn", &json!({}), "").unwrap_err();
        assert!(err.contains("control protocol"), "{err}");
    }

    #[test]
    fn a_missing_fenix_is_reported_not_spawned() {
        let err = execute(
            "fenix/open",
            &json!({"paths": [{"path": "a.rs"}]}),
            "/no/such/fenix",
        )
        .unwrap_err();
        assert!(err.contains("doesn't exist"), "{err}");
    }
}
