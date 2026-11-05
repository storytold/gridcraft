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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use gridcraft_mcp::control_auth;
use gridcraft_ui_egui::ControlRequest;
use serde_json::{Value as Json, json};

const MAX_LINE: usize = 64 * 1024 * 1024;
/// How long a new connection may take to send its token, in total.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// How many connections may be waiting to authenticate at once. Anyone on the machine can
/// connect without the token, so past this a new connection is closed before anything is read:
/// each waiting one holds a thread and a socket.
const MAX_PENDING_AUTH: usize = 16;

/// How the server treats connections that haven't authenticated yet.
#[derive(Clone, Copy)]
struct AuthLimits {
    timeout: Duration,
    max_pending: usize,
}

const AUTH_LIMITS: AuthLimits = AuthLimits { timeout: AUTH_TIMEOUT, max_pending: MAX_PENDING_AUTH };

/// A connection that is still waiting to authenticate; dropping it frees its place.
struct Pending(Arc<AtomicUsize>);

impl Pending {
    /// A place among the `max` waiting connections, or `None` when they are all taken.
    fn claim(waiting: &Arc<AtomicUsize>, max: usize) -> Option<Pending> {
        waiting.try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < max).then_some(n + 1)).ok()?;
        Some(Pending(Arc::clone(waiting)))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

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
    serve_listener_with(listener, token, ctx, AUTH_LIMITS)
}

fn serve_listener_with(listener: TcpListener, token: Arc<str>, ctx: egui::Context, limits: AuthLimits) -> std::io::Result<Receiver<ControlRequest>> {
    let (tx, rx) = channel();
    let waiting = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                // Out of sockets or handles: wait for some to free instead of spinning on the error.
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };
            // Dropping the stream closes it before anything is read.
            let Some(place) = Pending::claim(&waiting, limits.max_pending) else { continue };
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = Arc::clone(&token);
            let _ =
                std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, &token, tx, ctx, limits.timeout, Some(place)));
        }
    })?;
    Ok(rx)
}

/// Read the connection's first line (at most [`control_auth::MAX_AUTH_LINE`] bytes, within
/// `timeout` in total) and check the token. `true` when the peer may go on.
#[allow(clippy::disallowed_methods)] // the desktop app (native only): wasm never runs it
fn authenticate(reader: &mut BufReader<TcpStream>, out: &mut TcpStream, token: &str, timeout: Duration) -> bool {
    let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
    let mut first = Vec::new();
    let read = read_auth_line(reader, &mut first, deadline);
    let verdict = match (read, std::str::from_utf8(&first)) {
        (Ok(()), Ok(line)) if line.ends_with('\n') => control_auth::check_first_line(line, token),
        _ => Err(json!({"ok": false, "error": "authentication required"})),
    };
    let allowed = verdict.is_ok();
    let reply = verdict.unwrap_or_else(|e| e);
    let _ = writeln!(out, "{reply}");
    let _ = reader.get_ref().set_read_timeout(None);
    allowed
}

