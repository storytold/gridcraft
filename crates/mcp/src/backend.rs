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
/// Longest reply payload accepted from the app, excluding its newline.
const MAX_REPLY: u64 = 256 * 1024 * 1024;

/// A running GridCraft app, reached through its loopback control port.
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    /// Connect to `addr` (`127.0.0.1:7979`), failing fast when nothing is listening.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1 };
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
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn prepare_connection(&mut self) -> std::io::Result<()> {
        if let Some((reader, stream)) = self.conn.as_ref() {
            if !reader.buffer().is_empty() {
                self.conn = None;
            } else {
                stream.set_nonblocking(true)?;
                let probe = stream.peek(&mut [0]);
                stream.set_nonblocking(false)?;
                match probe {
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    // EOF, stray bytes or a failed socket: reconnect before sending anything.
                    _ => self.conn = None,
                }
            }
        }
        if self.conn.is_none() {
            self.reconnect()?;
        }
        Ok(())
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        let Some((reader, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"));
        };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        read_reply(reader, MAX_REPLY)
    }
}

fn read_reply(reader: impl BufRead, max_bytes: u64) -> std::io::Result<String> {
    let mut reply = String::new();
    std::io::Read::take(reader, max_bytes + 1).read_line(&mut reply)?;
    if reply.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "control channel closed"));
    }
    if !reply.ends_with('\n') {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "control reply is oversized or missing its newline"));
    }
    Ok(reply)
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.prepare_connection().map_err(|e| {
            self.conn = None;
            format!("GridCraft app at {} is not reachable: {e}; `{method}`: nothing was sent", self.addr)
        })?;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        // Once transmission starts, even a failed write can have sent part of the request.
        let v: Value = self
            .roundtrip(&line)
            .and_then(|reply| serde_json::from_str(reply.trim()).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
            .map_err(|e| {
                self.conn = None;
                format!("GridCraft app at {}: `{method}` failed: {e}; operation may have completed; inspect state before retrying", self.addr)
            })?;
        if v.get("id").is_none()
            && v.get("ok").and_then(Value::as_bool) == Some(false)
            && let Some(error) = v.get("error").and_then(Value::as_str)
        {
            // The app refuses oversized/unparseable requests before dispatch, without an ID.
            self.conn = None;
            return Err(format!("GridCraft app at {} refused `{method}`: {error}; nothing was run", self.addr));
        }
        if v.get("id") != Some(&json!(id))
            || v.get("ok").and_then(Value::as_bool).is_none()
            || (v.get("ok").and_then(Value::as_bool) == Some(false) && v.get("error").and_then(Value::as_str).is_none())
        {
            self.conn = None;
            return Err(format!(
                "GridCraft app at {}: invalid or mismatched reply to `{method}` (expected id {id}); operation may have completed; inspect state before retrying",
                self.addr
            ));
        }
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
        format!("connected to the GridCraft app at {}", self.addr)
    }
}

#[cfg(test)]
mod transport_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn bounded_reply_requires_a_complete_frame() {
        assert_eq!(read_reply(&b"1234\nnext\n"[..], 4).unwrap(), "1234\n");
        assert_eq!(read_reply(&b"\n"[..], 4).unwrap(), "\n");
        for bytes in [&b""[..], &b"1234"[..], &b"12345\n"[..], &b"\xff\n"[..]] {
            assert!(read_reply(bytes, 4).is_err(), "{bytes:?}");
        }
    }

    #[test]
    fn late_reply_cannot_replay_a_mutation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&request).unwrap()["method"], "engine.execute");
            std::thread::sleep(Duration::from_millis(150));
            request.clear();
            assert_eq!(reader.read_line(&mut request).unwrap(), 0, "timed-out stream must close");
            let (mut fresh, _) = listener.accept().unwrap();
            fresh.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut reader = BufReader::new(fresh.try_clone().unwrap());
            reader.read_line(&mut request).unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "document.inspect");
            assert_eq!(request["id"], 2);
            writeln!(fresh, "{}", json!({"id":2,"ok":true,"result":null})).unwrap();
        });
        let mut remote = Remote::connect(&addr).unwrap();
        remote.conn.as_ref().unwrap().0.get_ref().set_read_timeout(Some(Duration::from_millis(30))).unwrap();
        let error = remote.call("engine.execute", json!({"command":"home.bold"})).unwrap_err();
        assert!(error.contains("operation may have completed"), "{error}");
        assert!(remote.conn.is_none());
        assert_eq!(remote.call("document.inspect", json!({})).unwrap(), Value::Null);
        handle.join().unwrap();
    }
}
