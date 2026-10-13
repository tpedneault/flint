//! The provider's side of `docs/PROTOCOL.md`, for the providers that ship
//! with Flint (`flint-jira`, `flint-gitlab`): a provider is its handlers,
//! and this is the conversation around them.
//!
//! [`serve`] says `hello`, registers, then answers until Flint closes the
//! pipe. What it takes care of so each provider needn't:
//!
//! - **Only the latest query.** A provider's answers come from a server
//!   that may take a second; meanwhile the user types on. Queries that
//!   arrive while one is being answered are coalesced to the newest, so
//!   a slow server costs one stale request, not a queue of them, and
//!   Flint would drop stale results anyway.
//! - **Refreshing static items.** A provider's static items ("my open
//!   issues") come from the network too: it registers with none, and
//!   [`Provider::refresh`] runs at once and then every
//!   [`Provider::refresh_every`], each result sent as `provider/setItems`.
//!   Registering doesn't wait for the network, so Flint has the provider
//!   (its prefix, its title) from the start.
//! - **Failures as one line.** A handler's `Err` becomes empty results and
//!   a `log` line for queries and refreshes (shown in the provider's log,
//!   never a crash), and an error reply for `execute`, which Flint shows
//!   in its footer.
//!
//! It deliberately isn't a general SDK: the examples in `examples/` show
//! that a provider needs nothing of Flint's, and third-party providers
//! should keep it that way. This exists so Flint's own providers don't
//! repeat a hundred lines each.

#[cfg(feature = "testing")]
pub mod testing;

use std::io::{self, BufReader, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use flint_protocol::frame::{read_frame, write_frame};
use flint_protocol::provider::{
    Execute, ExecuteResult, LogLevel, LogParams, Query, Register, Results, SetItems, MAX_BATCH,
};
use flint_protocol::{Hello, Id, Item, Message, Role, RpcError, PROTOCOL_VERSION};
use serde_json::{json, Value};

/// The error code `execute` failures are answered with: a provider's own
/// failure, outside JSON-RPC's reserved range.
pub const EXECUTE_FAILED: i64 = -32010;

/// What a provider does; [`serve`] does the talking.
pub trait Provider {
    /// What to register as. Its `items` are usually empty: static items
    /// come from [`Provider::refresh`].
    fn register(&self) -> Register;

    /// Fetches the static items afresh, `None` when it has none.
    fn refresh(&mut self) -> Option<Result<Vec<Item>, String>> {
        None
    }

    /// How often to call [`Provider::refresh`] after the first time.
    fn refresh_every(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }

    /// Answers a query (only called when registered `dynamic`).
    fn query(&mut self, query: &Query) -> Result<Vec<Item>, String> {
        let _ = query;
        Ok(Vec::new())
    }

    /// Carries out one of its `provider` actions.
    fn execute(&mut self, execute: &Execute) -> Result<ExecuteResult, String> {
        Err(format!("no action `{}` here", execute.action_id))
    }
}

/// Serves `provider` on standard input and output until Flint closes
/// them; a failure (Flint refusing the hello) as one line.
pub fn run(name: &str, provider: &mut impl Provider) -> Result<(), String> {
    serve(name, io::stdin(), io::stdout().lock(), provider)
}

/// Serves `provider` over `reader` and `writer`.
pub fn serve<R: Read + Send + 'static, W: Write>(
    name: &str,
    reader: R,
    mut writer: W,
    provider: &mut impl Provider,
) -> Result<(), String> {
    // A thread reads, so coalescing can look at what is already waiting.
    let (inbox, messages) = mpsc::channel::<Message>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        while let Ok(Some(body)) = read_frame(&mut reader) {
            if let Ok(message) = Message::parse(&body) {
                if inbox.send(message).is_err() {
                    break;
                }
            }
        }
    });

    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        role: Role::Provider,
        name: name.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        pid: std::process::id(),
    };
    send(
        &mut writer,
        &Message::request(Id::Number(1), "hello", json!(hello)),
    )?;
    match messages.recv() {
        Ok(Message::Response {
            outcome: Err(e), ..
        }) => {
            return Err(format!("Flint refused the hello: {}", e.message));
        }
        Ok(Message::Response { .. }) => {}
        _ => return Err("Flint didn't answer the hello".into()),
    }
    let register = provider.register();
    send(
        &mut writer,
        &Message::request(Id::Number(2), "provider/register", json!(register)),
    )?;

    let mut next_refresh = Instant::now();
    loop {
        if Instant::now() >= next_refresh {
            if let Some(found) = provider.refresh() {
                match found {
                    Ok(items) => {
                        notify(&mut writer, "provider/setItems", json!(SetItems { items }))?
                    }
                    Err(why) => log(&mut writer, LogLevel::Warn, &format!("refresh: {why}"))?,
                }
            }
            next_refresh = Instant::now() + provider.refresh_every();
        }
        let first =
            match messages.recv_timeout(next_refresh.saturating_duration_since(Instant::now())) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            };
        // Everything already waiting, so only the newest query is
        // answered; other messages keep their order.
        let mut batch = vec![first];
        batch.extend(messages.try_iter());
        let latest = batch.iter().rposition(is_query);
        for (n, message) in batch.into_iter().enumerate() {
            match message {
                Message::Notification { method, params } if method == "query" => {
                    if Some(n) == latest {
                        answer_query(&mut writer, provider, params)?;
                    }
                }
                Message::Request { id, method, params } if method == "execute" => {
                    let reply = match serde_json::from_value::<Execute>(params) {
                        Ok(request) => match provider.execute(&request) {
                            Ok(result) => Message::result(id, json!(result)),
                            Err(why) => {
                                Message::error(Some(id), RpcError::new(EXECUTE_FAILED, why))
                            }
                        },
                        Err(e) => Message::error(Some(id), RpcError::new(-32602, e.to_string())),
                    };
                    send(&mut writer, &reply)?;
                }
                Message::Request { id, method, .. } => send(
                    &mut writer,
                    &Message::error(
                        Some(id),
                        RpcError::new(-32601, format!("no method `{method}`")),
                    ),
                )?,
                // `cancel` (the next query supersedes it anyway),
                // `register`'s answer, anything newer than this.
                _ => {}
            }
        }
    }
}

