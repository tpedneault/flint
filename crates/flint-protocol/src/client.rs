//! The client messages: what `flint toggle`, `flint status` and the other
//! subcommands ask the daemon (PROTOCOL.md, "Client messages"), and the
//! shapes of the answers.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::message::{code, params, RpcError};

/// A request from a client, parsed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientRequest {
    /// Show the window, optionally with a query typed in.
    Show(ShowParams),
    /// Hide the window.
    Hide,
    /// Show if hidden, hide if shown.
    Toggle(ToggleParams),
    /// Read `settings.toml` again.
    Reload,
    /// Report on the daemon.
    Status(StatusParams),
    /// Exit the daemon.
    Quit,
}

/// `show`'s params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowParams {
    /// Replaces the query line, e.g. `"j "` to open in Jira's scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// `XDG_ACTIVATION_TOKEN` from the client's environment (Wayland).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_token: Option<String>,
}

/// `toggle`'s params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleParams {
    /// As in [`ShowParams::activation_token`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_token: Option<String>,
}

/// `status`'s params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusParams {
    /// Include the last timings (`flint status --timings`).
    #[serde(default)]
    pub timings: bool,
}

/// `toggle`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToggleResult {
    /// Whether the window is visible after the toggle.
    pub visible: bool,
}

/// `reload`'s result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadResult {
    /// One line per setting that couldn't be used; empty when all is well.
    pub errors: Vec<String>,
}

/// `status`'s result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The daemon's version.
    pub version: String,
    /// The daemon's process id.
    pub pid: u32,
    /// Milliseconds since the daemon started.
    pub uptime_ms: u64,
    /// Milliseconds from process start to ready (window and GPU created,
    /// endpoint listening), the 1.5 s budget.
    #[serde(default)]
    pub cold_start_ms: Option<u64>,
    /// Whether the window is showing.
    pub visible: bool,
    /// The GPU adapter in use; `None` before the window is created.
    #[serde(default)]
    pub adapter: Option<Adapter>,
    /// The global hotkey.
    pub hotkey: HotkeyStatus,
    /// Providers and their state; always empty before M5.
    #[serde(default)]
    pub providers: Vec<ProviderStatus>,
    /// Resident memory in bytes, where the OS reports it (budget 120 MB).
    #[serde(default)]
    pub memory_bytes: Option<u64>,
    /// Problems with `settings.toml`, one line each.
    #[serde(default)]
    pub settings_errors: Vec<String>,
    /// Where `settings.toml` is read from.
    #[serde(default)]
    pub settings_file: Option<String>,
    /// Where the log is written.
    #[serde(default)]
    pub log_file: Option<String>,
    /// Hotkey-to-frame timings, when asked for.
    #[serde(default)]
    pub timings: Option<Timings>,
}

/// The GPU adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adapter {
    /// As the driver names it.
    pub name: String,
    /// `Dx12`, `Vulkan`, `Gl`...
    pub backend: String,
    /// Whether this is the software fallback (WARP, llvmpipe), chosen
    /// because no hardware adapter was usable.
    pub fallback: bool,
}

/// What became of the global hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyStatus {
    /// The chord from settings, as written there.
    pub chord: String,
    /// The outcome.
    pub state: HotkeyState,
    /// Why, when it isn't registered.
    #[serde(default)]
    pub message: Option<String>,
}

/// Whether the hotkey works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HotkeyState {
    /// The OS accepted it; pressing it toggles Flint.
    Registered,
    /// The OS refused it, usually because another app owns the chord.
    Failed,
    /// This platform has no global hotkeys for applications (Wayland);
    /// bind `flint toggle` in the desktop's shortcut settings instead.
    Unsupported,
}

/// A provider as `flint status` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    /// Its id.
    pub name: String,
    /// `running`, `restarting`, `disabled`...
    pub state: String,
    /// Why, when it isn't running.
    #[serde(default)]
    pub message: Option<String>,
}

/// The last show-to-frame timings (DESIGN.md, "Performance budget").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    /// Oldest first, at most 100.
    pub samples: Vec<TimingSample>,
    /// The 50th percentile of `samples`, in milliseconds.
    #[serde(default)]
    pub p50_ms: Option<f64>,
    /// The 95th percentile, the one the 50 ms budget is about.
    #[serde(default)]
    pub p95_ms: Option<f64>,
}

