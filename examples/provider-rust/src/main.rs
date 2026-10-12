//! A Flint provider in Rust: numbers in other bases.
//!
//! Type a number (`255`, `0xff`, `0b1010`, `0o17`) and this answers with
//! it in decimal, hex, binary and octal, each copied by `Enter`, plus
//! "Prime factors", an action Flint hands back here (`execute`) because
//! only the provider knows how to do it. It is dynamic: Flint sends each
//! query as the user types and shows what comes back.
//!
//! It frames messages itself and depends only on `serde_json`, to show
//! that nothing of Flint's is needed to write a provider. Declare it:
//!
//! ```toml
//! [[providers]]
//! name = "numbers"
//! command = ["path/to/flint-example-provider"]
//! ```
//!
//! `tests/host.rs` runs it through Flint's own provider host.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

const NAME: &str = "numbers";

/// One `Content-Length`-framed message, or `None` at the end of input.
fn read_message(input: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let mut body = vec![0; length?];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write_message(output: &mut impl Write, mut message: Value) {
    message["jsonrpc"] = "2.0".into();
    let body = message.to_string();
    let _ = write!(output, "Content-Length: {}\r\n\r\n{body}", body.len());
    let _ = output.flush();
}

/// `255`, `0xff`, `0b1010`, `0o17` (underscores allowed).
fn parse(text: &str) -> Option<u64> {
    let text = text.trim().replace('_', "").to_ascii_lowercase();
    let (digits, radix) = match text.get(..2) {
        Some("0x") => (&text[2..], 16),
        Some("0b") => (&text[2..], 2),
        Some("0o") => (&text[2..], 8),
        _ => (&text[..], 10),
    };
    u64::from_str_radix(digits, radix).ok()
}

fn factors(mut n: u64) -> Vec<String> {
    let mut found = Vec::new();
    let mut d = 2;
    while d * d <= n {
        while n.is_multiple_of(d) {
            found.push(d.to_string());
            n /= d;
        }
        d += 1;
    }
    if n > 1 {
        found.push(n.to_string());
    }
    found
}

/// The number in four bases, as items.
fn results(n: u64) -> Vec<Value> {
    let forms = [
        ("dec", n.to_string()),
        ("hex", format!("{n:#x}")),
        ("bin", format!("{n:#b}")),
        ("oct", format!("{n:#o}")),
    ];
    let factor = json!({"id": "factor", "title": "Prime factors", "key": "p", "kind": "provider"});
    let item = |(base, text): (&str, String)| {
        let copy = json!({"id": "copy", "title": "Copy", "key": "c", "kind": "copy", "text": text});
        // The id is stable per base, so Flint learns which one you copy.
        let id = format!("{NAME}:{base}");
        let subtitle = format!("{n} in {base}");
        json!({"id": id, "title": text, "subtitle": subtitle, "actions": [copy, factor]})
    };
    forms.into_iter().map(item).collect()
}

/// The answer to `execute` (the factor action, the only one sent here).
fn execute(params: &Value) -> Value {
    match parse(params["query"].as_str().unwrap_or("")) {
        Some(n) if n > 1 => {
            let message = format!("{n} = {}", factors(n).join(" × "));
            json!({"result": {"close": false, "message": message}})
        }
        _ => json!({"error": {"code": -32010, "message": "factors need a number above 1"}}),
    }
}

fn main() {
    let (stdin, mut output) = (io::stdin(), io::stdout().lock());
    let mut input = stdin.lock();
    let pid = std::process::id();
    let hello =
        json!({"protocol": 1, "role": "provider", "name": NAME, "version": "1", "pid": pid});
    write_message(
        &mut output,
        json!({"id": 1, "method": "hello", "params": hello}),
    );
    read_message(&mut input); // the host's answer; it closes the pipe if it refuses
    let register = json!({"id": NAME, "title": "Numbers", "prefix": "#", "dynamic": true});
    write_message(
        &mut output,
        json!({"id": 2, "method": "provider/register", "params": register}),
    );

    while let Some(message) = read_message(&mut input) {
        let params = &message["params"];
        match message["method"].as_str() {
            Some("query") => {
                let text = params["text"].as_str().unwrap_or("");
                let items = parse(text).map(results).unwrap_or_default();
                let batch = json!({"queryId": params["queryId"], "items": items, "done": true});
                write_message(&mut output, json!({"method": "results", "params": batch}));
            }
            Some("execute") => {
                let mut reply = execute(params);
                reply["id"] = message["id"].clone();
                write_message(&mut output, reply);
            }
            _ => {} // register's answer, `cancel`: nothing here runs long enough to stop
        }
    }
}
