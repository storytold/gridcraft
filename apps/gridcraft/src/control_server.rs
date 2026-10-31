//! Localhost JSON-lines control server. Each connection may send many requests; each line
//! `{"id":…, "method":…, "params":…}` gets one reply line `{"id":…, "ok":…, …}`.
//!
//! The first line of a connection must authenticate with the app's token (#777; see
//! `gridcraft_mcp::control_auth`). A connection that doesn't, or later sends a line that isn't
//! JSON, is answered once and closed.
//!
//! The server only reads requests: a line that is not a JSON object with a string `method` (an
//! HTTP request line, a stray `nc`, binary data) gets one error reply and the connection is
//! closed, so nothing sent after it runs.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gridcraft_mcp::control_auth;
use gridcraft_ui_egui::ControlRequest;
use serde_json::{Value as Json, json};

const MAX_LINE: usize = 64 * 1024 * 1024;
/// How long a new connection may take to send its token.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// The secret of this run: the one the user chose (`--control-token`, `GRIDCRAFT_CONTROL_TOKEN`),
/// else 256 random bits from the operating system, written to a file only the user can read.
fn make_token(port: u16, chosen: Option<String>) -> std::io::Result<String> {
    if let Some(t) = chosen.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        if t.len() < control_auth::MIN_TOKEN_LEN {
            return Err(std::io::Error::other(format!("the control token must have at least {} characters", control_auth::MIN_TOKEN_LEN)));
        }
        eprintln!("gridcraft: control channel on 127.0.0.1:{port} requires the token you set (clients: GRIDCRAFT_CONTROL_TOKEN)");
        return Ok(t);
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random numbers for the control token: {e}")))?;
    let token = control_auth::hex(&bytes);
    let path = control_auth::write_token_file(port, &token)?;
    eprintln!("gridcraft: control channel on 127.0.0.1:{port}; its token is in {} (gridcraft-cli reads it)", path.display());
    Ok(token)
}

pub fn start(port: u16, chosen_token: Option<String>, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let token: Arc<str> = make_token(port, chosen_token)?.into();
    let rx = serve_listener(listener, token, ctx)?;
    log::info!("control channel listening on 127.0.0.1:{port}");
    Ok(rx)
}

/// Accept connections on `listener`; requests arrive on the returned receiver.
fn serve_listener(listener: TcpListener, token: Arc<str>, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    let (tx, rx) = channel();
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = Arc::clone(&token);
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, &token, tx, ctx));
        }
    })?;
    Ok(rx)
}

/// Read the connection's first line (at most [`control_auth::MAX_AUTH_LINE`] bytes, within
/// [`AUTH_TIMEOUT`]) and check the token. `true` when the peer may go on.
fn authenticate(reader: &mut BufReader<TcpStream>, out: &mut TcpStream, token: &str) -> bool {
    let _ = out.set_read_timeout(Some(AUTH_TIMEOUT));
    let mut first = String::new();
    let read = reader.by_ref().take(control_auth::MAX_AUTH_LINE as u64).read_line(&mut first);
    let verdict = match read {
        Ok(n) if n > 0 && first.ends_with('\n') => control_auth::check_first_line(&first, token),
        _ => Err(json!({"ok": false, "error": "authentication required"})),
    };
    let allowed = verdict.is_ok();
    let reply = verdict.unwrap_or_else(|e| e);
    let _ = writeln!(out, "{reply}");
    let _ = out.set_read_timeout(None);
    allowed
}

/// Read one `\n`-terminated line of at most `max` bytes into `buf` (newline stripped).
/// `Ok(false)` at end of stream; `Err` when the line is too long or the read fails.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> Result<bool, String> {
    buf.clear();
    // `take` bounds the bytes buffered for one line: a peer can't make us allocate more than `max`.
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let n = reader.take(limit).read_until(b'\n', buf).map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(true);
    }
    if buf.len() > max {
        return Err(format!("request too large (over {max} bytes); closing the connection"));
    }
    // Last line without a newline, then end of stream.
    Ok(true)
}

/// One request line, parsed.
enum Line {
    Request {
        id: Json,
        method: String,
        params: Json,
    },
    Blank,
    /// Not a request: reply with this error and close the connection.
    Reject(String),
}