/// One show, from the request reaching the daemon to the first frame
/// presented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingSample {
    /// What asked: `hotkey`, `toggle`, `show`.
    pub source: String,
    /// Milliseconds from the request to `present` returning.
    pub ms: f64,
    /// When, in milliseconds since the Unix epoch.
    pub at_unix_ms: u64,
}

impl ClientRequest {
    /// Parses a request a client sent. The error is the one to send back.
    pub fn parse(method: &str, raw: Value) -> Result<Self, RpcError> {
        Ok(match method {
            "show" => ClientRequest::Show(params(method, raw)?),
            "hide" => ClientRequest::Hide,
            "toggle" => ClientRequest::Toggle(params(method, raw)?),
            "reload" => ClientRequest::Reload,
            "status" => ClientRequest::Status(params(method, raw)?),
            "quit" => ClientRequest::Quit,
            other => {
                return Err(RpcError::new(
                    code::METHOD_NOT_FOUND,
                    format!("unknown method `{other}`"),
                ))
            }
        })
    }

    /// The method name.
    pub fn method(&self) -> &'static str {
        match self {
            ClientRequest::Show(_) => "show",
            ClientRequest::Hide => "hide",
            ClientRequest::Toggle(_) => "toggle",
            ClientRequest::Reload => "reload",
            ClientRequest::Status(_) => "status",
            ClientRequest::Quit => "quit",
        }
    }

    /// The params, as sent.
    pub fn params(&self) -> Value {
        match self {
            ClientRequest::Show(p) => json!(p),
            ClientRequest::Toggle(p) => json!(p),
            ClientRequest::Status(p) => json!(p),
            ClientRequest::Hide | ClientRequest::Reload | ClientRequest::Quit => json!({}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_request_survives_a_round_trip() {
        let requests = [
            ClientRequest::Show(ShowParams {
                query: Some("j ".into()),
                activation_token: Some("tok".into()),
            }),
            ClientRequest::Show(ShowParams::default()),
            ClientRequest::Hide,
            ClientRequest::Toggle(ToggleParams::default()),
            ClientRequest::Reload,
            ClientRequest::Status(StatusParams { timings: true }),
            ClientRequest::Quit,
        ];
        for request in requests {
            let parsed = ClientRequest::parse(request.method(), request.params()).unwrap();
            assert_eq!(parsed, request);
        }
    }

    #[test]
    fn wire_names_are_camel_case() {
        let show = ClientRequest::Show(ShowParams {
            query: None,
            activation_token: Some("t".into()),
        });
        assert_eq!(show.params(), json!({"activationToken": "t"}));
    }

    #[test]
    fn params_may_be_left_out() {
        assert_eq!(
            ClientRequest::parse("status", Value::Null).unwrap(),
            ClientRequest::Status(StatusParams { timings: false })
        );
    }

    #[test]
    fn unknown_methods_and_bad_params_are_named() {
        let err = ClientRequest::parse("frobnicate", Value::Null).unwrap_err();
        assert_eq!(err.code, code::METHOD_NOT_FOUND);
        assert!(err.message.contains("frobnicate"));
        let err = ClientRequest::parse("show", json!({"query": 5})).unwrap_err();
        assert_eq!(err.code, code::INVALID_PARAMS);
    }

    /// PROTOCOL.md's example Status is what this crate reads.
    #[test]
    fn the_documented_status_example_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/PROTOCOL.md");
        let doc = std::fs::read_to_string(path).unwrap().replace('\r', "");
        let from = doc
            .find("A Status:")
            .expect("PROTOCOL.md has a Status example");
        let block = &doc[from..];
        let start = block.find("```json\n").unwrap() + "```json\n".len();
        let end = start + block[start..].find("```").unwrap();
        let status: Status = serde_json::from_str(&block[start..end]).unwrap();
        assert_eq!(status.hotkey.state, HotkeyState::Registered);
        assert_eq!(status.timings.unwrap().samples.len(), 1);
        assert!(!status.adapter.unwrap().fallback);
    }

    #[test]
    fn a_status_from_a_newer_daemon_with_extra_fields_still_reads() {
        let status = json!({
            "version": "0.2.0", "pid": 1, "uptimeMs": 5, "visible": false,
            "hotkey": {"chord": "Alt+Space", "state": "registered"},
            "somethingNew": [1, 2, 3]
        });
        let status: Status = serde_json::from_value(status).unwrap();
        assert_eq!(status.hotkey.state, HotkeyState::Registered);
        assert!(status.timings.is_none());
    }
}
