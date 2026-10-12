//! The example, run by Flint's own provider host: it registers, answers a
//! query with conversions, and carries out its provider action.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flint_host::{HostEvent, ProviderConfig, ProviderHost, Timing};
use flint_protocol::provider::{Context, Execute};
use flint_protocol::HostInfo;

#[test]
fn flint_runs_the_example() {
    let (send, events) = mpsc::channel();
    let send = Mutex::new(send);
    let host = ProviderHost::new(
        HostInfo {
            name: "flint".into(),
            version: "0".into(),
            pid: 1,
        },
        std::env::temp_dir().join("flint-example-provider-test"),
        std::env::temp_dir(),
        Timing::default(),
        Arc::new(move |e| {
            let _ = send.lock().unwrap().send(e);
        }),
    );
    host.configure(vec![ProviderConfig {
        name: "numbers".into(),
        command: vec![env!("CARGO_BIN_EXE_flint-example-provider").into()],
        ..ProviderConfig::default()
    }]);
    let next = |want: &dyn Fn(&HostEvent) -> bool| loop {
        let event = events
            .recv_timeout(Duration::from_secs(10))
            .expect("an event in time");
        if want(&event) {
            return event;
        }
    };
    next(&|e| matches!(e, HostEvent::Registered { dynamic: true, .. }));

    let id = host.query("0xff", None, &Context::default());
    let HostEvent::Results {
        query_id, items, ..
    } = next(&|e| matches!(e, HostEvent::Results { .. }))
    else {
        unreachable!()
    };
    assert_eq!(query_id, id);
    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["255", "0xff", "0b11111111", "0o377"]);
    assert!(items.iter().all(|i| i.check().is_ok()));

    let execute = Execute {
        item_id: "numbers:dec".into(),
        action_id: "factor".into(),
        query: "84".into(),
    };
    host.execute("numbers", execute).unwrap();
    let HostEvent::Executed { outcome, .. } = next(&|e| matches!(e, HostEvent::Executed { .. }))
    else {
        unreachable!()
    };
    assert_eq!(
        outcome.unwrap().message.as_deref(),
        Some("84 = 2 × 2 × 3 × 7")
    );
    host.shutdown();
}
