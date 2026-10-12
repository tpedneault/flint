//! What the daemon says about itself: `flint status` as text, and the
//! footer's standing notice. Pure functions of a [`Status`], so the
//! wording is tested without a daemon.

use flint_gui::FooterLine;
use flint_protocol::client::{HotkeyState, Status};

/// `flint status`, as printed.
pub fn status_text(status: &Status) -> String {
    let mut lines = vec![format!(
        "Flint {} (pid {}), up {}, {}",
        status.version,
        status.pid,
        duration(status.uptime_ms),
        if status.visible { "shown" } else { "hidden" }
    )];
    let mut field = |name: &str, value: String| lines.push(format!("{name:<11}{value}"));
    field("GPU", gpu(status));
    field("Hotkey", hotkey(status));
    if let Some(bytes) = status.memory_bytes {
        field("Memory", format!("{:.1} MB", bytes as f64 / 1_000_000.0));
    }
    if let Some(ms) = status.cold_start_ms {
        field("Cold start", format!("{ms} ms"));
    }
    if let Some(theme) = &status.theme {
        let font = status
            .font
            .as_deref()
            .map(|f| format!(", font {f}"))
            .unwrap_or_default();
        field("Theme", format!("{theme}{font}"));
    }
    if let Some(path) = &status.settings_file {
        field("Settings", path.clone());
    }
    if let Some(path) = &status.log_file {
        field("Log", path.clone());
    }
    field(
        "Providers",
        if status.providers.is_empty() {
            "none".into()
        } else {
            status
                .providers
                .iter()
                .map(|p| format!("{} ({})", p.name, p.message.as_deref().unwrap_or(&p.state)))
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    for error in &status.settings_errors {
        lines.push(format!("Problem    {error}"));
    }
    if let Some(timings) = &status.timings {
        if timings.samples.is_empty() {
            lines.push("Timings    no shows yet".into());
        } else {
            let ms = |v: Option<f64>| v.map_or("-".into(), |v| format!("{v:.1} ms"));
            lines.push(format!(
                "Timings    {} shows, p50 {}, p95 {} (budget 50 ms)",
                timings.samples.len(),
                ms(timings.p50_ms),
                ms(timings.p95_ms)
            ));
            for s in &timings.samples {
                lines.push(format!("           {:>7.1} ms  {}", s.ms, s.source));
            }
        }
    }
    lines.join("\n")
}

fn gpu(status: &Status) -> String {
    match &status.adapter {
        Some(a) if a.fallback => format!("{} ({}, software fallback)", a.name, a.backend),
        Some(a) => format!("{} ({})", a.name, a.backend),
        None => "not ready".into(),
    }
}

fn hotkey(status: &Status) -> String {
    let h = &status.hotkey;
    match h.state {
        HotkeyState::Registered => format!("{}, registered", h.chord),
        HotkeyState::Failed => format!(
            "{}, not registered: {}",
            h.chord,
            h.message.as_deref().unwrap_or("refused")
        ),
        HotkeyState::Unsupported => format!(
            "none here: {}",
            h.message
                .as_deref()
                .unwrap_or("bind `flint toggle` to a shortcut")
        ),
    }
}

fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} min", secs / 60),
        _ => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
    }
}

