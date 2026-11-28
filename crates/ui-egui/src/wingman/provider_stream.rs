//! Assemble one complete assistant message before releasing tool calls to the workbook.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

const MAX_CALLS: usize = 32;
const ARG_LIMIT: usize = 256 * 1024;
// A byte limit on raw SSE frames alone still permits a single enormous egui label.
const TEXT_LIMIT: usize = 512 * 1024;

// Apply the same recovery policy to HTTP error bodies and errors inside a successful stream.
// Never echo the provider's text: even an error can contain the original prompt.
pub(super) fn context_overflow(error: &Value) -> bool {
    if error["code"] == "context_length_exceeded" || error["type"] == "context_length_exceeded" {
        return true;
    }
    let Some(msg) = error["message"].as_str().or_else(|| error.as_str()) else { return false };
    let msg = msg[..msg.floor_char_boundary(msg.len().min(4096))].to_ascii_lowercase();
    ["maximum context length", "exceeds the context window", "exceeds the available context size"].iter().any(|s| msg.contains(s))
}

fn check_error(frame: &Value, fallback: &'static str) -> Result<(), String> {
    let Some(err) = frame.get("error").filter(|e| !e.is_null()) else { return Ok(()) };
    Err(if context_overflow(err) { super::super::compact::OVERFLOW } else { fallback }.into())
}

pub(super) enum Completion {
    Answer(Reply),
    MissingFinal,
}

pub(super) enum Delta<'a> {
    Text(&'a str),
    Thinking(&'a str),
}

pub(in super::super) struct Reply {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub message: Value,
}
#[derive(Clone, Default)]
pub(in super::super) struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pretty: std::sync::OnceLock<String>,
    plan: std::sync::OnceLock<String>,
}