/// Read the first line, at most [`control_auth::MAX_AUTH_LINE`] bytes, by `deadline`.
///
/// The timeout goes on the handle the reader reads: on Windows a `try_clone` of the stream keeps
/// its own, so one set on the writing half never fired (a silent peer held its thread forever).
/// It is shortened before every read, so a peer sending one byte at a time can't stretch it.
#[allow(clippy::disallowed_methods)] // the desktop app (native only): wasm never runs it
fn read_auth_line(reader: &mut BufReader<TcpStream>, line: &mut Vec<u8>, deadline: Instant) -> std::io::Result<()> {
    let max = control_auth::MAX_AUTH_LINE;
    while line.len() < max && line.last() != Some(&b'\n') {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        reader.get_ref().set_read_timeout(Some(left))?;
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        let room = max.saturating_sub(line.len());
        let take = buf.iter().take(room).position(|b| *b == b'\n').map_or(buf.len().min(room), |i| i + 1);
        line.extend(buf.iter().take(take));
        reader.consume(take);
    }
    Ok(())
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

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context, auth_timeout: Duration, place: Option<Pending>) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let allowed = authenticate(&mut reader, &mut out, token, auth_timeout);
    // Authenticated or refused, the connection no longer counts as waiting.
    drop(place);
    if !allowed {
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
                std::thread::spawn(move || serve(stream, TOKEN, tx, egui::Context::default(), AUTH_TIMEOUT, None));
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

    /// Whether the server closed `c` (end of stream, or a refusal and then end of stream) within
    /// `within`, and how long it took.
    #[allow(clippy::disallowed_methods)] // a native-only test of the desktop app
    fn closed_within(c: TcpStream, within: Duration) -> (bool, Duration) {
        let start = Instant::now();
        c.set_read_timeout(Some(within)).unwrap();
        let mut r = BufReader::new(c);
        let mut line = String::new();
        loop {
            line.clear();
            match r.read_line(&mut line) {
                Ok(0) => return (true, start.elapsed()),
                Ok(_) => continue,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return (false, start.elapsed()),
                Err(_) => return (true, start.elapsed()),
            }
        }
    }

    /// The token deadline is for the whole first line: neither a silent peer nor one sending a
    /// byte at a time keeps the connection (and its thread) past it.
    #[test]
    fn a_silent_or_slow_peer_is_closed_at_the_auth_deadline() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let limits = AuthLimits { timeout: Duration::from_millis(300), max_pending: MAX_PENDING_AUTH };
        let _rx = serve_listener_with(listener, TOKEN.into(), egui::Context::default(), limits).unwrap();

        let silent = TcpStream::connect(addr).unwrap();
        let mut slow = TcpStream::connect(addr).unwrap();
        let mut drip = slow.try_clone().unwrap();
        std::thread::spawn(move || {
            for b in control_auth::auth_line(1, TOKEN).bytes() {
                if drip.write_all(&[b]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let (closed, took) = closed_within(silent, Duration::from_secs(5));
        assert!(closed && took < Duration::from_secs(3), "silent peer kept for {took:?}");
        let (closed, took) = closed_within(slow.try_clone().unwrap(), Duration::from_secs(5));
        assert!(closed && took < Duration::from_secs(3), "byte-at-a-time peer kept for {took:?}");
        let _ = slow.flush();
    }

    /// Peers without the token can't pile up threads: past the cap a new connection is closed at
    /// once, and a waiting one that authenticates frees its place.
    #[test]
    fn connections_waiting_to_authenticate_are_capped() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let limits = AuthLimits { timeout: Duration::from_secs(20), max_pending: 2 };
        let _rx = serve_listener_with(listener, TOKEN.into(), egui::Context::default(), limits).unwrap();

        let mut first = TcpStream::connect(addr).unwrap();
        let _second = TcpStream::connect(addr).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let (closed, took) = closed_within(TcpStream::connect(addr).unwrap(), Duration::from_secs(5));
        assert!(closed && took < Duration::from_secs(3), "a third waiting connection was kept");

        first.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        first.write_all(format!("{}\n", control_auth::auth_line(1, TOKEN)).as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(first.try_clone().unwrap()).read_line(&mut reply).unwrap();
        assert_eq!(serde_json::from_str::<Json>(&reply).unwrap()["ok"], json!(true), "{reply}");
        std::thread::sleep(Duration::from_millis(200));
        let mut next = TcpStream::connect(addr).unwrap();
        next.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        next.write_all(format!("{}\n", control_auth::auth_line(1, TOKEN)).as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(next).read_line(&mut reply).unwrap();
        assert_eq!(serde_json::from_str::<Json>(&reply).unwrap()["ok"], json!(true), "a freed place was not reused: {reply}");
    }

    #[test]
    fn a_chosen_token_must_be_long_enough() {
        assert!(make_token(1, Some("short".into())).is_err());
        assert_eq!(make_token(1, Some(format!(" {TOKEN} "))).unwrap(), TOKEN);
    }
}
