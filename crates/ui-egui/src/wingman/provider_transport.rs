use std::collections::HashMap;
use std::future::Future;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::{AbortHandle, Abortable};
use reqwest::{Client, RequestBuilder, Response};
use serde_json::{Value, json};

use super::settings::{self, Agent, Host, Protocol};
use super::stream::{Assembly, Completion, Delta, Frames, Lines, Reply, decode};

const REPLY_LIMIT: usize = 16 * 1024 * 1024;
const CATALOG_LIMIT: usize = 2 * 1024 * 1024;
const REQUEST_LIMIT: usize = 8 * 1024 * 1024;

struct Task<T> {
    abort: AbortHandle,
    rx: Receiver<Result<T, String>>,
    finished: bool,
}
impl<T> Task<T> {
    fn poll(&mut self) -> Option<Result<T, String>> {
        if self.finished {
            return None;
        }
        let value = match self.rx.try_recv() {
            Ok(value) => value,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("The provider worker stopped.".into()),
        };
        self.finished = true;
        Some(value)
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.abort.abort();
    }
}
fn spawn<T, F>(ctx: egui::Context, work: F) -> Result<Task<T>, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    let (abort, registration) = AbortHandle::new_pair();
    let viewport = ctx.viewport_id();
    std::thread::Builder::new()
        .name("gridcraft-wingman".into())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "Cannot start the provider worker.".to_string())
                .and_then(|runtime| {
                    let done = runtime.block_on(Abortable::new(work, registration));
                    runtime.shutdown_timeout(Duration::from_millis(100));
                    done.unwrap_or_else(|_| Err("Request cancelled.".into()))
                });
            let _ = tx.send(result);
            ctx.request_repaint_of(viewport);
        })
        .map_err(|_| "Cannot spawn the provider worker.".to_string())?;
    Ok(Task { abort, rx, finished: false })
}
pub(in super::super) enum Event {
    FindingFinal { thinking: String, content: String },
    Text(String),
    Thinking(String),
    Done(Result<Reply, String>),
}
#[derive(Default)]
struct Progress {
    finding_final: Option<(String, String)>,
    text: String,
    thinking: String,
}
pub(in super::super) struct Job {
    task: Task<Reply>,
    progress: Arc<Mutex<Progress>>,
}
impl Job {
    pub(in super::super) fn poll(&mut self) -> Option<Event> {
        let mut progress = self.progress.lock().ok()?;
        if let Some((thinking, content)) = progress.finding_final.take() {
            return Some(Event::FindingFinal { thinking, content });
        }
        if !progress.thinking.is_empty() {
            return Some(Event::Thinking(std::mem::take(&mut progress.thinking)));
        }
        if !progress.text.is_empty() {
            return Some(Event::Text(std::mem::take(&mut progress.text)));
        }
        self.task.poll().map(Event::Done)
    }
}