impl ToolCall {
    pub(in super::super) fn plan_outline(&self) -> &str {
        self.plan.get_or_init(|| {
            let Some(plan) = serde_json::from_str::<Value>(&self.arguments).ok() else {
                return String::new();
            };
            let label = |s: &str| s.chars().filter(|c| !c.is_control()).take(64).collect::<String>();
            plan["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .take(16)
                .enumerate()
                .map(|(i, step)| {
                    let a = &step["args"];
                    let details = ["sheet", "range", "source", "target", "destinationSheet", "at", "cell", "lines", "table", "chart", "shape"]
                        .into_iter()
                        .filter_map(|k| a.get(k).map(|v| format!("{k}: {}", v.as_str().map(label).unwrap_or_else(|| label(&v.to_string())))))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    format!("{}. {}  {details}", i + 1, label(step["tool"].as_str().unwrap_or("?")))
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
    pub(in super::super) fn preview(&self) -> &str {
        self.pretty.get_or_init(|| {
            serde_json::from_str::<Value>(&self.arguments)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or_else(|| self.arguments.clone())
        })
    }
}

#[derive(Default)]
pub(super) struct Assembly {
    content: String,
    reasoning: String,
    calls: BTreeMap<usize, ToolCall>,
    finish: Option<String>,
    refusal: bool,
}
impl Assembly {
    pub(super) fn completion(&mut self, v: &Value, notify: &impl Fn(Delta<'_>)) -> Result<(), String> {
        check_error(v, "The provider reported a completion error.")?;
        let choices = v["choices"].as_array().ok_or("Provider reply has no choices.")?;
        // Some servers include a final, empty usage frame after the last choice.
        let Some(choice) = choices.iter().find(|c| c["index"].as_u64().unwrap_or(0) == 0) else {
            return Ok(());
        };
        if !choice.is_object() {
            return Err("Invalid completion choice.".into());
        }
        let partial = choice.get("delta").filter(|v| !v.is_null());
        if let Some(message) = partial.or_else(|| choice.get("message").filter(|v| !v.is_null())) {
            self.text(message, notify)?;
            if !message["function_call"].is_null() {
                return Err("Legacy function calls are unsupported; no actions were taken.".into());
            }
            if !message["tool_calls"].is_null() {
                let calls = message["tool_calls"].as_array().ok_or("Invalid provider tool-call list.")?;
                for (i, c) in calls.iter().enumerate() {
                    let index = match c["index"].as_u64() {
                        Some(n) => usize::try_from(n).map_err(|_| "Invalid tool index.")?,
                        None if partial.is_none() || calls.len() == 1 => i,
                        None => return Err("Streamed parallel tool calls need distinct indices.".into()),
                    };
                    self.call(index, c)?;
                }
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.record_finish(reason);
        }
        Ok(())
    }

    pub(super) fn ollama(&mut self, v: &Value, notify: &impl Fn(Delta<'_>)) -> Result<(), String> {
        check_error(v, "Ollama reported a completion error.")?;
        if let Some(m) = v.get("message").filter(|m| !m.is_null()) {
            self.text(m, notify)?;
            if !m["tool_calls"].is_null() {
                let calls = m["tool_calls"].as_array().ok_or("Invalid Ollama tool-call list.")?;
                for c in calls {
                    let i = self.calls.len();
                    // Native Ollama emits whole argument objects, and identifies the
                    // returning tool by name; supply a stable id for the conversation.
                    let args = &c["function"]["arguments"];
                    if !args.is_object() {
                        return Err("Invalid Ollama function arguments.".into());
                    }
                    let native = json!({"id":format!("ollama_{i}"),"function":{"name":c["function"]["name"],"arguments":args.to_string()}});
                    self.call(i, &native)?;
                }
            }
        }
        if v["done"] == true {
            self.record_finish(v["done_reason"].as_str().unwrap_or("stop"));
        }
        Ok(())
    }

    fn text(&mut self, message: &Value, notify: &impl Fn(Delta<'_>)) -> Result<(), String> {
        if !message.is_object() {
            return Err("Invalid assistant message.".into());
        }
        let mut append = |s: &str| -> Result<(), String> {
            if self.content.len() + s.len() > TEXT_LIMIT {
                return Err("Assistant reply exceeds 512 KiB. Ask for a smaller result.".into());
            }
            self.content.push_str(s);
            notify(Delta::Text(s));
            Ok(())
        };
        match &message["content"] {
            Value::Null => {}
            Value::String(s) => append(s)?,
            Value::Array(parts) => {
                for part in parts {
                    if part["type"] == "refusal" {
                        self.refusal = true;
                        if let Some(s) = part["refusal"].as_str() {
                            append(s)?;
                        }
                    } else if let Some(s) = part["text"].as_str() {
                        append(s)?;
                    }
                }
            }
            _ => return Err("Unsupported assistant content.".into()),
        }
        if !message["refusal"].is_null() {
            self.refusal = true;
            append(message["refusal"].as_str().ok_or("Invalid refusal response.")?)?;
        }
        if let Some(s) = message["reasoning_content"].as_str().or_else(|| message["thinking"].as_str()) {
            if self.reasoning.len() + s.len() > TEXT_LIMIT {
                return Err("Assistant reasoning exceeds 512 KiB.".into());
            }
            self.reasoning.push_str(s);
            notify(Delta::Thinking(s));
        }
        Ok(())
    }
    fn call(&mut self, index: usize, c: &Value) -> Result<(), String> {
        if index >= MAX_CALLS {
            return Err("Too many tool calls in one reply.".into());
        }
        if c["type"].as_str().is_some_and(|kind| kind != "function") {
            return Err("Unsupported tool-call type.".into());
        }
        let call = self.calls.entry(index).or_default();
        if let Some(s) = c["id"].as_str() {
            call.id.push_str(s);
        }
        if let Some(s) = c["function"]["name"].as_str() {
            call.name.push_str(s);
        }
        if let Some(s) = c["function"]["arguments"].as_str() {
            call.arguments.push_str(s);
        }
        if call.id.len() > 1024 || call.name.len() > 256 || call.arguments.len() > ARG_LIMIT {
            return Err("Tool call exceeds the size limit.".into());
        }
        Ok(())
    }
    fn record_finish(&mut self, reason: &str) {
        match self.finish.as_deref() {
            None => self.finish = Some(reason.to_owned()),
            Some(previous) if previous != reason => self.finish = Some("conflicting completion reasons".into()),
            _ => {}
        }
    }
    pub(super) fn finished(&self) -> bool {
        self.finish.is_some()
    }
    pub(super) fn reply(self, terminated: bool, retain_reasoning: bool) -> Result<Completion, String> {
        if !terminated && self.finish.is_none() {
            return Err("Connection ended before a complete reply arrived.".into());
        }
        if self.refusal && (!self.calls.is_empty() || self.content.trim().is_empty()) {
            return Err("The provider declined to answer. No tool calls were applied.".into());
        }
        // A clean end (or a thinking model's output limit) can contain reasoning but
        // neither an answer nor an action. Only this case permits a text-only follow-up.
        let (_, answer) = super::super::transcript::assistant_parts(&self.reasoning, &self.content, true);
        if self.calls.is_empty() && answer.trim().is_empty() && matches!(self.finish.as_deref(), None | Some("stop" | "length")) {
            return Ok(Completion::MissingFinal);
        }
        if self.finish.as_deref().is_some_and(|r| !matches!(r, "stop" | "tool_calls")) {
            return Err("The provider stopped before completing the reply; no tool calls were applied.".into());
        }
        let calls: Vec<_> = self.calls.into_values().collect();
        if calls.is_empty() && self.finish.as_deref() == Some("tool_calls") {
            return Err("The provider indicated a tool call but supplied none.".into());
        }
        let mut seen = BTreeSet::new();
        let mut wire = Vec::new();
        for c in &calls {
            if c.name.is_empty()
                || c.id.is_empty()
                || !seen.insert(&c.id)
                || !serde_json::from_str::<Value>(&c.arguments).is_ok_and(|a| a.is_object())
            {
                return Err("The provider returned an incomplete or invalid tool call; no calls were applied.".into());
            }
            wire.push(json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}}));
        }
        let mut message = json!({"role":"assistant","content":self.content});
        if !wire.is_empty() {
            message["tool_calls"] = Value::Array(wire);
        }
        if retain_reasoning && !self.reasoning.is_empty() {
            message["reasoning_content"] = self.reasoning.into();
        }
        Ok(Completion::Answer(Reply { content: self.content, tool_calls: calls, message }))
    }
}

/// Lines split across network packets are combined as bytes before UTF-8 and
/// JSON decoding. An SSE event may comprise several data lines.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Frames {
    Sse,
    JsonLines,
}
pub(super) struct Lines {
    frames: Frames,
    native: bool,
    skip_lf: bool,
    pending: Vec<u8>,
    data: Vec<u8>,
    done: bool,
}
impl Lines {
    pub(super) fn new(frames: Frames, native: bool) -> Self {
        Self { frames, native, skip_lf: false, pending: Vec::new(), data: Vec::new(), done: false }
    }
    pub(super) fn push(&mut self, chunk: &[u8], a: &mut Assembly, notify: &impl Fn(Delta<'_>)) -> Result<(), String> {
        let sse = self.frames == Frames::Sse;
        for segment in chunk.split_inclusive(|&b| b == b'\n' || (sse && b == b'\r')) {
            if self.done {
                break;
            }
            if std::mem::take(&mut self.skip_lf) && segment == b"\n" {
                continue;
            }
            if let Some((&end, body)) = segment.split_last()
                && (end == b'\n' || (sse && end == b'\r'))
            {
                self.skip_lf = sse && end == b'\r';
                self.pending.extend_from_slice(body);
                self.line(a, notify)?;
            } else {
                self.pending.extend_from_slice(segment);
            }
        }
        Ok(())
    }
    fn line(&mut self, a: &mut Assembly, notify: &impl Fn(Delta<'_>)) -> Result<(), String> {
        let line = std::mem::take(&mut self.pending);
        if self.frames == Frames::JsonLines {
            if !line.trim_ascii().is_empty() {
                if self.native {
                    a.ollama(&decode(&line)?, notify)?;
                } else {
                    a.completion(&decode(&line)?, notify)?;
                }
            }
            self.done = self.native && a.finished();
        } else if line.is_empty() {
            self.dispatch(a, notify)?;
        } else if let Some(s) = line.strip_prefix(b"data:") {
            self.data.extend_from_slice(s.strip_prefix(b" ").unwrap_or(s));
            self.data.push(b'\n');
        }
        Ok(())
    }
    fn dispatch(&mut self, a: &mut Assembly, notify: &impl Fn(Delta<'_>)) -> Result<(), String> {
        if self.data.is_empty() {
            return Ok(());
        }
        let bytes = std::mem::take(&mut self.data);
        if bytes.trim_ascii() == b"[DONE]" {
            if self.native && !a.finished() {
                return Err("Ollama stream ended before the final done message.".into());
            }
            self.done = true;
        } else {
            if self.native {
                a.ollama(&decode(&bytes)?, notify)?;
                self.done = a.finished();
            } else {
                a.completion(&decode(&bytes)?, notify)?;
            }
        }
        Ok(())
    }
    pub(super) fn end(mut self, a: &mut Assembly, notify: &impl Fn(Delta<'_>)) -> Result<bool, String> {
        // JSON lines may end at EOF. SSE requires the blank line, including for [DONE]:
        // a disconnect must discard both pending line bytes and an unfinished data event.
        if self.frames == Frames::JsonLines && !self.pending.is_empty() {
            self.line(a, notify)?;
        }
        Ok(self.done)
    }
    pub(super) fn done(&self) -> bool {
        self.done
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|_| "Provider returned malformed JSON.".into())
}
