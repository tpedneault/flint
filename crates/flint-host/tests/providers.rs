//! The provider host against real processes: a well-behaved provider and
//! deliberately misbehaving ones (`src/bin/flint-test-provider.rs`), with
//! the timings shrunk so crash, restart and timeout paths run in
//! milliseconds.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flint_host::{HostEvent, ProviderConfig, ProviderHost, Secret, State, Timing};
use flint_protocol::provider::{Context, Execute};
use flint_protocol::HostInfo;

fn timing() -> Timing {
    Timing {
        backoff: [
            Duration::from_millis(50),
            Duration::from_millis(100),
            Duration::from_millis(200),
        ],
        crash_window: Duration::from_secs(30),
        crash_limit: 3,
        execute_timeout: Duration::from_millis(500),
        result_deadline: Duration::from_secs(5),
        reindex_every: Duration::from_millis(20),
    }
}

fn host() -> (ProviderHost, Receiver<HostEvent>) {
    let (send, receive) = mpsc::channel();
    let send = std::sync::Mutex::new(send);
    let logs = std::env::temp_dir().join(format!("flint-host-tests-{}", std::process::id()));
    let host = ProviderHost::new(
        HostInfo {
            name: "flint".into(),
            version: "0".into(),
            pid: std::process::id(),
        },
        logs,
        std::env::temp_dir(),
        timing(),
        Arc::new(move |event| {
            let _ = send.lock().unwrap().send(event);
        }),
    );
    (host, receive)
}

fn test_provider(mode: &str, name: &str) -> ProviderConfig {
    ProviderConfig {
        name: name.into(),
        command: vec![
            env!("CARGO_BIN_EXE_flint-test-provider").into(),
            mode.into(),
            name.into(),
        ],
        ..ProviderConfig::default()
    }
}

/// The first event matching `want` within a few seconds.
fn wait_for(events: &Receiver<HostEvent>, want: impl Fn(&HostEvent) -> bool) -> HostEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(event) if want(&event) => return event,
            Ok(_) => {}
            Err(_) => panic!("no matching event in time"),
        }
    }
}

fn state(host: &ProviderHost, name: &str) -> State {
    host.statuses()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap()
        .state
}

fn settle(host: &ProviderHost, name: &str, want: impl Fn(&State) -> bool) -> State {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let now = state(host, name);
        if want(&now) {
            return now;
        }
        assert!(Instant::now() < deadline, "{name} stuck in {now:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_good_provider_registers_answers_and_executes() {
    let (host, events) = host();
    host.configure(vec![test_provider("normal", "good")]);
    let registered = wait_for(&events, |e| matches!(e, HostEvent::Registered { .. }));
    assert!(
        matches!(registered, HostEvent::Registered { ref title, dynamic: true, .. } if title == "Test")
    );
    let HostEvent::Items { items, .. } =
        wait_for(&events, |e| matches!(e, HostEvent::Items { .. }))
    else {
        unreachable!()
    };
    assert_eq!(items.len(), 2);
    settle(&host, "good", |s| *s == State::Running);

    let id = host.query("hello", None, &Context::default());
    let HostEvent::Results {
        query_id,
        items,
        done,
        ..
    } = wait_for(&events, |e| matches!(e, HostEvent::Results { .. }))
    else {
        unreachable!()
    };
    assert_eq!((query_id, done), (id, true));
    assert_eq!(items[0].title, "Echo hello");

    let token = host
        .execute(
            "good",
            Execute {
                item_id: "good:one".into(),
                action_id: "act".into(),
                query: "".into(),
            },
        )
        .unwrap();
    let HostEvent::Executed {
        token: t, outcome, ..
    } = wait_for(&events, |e| matches!(e, HostEvent::Executed { .. }))
    else {
        unreachable!()
    };
    assert_eq!(t, token);
    let result = outcome.unwrap();
    assert!(!result.close);
    assert_eq!(result.message.as_deref(), Some("did act"));

    // `setItems` replaces the static items.
    host.query("set", None, &Context::default());
    let HostEvent::Items { items, .. } =
        wait_for(&events, |e| matches!(e, HostEvent::Items { .. }))
    else {
        unreachable!()
    };
    assert_eq!(items[0].title, "Three");
    host.shutdown();
}

#[test]
fn a_crashing_provider_is_restarted_then_disabled_with_the_reason() {
    let (host, _events) = host();
    host.configure(vec![test_provider("crash", "crashy")]);
    let disabled = settle(&host, "crashy", |s| matches!(s, State::Disabled(_)));
    let why = disabled.reason().unwrap().to_owned();
    assert!(why.contains("stopped 3 times"), "{why}");
    assert!(why.contains("exit"), "the exit status is named: {why}");
    assert!(
        why.ends_with("crashing on purpose"),
        "and what it said: {why}"
    );
    // A reload with the same settings gives it another chance.
    host.configure(vec![test_provider("crash", "crashy")]);
    settle(&host, "crashy", |s| !matches!(s, State::Disabled(_)));
    host.shutdown();
}

#[test]
fn an_execute_nobody_answers_times_out() {
    let (host, events) = host();
    host.configure(vec![test_provider("hang", "slow")]);
    settle(&host, "slow", |s| *s == State::Running);
    let token = host
        .execute(
            "slow",
            Execute {
                item_id: "x".into(),
                action_id: "act".into(),
                query: "".into(),
            },
        )
        .unwrap();
    let HostEvent::Executed {
        token: t, outcome, ..
    } = wait_for(&events, |e| matches!(e, HostEvent::Executed { .. }))
    else {
        unreachable!()
    };
    assert_eq!(t, token);
    assert_eq!(outcome.unwrap_err(), "slow didn't answer");
    host.shutdown();
}

#[test]
fn garbage_and_a_wrong_name_end_the_provider_with_the_reason() {
    let (host, _events) = host();
    host.configure(vec![
        test_provider("garbage", "noisy"),
        test_provider("wrongname", "named"),
    ]);
    let noisy = settle(&host, "noisy", |s| {
        matches!(s, State::Restarting(_) | State::Disabled(_))
    });
    assert!(
        noisy.reason().unwrap().contains("broke the protocol"),
        "{noisy:?}"
    );
    let named = settle(&host, "named", |s| {
        matches!(s, State::Restarting(_) | State::Disabled(_))
    });
    assert!(
        named.reason().unwrap().contains("hello was refused"),
        "{named:?}"
    );
    host.shutdown();
}

#[test]
fn oversized_and_stale_results_are_dropped() {
    let (host, events) = host();
    host.configure(vec![test_provider("flood", "flood")]);
    settle(&host, "flood", |s| *s == State::Running);
    host.query("first", None, &Context::default());
    let id = host.query("second", None, &Context::default());
    let deadline = Instant::now() + Duration::from_millis(800);
    let mut seen = Vec::new();
    while let Ok(event) = events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if let HostEvent::Results {
            query_id, items, ..
        } = event
        {
            seen.push((query_id, items.len(), items[0].title.clone()));
        }
    }
    assert_eq!(
        seen,
        [(id, 1, "Echo second".to_owned())],
        "only the latest, well-formed batch"
    );
    host.shutdown();
}

