use super::stream::{Assembly, Lines, Reply};
use serde_json::{Value, json};

fn receive(bytes: &[u8], native: bool) -> Result<Reply, String> {
    let mut lines = Lines::default();
    let mut assembly = Assembly::default();
    for chunk in bytes.chunks(1) {
        lines.push(chunk, native, &mut assembly, &|_| {})?;
    }
    let terminated = lines.end(native, &mut assembly, &|_| {})?;
    assembly.reply(terminated, false)
}

fn event(delta: Value, finish: Value) -> String {
    format!("data: {}\r\n\r\n", json!({"choices":[{"delta":delta,"finish_reason":finish}]}))
}

#[test]
fn a_fragmented_function_batch_keeps_its_arguments_and_waits_for_completion() {
    let head = json!({"index":0,"id":"one","type":"function","function":{"name":"read","arguments":"{\"cell\":\""}});
    let tail = json!({"index":0,"function":{"arguments":"É1\"}"}});
    let next = json!({"index":1,"id":"two","type":"function","function":{"name":"list","arguments":"{}"}});
    let partial = event(json!({"tool_calls":[head,next]}), Value::Null) + &event(json!({"tool_calls":[tail]}), Value::Null);
    assert!(receive(partial.as_bytes(), false).is_err(), "A disconnect after valid arguments must not release the calls.");
    let data = partial + &event(json!({}), json!("tool_calls")) + "data: [DONE]\r\n\r\n";
    let reply = receive(data.as_bytes(), false).unwrap();
    let [first, second] = reply.tool_calls.as_slice() else { panic!("both calls must survive") };
    assert_eq!((&*first.name, &*second.name), ("read", "list"));
    assert_eq!(serde_json::from_str::<Value>(&first.arguments).unwrap(), json!({"cell":"É1"}));
    assert_eq!(reply.message["tool_calls"][1]["id"], second.id);
}

#[test]
fn a_bad_second_call_or_filtered_finish_rejects_the_whole_batch() {
    let call = |idx, id, args| json!({"index":idx,"id":id,"function":{"name":"write","arguments":args}});
    for (id, args, finish) in [("next", "{", "tool_calls"), ("one", "{}", "tool_calls"), ("next", "{}", "content_filter")] {
        let data = event(json!({"tool_calls":[call(0,"one","{}"),call(1,id,args)]}), json!(finish)) + "data: [DONE]\n\n";
        assert!(receive(data.as_bytes(), false).is_err());
    }
}

#[test]
fn native_ollama_requires_done_before_releasing_a_complete_argument_object() {
    let args = json!({"range":"A1:B2"});
    let raw = format!("{}\n", json!({"message":{"tool_calls":[{"function":{"name":"read","arguments":args}}]},"done":false}));
    assert!(receive(raw.as_bytes(), true).is_err());
    let reply = receive((raw + "{\"done\":true}\n").as_bytes(), true).unwrap();
    let [call] = reply.tool_calls.as_slice() else { panic!("missing native call") };
    assert_eq!(serde_json::from_str::<Value>(&call.arguments).unwrap(), args);
    assert_eq!(reply.message["tool_calls"][0]["id"], call.id);
}
