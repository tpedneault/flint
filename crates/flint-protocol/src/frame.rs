//! `Content-Length` framing with limits.
//!
//! Writing is `fenix_rpc::write_message`, unchanged, so the bytes on the
//! wire are exactly what Fenix's language-server client writes. Reading is
//! our own: `fenix_rpc::read_message` allocates whatever the header says
//! and reads header lines without a bound, which a peer on a socket could
//! use to make Flint allocate gigabytes or read forever (F16 in
//! FENIX-INTEGRATION.md). This reader keeps its semantics — a clean EOF
//! between messages is `Ok(None)`, unknown headers are ignored — and adds
//! the limits, checked before anything of the claimed size is allocated.
//! It is a little stricter about truncation: EOF partway through any
//! header is an error, where `fenix-rpc` only notices once it has seen a
//! `Content-Length`.

use std::io::{self, BufRead, Read, Write};

/// The largest body accepted (PROTOCOL.md, "Transport").
pub const MAX_BODY: usize = 4 * 1024 * 1024;
/// The longest header line accepted, terminator included.
pub const MAX_HEADER_LINE: usize = 1024;
/// How many header lines one message may have. LSP and DAP send one or
/// two; the cap stops a peer from streaming headers forever.
pub const MAX_HEADERS: usize = 16;

/// Reads one framed message and returns its body.
///
/// `Ok(None)` is a clean close: EOF before the first byte of a message.
/// Anything else that ends early, or breaks a limit, is an error of kind
/// `InvalidData` or `UnexpectedEof`, after which the connection should be
/// closed: there is no way to find the next message boundary.
pub fn read_frame<R: BufRead>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    let mut started = false;
    for _ in 0..=MAX_HEADERS {
        let mut line = Vec::new();
        reader
            .by_ref()
            .take(MAX_HEADER_LINE as u64)
            .read_until(b'\n', &mut line)?;
        if line.is_empty() {
            return if started {
                Err(eof("connection closed mid-header"))
            } else {
                Ok(None)
            };
        }
        started = true;
        if line.last() != Some(&b'\n') {
            return Err(if line.len() >= MAX_HEADER_LINE {
                invalid(format!("header line longer than {MAX_HEADER_LINE} bytes"))
            } else {
                eof("connection closed mid-header")
            });
        }
        let line = std::str::from_utf8(&line)
            .map_err(|_| invalid("header line is not UTF-8".into()))?
            .trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            let Some(length) = content_length else {
                return Err(invalid(
                    "message body started before any Content-Length header".into(),
                ));
            };
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            return Ok(Some(body));
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            let value = value.trim();
            let length: usize = value
                .parse()
                .map_err(|_| invalid(format!("malformed Content-Length header: {value:?}")))?;
            if length > MAX_BODY {
                return Err(invalid(format!(
                    "message of {length} bytes is over the {MAX_BODY}-byte limit"
                )));
            }
            content_length = Some(length);
        }
        // Other headers (`Content-Type`) are allowed and ignored.
    }
    Err(invalid(format!("more than {MAX_HEADERS} header lines")))
}

