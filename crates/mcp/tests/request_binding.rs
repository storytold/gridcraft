#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use gridcraft_mcp::{Backend, Remote};
use serde_json::{Value, json};

#[test]
fn an_error_for_another_explicit_request_keeps_the_valid_binding() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let peer = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        writeln!(writer, "{}", json!({"id": 0, "ok": true})).unwrap();
        for step in 0..3 {
            line.clear();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let reply = match step {
                0 => json!({"id": request["id"], "ok": true, "result": {"id": 43}}),
                1 => {
                    assert_eq!(request["params"]["request"], 99);
                    json!({"id": request["id"], "ok": false, "errorCode": "NO_REQUEST", "error": "no request 99"})
                }
                _ => {
                    assert_eq!(request["params"]["request"], 43, "the unrelated rejection must not unbind request 43");
                    json!({"id": request["id"], "ok": true, "result": {"edited": true}})
                }
            };
            writeln!(writer, "{reply}").unwrap();
        }
    });
    let mut remote = Remote::connect_with_token(&addr, "synthetic-token-for-test".into()).unwrap();
    remote.call("agent.listen", json!({"timeoutMs": 0})).unwrap();
    assert!(remote.call("engine.execute", json!({"request": 99, "command": "cell.set", "params": {"cell": "A1", "input": "1"}})).is_err());
    let result = remote.call("engine.execute", json!({"command": "cell.set", "params": {"cell": "A1", "input": "2"}})).unwrap();
    assert_eq!(result["edited"], true);
    peer.join().unwrap();
}