/// The footer's standing notice: what is wrong, if anything, in one line.
/// A hotkey failure comes first because it changes how Flint is reached;
/// then settings problems, which are often why a provider is disabled;
/// then a disabled provider, which otherwise would just be missing.
pub fn notice(status: &Status) -> Option<FooterLine> {
    if status.hotkey.state == HotkeyState::Failed {
        return Some(FooterLine {
            text: format!(
                "Hotkey {}: {}; `flint toggle` still works",
                status.hotkey.chord,
                status.hotkey.message.as_deref().unwrap_or("refused")
            ),
            error: true,
        });
    }
    if let Some(first) = status.settings_errors.first() {
        let more = match status.settings_errors.len() {
            1 => String::new(),
            n => format!(" (and {} more; `flint reload` lists them)", n - 1),
        };
        return Some(FooterLine {
            text: format!("{first}{more}"),
            error: true,
        });
    }
    let disabled = status.providers.iter().find(|p| p.state == "disabled")?;
    Some(FooterLine {
        text: format!(
            "{} is disabled: {}",
            disabled.name,
            disabled.message.as_deref().unwrap_or("it kept stopping")
        ),
        error: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_protocol::client::{Adapter, HotkeyStatus, TimingSample, Timings};

    fn status() -> Status {
        Status {
            version: "0.1.0".into(),
            pid: 42,
            uptime_ms: 125_000,
            cold_start_ms: Some(812),
            visible: false,
            adapter: Some(Adapter {
                name: "Microsoft Basic Render Driver".into(),
                backend: "Dx12".into(),
                fallback: true,
            }),
            hotkey: HotkeyStatus {
                chord: "Alt+Space".into(),
                state: HotkeyState::Registered,
                message: None,
            },
            providers: vec![],
            memory_bytes: Some(45_200_000),
            settings_errors: vec![],
            settings_file: Some("C:\\flint\\settings.toml".into()),
            log_file: Some("C:\\flint\\logs\\flint.log".into()),
            theme: Some("Nord (following Fenix)".into()),
            font: Some("Consolas".into()),
            timings: None,
        }
    }

    #[test]
    fn status_reads_as_a_short_table() {
        let text = status_text(&status());
        assert!(
            text.starts_with("Flint 0.1.0 (pid 42), up 2 min, hidden"),
            "{text}"
        );
        assert!(text.contains("GPU        Microsoft Basic Render Driver (Dx12, software fallback)"));
        assert!(text.contains("Hotkey     Alt+Space, registered"));
        assert!(text.contains("Memory     45.2 MB"));
        assert!(text.contains("Providers  none"));
        assert!(text.contains("Theme      Nord (following Fenix), font Consolas"));
        assert!(!text.contains("Timings"));
    }

    #[test]
    fn timings_list_each_show_with_percentiles() {
        let mut s = status();
        s.timings = Some(Timings {
            samples: vec![TimingSample {
                source: "hotkey".into(),
                ms: 21.5,
                at_unix_ms: 0,
            }],
            p50_ms: Some(21.5),
            p95_ms: Some(21.5),
        });
        let text = status_text(&s);
        assert!(
            text.contains("Timings    1 shows, p50 21.5 ms, p95 21.5 ms"),
            "{text}"
        );
        assert!(text.contains("21.5 ms  hotkey"));
    }

    #[test]
    fn no_notice_when_all_is_well() {
        assert_eq!(notice(&status()), None);
    }

    #[test]
    fn a_failed_hotkey_is_the_notice_and_says_toggle_still_works() {
        let mut s = status();
        s.hotkey.state = HotkeyState::Failed;
        s.hotkey.message = Some("Alt+Space is already taken by another application".into());
        s.settings_errors = vec!["`appearance.width`: bad".into()];
        let n = notice(&s).unwrap();
        assert!(n.error);
        assert!(n.text.contains("already taken"));
        assert!(n.text.ends_with("`flint toggle` still works"));
    }

    #[test]
    fn settings_problems_show_the_first_and_count_the_rest() {
        let mut s = status();
        s.settings_errors = vec!["one".into(), "two".into(), "three".into()];
        assert_eq!(
            notice(&s).unwrap().text,
            "one (and 2 more; `flint reload` lists them)"
        );
    }

    #[test]
    fn a_disabled_provider_is_the_notice_with_its_reason() {
        let mut s = status();
        s.providers = vec![flint_protocol::client::ProviderStatus {
            name: "numbers".into(),
            state: "disabled".into(),
            message: Some("couldn't start numbers.exe: not found".into()),
        }];
        assert_eq!(
            notice(&s).unwrap().text,
            "numbers is disabled: couldn't start numbers.exe: not found"
        );
        s.providers[0].state = "restarting".into();
        assert_eq!(notice(&s), None, "a restart is not yet a problem");
    }

    #[test]
    fn unsupported_hotkeys_are_explained_not_flagged() {
        let mut s = status();
        s.hotkey.state = HotkeyState::Unsupported;
        s.hotkey.message = Some("Wayland has no global hotkeys".into());
        assert_eq!(notice(&s), None);
        assert!(status_text(&s).contains("Hotkey     none here: Wayland"));
    }
}