/// Writes one framed message. `body` must already be JSON; this only
/// frames it.
pub fn write_frame<W: Write>(writer: &mut W, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_BODY {
        return Err(invalid(format!(
            "message of {} bytes is over the {MAX_BODY}-byte limit",
            body.len()
        )));
    }
    fenix_rpc::write_message(writer, body)
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn eof(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame(body: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }

    type Read = io::Result<Option<Vec<u8>>>;

    /// Reads every message from `bytes` with both readers, so each case
    /// shows where Flint agrees with `fenix-rpc` and where it is stricter.
    fn both(bytes: &[u8]) -> (Vec<Read>, Read) {
        let mut ours = Cursor::new(bytes.to_vec());
        let mut results = Vec::new();
        loop {
            let r = read_frame(&mut ours);
            let stop = !matches!(r, Ok(Some(_)));
            results.push(r);
            if stop {
                break;
            }
        }
        let theirs = fenix_rpc::read_message(&mut Cursor::new(bytes.to_vec()));
        (results, theirs)
    }

    #[test]
    fn reads_what_fenix_rpc_writes() {
        let mut bytes = Vec::new();
        fenix_rpc::write_message(&mut bytes, br#"{"a":1}"#).unwrap();
        fenix_rpc::write_message(&mut bytes, br#"{"b":"two"}"#).unwrap();
        let mut reader = Cursor::new(bytes);
        assert_eq!(read_frame(&mut reader).unwrap().unwrap(), br#"{"a":1}"#);
        assert_eq!(read_frame(&mut reader).unwrap().unwrap(), br#"{"b":"two"}"#);
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    #[test]
    fn fenix_rpc_reads_what_we_write() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, br#"{"x":[1,2]}"#).unwrap();
        let read = fenix_rpc::read_message(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(read.unwrap(), br#"{"x":[1,2]}"#);
    }

    #[test]
    fn agrees_with_fenix_rpc_on_ordinary_input() {
        let mut with_extra_header =
            b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".to_vec();
        with_extra_header.extend(frame("{}"));
        for bytes in [frame(r#"{"k":"v"}"#), with_extra_header, Vec::new()] {
            let (ours, theirs) = both(&bytes);
            assert_eq!(
                ours[0].as_ref().unwrap(),
                theirs.as_ref().unwrap(),
                "{:?}",
                String::from_utf8_lossy(&bytes)
            );
        }
    }

    #[test]
    fn agrees_with_fenix_rpc_on_malformed_input() {
        for bytes in [
            b"\r\n{}".to_vec(),
            b"Content-Length: nope\r\n\r\n".to_vec(),
            b"Content-Length: 10\r\n\r\n{}".to_vec(),
            b"Content-Length: 2\r\n".to_vec(),
        ] {
            let (ours, theirs) = both(&bytes);
            assert!(ours[0].is_err(), "{:?}", String::from_utf8_lossy(&bytes));
            assert!(theirs.is_err(), "{:?}", String::from_utf8_lossy(&bytes));
        }
    }

    #[test]
    fn an_oversized_length_is_refused_before_allocating() {
        let bytes = format!("Content-Length: {}\r\n\r\n", usize::MAX);
        let err = read_frame(&mut Cursor::new(bytes)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let bytes = format!("Content-Length: {}\r\n\r\n", MAX_BODY + 1);
        let err = read_frame(&mut Cursor::new(bytes)).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }

    #[test]
    fn exactly_the_limit_is_accepted() {
        let body = vec![b' '; MAX_BODY];
        let mut bytes = format!("Content-Length: {MAX_BODY}\r\n\r\n").into_bytes();
        bytes.extend(&body);
        assert_eq!(
            read_frame(&mut Cursor::new(bytes)).unwrap().unwrap().len(),
            MAX_BODY
        );
    }

    #[test]
    fn a_header_line_without_end_is_cut_off_at_the_limit() {
        let bytes = vec![b'X'; MAX_HEADER_LINE * 10];
        let err = read_frame(&mut Cursor::new(bytes)).unwrap_err();
        assert!(err.to_string().contains("longer than"), "{err}");
    }

    #[test]
    fn endless_headers_are_cut_off() {
        let bytes = b"X-Pad: 1\r\n".repeat(MAX_HEADERS + 5);
        let err = read_frame(&mut Cursor::new(bytes)).unwrap_err();
        assert!(err.to_string().contains("header lines"), "{err}");
    }

    #[test]
    fn eof_inside_a_header_is_an_error_not_a_clean_close() {
        let err = read_frame(&mut Cursor::new(b"Content-Len".to_vec())).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn writing_more_than_the_limit_is_refused() {
        let body = vec![b' '; MAX_BODY + 1];
        assert!(write_frame(&mut Vec::new(), &body).is_err());
    }
}
