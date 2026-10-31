use gridcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

#[test]
fn ask_agent_mcp_contract_is_advertised_and_headless_explains_how_to_connect() {
    let mut server = Server::new(Box::new(Headless::new()));
    let listed = server.handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string()).unwrap();
    let listed: Value = serde_json::from_str(&listed).unwrap();
    for tool in ["wait_for_request", "reply_to_request"] {
        assert!(listed["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == tool), "missing {tool}");
        let args = if tool == "wait_for_request" { json!({"timeout_s":0}) } else { json!({"id":1,"text":"Done"}) };
        let reply =
            server.handle_line(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string()).unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("no window") && text.contains("mcp --connect"), "{text}");
    }
}

fn rpc(server: &mut Server, name: &str, args: Value) -> Value {
    serde_json::from_str(
        &server.handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string()).unwrap(),
    )
    .unwrap()
}
#[test]
fn common_schemas_and_compact_validation_match() {
    let mut server = Server::new(Box::new(Headless::new()));
    let listed: Value = serde_json::from_str(&server.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap()).unwrap();
    let defs = listed["result"]["tools"].as_array().unwrap();
    let wait = &defs.iter().find(|d| d["name"] == "wait_for_request").unwrap()["inputSchema"];
    assert_eq!(wait["required"], json!([]));
    assert_eq!(wait["properties"]["timeout_s"]["default"], 30);
    assert!(wait["properties"]["agent"].is_object());
    let reply = &defs.iter().find(|d| d["name"] == "reply_to_request").unwrap()["inputSchema"];
    assert_eq!(reply["required"], json!(["id"]));
    assert_eq!(reply["properties"]["id"]["maximum"], 9_007_199_254_740_991_u64);
    assert_eq!(reply["anyOf"].as_array().unwrap().len(), 2);
    for (name, args) in [
        ("wait_for_request", json!({"timeout_s":-1})),
        ("wait_for_request", json!({"timeoutMs":0})),
        ("reply_to_request", json!({"id":1,"text":"x","done":"true"})),
        ("reply_to_request", json!({"id":1})),
        ("reply_to_request", json!({"id":1,"error":" ","done":true})),
        ("reply_to_request", json!({"id":1,"proposal":"x","error":"bad"})),
        ("reply_to_request", json!({"id":1,"comment":"early","text":"x","done":false})),
        ("reply_to_request", json!({"id":1,"error":"bad","done":false})),
    ] {
        assert_eq!(rpc(&mut server, name, args.clone())["error"]["code"], -32602);
        assert_eq!(rpc(&mut server, "tool_call", json!({"name":name,"arguments":args}))["error"]["code"], -32602);
    }
    for args in [json!({"id":1,"text":"done"}), json!({"id":1,"error":"failed"})] {
        let r = rpc(&mut server, "reply_to_request", args);
        assert!(r.get("error").is_none());
        let e: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(e["errorCode"], "NO_UI");
    }
}
