//! A fake HTTP server for providers' tests: answers each request with the
//! first canned reply whose path prefix matches, and records what it was
//! asked. Enough HTTP/1.1 for `ureq`, nothing more; plain HTTP on
//! `127.0.0.1`, so no certificates.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// What the server was asked: each request line and its headers.
pub type Seen = Arc<Mutex<Vec<(String, Vec<String>)>>>;

/// A running fake server.
pub struct FakeHttp {
    /// `http://127.0.0.1:<port>`, the base URL to give the client.
    pub url: String,
    /// Each request's method, path and query (`GET /rest/api/2/search?jql=…`),
    /// percent-decoded, and its headers.
    pub seen: Seen,
}

impl FakeHttp {
    /// Starts a server answering with `routes`: (path prefix, status,
    /// JSON body). Unmatched paths get 404.
    pub fn start(routes: Vec<(&'static str, u16, String)>) -> FakeHttp {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("its address"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    headers.push(line.trim().to_owned());
                }
                let target = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let decoded = decode(&format!(
                    "{} {target}",
                    request.split_whitespace().next().unwrap_or("GET")
                ));
                record.lock().unwrap().push((decoded, headers));
                let path = target.split('?').next().unwrap_or("/");
                let (status, body) = routes
                    .iter()
                    .find(|(prefix, ..)| path.starts_with(prefix))
                    .map_or((404, "{}".to_owned()), |(_, s, b)| (*s, b.clone()));
                let mut stream = &stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        FakeHttp { url, seen }
    }

    /// The requests so far, as `METHOD path?query`.
    pub fn requests(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(r, _)| r.clone())
            .collect()
    }
}

/// Percent-decoding (and `+` as space) for readable assertions.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
