//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};

/// Something that answers control-channel methods (`engine.execute`, `engine.commands`,
/// `document.inspect`, `sheet.read`, `ui.screenshot`, …).
pub trait Backend {
    /// Call one method. `Ok` carries the `result`, `Err` the error message.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.screenshot` and dialogs work).
    fn has_ui(&self) -> bool;
    /// Short human description ("headless", "connected to 127.0.0.1:7979").
    fn describe(&self) -> String;
}

/// Read/write timeout on the control connection.
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for the reply to `method`: `agent.listen` waits up to its `timeoutMs` (at most
/// 60 s in the app) before it answers, so its read timeout is that plus [`IO_TIMEOUT`].
pub(crate) fn read_timeout(method: &str, params: &Value) -> Duration {
    if method != "agent.listen" {
        return IO_TIMEOUT;
    }
    let ms = params.get("timeoutMs").and_then(Value::as_f64).filter(|ms| ms.is_finite() && *ms >= 0.0).unwrap_or(30_000.0).min(60_000.0);
    IO_TIMEOUT + Duration::from_millis(ms as u64)
}
/// Longest reply line accepted from the app (guards against a runaway peer).
const MAX_REPLY: u64 = 256 * 1024 * 1024;

/// A running GridCraft app, reached through its loopback control port.
pub struct Remote {
    addr: String,
    token: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    request: Option<u64>,
}

impl Remote {
    /// Connect to `addr` (`127.0.0.1:7979`), failing fast when nothing is listening. The app
    /// requires a token (#777): `GRIDCRAFT_CONTROL_TOKEN`, else the file the app wrote for this port.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let token = crate::control_auth::client_token(addr).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
        Self::connect_with_token(addr, token)
    }

    /// Like [`Remote::connect`] with a token the caller already has.
    pub fn connect_with_token(addr: &str, token: String) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), token, conn: None, next_id: 1, request: None };
        r.reconnect()?;
        Ok(r)
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_secs(2)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(IO_TIMEOUT)).ok();
                    s.set_write_timeout(Some(IO_TIMEOUT)).ok();
                    let read = s.try_clone()?;
                    let mut conn = (BufReader::new(read), s);
                    Self::authenticate(&mut conn, &self.token)?;
                    self.conn = Some(conn);
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// The handshake every connection starts with: the app closes one that doesn't send the token first.
    fn authenticate(conn: &mut (BufReader<TcpStream>, TcpStream), token: &str) -> std::io::Result<()> {
        let (reader, writer) = conn;
        writer.write_all(crate::control_auth::auth_line(0, token).as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        let n = std::io::Read::take(reader, 64 * 1024).read_line(&mut reply)?;
        let ok = n > 0 && serde_json::from_str::<Value>(reply.trim()).ok().and_then(|v| v.get("ok").and_then(Value::as_bool)) == Some(true);
        if ok {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the app refused the control token (is GRIDCRAFT_CONTROL_TOKEN or the app's token file for this port out of date?)",
            ))
        }
    }

    fn roundtrip(&mut self, line: &str, wait: Duration) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"));
        };
        writer.set_read_timeout(Some(wait)).ok();
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        let n = std::io::Read::take(reader, MAX_REPLY).read_line(&mut reply)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "control channel closed"));
        }
        Ok(reply)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, mut params: Value) -> Result<Value, String> {
        let bound = self.request;
        if let Some(request) = bound
            && (matches!(method, "engine.execute" | "command" | "ui.menu.invoke") || gridcraft_engine::find_command(method).is_some())
            && let Some(p) = params.as_object_mut()
        {
            p.entry("request").or_insert(json!(request));
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let wait = read_timeout(method, &params);
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        // One retry with a fresh connection (the app may have restarted).
        let reply = match self.roundtrip(&line, wait) {
            Ok(r) => r,
            Err(e) if bound.is_some() => {
                // A request-bound write may already have run. Never replay it on a fresh
                // connection (which does not own that request).
                self.conn = None;
                return Err(format!("Ask Agent connection lost; the command was not retried: {e}. Reconnect and ask again."));
            }
            Err(_) => {
                self.conn = None;
                self.roundtrip(&line, wait).map_err(|e| {
                    self.conn = None;
                    format!("GridCraft app at {} is not reachable: {e}", self.addr)
                })?
            }
        };
        let v: Value = serde_json::from_str(reply.trim()).map_err(|e| format!("bad reply from app: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            let result = v.get("result").cloned().unwrap_or(Value::Null);
            if method == "agent.listen" {
                if let Some(request) = result.get("id").and_then(Value::as_u64) {
                    self.request = Some(request);
                }
            } else if method == "agent.reply"
                && (result["done"] == true || result["cancelled"] == true)
                && result.get("id").and_then(Value::as_u64) == self.request
            {
                self.request = None;
            }
            Ok(result)
        } else {
            Err(format!(
                "{}: {}",
                v.get("errorCode").and_then(Value::as_str).unwrap_or("APPLY_FAILED"),
                v.get("error").and_then(Value::as_str).unwrap_or("unknown error")
            ))
        }
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("connected to the GridCraft app at {}", self.addr)
    }
}