fn parse_line(bytes: &[u8]) -> Line {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Line::Reject("not a control request (invalid UTF-8); closing the connection".into());
    };
    let text = text.trim();
    if text.is_empty() {
        return Line::Blank;
    }
    if is_http_request_line(text) {
        return Line::Reject("HTTP is not served here: this port takes JSON lines; closing the connection".into());
    }
    let msg = match serde_json::from_str::<Json>(text) {
        Ok(v) => v,
        Err(e) => return Line::Reject(format!("invalid JSON ({e}); expected one JSON request object per line; closing the connection")),
    };
    let Some(obj) = msg.as_object() else {
        return Line::Reject("not a control request (expected a JSON object); closing the connection".into());
    };
    let Some(method) = obj.get("method").and_then(Json::as_str) else {
        return Line::Reject("not a control request (missing string `method`); closing the connection".into());
    };
    Line::Request {
        id: obj.get("id").cloned().unwrap_or(Json::Null),
        method: method.to_string(),
        params: obj.get("params").cloned().unwrap_or(json!({})),
    }
}

/// An HTTP/1 request line (`POST / HTTP/1.1`) or the HTTP/2 preface (`PRI * HTTP/2.0`).
fn is_http_request_line(text: &str) -> bool {
    const METHODS: [&str; 10] = ["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH", "PRI"];
    text.split_once(' ').is_some_and(|(method, _)| METHODS.contains(&method))
        || text.rsplit_once(' ').is_some_and(|(_, version)| version.starts_with("HTTP/"))
}

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    if !authenticate(&mut reader, &mut out, token) {
        return;
    }
    let mut buf = Vec::new();
    loop {
        let reject = match read_bounded_line(&mut reader, &mut buf, MAX_LINE) {
            Ok(false) => break,
            // The peer is gone before sending a line: nobody to tell.
            Err(_) if buf.is_empty() => break,
            Err(e) => e,
            Ok(true) => match parse_line(&buf) {
                Line::Blank => continue,
                Line::Reject(e) => e,
                Line::Request { id, method, params } => {
                    let (req, rx) = ControlRequest::new(method, params);
                    let state = std::sync::Arc::clone(&req.state);
                    if tx.send(req).is_err() {
                        break;
                    }
                    // Wake the UI thread: egui only runs frames when something happens.
                    ctx.request_repaint();
                    let mut reply = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| {
                        // Give up on the request as well as on its reply. While it is still
                        // queued this keeps it from running late -- the client was told it had
                        // failed, and an agent that retried after the error ran it twice (#1914);
                        // if the app had already started it, its outcome is unknown instead.
                        if ControlRequest::abandon(&state) {
                            json!({"ok": false, "error": "timed out waiting for the UI (is the window minimised?)"})
                        } else {
                            json!({"ok": false, "error": "timed out waiting for the UI, which had already started the request, so it may still finish; its outcome is unknown"})
                        }
                    });
                    if let Some(o) = reply.as_object_mut() {
                        o.insert("id".into(), id);
                    }
                    if writeln!(out, "{reply}").is_err() {
                        break;
                    }
                    continue;
                }
            },
        };
        // The connection closes either way; a peer that can't read the reason loses nothing.
        let _ = writeln!(out, "{}", json!({"ok": false, "error": reject}));
        break;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// Authenticate a new connection, send `input`, and read reply lines (after the
    /// authentication reply) until the server closes it.
    fn exchange(addr: std::net::SocketAddr, input: &[u8]) -> Vec<Json> {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(format!("{}\n", control_auth::auth_line(1, TOKEN)).as_bytes()).unwrap();
        c.write_all(input).unwrap();
        // A read timeout ends the list too: the server kept the connection open.
        let mut replies: Vec<Json> = BufReader::new(c).lines().map_while(Result::ok).map(|l| serde_json::from_str(&l).unwrap()).collect();
        assert_eq!(replies.first().map(|r| r["ok"].clone()), Some(json!(true)), "{replies:?}");
        replies.remove(0);
        replies
    }

    fn serve_on_loopback() -> (std::net::SocketAddr, Receiver<ControlRequest>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        (addr, serve_listener(listener, TOKEN.into(), egui::Context::default()).unwrap())
    }

    /// A browser can POST to the port: its request line closes the connection before the JSON
    /// line in its body is read, so nothing is dispatched.
    #[test]
    fn an_http_request_closes_the_connection_before_its_body_is_dispatched() {
        let (addr, rx) = serve_on_loopback();
        let http = b"POST / HTTP/1.1\r\nHost: 127.0.0.1:7979\r\nContent-Type: text/plain\r\n\r\n\
            {\"id\":1,\"method\":\"engine.execute\",\"params\":{\"command\":\"cell.set\",\"params\":{\"cell\":\"A1\",\"input\":\"x\"}}}\n";
        let replies = exchange(addr, http);
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert!(replies[0]["error"].as_str().unwrap().contains("closing the connection"), "{replies:?}");
        assert!(rx.try_recv().is_err(), "the body was dispatched");
    }

    #[test]
    fn a_line_that_is_not_a_request_closes_the_connection() {
        let (addr, rx) = serve_on_loopback();
        std::thread::spawn(move || {
            for req in rx {
                let _ = req.reply.send(json!({"ok": true}));
            }
        });
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(format!("{}\n", control_auth::auth_line(1, TOKEN)).as_bytes()).unwrap();
        c.write_all(b"{\"id\":1,\"method\":\"a\"}\r\n\n{\"id\":2,\"method\":\"b\"}\n").unwrap();
        let mut r = BufReader::new(c);
        let mut auth_reply = String::new();
        r.read_line(&mut auth_reply).unwrap();
        for id in [1, 2] {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            assert_eq!(serde_json::from_str::<Json>(&line).unwrap()["id"], id, "{line}");
        }
        for junk in [&b"hello\n"[..], b"[1,2]\n", b"{\"id\":1}\n", b"{\"method\":5}\n", b"\xff\xfe\n", b"GET /x HTTP/1.1\n"] {
            let mut input = junk.to_vec();
            input.extend_from_slice(b"{\"id\":2,\"method\":\"b\"}\n");
            let replies = exchange(addr, &input);
            assert_eq!(replies.len(), 1, "{replies:?}");
            assert_eq!(replies[0]["ok"], false);
        }
        assert!(is_http_request_line("OPTIONS * HTTP/1.1"));
        assert!(is_http_request_line("PRI * HTTP/2.0"));
        assert!(!is_http_request_line("{\"method\":\"GET /\"}"));
    }

    #[test]
    fn an_overlong_line_is_refused_without_reading_the_rest() {
        let mut buf = Vec::new();
        let long = [b'x'; 100];
        let mut input: &[u8] = &long;
        assert!(read_bounded_line(&mut input, &mut buf, 10).is_err());
        assert_eq!(input.len(), 100 - 11, "the rest of the line is left unread");
    }

    /// A server on a loopback port with a stand-in UI that echoes each method.
    fn spawn() -> std::net::SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        std::thread::spawn(move || {
            for req in rx {
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, TOKEN, tx, egui::Context::default()));
            }
        });
        addr
    }

    fn lines(addr: std::net::SocketAddr, send: &str) -> Vec<Json> {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(send.as_bytes()).unwrap();
        // Replies up to the server closing the connection.
        BufReader::new(c).lines().map_while(Result::ok).map(|l| serde_json::from_str(&l).unwrap()).collect()
    }

    #[test]
    fn the_token_opens_the_connection() {
        let addr = spawn();
        let auth = control_auth::auth_line(1, TOKEN);
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(format!("{auth}\n{{\"id\":2,\"method\":\"ping\"}}\n").as_bytes()).unwrap();
        let mut r = BufReader::new(c);
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Json>(&line).unwrap()["ok"], json!(true));
        line.clear();
        r.read_line(&mut line).unwrap();
        let v: Json = serde_json::from_str(&line).unwrap();
        assert_eq!((v["id"].clone(), v["result"].clone()), (json!(2), json!("ping")));
    }

    #[test]
    fn anything_but_the_token_first_closes_the_connection() {
        let addr = spawn();
        let wrong = control_auth::auth_line(1, "ffffffffffffffffffffffffffffffff");
        for first in [wrong.as_str(), "{\"id\":1,\"method\":\"app.save\",\"params\":{\"path\":\"/tmp/x\"}}", "POST / HTTP/1.1", ""] {
            // The second request is never served: one refusal, then end of stream.
            let got = lines(addr, &format!("{first}\n{{\"id\":2,\"method\":\"ping\"}}\n"));
            assert_eq!(got.len(), 1, "{first}: {got:?}");
            assert_eq!(got[0]["ok"], json!(false), "{first}");
        }
    }

    #[test]
    fn an_unparsable_line_after_auth_closes_the_connection() {
        let addr = spawn();
        let auth = control_auth::auth_line(1, TOKEN);
        let got = lines(addr, &format!("{auth}\nnot json\n{{\"id\":3,\"method\":\"ping\"}}\n"));
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[1]["ok"], json!(false));
    }

    #[test]
    fn a_chosen_token_must_be_long_enough() {
        assert!(make_token(1, Some("short".into())).is_err());
        assert_eq!(make_token(1, Some(format!(" {TOKEN} "))).unwrap(), TOKEN);
    }
}
