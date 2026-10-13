//! `flint-gitlab`: started by Flint from a `[[providers]]` entry; see the
//! library's header (`lib.rs`) for what it does and how to declare it.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let started = flint_provider_kit::secret()
        .and_then(|token| flint_gitlab::GitLab::from_args(&args, token))
        .and_then(|mut gitlab| {
            let name = flint_provider_kit::flag_values(&args, "--name")
                .pop()
                .unwrap_or_else(|| "gitlab".into());
            flint_provider_kit::run(&name, &mut gitlab)
        });
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            // Flint keeps stderr in this provider's log, and shows the last
            // line as the reason it stopped.
            eprintln!("flint-gitlab: {why}");
            ExitCode::FAILURE
        }
    }
}