#[test]
fn a_missing_program_is_disabled_at_once() {
    let (host, _events) = host();
    host.configure(vec![ProviderConfig {
        name: "ghost".into(),
        command: vec!["definitely-not-a-provider-xyz".into()],
        ..ProviderConfig::default()
    }]);
    let state = settle(&host, "ghost", |s| matches!(s, State::Disabled(_)));
    assert!(
        state.reason().unwrap().starts_with("couldn't start"),
        "{state:?}"
    );
}

#[test]
fn removing_a_provider_stops_it() {
    let (host, events) = host();
    host.configure(vec![test_provider("normal", "gone")]);
    settle(&host, "gone", |s| *s == State::Running);
    host.configure(vec![]);
    assert!(host.statuses().is_empty());
    wait_for(
        &events,
        |e| matches!(e, HostEvent::Changed { name } if name == "gone"),
    );
    assert!(host
        .execute(
            "gone",
            Execute {
                item_id: "x".into(),
                action_id: "y".into(),
                query: "".into()
            }
        )
        .is_err());
}

/// The Python example, when a Python is installed (CI has one).
#[test]
fn the_python_example_provider_works() {
    let python = ["python3", "python"].into_iter().find(|p| {
        std::process::Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    });
    let Some(python) = python else {
        eprintln!("no Python; skipped");
        return;
    };
    let script: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "examples",
        "provider-python",
        "bookmarks.py",
    ]
    .iter()
    .collect();
    let (host, events) = host();
    host.configure(vec![ProviderConfig {
        name: "bookmarks".into(),
        command: vec![python.into(), script.display().to_string()],
        ..ProviderConfig::default()
    }]);
    let HostEvent::Items { items, .. } =
        wait_for(&events, |e| matches!(e, HostEvent::Items { .. }))
    else {
        unreachable!()
    };
    assert!(!items.is_empty(), "it lists its bookmarks");
    assert!(items.iter().all(|i| i.check().is_ok()));
    let token = host
        .execute(
            "bookmarks",
            Execute {
                item_id: items[0].id.clone(),
                action_id: "count".into(),
                query: "".into(),
            },
        )
        .unwrap();
    let HostEvent::Executed {
        token: t, outcome, ..
    } = wait_for(&events, |e| matches!(e, HostEvent::Executed { .. }))
    else {
        unreachable!()
    };
    assert_eq!(t, token);
    assert!(outcome.unwrap().message.unwrap().contains("bookmark"));
    host.shutdown();
}

#[test]
fn a_secret_reaches_its_provider_and_a_missing_one_stops_it_starting() {
    let (host, events) = host();
    let mut given = test_provider("secret", "keyed");
    given.secret = Secret::Value("s3cret".into());
    let mut missing = test_provider("secret", "locked");
    missing.secret = Secret::Missing("no `jira:x` in the credential store".into());
    host.configure(vec![given, missing]);
    let HostEvent::Items { items, .. } = wait_for(
        &events,
        |e| matches!(e, HostEvent::Items { name, .. } if name == "keyed"),
    ) else {
        unreachable!()
    };
    assert_eq!(
        items[0].title, "s3cret",
        "the secret came through the environment"
    );
    let state = settle(&host, "locked", |s| matches!(s, State::Disabled(_)));
    assert_eq!(state.reason(), Some("no `jira:x` in the credential store"));
    assert!(!format!("{:?}", Secret::Value("s3cret".into())).contains("s3cret"));
    host.shutdown();
}