fn is_query(message: &Message) -> bool {
    matches!(message, Message::Notification { method, .. } if method == "query")
}

fn answer_query(
    writer: &mut impl Write,
    provider: &mut impl Provider,
    params: Value,
) -> Result<(), String> {
    let Ok(query) = serde_json::from_value::<Query>(params) else {
        return Ok(());
    };
    let items = provider.query(&query).unwrap_or_else(|why| {
        let _ = log(writer, LogLevel::Warn, &format!("`{}`: {why}", query.text));
        Vec::new()
    });
    let mut batches: Vec<Vec<Item>> = items.chunks(MAX_BATCH).map(<[Item]>::to_vec).collect();
    if batches.is_empty() {
        batches.push(Vec::new());
    }
    let last = batches.len() - 1;
    for (n, items) in batches.into_iter().enumerate() {
        let results = Results {
            query_id: query.query_id,
            items,
            done: n == last,
        };
        notify(writer, "results", json!(results))?;
    }
    Ok(())
}

fn send(writer: &mut impl Write, message: &Message) -> Result<(), String> {
    write_frame(writer, &message.to_bytes()).map_err(|e| format!("Flint went away: {e}"))
}

fn notify(writer: &mut impl Write, method: &str, params: Value) -> Result<(), String> {
    send(
        writer,
        &Message::Notification {
            method: method.into(),
            params,
        },
    )
}

/// Sends a line to the provider's log in Flint.
pub fn log(writer: &mut impl Write, level: LogLevel, message: &str) -> Result<(), String> {
    notify(
        writer,
        "log",
        json!(LogParams {
            level,
            message: message.to_owned(),
        }),
    )
}

/// The value of `--name value` (or `--name=value`) among `args`, every one
/// of them for a flag given more than once.
pub fn flag_values(args: &[String], name: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == name {
            if let Some(value) = args.next() {
                found.push(value.clone());
            }
        } else if let Some(value) = arg.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
            found.push(value.to_owned());
        }
    }
    found
}

