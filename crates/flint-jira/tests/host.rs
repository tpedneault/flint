//! `flint-jira` as Flint runs it: started by Flint's own provider host
//! with its secret in the environment, against a fake Jira, its open
//! issues arriving as items.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flint_host::{HostEvent, ProviderConfig, ProviderHost, Secret, Timing};
use flint_protocol::HostInfo;
use flint_provider_kit::testing::FakeHttp;

#[test]
fn flint_starts_flint_jira_with_its_token() {
    let jira = FakeHttp::start(vec![(
        "/rest/api/2/search",
        200,
        r#"{"issues": [{"key": "OPS-1", "fields": {"summary": "First", "status": {"name": "Open"}}}]}"#
            .into(),
    )]);
    let (send, events) = mpsc::channel();
    let send = Mutex::new(send);
    let host = ProviderHost::new(
        HostInfo {
            name: "flint".into(),
            version: "0".into(),
            pid: 1,
        },
        std::env::temp_dir().join("flint-jira-test"),
        std::env::temp_dir(),
        Timing::default(),
        Arc::new(move |e| {
            let _ = send.lock().unwrap().send(e);
        }),
    );
    host.configure(vec![ProviderConfig {
        name: "jira".into(),
        command: vec![
            env!("CARGO_BIN_EXE_flint-jira").into(),
            "--url".into(),
            jira.url.clone(),
        ],
        secret: Secret::Value("t0ken".into()),
        ..ProviderConfig::default()
    }]);
    let items = loop {
        match events
            .recv_timeout(Duration::from_secs(15))
            .expect("items in time")
        {
            // The first is registration's (none); then the refresh.
            HostEvent::Items { items, .. } if !items.is_empty() => break items,
            _ => continue,
        }
    };
    assert_eq!(items[0].title, "OPS-1 First");
    let (_, headers) = &jira.seen.lock().unwrap()[0];
    assert!(headers.iter().any(|h| h == "Authorization: Bearer t0ken"));
    host.shutdown();
}
