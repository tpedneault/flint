//! A provider for `flint-host`'s tests, good or bad on request:
//! `flint-test-provider <mode> <name>`.
//!
//! - `normal`: registers two static items, answers each query with one
//!   result echoing it, answers `execute` with a message and stays open;
//!   a query `set` makes it replace its static items.
//! - `crash`: says `hello`, then complains on stderr and exits with code 3.
//! - `hang`: registers, then never answers `execute`.
//! - `garbage`: sends a frame header claiming a huge body.
//! - `flood`: answers a query with an oversized batch, then a stale one,
//!   then a good one.
//! - `wrongname`: says `hello` under another name.
//! - `secret`: as `normal`, its first item titled with the secret it was
//!   started with (`none` without one).
//!
//! Not an example of how to write a provider (see `examples/` for those);
//! it exists to misbehave on purpose.

use std::io::{self, BufReader, Write};

use flint_protocol::frame::{read_frame, write_frame};
use flint_protocol::{Id, Message};
use serde_json::{json, Value};

fn send(out: &mut impl Write, message: &Message) {
    let _ = write_frame(out, &message.to_bytes());
}

fn item(id: &str, title: &str) -> Value {
    json!({ "id": id, "title": title,
            "actions": [{ "id": "act", "title": "Act", "kind": "provider" }] })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map_or("normal", String::as_str);
    let name = args.get(2).map_or("test", String::as_str);
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let mut out = io::stdout().lock();
    eprintln!("{name} starting in {mode} mode");

    if mode == "garbage" {
        let _ = out.write_all(b"Content-Length: 99999999999\r\n\r\n");
        let _ = out.flush();
        std::thread::sleep(std::time::Duration::from_secs(5));
        return;
    }
    let hello_name = if mode == "wrongname" {
        "someone-else"
    } else {
        name
    };
    send(
        &mut out,
        &Message::request(
            Id::Number(1),
            "hello",
            json!({
                "protocol": 1, "role": "provider", "name": hello_name, "version": "0", "pid": std::process::id()
            }),
        ),
    );
    if read_frame(&mut input).ok().flatten().is_none() {
        return;
    }
    if mode == "crash" {
        eprintln!("crashing on purpose");
        std::process::exit(3);
    }
    let first_title = if mode == "secret" {
        std::env::var(flint_protocol::provider::SECRET_ENV).unwrap_or_else(|_| "none".into())
    } else {
        "One".into()
    };
    send(
        &mut out,
        &Message::request(
            Id::Number(2),
            "provider/register",
            json!({
                "id": name, "title": "Test", "prefix": "t", "dynamic": true,
                "items": [item(&format!("{name}:one"), &first_title), item(&format!("{name}:two"), "Two")]
            }),
        ),
    );

    while let Ok(Some(body)) = read_frame(&mut input) {
        let Ok(message) = Message::parse(&body) else {
            continue;
        };
        match message {
            Message::Notification { method, params } if method == "query" => {
                let id = params["queryId"].as_u64().unwrap_or(0);
                let text = params["text"].as_str().unwrap_or("").to_owned();
                if mode == "flood" {
                    let many: Vec<Value> = (0..201)
                        .map(|n| item(&format!("{name}:{n}"), "x"))
                        .collect();
                    send(
                        &mut out,
                        &Message::Notification {
                            method: "results".into(),
                            params: json!({"queryId": id, "items": many}),
                        },
                    );
                    send(
                        &mut out,
                        &Message::Notification {
                            method: "results".into(),
                            params: json!({"queryId": id.saturating_sub(1), "items": [item("stale", "Stale")]}),
                        },
                    );
                }
                if text == "set" {
                    send(
                        &mut out,
                        &Message::Notification {
                            method: "provider/setItems".into(),
                            params: json!({"items": [item(&format!("{name}:three"), "Three")]}),
                        },
                    );
                }
                let echo = item(&format!("{name}:echo:{text}"), &format!("Echo {text}"));
                send(
                    &mut out,
                    &Message::Notification {
                        method: "results".into(),
                        params: json!({"queryId": id, "items": [echo], "done": true}),
                    },
                );
            }
            Message::Request { id, method, params } if method == "execute" => {
                if mode == "hang" {
                    continue;
                }
                let action = params["actionId"].as_str().unwrap_or("").to_owned();
                send(
                    &mut out,
                    &Message::result(
                        id,
                        json!({"close": false, "message": format!("did {action}")}),
                    ),
                );
            }
            _ => {}
        }
    }
}
