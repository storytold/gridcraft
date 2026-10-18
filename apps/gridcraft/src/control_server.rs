//! Localhost JSON-lines control server. Each connection may send many requests; each line
//! `{"id":…, "method":…, "params":…}` gets one reply line `{"id":…, "ok":…, …}`.
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
use gridcraft_ui_egui::ask_agent::AgentHub;
use serde_json::{Value as Json, json};

const MAX_LINE: usize = 64 * 1024 * 1024;
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

fn make_token(port: u16, chosen: Option<String>) -> std::io::Result<String> {
    if let Some(t) = chosen.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        if t.len() < control_auth::MIN_TOKEN_LEN {
            return Err(std::io::Error::other(format!("the control token must have at least {} characters", control_auth::MIN_TOKEN_LEN)));
        }
        eprintln!("gridcraft: control channel on 127.0.0.1:{port} requires the token you set (clients: GRIDCRAFT_CONTROL_TOKEN)");
        return Ok(t);
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random numbers: {e}")))?;
    let (token, path) = (control_auth::hex(&bytes), control_auth::write_token_file(port, &control_auth::hex(&bytes))?);
    eprintln!("gridcraft: control channel on 127.0.0.1:{port}; its token is in {} (gridcraft-cli reads it)", path.display());
    Ok(token)
}

pub fn start(port: u16, chosen_token: Option<String>, ctx: egui::Context, agents: AgentHub) -> std::io::Result<Receiver<ControlRequest>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let token: Arc<str> = make_token(port, chosen_token)?.into();
    let rx = serve_listener_with_auth(listener, ctx, Some(token), agents)?;
    log::info!("control channel listening on 127.0.0.1:{port}");
    Ok(rx)
}

/// Accept connections on `listener`; requests arrive on the returned receiver.
#[cfg(test)]
fn serve_listener(listener: TcpListener, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    serve_listener_with_auth(listener, ctx, None, AgentHub::default())
}

fn serve_listener_with_auth(
    listener: TcpListener,
    ctx: egui::Context,
    token: Option<Arc<str>>,
    agents: AgentHub,
) -> std::io::Result<Receiver<ControlRequest>> {
    let (tx, rx) = channel();
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, ctx, token, agents) = (tx.clone(), ctx.clone(), token.clone(), agents.clone());
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || {
                serve(stream, token.as_deref(), tx, ctx, &agents);
            });
        }
    })?;
    Ok(rx)
}

fn authenticate(reader: &mut BufReader<TcpStream>, out: &mut TcpStream, token: &str) -> bool {
    let _ = out.set_read_timeout(Some(AUTH_TIMEOUT));
    let mut first = String::new();
    let verdict = match reader.by_ref().take(control_auth::MAX_AUTH_LINE as u64).read_line(&mut first) {
        Ok(n) if n > 0 && first.ends_with('\n') => control_auth::check_first_line(&first, token),
        _ => Err(json!({"ok": false, "error": "authentication required"})),
    };
    let _ = writeln!(out, "{}", verdict.as_ref().unwrap_or_else(|e| e));
    let _ = out.set_read_timeout(None);
    verdict.is_ok()
}

fn peer_open(s: &TcpStream) -> bool {
    if s.set_nonblocking(true).is_err() {
        return true;
    }
    let mut b = [0u8; 1];
    let r = s.peek(&mut b);
    let _ = s.set_nonblocking(false);
    matches!(r, Ok(n) if n > 0) || matches!(r, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
}

/// A number for each control connection, for Ask Agent's ownership rule.
fn next_connection() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

struct Leaving<'a>(&'a AgentHub, u64);
impl Drop for Leaving<'_> {
    fn drop(&mut self) {
        self.0.disconnect(self.1);
    }
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