/// The secret Flint started this provider with (`[[providers]]
/// credential`), or why there isn't one.
pub fn secret() -> Result<String, String> {
    match std::env::var(flint_protocol::provider::SECRET_ENV) {
        Ok(secret) if !secret.trim().is_empty() => Ok(secret.trim().to_owned()),
        _ => Err(
            "Flint gave this provider no secret; set `credential` in its [[providers]] entry"
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_protocol::item::{Action, ActionKind};
    use std::io::{PipeReader, PipeWriter};

    struct Echo {
        refreshed: usize,
    }

    fn item(id: &str, title: &str) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            icon: None,
            keywords: vec![],
            actions: vec![Action {
                id: "copy".into(),
                title: "Copy".into(),
                key: None,
                kind: ActionKind::Copy { text: title.into() },
            }],
        }
    }

    impl Provider for Echo {
        fn register(&self) -> Register {
            Register {
                id: "echo".into(),
                title: "Echo".into(),
                prefix: Some("e".into()),
                dynamic: true,
                debounce_ms: 0,
                min_query_length: 0,
                items: vec![],
            }
        }
        fn refresh(&mut self) -> Option<Result<Vec<Item>, String>> {
            self.refreshed += 1;
            Some(Ok(vec![item("echo:static", "Static")]))
        }
        fn query(&mut self, query: &Query) -> Result<Vec<Item>, String> {
            match query.text.as_str() {
                "fail" => Err("the server said no".into()),
                "many" => Ok((0..450).map(|n| item(&format!("echo:{n}"), "x")).collect()),
                text => Ok(vec![item(&format!("echo:{text}"), text)]),
            }
        }
        fn execute(&mut self, execute: &Execute) -> Result<ExecuteResult, String> {
            match execute.action_id.as_str() {
                "ok" => Ok(ExecuteResult {
                    close: false,
                    message: Some(format!("did {}", execute.item_id)),
                }),
                _ => Err("can't".into()),
            }
        }
    }

    /// The host's end of a served provider.
    struct Host {
        to: PipeWriter,
        from: BufReader<PipeReader>,
    }

    impl Host {
        fn start() -> Host {
            let (host_read, provider_write) = std::io::pipe().unwrap();
            let (provider_read, host_write) = std::io::pipe().unwrap();
            std::thread::spawn(move || {
                let _ = serve(
                    "echo",
                    provider_read,
                    provider_write,
                    &mut Echo { refreshed: 0 },
                );
            });
            let mut host = Host {
                to: host_write,
                from: BufReader::new(host_read),
            };
            let Message::Request { id, method, .. } = host.next() else {
                panic!("no hello")
            };
            assert_eq!(method, "hello");
            host.send(Message::result(id, json!({})));
            host
        }
        fn next(&mut self) -> Message {
            Message::parse(&read_frame(&mut self.from).unwrap().unwrap()).unwrap()
        }
        fn send(&mut self, message: Message) {
            write_frame(&mut self.to, &message.to_bytes()).unwrap();
        }
        fn query(&mut self, id: u64, text: &str) {
            self.send(Message::Notification {
                method: "query".into(),
                params: json!({"queryId": id, "text": text, "scoped": false, "context": {}}),
            });
        }
        /// The next notification called `method`, skipping others.
        fn next_named(&mut self, wanted: &str) -> Value {
            loop {
                if let Message::Notification { method, params } = self.next() {
                    if method == wanted {
                        return params;
                    }
                }
            }
        }
    }

    #[test]
    fn registers_then_sends_its_static_items() {
        let mut host = Host::start();
        let Message::Request { method, params, .. } = host.next() else {
            panic!()
        };
        assert_eq!(method, "provider/register");
        assert_eq!(params["prefix"], "e");
        let items = host.next_named("provider/setItems");
        assert_eq!(items["items"][0]["title"], "Static");
    }

    #[test]
    fn queries_are_answered_in_batches_and_failures_are_logged() {
        let mut host = Host::start();
        host.query(1, "hello");
        let results = host.next_named("results");
        assert_eq!(results["queryId"], 1);
        assert_eq!(results["items"][0]["title"], "hello");
        assert_eq!(results["done"], true);

        host.query(2, "many");
        let sizes: Vec<usize> = (0..3)
            .map(|_| {
                host.next_named("results")["items"]
                    .as_array()
                    .unwrap()
                    .len()
            })
            .collect();
        assert_eq!(sizes, [200, 200, 50], "batches of at most 200");

        host.query(3, "fail");
        let logged = host.next_named("log");
        assert!(logged["message"]
            .as_str()
            .unwrap()
            .contains("the server said no"));
        let results = host.next_named("results");
        assert_eq!(results["items"].as_array().unwrap().len(), 0);
        assert_eq!(results["done"], true);
    }

    #[test]
    fn execute_answers_with_a_result_or_an_error() {
        let mut host = Host::start();
        let execute = |action: &str| json!({"itemId": "echo:x", "actionId": action, "query": "x"});
        host.send(Message::request(Id::Number(7), "execute", execute("ok")));
        host.send(Message::request(Id::Number(8), "execute", execute("nope")));
        let mut answers = Vec::new();
        while answers.len() < 2 {
            if let Message::Response { id, outcome } = host.next() {
                answers.push((id, outcome));
            }
        }
        assert_eq!(answers[0].0, Some(Id::Number(7)));
        assert_eq!(answers[0].1.as_ref().unwrap()["message"], "did echo:x");
        let err = answers[1].1.as_ref().unwrap_err();
        assert_eq!((err.code, err.message.as_str()), (EXECUTE_FAILED, "can't"));
    }

    #[test]
    fn flags_are_read_both_ways_and_repeated() {
        let args: Vec<String> = ["--url", "https://a", "--project=X", "--project", "Y"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(flag_values(&args, "--url"), ["https://a"]);
        assert_eq!(flag_values(&args, "--project"), ["X", "Y"]);
        assert!(flag_values(&args, "--nope").is_empty());
    }
}