fn client(a: &Agent) -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(a.idle_secs))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|_| "Cannot initialize the HTTP client.".into())
}
fn authorize(req: RequestBuilder, a: &Agent) -> Result<RequestBuilder, String> {
    if a.headers.is_empty() {
        return Ok(req);
    }
    let url = reqwest::Url::parse(&a.endpoint).map_err(|_| "Invalid provider URL.")?;
    if url.scheme() != "https" && !a.is_loopback {
        return Err("Use HTTPS to send API keys or custom headers outside loopback.".into());
    }
    Ok(req.headers((*a.headers).clone()))
}
async fn send(req: RequestBuilder) -> Result<Response, String> {
    let r = req.send().await.map_err(network_error)?;
    let status = r.status();
    if status.is_success() {
        Ok(r)
    } else if status.is_redirection() {
        Err(format!("HTTP {}: redirects are disabled. Configure the final API address.", status.as_u16()))
    } else {
        // Response bodies may echo prompts, headers, or keys. Surface the status,
        // never the raw body (or a reqwest error containing its request URL).
        if matches!(status.as_u16(), 400 | 413 | 422)
            && let Ok(raw) = bytes(r, 16 * 1024).await
            && let Ok(body) = serde_json::from_slice::<Value>(&raw)
            && super::stream::context_overflow(&body["error"])
        {
            return Err(super::super::compact::OVERFLOW.into());
        }
        Err(format!("Provider returned HTTP {} {}.", status.as_u16(), status.canonical_reason().unwrap_or("")))
    }
}
fn network_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "Provider timed out.".into()
    } else if e.is_connect() {
        "Cannot connect to the provider; check the URL, server and TLS certificate.".into()
    } else {
        "The provider request could not be completed.".into()
    }
}
async fn bytes(mut r: Response, limit: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(network_error)? {
        if chunk.len() > limit - out.len() {
            return Err("Provider response exceeds the size limit.".into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
fn native_history(messages: &mut [Value]) -> Result<(), String> {
    let mut names = HashMap::new();
    for m in messages {
        if m["role"] == "user" && m["content"].is_array() {
            let parts = std::mem::take(&mut m["content"]);
            let mut text = Vec::new();
            let mut images = Vec::new();
            for part in parts.as_array().ok_or("Expected multimodal content.")? {
                match part["type"].as_str() {
                    Some("text") => text.push(part["text"].as_str().ok_or("Invalid visual caption.")?),
                    Some("image_url") => {
                        let url = part["image_url"]["url"].as_str().ok_or("Missing visual data.")?;
                        let data = url
                            .strip_prefix("data:image/png;base64,")
                            .or_else(|| url.strip_prefix("data:image/jpeg;base64,"))
                            .ok_or("Only local PNG and JPEG data can be sent as visual input.")?;
                        images.push(data);
                    }
                    _ => return Err("Unsupported multimodal input.".into()),
                }
            }
            m["content"] = text.join("\n").into();
            if !images.is_empty() {
                m["images"] = json!(images);
            }
        }
        if let Some(r) = m.as_object_mut().and_then(|o| o.remove("reasoning_content")) {
            m["thinking"] = r;
        }
        if let Some(calls) = m.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for c in calls {
                let id = c["id"].as_str().ok_or("Missing tool call id.")?.to_string();
                let name = c["function"]["name"].as_str().ok_or("Missing tool call name.")?.to_string();
                names.insert(id, name.clone());
                let args = decode(c["function"]["arguments"].as_str().ok_or("Missing tool arguments.")?.as_bytes())?;
                *c = json!({"function":{"name":name,"arguments":args}});
            }
        }
        if m["role"] == "tool" {
            let id = m["tool_call_id"].as_str().ok_or("Missing tool response id.")?;
            let name = names.get(id).ok_or("No preceding function call for this response.")?;
            *m = json!({"role":"tool","tool_name":name,"content":m["content"]});
        }
    }
    Ok(())
}
// This private marker is held only in the in-memory transcript. Preserve reasoning for
// tool continuations that require it, never forward provider state from a different mode.
const THINKING_TAG: &str = "_wingman_thinking";
pub(super) fn prepare_reasoning(messages: &mut [Value], choice: &Value, retain: bool) {
    let current = messages.iter().rposition(|m| m["role"] == "user").unwrap_or(messages.len());
    for (i, m) in messages.iter_mut().enumerate() {
        if let Some(obj) = m.as_object_mut() {
            let previous = obj.remove(THINKING_TAG);
            if !retain || previous.as_ref() != Some(choice) || (i < current && (choice.as_bool() == Some(false) || choice.as_str() == Some("none"))) {
                obj.remove("reasoning_content");
            }
        }
    }
}
pub(super) fn payload(a: &Agent, mut messages: Vec<Value>, tools: Vec<Value>) -> Result<Value, String> {
    if !a.supports_images && messages.iter().any(|m| m["content"].as_array().is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))) {
        return Err("Image input is disabled for this model.".into());
    }
    if a.input_token_budget() == 0 {
        return Err("The model's context limit leaves no room for input. Lower max_output_tokens in wingman.json and reload.".into());
    }
    let thinking = a.thinking.wire(a.protocol);
    prepare_reasoning(&mut messages, &thinking, a.retain_reasoning());
    if a.protocol == Protocol::Ollama {
        native_history(&mut messages)?;
    }
    let mut body = json!({"model":a.model,"messages":messages,"stream":true});
    if a.supports_tools && !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        if a.parallel && a.protocol == Protocol::Completions {
            body["parallel_tool_calls"] = true.into();
        }
    }
    if a.protocol == Protocol::Ollama {
        // Zed documents 4096 as the Ollama default when neither per-model
        // max_tokens nor the provider's context_window is configured.
        let mut options = json!({"num_ctx":a.max_tokens.unwrap_or(4096)});
        if let Some(n) = a.max_output_tokens {
            options["num_predict"] = n.into();
        }
        body["options"] = options;
    } else {
        if let Some(n) = a.max_output_tokens {
            body[if a.legacy_limit { "max_tokens" } else { "max_completion_tokens" }] = n.into();
        }
    }
    if !thinking.is_null() {
        body[if a.protocol == Protocol::Ollama { "think" } else { "reasoning_effort" }] = thinking;
    }
    Ok(body)
}
// Keep the original request's evidence and settings, not the empty assistant reply.
// This instruction exists on the wire only: no synthetic user turn enters chat history.
pub(super) fn final_only(body: &mut Value) -> Result<(), String> {
    let body = body.as_object_mut().ok_or("Invalid provider request.")?;
    body.remove("tools");
    body.remove("parallel_tool_calls");
    let system = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|m| m.first_mut())
        .filter(|m| m["role"] == "system")
        .and_then(|m| m.get_mut("content"));
    let Some(Value::String(prompt)) = system else { return Err("Missing Wingman system message.".into()) };
    prompt.push_str("\nGive the user a final text answer now, using only the information and tool receipts already available. Identify unfinished work. Do not issue or repeat any actions.");
    Ok(())
}

