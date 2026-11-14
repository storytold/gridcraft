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
/// Longest reply line accepted from the app (guards against a runaway peer).
const MAX_REPLY: u64 = 256 * 1024 * 1024;

/// A running GridCraft app, reached through its loopback control port.
pub struct Remote {
    addr: String,
    /// The token the caller gave; `None` looks it up for every new connection (see `connect`).
    token: Option<String>,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    /// Whether a connection ever succeeded (`lazy` starts without one; the app may open later).
    connected: bool,
}

impl Remote {
    /// Connect to `addr` (`127.0.0.1:7979`), failing fast when nothing is listening. The app
    /// requires a token (#777): `GRIDCRAFT_CONTROL_TOKEN`, else the file the app wrote for this port.
    /// It is looked up again for every new connection, so a restarted app's new token is picked up.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self::lazy(addr);
        r.reconnect()?;
        Ok(r)
    }

    /// Like [`Remote::connect`] with a token the caller already has.
    pub fn connect_with_token(addr: &str, token: String) -> std::io::Result<Self> {
        let mut r = Self { token: Some(token), ..Self::lazy(addr) };
        r.reconnect()?;
        Ok(r)
    }

    /// A remote that connects (and finds the app's token) on first use. The app may start after
    /// the MCP server, and a server that exits at once reads as failed for the client's whole
    /// session.
    pub fn lazy(addr: &str) -> Self {
        Self { addr: addr.to_string(), token: None, conn: None, next_id: 1, connected: false }
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// The token for a new connection: the caller's, else the one the app wrote at its last start.
    fn token(&self) -> std::io::Result<String> {
        match &self.token {
            Some(t) => Ok(t.clone()),
            None => crate::control_auth::client_token(&self.addr).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e)),
        }
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            // The token goes over plain TCP, so only to this machine: a network peer could read it,
            // and the app's control port listens on loopback only.
            if !sa.ip().is_loopback() {
                last = std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("{sa} is not a loopback address: the control token is sent only to 127.0.0.1 or ::1"),
                );
                continue;
            }
            match TcpStream::connect_timeout(&sa, Duration::from_secs(2)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(IO_TIMEOUT)).ok();
                    s.set_write_timeout(Some(IO_TIMEOUT)).ok();
                    let read = s.try_clone()?;
                    let mut conn = (BufReader::new(read), s);
                    // Read only once something listens: a closed app's token file may be stale.
                    Self::authenticate(&mut conn, &self.token()?)?;
                    self.conn = Some(conn);
                    self.connected = true;
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

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"));
        };
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
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        // One retry with a fresh connection (the app may have restarted).
        let reply = match self.roundtrip(&line) {
            Ok(r) => r,
            Err(_) => {
                self.conn = None;
                self.roundtrip(&line).map_err(|e| {
                    self.conn = None;
                    let retry = if self.connected { "" } else { "; will retry on each call" };
                    format!("GridCraft app at {} is not reachable: {e}{retry}", self.addr)
                })?
            }
        };
        let v: Value = serde_json::from_str(reply.trim()).map_err(|e| format!("bad reply from app: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("unknown error").to_string())
        }
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        if self.connected {
            format!("connected to the GridCraft app at {}", self.addr)
        } else {
            format!("will connect to the GridCraft app at {} when it opens", self.addr)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::net::TcpListener;

    const TOKEN: &str = "secret-token-0123456789";

    /// A client that starts before the app keeps its server, and the server connects (sending the
    /// token) once the app's port opens.
    #[test]
    fn a_lazy_remote_serves_once_the_app_opens() {
        // Reserve a free port, then close it: nothing listens there yet.
        let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        let mut r = Remote { token: Some(TOKEN.into()), ..Remote::lazy(&addr) };
        assert!(r.describe().contains("will connect to"), "{}", r.describe());
        let e = r.call("document.inspect", json!({})).unwrap_err();
        assert!(e.contains("not reachable") && e.contains("will retry on each call"), "{e}");
        // The app opens on that port: the next call authenticates and is answered.
        let listener = TcpListener::bind(&addr).unwrap();
        let app = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut w = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let hello = crate::control_auth::check_first_line(&first, TOKEN).unwrap();
            w.write_all(format!("{hello}\n").as_bytes()).unwrap();
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            w.write_all(format!("{}\n", json!({"id": req["id"], "ok": true, "result": 7})).as_bytes()).unwrap();
        });
        assert_eq!(r.call("document.inspect", json!({})), Ok(json!(7)));
        app.join().unwrap();
        assert!(r.describe().contains("connected to"), "{}", r.describe());
    }
}
