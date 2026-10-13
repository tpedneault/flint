//! `flint-jira`: started by Flint from a `[[providers]]` entry; see the
//! library's header (`lib.rs`) for what it does and how to declare it.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let started = flint_provider_kit::secret()
        .and_then(|token| flint_jira::Jira::from_args(&args, token))
        .and_then(|mut jira| {
            let name = flint_provider_kit::flag_values(&args, "--name")
                .pop()
                .unwrap_or_else(|| "jira".into());
            flint_provider_kit::run(&name, &mut jira)
        });
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            // Flint keeps stderr in this provider's log, and shows the last
            // line as the reason it stopped.
            eprintln!("flint-jira: {why}");
            ExitCode::FAILURE
        }
    }
}
