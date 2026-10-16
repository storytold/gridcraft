//! Localhost JSON-lines control server. Each connection may send many requests; each line
//! `{"id":…, "method":…, "params":…}` gets one reply line `{"id":…, "ok":…, …}`.

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
    let (tx, rx) = channel();
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, ctx, token, agents) = (tx.clone(), ctx.clone(), Arc::clone(&token), agents.clone());
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, &token, tx, ctx, &agents));
        }
    })?;
    log::info!("control channel listening on 127.0.0.1:{port}");
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

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context, agents: &AgentHub) {
    let (Ok(read), Ok(probe)) = (stream.try_clone(), stream.try_clone()) else { return };
    let (mut out, mut reader) = (stream, BufReader::new(read));
    if !authenticate(&mut reader, &mut out, token) {
        return;
    }
    let conn = next_connection();
    let _leaving = Leaving(agents, conn);
    let mut request: Option<u64> = None;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_LINE {
            let _ = writeln!(out, "{}", json!({"ok": false, "error": "request too large"}));
            continue;
        }
        let msg: Json = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": format!("invalid JSON: {e}")}));
                break;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Json::Null);
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("").to_string();
        let mut params = msg.get("params").cloned().unwrap_or(json!({}));

        // Ask Agent: answered here, on the connection's own thread.
        if let Some(mut reply) = agents.handle(conn, &method, &params, || peer_open(&probe)) {
            match (method.as_str(), reply["result"]["id"].as_u64()) {
                ("agent.listen", Some(taken)) => request = Some(taken),
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
            let _ = writeln!(out, "{}", json!({"id": id, "ok": false, "error": "control params must be an object", "errorCode": "INVALID_PARAMS"}));
            break;
        }

        let command = if matches!(method.as_str(), "engine.execute" | "command" | "ui.menu.invoke") {
            params.get("command").or_else(|| params.get("id")).and_then(Json::as_str).unwrap_or("")
        } else {
            method.as_str()
        };

        let request_id =
            if command == "ui.askAgent" || matches!(method.as_str(), "ui.inspect" | "ui.screenshot" | "engine.commands" | "engine.journal") {
                None
            } else {
                params.get("request").and_then(Json::as_u64).or(request)
            };

        let is_write = (gridcraft_engine::find_command(command).is_some() && !gridcraft_ui_egui::ask_agent::is_query_method(command))
            || matches!(command, "edit.undo" | "edit.redo" | "automate.runScript")
            || command.starts_with("file.")
            || method.starts_with("app.");
        if is_write && let Err(e) = agents.may_edit(conn, request_id) {
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
        ctx.request_repaint();
        let mut reply = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| json!({"ok": false, "error": "timed out waiting for the UI"}));
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
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::disallowed_methods)]
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

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
                std::thread::spawn(move || serve(s, TOKEN, tx, egui::Context::default(), &hub));
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
