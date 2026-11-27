use super::stream::{Assembly, Completion, Frames, Lines, Reply};
use super::{
    settings::Protocol,
    thinking::{Levels, Thinking},
    transport::prepare_reasoning,
};
use serde_json::{Value, json};

fn receive(bytes: &[u8], native: bool) -> Result<Reply, String> {
    framed(bytes, if native { Frames::JsonLines } else { Frames::Sse }, native)
}
fn framed(bytes: &[u8], frames: Frames, native: bool) -> Result<Reply, String> {
    match completed(bytes, frames, native)? {
        Completion::Answer(reply) => Ok(reply),
        Completion::MissingFinal => Err("No final answer".into()),
    }
}
fn completed(bytes: &[u8], frames: Frames, native: bool) -> Result<Completion, String> {
    let mut lines = Lines::new(frames, native);
    let mut assembly = Assembly::default();
    for chunk in bytes.chunks(1) {
        lines.push(chunk, &mut assembly, &|_| {})?;
    }
    let terminated = lines.end(&mut assembly, &|_| {})?;
    assembly.reply(terminated, false)
}

fn event(delta: Value, finish: Value) -> String {
    format!("data: {}\r\n\r\n", json!({"choices":[{"delta":delta,"finish_reason":finish}]}))
}

#[test]
fn empty_answers_recover_but_truncated_calls_and_refusals_do_not() {
    for finish in ["length", "stop"] {
        let reasoning = event(json!({"reasoning_content":"draft","content":"  "}), json!(finish));
        assert!(matches!(completed(reasoning.as_bytes(), Frames::Sse, false), Ok(Completion::MissingFinal)));
        let native = json!({"message":{"thinking":"draft"},"done":true,"done_reason":finish}).to_string();
        assert!(matches!(completed(native.as_bytes(), Frames::JsonLines, true), Ok(Completion::MissingFinal)));
    }
    for data in [
        event(json!({"content":null}), json!("content_filter")),
        event(json!({"content":null}), json!("length")) + &event(json!({}), json!("content_filter")),
        event(json!(42), json!("stop")),
        event(json!({"refusal":" "}), json!("stop")),
        event(json!({"tool_calls":[{"id":"one","function":{"arguments":"{"}}]}), json!("length")),
        event(json!({"reasoning_content":"draft"}), Value::Null),
    ] {
        assert!(completed(data.as_bytes(), Frames::Sse, false).is_err());
    }
    let unfinished = format!("data: {}\n\ndata: [DONE]\n\n", json!({"message":{"thinking":"draft"},"done":false}));
    assert!(completed(unfinished.as_bytes(), Frames::Sse, true).is_err());
}

#[test]
fn final_only_request_keeps_prior_tool_receipts_but_withdraws_tools() {
    let messages = json!([{"role":"system","content":"GridCraft"},{"role":"user","content":"Edit A1"},
        {"role":"assistant","content":"","tool_calls":[{"id":"one"}]},{"role":"tool","tool_call_id":"one","content":"saved"}]);
    let mut body = json!({"messages":messages,"tools":[{"type":"function"}],"parallel_tool_calls":true,"model":"local"});
    super::transport::final_only(&mut body).unwrap();
    assert!(body.get("tools").is_none() && body.get("parallel_tool_calls").is_none());
    assert_eq!(body["messages"].as_array().unwrap()[1..], messages.as_array().unwrap()[1..]);
    assert_eq!(body["model"], "local");
    assert_ne!(body["messages"][0]["content"], messages[0]["content"]);
}

#[test]
fn stream_framing_and_completion_reasons_gate_tool_dispatch() {
    let delta = json!({"tool_calls":[{"id":"one","function":{"name":"read","arguments":"{}"}}]});
    let pending = event(delta.clone(), Value::Null);
    assert!(receive((pending.clone() + "data: [DONE]\n").as_bytes(), false).is_err());
    assert!(receive((pending + "data: [DONE]\r\r").replace("\r\n", "\r").as_bytes(), false).is_ok());
    let filtered = event(delta.clone(), json!("content_filter")) + &event(json!({}), json!("stop"));
    assert!(receive(filtered.as_bytes(), false).is_err());
    let line = json!({"choices":[{"delta":delta,"finish_reason":"tool_calls"}]}).to_string();
    assert_eq!(framed(line.as_bytes(), Frames::JsonLines, false).unwrap().tool_calls.len(), 1);
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
