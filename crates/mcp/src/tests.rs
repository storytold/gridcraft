#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};

use serde_json::{Value, json};

use crate::{Backend, Headless, PROTOCOL_VERSION, Remote, Server, control_addr, tool_definitions};

fn server() -> Server {
    Server::new(Box::new(Headless::new()))
}

fn rpc(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let reply = s.handle_line(&line).expect("reply");
    let v: Value = serde_json::from_str(&reply).expect("reply is JSON");
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], id);
    v
}

fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let v = rpc(s, 99, "tools/call", json!({"name": name, "arguments": args}));
    assert!(v.get("error").is_none(), "{v}");
    v["result"].clone()
}

fn text_of(result: &Value) -> String {
    result["content"].as_array().unwrap().iter().filter(|c| c["type"] == "text").map(|c| c["text"].as_str().unwrap().to_string()).collect()
}

/// Call a tool that must succeed; returns its JSON payload.
fn ok(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, name, args);
    assert_eq!(r["isError"], false, "{name}: {r}");
    serde_json::from_str(&text_of(&r)).unwrap_or(Value::Null)
}

/// Call a tool that must fail; returns its message.
fn fails(s: &mut Server, name: &str, args: Value) -> String {
    let r = call(s, name, args);
    assert_eq!(r["isError"], true, "{name} should fail: {r}");
    text_of(&r)
}

