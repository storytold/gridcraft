use super::stream::{Assembly, Lines, Reply};
use super::{
    settings::Protocol,
    thinking::{Levels, Thinking},
    transport::prepare_reasoning,
};
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

#[test]
fn model_efforts_respect_the_declared_set_and_native_boolean_type() {
    let levels = serde_json::from_value::<Levels>(json!(["none", "low", "high"])).unwrap();
    let mut chat = Thinking::configured(Protocol::Completions, None, None, Some(&levels)).unwrap();
    assert_eq!(chat.wire(Protocol::Completions), "high");
    assert!(chat.preference(Protocol::Completions, Some(true), Some("medium")).is_err());
    chat.preference(Protocol::Completions, Some(false), None).unwrap();
    assert_eq!(chat.wire(Protocol::Completions), "none");
    let fixed = serde_json::from_value::<Levels>(json!("low,high")).unwrap();
    let mut fixed = Thinking::configured(Protocol::Completions, None, None, Some(&fixed)).unwrap();
    fixed.preference(Protocol::Completions, Some(false), None).unwrap();
    assert!(fixed.wire(Protocol::Completions).is_null(), "Never send an unsupported off value to an always-thinking model.");
    let mut native = Thinking::configured(Protocol::Ollama, Some(true), None, None).unwrap();
    assert!(native.wire(Protocol::Ollama).is_null());
    native.preference(Protocol::Ollama, Some(true), None).unwrap();
    assert_eq!(native.wire(Protocol::Ollama), true);
    native.preference(Protocol::Ollama, Some(false), None).unwrap();
    assert_eq!(native.wire(Protocol::Ollama), false);
}

#[test]
fn a_new_effort_drops_old_private_reasoning_but_keeps_matching_tool_continuations() {
    let mut messages = [
        json!({"role":"assistant","reasoning_content":"previous","_wingman_thinking":"high"}),
        json!({"role":"user","content":"follow up"}),
        json!({"role":"assistant","reasoning_content":"ongoing","_wingman_thinking":"low"}),
        json!({"role":"tool","content":"result"}),
    ];
    prepare_reasoning(&mut messages, &json!("low"), true);
    assert!(messages[0].get("reasoning_content").is_none());
    assert_eq!(messages[2]["reasoning_content"], "ongoing");
    assert!(messages.iter().all(|m| m.get("_wingman_thinking").is_none()));
}