fn serve(stream: TcpStream, token: Option<&str>, tx: Sender<ControlRequest>, ctx: egui::Context, agents: &AgentHub) {
    let (Ok(read), Ok(probe)) = (stream.try_clone(), stream.try_clone()) else { return };
    let (mut out, mut reader) = (stream, BufReader::new(read));
    if let Some(token) = token
        && !authenticate(&mut reader, &mut out, token)
    {
        return;
    }
    let conn = next_connection();
    let _leaving = Leaving(agents, conn);
    let mut request: Option<u64> = None;
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
                Line::Request { id, method, mut params } => {
                    // Ask Agent: answered here, on the connection's own thread.
                    if let Some(mut reply) = agents.handle(conn, &method, &params, || peer_open(&probe)) {
                        match (method.as_str(), reply["result"]["id"].as_u64()) {
                            ("agent.listen", taken) => request = taken,
                            ("agent.reply", Some(done)) if reply["result"]["done"] == true && Some(done) == request => request = None,
                            _ => {}
                        }
                        reply["id"] = id;
                        ctx.request_repaint();
                        if writeln!(out, "{reply}").is_err() {
                            break;
                        }
                        continue;
                    }

                    if params.is_null() {
                        params = json!({});
                    }
                    if !params.is_object() {
                        let _ = writeln!(
                            out,
                            "{}",
                            json!({"id": id, "ok": false, "error": "control params must be an object", "errorCode": "INVALID_PARAMS"})
                        );
                        break;
                    }

                    let command = if matches!(method.as_str(), "engine.execute" | "command" | "ui.menu.invoke") {
                        params.get("command").or_else(|| params.get("id")).and_then(Json::as_str).unwrap_or("")
                    } else {
                        method.as_str()
                    };

                    let request_id = if command == "ui.askAgent"
                        || matches!(method.as_str(), "ui.inspect" | "ui.screenshot" | "engine.commands" | "engine.journal")
                    {
                        None
                    } else {
                        params.get("request").and_then(Json::as_u64).or(request)
                    };

                    let is_write = (gridcraft_engine::find_command(command).is_some() && !gridcraft_ui_egui::ask_agent::is_query_method(command))
                        || matches!(command, "edit.undo" | "edit.redo" | "automate.runScript")
                        || command.starts_with("file.")
                        || method.starts_with("app.");
                    if is_write && let Err(e) = agents.may_edit(conn, request_id) {
                        if matches!(e.code, "NO_REQUEST" | "NOT_OWNER" | "READ_ONLY_REQUEST") && request == request_id {
                            request = None;
                        }
                        let _ = writeln!(out, "{}", json!({"id": id, "ok": false, "error": e.message, "errorCode": e.code}));
                        continue;
                    }

                    if let Some(p) = params.as_object_mut() {
                        if let Some(request_id) = request_id {
                            p.insert("request".into(), json!(request_id));
                            p.insert("_agentConnection".into(), json!(conn));
                        }
                        p.insert("_externalAgent".into(), json!(true));
                    }

                    let (req, rx) = ControlRequest::new(method, params);
                    if tx.send(req).is_err() {
                        break;
                    }
                    // Wake the UI thread: egui only runs frames when something happens.
                    ctx.request_repaint();
                    let mut reply = rx
                        .recv_timeout(Duration::from_secs(60))
                        .unwrap_or_else(|_| json!({"ok": false, "error": "timed out waiting for the UI (is the window minimised?)"}));
                    if let Some(o) = reply.as_object_mut() {
                        o.insert("id".into(), id);
                        if o.get("ok") == Some(&Json::Bool(false)) && !o.contains_key("errorCode") {
                            let raw = o.get("error").and_then(Json::as_str).unwrap_or("operation failed").to_owned();
                            let (code, message) = raw.split_once(": ").unwrap_or(("APPLY_FAILED", &raw));
                            o.insert("errorCode".into(), json!(code));
                            o.insert("error".into(), json!(message));
                        }
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
    #![allow(clippy::unwrap_used, clippy::disallowed_methods)]
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// Send `input` on a new connection and read reply lines until the server closes it.
    fn exchange(addr: std::net::SocketAddr, input: &[u8]) -> Vec<Json> {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(input).unwrap();
        // A read timeout ends the list too: the server kept the connection open.
        BufReader::new(c).lines().map_while(Result::ok).map(|l| serde_json::from_str(&l).unwrap()).collect()
    }

    fn serve_on_loopback() -> (std::net::SocketAddr, Receiver<ControlRequest>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        (addr, serve_listener(listener, egui::Context::default()).unwrap())
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
        c.write_all(b"{\"id\":1,\"method\":\"a\"}\r\n\n{\"id\":2,\"method\":\"b\"}\n").unwrap();
        let mut r = BufReader::new(c);
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

    #[test]
    fn control_token_and_mcp_socket_request_grouped_undo() {
        assert!(make_token(1, Some("short".into())).is_err() && make_token(1, Some(format!(" {TOKEN} "))).unwrap() == TOKEN);

        let (hub, listener) = (AgentHub::default(), TcpListener::bind(("127.0.0.1", 0)).unwrap());
        let (addr, (tx, rx), (rtx, rrx)) = (listener.local_addr().unwrap(), channel::<ControlRequest>(), channel());
        let (ui_hub, server_hub) = (hub.clone(), hub.clone());
        std::thread::spawn(move || {
            let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), Default::default()).with_agent_hub(ui_hub);
            app.session.new_workbook();
            let c = next_connection();
            app.ask_agent.hub.as_ref().unwrap().listen(c, None, Duration::ZERO, || true);
            rtx.send(app.run("ui.askAgent", json!({"instruction":"test"})).unwrap()["id"].as_u64().unwrap()).unwrap();
            for r in rx {
                let _ = r.reply.send(match gridcraft_ui_egui::control::handle(&mut app, &egui::Context::default(), &r.method, &r.params) {
                    gridcraft_ui_egui::control::Outcome::Done(res) => res,
                    _ => json!({"ok": false}),
                });
            }
        });
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let (tx, hub) = (tx.clone(), server_hub.clone());
                std::thread::spawn(move || serve(s, Some(TOKEN), tx, egui::Context::default(), &hub));
            }
        });
        let req = rrx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut mcp = gridcraft_mcp::Server::new(Box::new(gridcraft_mcp::Remote::connect_with_token(&addr.to_string(), TOKEN.into()).unwrap()));
        let mut call = |name: &str, args: Json| -> Json {
            let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string();
            let reply: Json = serde_json::from_str(&mcp.handle_line(&line).unwrap()).unwrap();
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        assert_eq!(call("wait_for_request", json!({"timeout_s": 0}))["id"], req);
        call("reply_to_request", json!({"id": req, "text": "...", "done": false}));
        call("set_cell", json!({"cell": "A1", "input": "7"}));
        call("reply_to_request", json!({"id": req, "text": "Done"}));
        assert_eq!(call("get_cell", json!({"cell": "A1"}))["text"], "7");
        call("undo", json!({}));
        assert_eq!(call("get_cell", json!({"cell": "A1"}))["text"], "");
        call("redo", json!({}));
        assert_eq!(call("get_cell", json!({"cell": "A1"}))["text"], "7");
    }
}