fn err_code(v: &Value) -> i64 {
    v["error"]["code"].as_i64().unwrap_or_else(|| panic!("expected an error: {v}"))
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-mcp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

// ---------- protocol ----------

#[test]
fn initialize_and_version_negotiation() {
    let mut s = server();
    let v =
        rpc(&mut s, 1, "initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}));
    let r = &v["result"];
    assert_eq!(r["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(r["serverInfo"]["name"], "gridcraft");
    assert!(r["capabilities"]["tools"].is_object());
    assert!(r["capabilities"]["resources"].is_object());
    assert!(r["instructions"].as_str().unwrap().contains("headless"));
    // Older supported version is echoed back; unknown versions get ours.
    let v = rpc(&mut s, 2, "initialize", json!({"protocolVersion": "2024-11-05"}));
    assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    let v = rpc(&mut s, 3, "initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(!s.is_initialized());
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    assert!(s.is_initialized());
    assert_eq!(rpc(&mut s, 4, "ping", json!({}))["result"], json!({}));
    assert_eq!(rpc(&mut s, 5, "prompts/list", json!({}))["result"]["prompts"], json!([]));
}

#[test]
fn tools_list_has_schemas() {
    let mut s = server();
    let v = rpc(&mut s, 1, "tools/list", json!({}));
    let tools = v["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "execute_command",
        "list_commands",
        "inspect_workbook",
        "read_range",
        "write_range",
        "set_cell",
        "get_cell",
        "evaluate_formula",
        "select",
        "format_range",
        "insert_chart",
        "create_table",
        "sort_range",
        "filter",
        "open_workbook",
        "save_workbook",
        "new_workbook",
        "undo",
        "redo",
        "screenshot",
        "list_functions",
    ] {
        assert!(names.contains(&want), "missing tool {want}");
    }
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].as_str().unwrap().len() > 20, "{t}");
        let name = t["name"].as_str().unwrap();
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name} is not snake_case");
        for r in t["inputSchema"]["required"].as_array().into_iter().flatten() {
            assert!(t["inputSchema"]["properties"].get(r.as_str().unwrap()).is_some(), "{name}: required {r} not in properties");
        }
    }
    assert_eq!(tools.len(), tool_definitions().len());
}

#[test]
fn malformed_input_never_panics() {
    let mut s = server();
    assert_eq!(s.handle_line("   "), None);
    let parse = |s: &mut Server, l: &str| -> Value { serde_json::from_str(&s.handle_line(l).unwrap()).unwrap() };
    assert_eq!(err_code(&parse(&mut s, "{not json")), -32700);
    assert_eq!(err_code(&parse(&mut s, "\"just a string\"")), -32600);
    assert_eq!(err_code(&parse(&mut s, "[]")), -32600);
    assert_eq!(err_code(&parse(&mut s, r#"{"jsonrpc":"2.0","id":1}"#)), -32600);
    assert_eq!(err_code(&parse(&mut s, r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#)), -32600);
    assert_eq!(err_code(&parse(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[1,2]}"#)), -32602);
    // Unknown method.
    assert_eq!(err_code(&rpc(&mut s, 1, "nope/nothing", json!({}))), -32601);
    // Bad tools/call params.
    assert_eq!(err_code(&rpc(&mut s, 2, "tools/call", json!({}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 3, "tools/call", json!({"name": "no_such_tool"}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 4, "tools/call", json!({"name": "set_cell", "arguments": "x"}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 5, "tools/call", json!({"name": "set_cell", "arguments": {"cell": "A1"}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 6, "tools/call", json!({"name": "set_cell", "arguments": {"cell": 5, "input": "x"}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 7, "tools/call", json!({"name": "insert_chart", "arguments": {"range": "A1", "type": "piechart"}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 8, "tools/call", json!({"name": "get_cell", "arguments": {"cell": "A1", "bogus": 1}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 9, "resources/read", json!({}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 10, "resources/read", json!({"uri": "gridcraft://nope"}))), -32002);
    // Responses and unknown notifications are ignored.
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#), None);
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#), None);
    // A batch still gets answers.
    let v = parse(&mut s, r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#);
    assert_eq!(v.as_array().unwrap().len(), 1);
    // Tool-level errors are isError results, not protocol errors.
    assert!(fails(&mut s, "set_cell", json!({"cell": "not a cell!!", "input": "1"})).contains("set_cell"));
    assert!(fails(&mut s, "execute_command", json!({"command": "no.such.command"})).contains("unknown command"));
    fails(&mut s, "read_range", json!({"range": "ZZZZZZZZ99999999999"}));
    fails(&mut s, "format_range", json!({"range": "A1"}));
    fails(&mut s, "sort_range", json!({"keys": []}));
    fails(&mut s, "open_workbook", json!({"path": "/definitely/not/here.xlsx"}));
    fails(&mut s, "save_workbook", json!({}));
    assert!(fails(&mut s, "screenshot", json!({"path": "/tmp/x.png"})).contains("no UI in headless mode"));
}

#[test]
fn every_tool_survives_hostile_arguments() {
    let hostile = [json!({}), json!({"range": "A1:XFD1048576"}), json!({"range": "", "cell": "", "path": "", "formula": "", "command": ""})];
    for t in tool_definitions() {
        let name = t["name"].as_str().unwrap();
        for args in &hostile {
            let mut s = server();
            // Either a protocol error or a tool result; never a panic.
            let _ =
                s.handle_line(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string());
        }
    }
}

#[test]
fn resources() {
    let mut s = server();
    let v = rpc(&mut s, 1, "resources/list", json!({}));
    let uris: Vec<&str> = v["result"]["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    assert_eq!(uris, ["gridcraft://workbook", "gridcraft://commands", "gridcraft://functions"]);
    for uri in uris {
        let v = rpc(&mut s, 2, "resources/read", json!({"uri": uri}));
        let text = v["result"]["contents"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(!parsed.is_null(), "{uri}");
    }
    let v = rpc(&mut s, 3, "resources/read", json!({"uri": "gridcraft://functions"}));
    assert!(v["result"]["contents"][0]["text"].as_str().unwrap().contains("VLOOKUP"));
}

#[test]
fn basic_tools() {
    let mut s = server();
    let cmds = ok(&mut s, "list_commands", json!({"search": "bold"}));
    assert!(cmds.as_array().unwrap().iter().any(|c| c["id"] == "home.bold"));
    let all = ok(&mut s, "list_commands", json!({}));
    assert!(all.as_array().unwrap().len() > 100);
    let f = ok(&mut s, "list_functions", json!({"search": "vlookup"}));
    assert!(f.as_array().unwrap().iter().any(|x| x["name"] == "VLOOKUP"));
    assert_eq!(ok(&mut s, "evaluate_formula", json!({"formula": "SUM(1,2,3)"}))["result"], json!(6.0));
    ok(&mut s, "set_cell", json!({"cell": "A1", "input": "5"}));
    ok(&mut s, "execute_command", json!({"command": "cell.set", "params": {"cell": "A2", "input": "=A1*2"}}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A2"}))["value"], json!(10.0));
    assert_eq!(ok(&mut s, "evaluate_formula", json!({"formula": "=A1+A2", "cell": "C1"}))["result"], json!(15.0));
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A2"}))["value"], Value::Null);
    ok(&mut s, "redo", json!({}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A2"}))["value"], json!(10.0));
    ok(&mut s, "select", json!({"range": "A1:A2"}));
    assert_eq!(ok(&mut s, "inspect_workbook", json!({}))["selection"], "A1:A2");
    ok(&mut s, "new_workbook", json!({"sample": "sales"}));
    let info = ok(&mut s, "inspect_workbook", json!({}));
    assert!(info["sheets"][0]["cells"].as_u64().unwrap() > 10, "{info}");
}

#[test]
fn sort_and_filter() {
    let mut s = server();
    ok(&mut s, "write_range", json!({"range": "A1", "values": [["Name", "Score"], ["b", 2], ["c", 3], ["a", 1]]}));
    ok(&mut s, "sort_range", json!({"range": "A1:B4", "keys": [{"column": "B", "order": "desc"}], "header": true}));
    let v = ok(&mut s, "read_range", json!({"range": "A2:A4"}));
    assert_eq!(v["values"], json!([["c"], ["b"], ["a"]]));
    ok(&mut s, "sort_range", json!({"range": "A1:B4", "keys": ["A"], "header": true}));
    let v = ok(&mut s, "read_range", json!({"range": "A2:A4"}));
    assert_eq!(v["values"], json!([["a"], ["b"], ["c"]]));
    ok(&mut s, "filter", json!({"range": "A1:B4", "column": "A", "values": ["a", "c"]}));
    let info = ok(&mut s, "inspect_workbook", json!({}));
    assert_eq!(info["sheets"][0]["autofilter"], "A1:B4");
}

#[test]
fn save_and_open_roundtrip() {
    let mut s = server();
    ok(&mut s, "write_range", json!({"range": "B2", "values": [[1, 2], [3, "=SUM(B2:C2)+B3"]]}));
    let path = tmp("roundtrip.xlsx");
    let p = path.to_string_lossy().to_string();
    ok(&mut s, "save_workbook", json!({"path": p}));
    ok(&mut s, "save_workbook", json!({}));
    ok(&mut s, "new_workbook", json!({}));
    ok(&mut s, "open_workbook", json!({"path": p}));
    let v = ok(&mut s, "read_range", json!({"range": "B2:C3", "formulas": true}));
    assert_eq!(v["values"], json!([[1.0, 2.0], [3.0, "=SUM(B2:C2)+B3"]]));
    let v = ok(&mut s, "read_range", json!({"range": "C3"}));
    assert_eq!(v["values"], json!([[6.0]]));
    let _ = std::fs::remove_file(path);
}

/// An agent builds a small monthly budget through MCP tool calls only.
#[test]
fn agent_builds_a_budget() {
    let mut s = server();
    rpc(&mut s, 1, "initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "agent", "version": "1"}}));
    s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    ok(
        &mut s,
        "write_range",
        json!({"range": "A1", "values": [
            ["Category", "January", "February", "March"],
            ["Rent", 1200, 1200, 1250],
            ["Groceries", 410.5, 389.25, 402],
            ["Utilities", 150, 162, 140],
            ["Transport", 90, 85, 110],
        ]}),
    );
    ok(&mut s, "set_cell", json!({"cell": "A6", "input": "Total"}));
    for col in ["B", "C", "D"] {
        ok(&mut s, "set_cell", json!({"cell": format!("{col}6"), "input": format!("=SUM({col}2:{col}5)")}));
    }
    ok(&mut s, "set_cell", json!({"cell": "E1", "input": "Quarter"}));
    ok(
        &mut s,
        "write_range",
        json!({"range": "E2", "values": [["=SUM(B2:D2)"], ["=SUM(B3:D3)"], ["=SUM(B4:D4)"], ["=SUM(B5:D5)"], ["=SUM(B6:D6)"]]}),
    );

    let fmt =
        ok(&mut s, "format_range", json!({"range": "A1:E1", "bold": true, "fillColor": "#DDEBF7", "horizontalAlign": "center", "borders": "bottom"}));
    assert_eq!(fmt["applied"].as_array().unwrap().len(), 4, "{fmt}");
    ok(&mut s, "format_range", json!({"range": "B2:E6", "numberFormat": "#,##0.00"}));
    ok(&mut s, "format_range", json!({"range": "A6:E6", "bold": true, "borders": {"preset": "top", "style": "double"}}));

    ok(&mut s, "create_table", json!({"range": "A1:E5", "style": "TableStyleLight9"}));
    ok(&mut s, "insert_chart", json!({"range": "A1:D5", "type": "column", "title": "Monthly spend", "at": "G2"}));

    // Read back computed values.
    let totals = ok(&mut s, "read_range", json!({"range": "B6:E6"}));
    let row = totals["values"][0].as_array().unwrap();
    let nums: Vec<f64> = row.iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((nums[0] - 1850.5).abs() < 1e-9, "{nums:?}");
    assert!((nums[1] - 1836.25).abs() < 1e-9, "{nums:?}");
    assert!((nums[2] - 1902.0).abs() < 1e-9, "{nums:?}");
    assert!((nums[3] - 5588.75).abs() < 1e-9, "{nums:?}");
    let shown = ok(&mut s, "read_range", json!({"range": "E6", "formatted": true}));
    assert_eq!(shown["values"][0][0], "5,588.75");
    let formulas = ok(&mut s, "read_range", json!({"range": "B6", "formulas": true}));
    assert_eq!(formulas["values"][0][0], "=SUM(B2:B5)");
    let cell = ok(&mut s, "get_cell", json!({"cell": "A1"}));
    assert_eq!(cell["style"]["font"]["bold"], true, "{cell}");

    let info = ok(&mut s, "inspect_workbook", json!({}));
    let sheet = &info["sheets"][0];
    assert_eq!(sheet["tables"].as_array().unwrap().len(), 1, "{info}");
    assert_eq!(sheet["tables"][0]["range"], "A1:E5");
    assert_eq!(sheet["charts"].as_array().unwrap().len(), 1, "{info}");
    assert_eq!(sheet["charts"][0]["title"], "Monthly spend");

    // Change an input: totals follow.
    ok(&mut s, "set_cell", json!({"cell": "B2", "input": "1300"}));
    let v = ok(&mut s, "read_range", json!({"range": "E6"}));
    assert!((v["values"][0][0].as_f64().unwrap() - 5688.75).abs() < 1e-9);
    // And the whole thing evaluates without touching the sheet.
    let v = ok(&mut s, "evaluate_formula", json!({"formula": "=AVERAGE(B6:D6)"}));
    assert!((v["result"].as_f64().unwrap() - (1950.5 + 1836.25 + 1902.0) / 3.0).abs() < 1e-9);
}

// ---------- serve loop + remote ----------

#[test]
fn serve_loop_over_buffers() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        "garbage\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"evaluate_formula","arguments":{"formula":"=2*21"}}}"#,
        "\n"
    );
    let mut out = Vec::new();
    let mut s = server();
    s.serve(input.as_bytes(), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert!(lines[2]["result"]["content"][0]["text"].as_str().unwrap().contains("42"));
}

#[test]
fn remote_backend_over_tcp() {
    // A fake app: answers engine.execute by running a headless session; closes after the first
    // request to exercise the reconnect path.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut h = Headless::new();
        for (i, stream) in listener.incoming().take(2).enumerate() {
            let stream = stream.unwrap();
            let mut w = stream.try_clone().unwrap();
            let mut r = BufReader::new(stream);
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let req: Value = serde_json::from_str(&line).unwrap();
                let reply = match h.call(req["method"].as_str().unwrap(), req["params"].clone()) {
                    Ok(v) => json!({"id": req["id"], "ok": true, "result": v}),
                    Err(e) => json!({"id": req["id"], "ok": false, "error": e}),
                };
                w.write_all(format!("{reply}\n").as_bytes()).unwrap();
                if i == 0 {
                    break; // drop the first connection
                }
            }
        }
    });
    let addr = control_addr(&port.to_string());
    assert_eq!(addr, format!("127.0.0.1:{port}"));
    let mut s = Server::new(Box::new(Remote::connect(&addr).unwrap()));
    assert_eq!(ok(&mut s, "evaluate_formula", json!({"formula": "=1+1"}))["result"], json!(2.0));
    // Second call reconnects transparently.
    ok(&mut s, "set_cell", json!({"cell": "A1", "input": "7"}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A1"}))["value"], json!(7.0));
    let msg = fails(&mut s, "execute_command", json!({"command": "nope"}));
    assert!(msg.contains("unknown command"), "{msg}");
    drop(s);
    handle.join().unwrap();
    assert!(Remote::connect("127.0.0.1:1").is_err());
}

#[test]
fn file_commands_without_a_file_fail() {
    let mut s = server();
    // `file.save` on Book1, which has no file yet, would ask for one too.
    for command in ["file.open", "file.saveAs", "file.save", "data.getData", "data.fromTextCsv"] {
        let msg = fails(&mut s, "execute_command", json!({"command": command}));
        assert!(msg.contains("missing") && msg.contains("never open dialogs"), "{command}: {msg}");
        fails(&mut s, "execute_command", json!({"command": command, "params": {}}));
    }
}
