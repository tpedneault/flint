//! The `hello` exchange that opens every channel (PROTOCOL.md, "Roles and
//! handshake"). The non-host side speaks first; nothing else may be sent
//! before the host answers.

use serde::{Deserialize, Serialize};

/// The protocol version this build speaks. It changes only for
/// incompatible changes (PROTOCOL.md, "Versioning").
pub const PROTOCOL_VERSION: u32 = 1;

/// Who is on the other side of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Gives Flint items and carries out actions (from M5).
    Provider,
    /// Asks the daemon to show, hide, report status... (`flint toggle`).
    Client,
}

/// `hello`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The protocol version the peer speaks.
    pub protocol: u32,
    /// What the peer is.
    pub role: Role,
    /// A provider's id, or a client's program name.
    pub name: String,
    /// The peer's own version, for `flint status` and logs.
    pub version: String,
    /// Lets the host grant the peer focus on Windows.
    pub pid: u32,
}

impl Hello {
    /// The `hello` this build of the `flint` CLI sends.
    pub fn client(version: &str) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            role: Role::Client,
            name: "flint".to_owned(),
            version: version.to_owned(),
            pid: std::process::id(),
        }
    }
}

/// Who the host is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    /// Always `flint`.
    pub name: String,
    /// The daemon's version.
    pub version: String,
    /// The daemon's process id. A client on Windows passes it to
    /// `AllowSetForegroundWindow` so the daemon may come to the front.
    pub pid: u32,
}

/// `hello`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResult {
    /// The protocol version the host speaks.
    pub protocol: u32,
    /// Who answered.
    pub host: HostInfo,
}
