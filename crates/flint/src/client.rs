//! The client role: `flint toggle`, `flint status` and the rest, talking
//! to a running daemon over the control endpoint.

use std::io::BufReader;
use std::process::ExitCode;

use flint_platform::{focus, Stream};
use flint_protocol::client::{ReloadResult, Status};
use flint_protocol::{ClientConnection, ClientRequest, Hello};

/// Sends `request` to the daemon on `stream` and prints what the user
/// should see.
pub fn run(stream: Stream, request: ClientRequest) -> ExitCode {
    match call(stream, &request) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("flint: {message}");
            ExitCode::FAILURE
        }
    }
}

fn call(stream: Stream, request: &ClientRequest) -> Result<String, String> {
    let reader = stream
        .try_clone()
        .map_err(|e| format!("couldn't use the connection to Flint: {e}"))?;
    let mut connection = ClientConnection::open(
        BufReader::new(reader),
        stream,
        &Hello::client(env!("CARGO_PKG_VERSION")),
    )
    .map_err(|e| format!("Flint didn't answer: {e}"))?;
    // This process is in the foreground (the user just ran it); let the
    // daemon take the foreground when it shows itself.
    focus::allow_foreground(connection.host().host.pid);
    let result = connection
        .call(request)
        .map_err(|e| format!("`{}` failed: {e}", request.method()))?;
    Ok(match request {
        ClientRequest::Status(_) => {
            let status: Status = serde_json::from_value(result)
                .map_err(|e| format!("unexpected answer to `status`: {e}"))?;
            crate::report::status_text(&status)
        }
        ClientRequest::Reload => {
            let reload: ReloadResult = serde_json::from_value(result)
                .map_err(|e| format!("unexpected answer to `reload`: {e}"))?;
            if reload.errors.is_empty() {
                "Settings reloaded.".into()
            } else {
                let mut lines = vec![format!(
                    "Settings reloaded with {} problem(s):",
                    reload.errors.len()
                )];
                lines.extend(reload.errors.iter().map(|e| format!("  {e}")));
                lines.join("\n")
            }
        }
        _ => String::new(),
    })
}
