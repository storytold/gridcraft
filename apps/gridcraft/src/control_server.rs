//! Localhost JSON-lines control server. Each connection may send many requests; each line
//! `{"id":…, "method":…, "params":…}` gets one reply line `{"id":…, "ok":…, …}`.
//!
//! Loopback TCP is open to every process of every local user, so a connection is served only
//! after its first line, `{"auth": "<token>"}`, carries this run's token (#2174; see
//! `gridcraft_mcp::control_auth`). The token is `GRIDCRAFT_CONTROL_TOKEN`, else 256 random bits
//! written to a file only the user can read. A peer that sends anything else first, or nothing
//! within [`AUTH_TIMEOUT`], gets one refusal line and the connection is closed.
//!
//! At most [`MAX_CONNECTIONS`] connections are open at once (each has a thread and may buffer a
//! line of up to [`MAX_LINE`] bytes); one more is told so and closed.
//!
//! After the handshake the server only reads requests: a line that is not a JSON object with a
//! string `method` (an HTTP request line, a stray `nc`, binary data) gets one error reply and the
//! connection is closed, so nothing sent after it runs.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gridcraft_mcp::control_auth;
use gridcraft_ui_egui::ControlRequest;
use serde_json::{Value as Json, json};

/// Longest request line from an authenticated peer. Requests carry paths and cell values, not
/// file contents; 16 MiB still takes a large `range.setValues`.
const MAX_LINE: usize = 16 * 1024 * 1024;
/// Most connections served at once, authenticated or not.
const MAX_CONNECTIONS: usize = 8;
/// How long a new connection may take to send its token.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a reply may wait for a peer that stopped reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);
const TOO_MANY: &str = "too many control connections are open; closing the connection";

/// The knobs tests turn down.
#[derive(Clone, Copy)]
struct Limits {
    max_connections: usize,
    auth_timeout: Duration,
}

const LIMITS: Limits = Limits { max_connections: MAX_CONNECTIONS, auth_timeout: AUTH_TIMEOUT };

pub fn start(port: u16, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let token: Arc<str> = make_token(port, std::env::var(control_auth::TOKEN_ENV).ok())?.into();
    let rx = serve_listener(listener, token, LIMITS, ctx)?;
    log::info!("control channel listening on 127.0.0.1:{port}");
    Ok(rx)
}

/// This run's secret: the one in `chosen` (`GRIDCRAFT_CONTROL_TOKEN`) when set, else 256 random
/// bits from the operating system, written to [`control_auth::token_path`] for clients. The
/// token itself is never logged.
fn make_token(port: u16, chosen: Option<String>) -> std::io::Result<String> {
    if let Some(chosen) = chosen.filter(|t| !t.trim().is_empty()) {
        let token = control_auth::validate_chosen(&chosen).map_err(std::io::Error::other)?;
        log::info!("control channel on port {port} uses the token in {}", control_auth::TOKEN_ENV);
        return Ok(token);
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random numbers for the control token: {e}")))?;
    let token = control_auth::hex(&bytes);
    let path = control_auth::token_path(port)
        .ok_or_else(|| std::io::Error::other(format!("no settings folder for the control token; set {} instead", control_auth::TOKEN_ENV)))?;
    control_auth::write_token_file(&path, &token)
        .map_err(|e| std::io::Error::new(e.kind(), format!("cannot write the control token to {}: {e}", path.display())))?;
    log::info!("control channel token for port {port} is in {} (gridcraft-cli reads it)", path.display());
    Ok(token)
}

/// One open connection's place under [`Limits::max_connections`]; dropping it frees the place.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(open: &Arc<AtomicUsize>, max: usize) -> Option<Self> {
        let before = open.fetch_add(1, Ordering::SeqCst);
        let slot = Self(Arc::clone(open));
        // Over the cap: dropping the slot gives the place back.
        (before < max).then_some(slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Accept connections on `listener`; requests arrive on the returned receiver.
fn serve_listener(listener: TcpListener, token: Arc<str>, limits: Limits, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    let (tx, rx) = channel();
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                // Out of file descriptors and the like: don't spin while it lasts.
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            let Some(slot) = Slot::take(&open, limits.max_connections) else {
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                let _ = writeln!(stream, "{}", json!({"ok": false, "error": TOO_MANY}));
                continue;
            };
            let (tx, ctx, token) = (tx.clone(), ctx.clone(), Arc::clone(&token));
            // A failed spawn drops the closure, and with it the slot.
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || {
                let _slot = slot;
                serve(stream, &token, limits.auth_timeout, tx, ctx);
            });
        }
    })?;
    Ok(rx)
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