type Projection = Option<(Arc<Mutex<super::super::snap::Cache>>, usize)>;
async fn chat(
    a: Agent,
    messages: Vec<Value>,
    tools: Vec<Value>,
    projection: Projection,
    notify: impl Fn(Delta<'_>),
    finding_final: impl Fn(),
) -> Result<Reply, String> {
    let native = a.protocol == Protocol::Ollama;
    let url = format!("{}/{}", a.endpoint, if native { "api/chat" } else { "chat/completions" });
    let mut body = payload(&a, messages, tools)?;
    if a.snap_tool_results
        && a.supports_images
        && let Some((cache, needed)) = projection
    {
        let mut locked = cache.lock().ok();
        let saved = super::super::snap::project(&mut body, native, REQUEST_LIMIT, &a.model, locked.as_deref_mut());
        if saved < needed {
            return Err(super::super::compact::SNAP_INSUFFICIENT.into());
        }
    }
    let client = client(&a)?;
    let mut recovering = false;
    loop {
        let encoded = serde_json::to_vec(&body).map_err(|_| "Cannot encode the request.")?;
        if encoded.len() > REQUEST_LIMIT {
            return Err("Conversation exceeds the 8 MiB request limit. Start a new thread or attach a smaller range.".into());
        }
        let req = client.post(&url).header("content-type", "application/json").body(encoded);
        let response = send(authorize(req, &a)?).await?;
        match receive(response, native, a.retain_reasoning(), &notify).await? {
            Completion::Answer(mut reply) => {
                if recovering && !reply.tool_calls.is_empty() {
                    return Err("The final-answer follow-up returned a tool call instead of an answer. No new action was taken.".into());
                }
                if reply.message.get("reasoning_content").is_some() {
                    reply.message[THINKING_TAG] = a.thinking.wire(a.protocol);
                }
                return Ok(reply);
            }
            Completion::MissingFinal if recovering => return Err("No final answer arrived after the automatic follow-up.".into()),
            Completion::MissingFinal => {
                final_only(&mut body)?;
                recovering = true;
                finding_final();
            }
        }
    }
}

async fn receive(mut r: Response, native: bool, reasoning: bool, notify: impl Fn(Delta<'_>)) -> Result<Completion, String> {
    let media = r.headers().get("content-type").and_then(|h| h.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim();
    let frames = if media.eq_ignore_ascii_case("text/event-stream") {
        Some(Frames::Sse)
    } else if native || media.eq_ignore_ascii_case("application/x-ndjson") || media.eq_ignore_ascii_case("application/ndjson") {
        Some(Frames::JsonLines)
    } else {
        None
    };
    let mut a = Assembly::default();
    let Some(frames) = frames else {
        let raw = bytes(r, REPLY_LIMIT).await?;
        // A few local runtimes emit SSE without setting its content type.
        if raw.trim_ascii_start().starts_with(b"data:") {
            let mut lines = Lines::new(Frames::Sse, false);
            lines.push(&raw, &mut a, &notify)?;
            let done = lines.end(&mut a, &notify)?;
            return a.reply(done, reasoning);
        }
        a.completion(&decode(&raw)?, &notify)?;
        return a.reply(true, reasoning);
    };
    let mut size = 0;
    let mut lines = Lines::new(frames, native);
    while let Some(chunk) = r.chunk().await.map_err(network_error)? {
        if chunk.len() > REPLY_LIMIT - size {
            return Err("Provider response exceeds 16 MiB.".into());
        }
        size += chunk.len();
        lines.push(&chunk, &mut a, &notify)?;
        if lines.done() {
            break;
        }
    }
    let done = lines.end(&mut a, &notify)?;
    a.reply(done, reasoning)
}

pub(in super::super) fn start(a: &Agent, messages: Vec<Value>, tools: Vec<Value>, ctx: egui::Context, projection: Projection) -> Result<Job, String> {
    if !a.completions {
        return Err(format!("{} requires the Responses API. Select a Chat Completions model.", a.name));
    }
    let progress = Arc::new(Mutex::new(Progress::default()));
    let pending = Arc::clone(&progress);
    let retry = Arc::clone(&progress);
    let paint = ctx.clone();
    let viewport = ctx.viewport_id();
    let a = a.clone();
    let repaint = paint.clone();
    let run = chat(
        a,
        messages,
        tools,
        projection,
        move |delta| {
            if let Ok(mut p) = pending.lock() {
                match delta {
                    Delta::Text(s) => p.text.push_str(s),
                    Delta::Thinking(s) => p.thinking.push_str(s),
                }
            }
            paint.request_repaint_of(viewport);
        },
        move || {
            if let Ok(mut p) = retry.lock() {
                p.finding_final = Some((std::mem::take(&mut p.thinking), std::mem::take(&mut p.text)));
            }
            repaint.request_repaint_of(viewport);
        },
    );
    Ok(Job { task: spawn(ctx, run)?, progress })
}

type Listings = (Vec<Agent>, Vec<String>);

pub(in super::super) fn summarize(a: &Agent, messages: Vec<Value>, limit: u32, ctx: egui::Context) -> Result<Job, String> {
    let mut a = a.clone();
    a.max_output_tokens = Some(a.max_output_tokens.unwrap_or(limit).min(limit));
    start(&a, messages, Vec::new(), ctx, None)
}
pub(in super::super) struct Discovery(Task<Listings>);
impl Discovery {
    pub(in super::super) fn poll(&mut self) -> Option<Result<Listings, String>> {
        self.0.poll()
    }
}
async fn list(h: &Host) -> Result<Vec<(String, Option<u32>)>, String> {
    let a = &h.template;
    let native = a.protocol == Protocol::Ollama;
    let req = client(a)?.get(format!("{}/{}", a.endpoint, if native { "api/tags" } else { "models" })).timeout(Duration::from_secs(10));
    let entries = decode(&bytes(send(authorize(req, a)?).await?, CATALOG_LIMIT).await?)?;
    let models = entries[if native { "models" } else { "data" }].as_array().ok_or("Provider returned no model list.")?;
    if models.len() > 2048 {
        return Err("Provider advertised more than 2048 models.".into());
    }
    Ok(models
        .iter()
        .filter_map(|v| {
            let id = v[if native { "name" } else { "id" }].as_str().filter(|n| !n.is_empty() && n.len() <= 512)?;
            let limit = ["max_model_len", "context_length"].iter().filter_map(|&k| u32::try_from(v[k].as_u64()?).ok().filter(|&n| n > 0)).min();
            Some((id.to_owned(), limit))
        })
        .collect())
}
/// Discovery can cap a context budget; it never grants capabilities or changes credentials.
pub(in super::super) fn discover(ctx: egui::Context) -> Result<Discovery, String> {
    let mut catalog = settings::catalog()?;
    Ok(Discovery(spawn(ctx, async move {
        let mut issues = Vec::new();
        for h in catalog.hosts.iter().filter(|h| h.discover) {
            match list(h).await {
                Ok(models) => {
                    for (id, limit) in models {
                        let mut existing = catalog.declared.iter_mut().filter(|a| a.provider == h.template.provider && a.model == id).peekable();
                        if existing.peek().is_some() {
                            for agent in existing {
                                agent.reported_context = limit;
                            }
                        } else {
                            let mut a = h.template.clone();
                            a.name.clone_from(&id);
                            a.model = id;
                            a.reported_context = limit;
                            catalog.declared.push(a);
                        }
                    }
                }
                Err(e) => issues.push(format!("{}: {e}", h.template.provider)),
            }
        }
        catalog.sort();
        Ok((catalog.declared, issues))
    })?))
}

#[cfg(test)]
mod vision_tests {
    use super::*;

    #[test]
    fn native_vision_keeps_tool_receipts_and_places_pixels_only_on_the_user_message() {
        let mut messages = [
            json!({"role":"assistant","tool_calls":[{"id":"c","function":{"name":"inspect_visual","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"c","content":"prepared"}),
            json!({"role":"user","content":[{"type":"text","text":"The selected chart"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}]}),
        ];
        native_history(&mut messages).unwrap();
        assert_eq!(messages[1], json!({"role":"tool","tool_name":"inspect_visual","content":"prepared"}));
        assert_eq!(messages[2], json!({"role":"user","content":"The selected chart","images":["AA=="]}));
        let mut linked = [json!({"role":"user","content":[{"type":"image_url","image_url":{"url":"https://external.invalid/picture.png"}}]})];
        assert!(native_history(&mut linked).is_err());
    }
}