/// Reads the connection's first line (at most [`control_auth::MAX_AUTH_LINE`] bytes, within
/// `timeout`) and checks the token, replying either way. `true` when the peer may go on.
fn authenticate(reader: &mut BufReader<TcpStream>, out: &mut TcpStream, token: &str, timeout: Duration) -> bool {
    let _ = reader.get_ref().set_read_timeout(Some(timeout));
    let mut first = Vec::new();
    let welcome = match read_bounded_line(reader, &mut first, control_auth::MAX_AUTH_LINE) {
        Ok(true) => control_auth::check_auth_line(&first, token).ok(),
        _ => None,
    };
    let _ = reader.get_ref().set_read_timeout(None);
    match welcome {
        Some(reply) => writeln!(out, "{reply}").is_ok(),
        None => {
            let _ = writeln!(out, "{}", json!({"ok": false, "error": control_auth::REFUSED}));
            false
        }
    }
}

fn serve(stream: TcpStream, token: &str, auth_timeout: Duration, tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let _ = out.set_write_timeout(Some(WRITE_TIMEOUT));
    let mut reader = BufReader::new(read);
    if !authenticate(&mut reader, &mut out, token, auth_timeout) {
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    const TOKEN: &str = "unit-test-token-0123456789abcdef";

    fn connect(addr: std::net::SocketAddr) -> TcpStream {
        let c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c
    }

    /// Reply lines until the server closes the connection (or a read times out, i.e. it kept
    /// the connection open). A reset after a refusal also ends the list.
    fn replies(c: TcpStream) -> Vec<Json> {
        BufReader::new(c).lines().map_while(Result::ok).map(|l| serde_json::from_str(&l).unwrap()).collect()
    }

    /// Send `input` on a new connection without authenticating and collect the replies.
    fn exchange_raw(addr: std::net::SocketAddr, input: &[u8]) -> Vec<Json> {
        let mut c = connect(addr);
        // A refused peer may find the connection closed already; the replies tell.
        let _ = c.write_all(input);
        replies(c)
    }

    /// Authenticate a new connection, send `input`, and collect the replies after the welcome;
    /// `None` when there was no welcome.
    fn try_exchange(addr: std::net::SocketAddr, input: &[u8]) -> Option<Vec<Json>> {
        let mut all = control_auth::auth_line(TOKEN).into_bytes();
        all.push(b'\n');
        all.extend_from_slice(input);
        let mut got = exchange_raw(addr, &all);
        if got.first().map(|w| w["ok"].clone()) != Some(json!(true)) {
            return None;
        }
        got.remove(0);
        Some(got)
    }

    fn exchange(addr: std::net::SocketAddr, input: &[u8]) -> Vec<Json> {
        try_exchange(addr, input).expect("no welcome")
    }

    /// Retries `input` on authenticated connections until one is served (a freed place shows
    /// up once the server thread notices the close); its first reply.
    fn first_served(addr: std::net::SocketAddr, input: &[u8]) -> Option<Json> {
        for _ in 0..50 {
            if let Some(reply) = try_exchange(addr, input).and_then(|got| got.into_iter().next()).filter(|r| r["ok"] == true) {
                return Some(reply);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }

    /// An authenticated connection, its welcome read.
    fn session(addr: std::net::SocketAddr) -> BufReader<TcpStream> {
        let mut c = connect(addr);
        writeln!(c, "{}", control_auth::auth_line(TOKEN)).unwrap();
        let mut r = BufReader::new(c);
        let mut welcome = String::new();
        r.read_line(&mut welcome).unwrap();
        assert_eq!(serde_json::from_str::<Json>(&welcome).unwrap()["ok"], true, "{welcome}");
        r
    }

    /// One request on an open session; its reply.
    fn ask(r: &mut BufReader<TcpStream>, id: u64, method: &str) -> Json {
        writeln!(r.get_mut(), "{}", json!({"id": id, "method": method})).unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn serve_with(limits: Limits) -> (std::net::SocketAddr, Receiver<ControlRequest>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        (addr, serve_listener(listener, TOKEN.into(), limits, egui::Context::default()).unwrap())
    }

    fn serve_on_loopback() -> (std::net::SocketAddr, Receiver<ControlRequest>) {
        serve_with(LIMITS)
    }

    /// A stand-in UI thread that answers every request with its method name.
    fn echo(rx: Receiver<ControlRequest>) {
        std::thread::spawn(move || {
            for req in rx {
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
    }

    /// A browser can POST to the port: its request line closes the connection before the JSON
    /// line in its body is read, so nothing is dispatched (authenticated or not).
    #[test]
    fn an_http_request_closes_the_connection_before_its_body_is_dispatched() {
        let (addr, rx) = serve_on_loopback();
        let http = b"POST / HTTP/1.1\r\nHost: 127.0.0.1:7979\r\nContent-Type: text/plain\r\n\r\n\
            {\"id\":1,\"method\":\"engine.execute\",\"params\":{\"command\":\"cell.set\",\"params\":{\"cell\":\"A1\",\"input\":\"x\"}}}\n";
        for got in [exchange(addr, http), exchange_raw(addr, http)] {
            assert_eq!(got.len(), 1, "{got:?}");
            assert!(got[0]["error"].as_str().unwrap().contains("closing the connection"), "{got:?}");
        }
        assert!(rx.try_recv().is_err(), "the body was dispatched");
    }

    #[test]
    fn a_line_that_is_not_a_request_closes_the_connection() {
        let (addr, rx) = serve_on_loopback();
        echo(rx);
        let mut r = session(addr);
        r.get_mut().write_all(b"{\"id\":1,\"method\":\"a\"}\r\n\n{\"id\":2,\"method\":\"b\"}\n").unwrap();
        for id in [1, 2] {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            assert_eq!(serde_json::from_str::<Json>(&line).unwrap()["id"], id, "{line}");
        }
        for junk in [&b"hello\n"[..], b"[1,2]\n", b"{\"id\":1}\n", b"{\"method\":5}\n", b"\xff\xfe\n", b"GET /x HTTP/1.1\n"] {
            let mut input = junk.to_vec();
            input.extend_from_slice(b"{\"id\":2,\"method\":\"b\"}\n");
            let got = exchange(addr, &input);
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0]["ok"], false);
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

    /// #2174: a request (or a wrong token, or junk) as the first line is refused once, the
    /// connection is closed, and nothing reaches the UI, not even the request after it.
    #[test]
    fn an_unauthenticated_request_is_refused_and_never_dispatched() {
        let (addr, rx) = serve_on_loopback();
        let next = "{\"id\":2,\"method\":\"app.quit\"}\n";
        let near = format!("{}X", &TOKEN[..TOKEN.len() - 1]);
        for first in [
            "{\"id\":1,\"method\":\"app.save\",\"params\":{\"path\":\"stolen.xlsx\"}}".to_string(),
            "{\"id\":1,\"method\":\"engine.execute\",\"params\":{\"command\":\"cell.set\"},\"auth\":\"guess-0123456789\"}".to_string(),
            control_auth::auth_line(&near),
            control_auth::auth_line(""),
            format!("{{\"token\":\"{TOKEN}\"}}"),
            "GET / HTTP/1.1".to_string(),
            String::new(),
        ] {
            let got = exchange_raw(addr, format!("{first}\n{next}").as_bytes());
            assert_eq!(got.len(), 1, "{first}: {got:?}");
            assert_eq!(got[0]["ok"], false, "{first}");
            assert_eq!(got[0]["error"], control_auth::REFUSED, "{first}: the refusal must not say what was wrong");
        }
        // A first line over the handshake limit is refused without being buffered whole.
        let huge = format!("{{\"auth\":\"{}\"}}", "x".repeat(control_auth::MAX_AUTH_LINE));
        let got = exchange_raw(addr, huge.as_bytes());
        assert!(got.len() <= 1 && got.iter().all(|r| r["ok"] == false), "{got:?}");
        assert!(rx.try_recv().is_err(), "an unauthenticated request reached the UI");
    }

    #[test]
    fn an_authenticated_connection_is_served() {
        let (addr, rx) = serve_on_loopback();
        echo(rx);
        let mut r = session(addr);
        let reply = ask(&mut r, 2, "ui.inspect");
        assert_eq!((reply["id"].clone(), reply["ok"].clone(), reply["result"].clone()), (json!(2), json!(true), json!("ui.inspect")));
        assert_eq!(ask(&mut r, 3, "sheet.read")["result"], "sheet.read", "the connection stays open for more requests");
        // The handshake's own `id` comes back on the welcome.
        let got = exchange_raw(addr, format!("{{\"id\":0,\"auth\":\"{TOKEN}\"}}\n").as_bytes());
        assert_eq!(got.first().map(|w| (w["id"].clone(), w["ok"].clone())), Some((json!(0), json!(true))), "{got:?}");
    }

    /// A peer that connects and says nothing is dropped after the handshake timeout, freeing its place.
    #[test]
    fn a_silent_peer_is_dropped_after_the_auth_timeout() {
        let (addr, rx) = serve_with(Limits { max_connections: 1, auth_timeout: Duration::from_millis(200) });
        echo(rx);
        let got = replies(connect(addr));
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["error"], control_auth::REFUSED);
        let served = first_served(addr, b"{\"id\":5,\"method\":\"ping\"}\n");
        assert_eq!(served.map(|r| r["result"].clone()), Some(json!("ping")), "the silent peer's place was not freed");
    }

    /// #2174: no more than `max_connections` connections are open at once; one more is refused
    /// and closed without disturbing the others, and a closed one frees its place.
    #[test]
    fn the_connection_cap_holds() {
        let (addr, rx) = serve_with(Limits { max_connections: 3, auth_timeout: Duration::from_secs(5) });
        echo(rx);
        let mut held: Vec<BufReader<TcpStream>> = (0..3).map(|_| session(addr)).collect();
        for _ in 0..4 {
            let got = replies(connect(addr));
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0]["error"], TOO_MANY);
        }
        // The token doesn't get a peer past the cap either. (The server closes without reading
        // what was sent, so the peer may see a reset instead of the refusal line.)
        let got = exchange_raw(addr, format!("{}\n{{\"id\":1,\"method\":\"ping\"}}\n", control_auth::auth_line(TOKEN)).as_bytes());
        assert!(got.len() <= 1 && got.iter().all(|r| r["error"] == TOO_MANY), "{got:?}");
        for (i, r) in held.iter_mut().enumerate() {
            assert_eq!(ask(r, i as u64, "ping")["ok"], true, "connection {i} was disturbed");
        }
        drop(held.pop());
        let reopened = first_served(addr, b"{\"id\":9,\"method\":\"ping\"}\n");
        assert_eq!(reopened.map(|r| r["id"].clone()), Some(json!(9)), "a closed connection did not free its place");
    }

    #[test]
    fn a_chosen_token_must_be_long_enough() {
        assert!(make_token(1, Some("short".into())).is_err());
        assert_eq!(make_token(1, Some(format!("  {TOKEN}\n"))).unwrap(), TOKEN);
    }
}
