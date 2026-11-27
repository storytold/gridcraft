use std::collections::HashMap;
use std::future::Future;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::{AbortHandle, Abortable};
use reqwest::{Client, RequestBuilder, Response};
use serde_json::{Value, json};

use super::settings::{self, Agent, Host, Protocol};
use super::stream::{Assembly, Lines, Reply, decode};

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
    Text(String),
    Done(Result<Reply, String>),
}
pub(in super::super) struct Job {
    task: Task<Reply>,
    text: Arc<Mutex<String>>,
}
impl Job {
    pub(in super::super) fn poll(&mut self) -> Option<Event> {
        let mut text = self.text.lock().ok()?;
        if !text.is_empty() {
            return Some(Event::Text(std::mem::take(&mut *text)));
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
    let key = a.key_env.as_ref().and_then(std::env::var_os).filter(|v| !v.is_empty());
    if key.is_none() && a.headers.is_empty() {
        return Ok(req);
    }
    let url = reqwest::Url::parse(&a.endpoint).map_err(|_| "Invalid provider URL.")?;
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    if url.scheme() != "https" && host != "localhost" && !host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return Err("Use HTTPS to send API keys or custom headers outside loopback.".into());
    }
    let req = req.headers(a.headers.clone());
    match key {
        Some(k) => Ok(req.bearer_auth(k.to_str().ok_or("The API key is not valid text.")?)),
        None => Ok(req),
    }
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
async fn chat(a: Agent, messages: Vec<Value>, tools: Vec<Value>, notify: impl Fn(&str)) -> Result<Reply, String> {
    let native = a.protocol == Protocol::Ollama;
    let url = format!("{}/{}", a.endpoint, if native { "api/chat" } else { "chat/completions" });
    let payload = serde_json::to_vec(&payload(&a, messages, tools)?).map_err(|_| "Cannot encode the request.")?;
    if payload.len() > REQUEST_LIMIT {
        return Err("Conversation exceeds the 8 MiB request limit. Start a new thread or attach a smaller range.".into());
    }
    let req = client(&a)?.post(url).header("content-type", "application/json").body(payload);
    let response = send(authorize(req, &a)?).await?;
    let mut reply = receive(response, native, a.retain_reasoning(), notify).await?;
    if reply.message.get("reasoning_content").is_some() {
        reply.message[THINKING_TAG] = a.thinking.wire(a.protocol);
    }
    Ok(reply)
}

async fn receive(mut r: Response, native: bool, reasoning: bool, notify: impl Fn(&str)) -> Result<Reply, String> {
    let media = r.headers().get("content-type").and_then(|h| h.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim();
    let stream = media.eq_ignore_ascii_case("text/event-stream")
        || media.eq_ignore_ascii_case("application/x-ndjson")
        || media.eq_ignore_ascii_case("application/ndjson");
    let mut a = Assembly::default();
    if !stream && !native {
        let raw = bytes(r, REPLY_LIMIT).await?;
        // A few local runtimes emit SSE without setting its content type.
        if raw.trim_ascii_start().starts_with(b"data:") {
            let mut lines = Lines::default();
            lines.push(&raw, false, &mut a, &notify)?;
            let done = lines.end(false, &mut a, &notify)?;
            return a.reply(done, reasoning);
        }
        a.completion(&decode(&raw)?, &notify)?;
        return a.reply(true, reasoning);
    }
    let mut size = 0;
    let mut lines = Lines::default();
    while let Some(chunk) = r.chunk().await.map_err(network_error)? {
        if chunk.len() > REPLY_LIMIT - size {
            return Err("Provider response exceeds 16 MiB.".into());
        }
        size += chunk.len();
        lines.push(&chunk, native, &mut a, &notify)?;
        if lines.done() {
            break;
        }
    }
    let done = lines.end(native, &mut a, &notify)?;
    a.reply(done, reasoning)
}

pub(in super::super) fn start(a: &Agent, messages: Vec<Value>, tools: Vec<Value>, ctx: egui::Context) -> Result<Job, String> {
    if !a.completions {
        return Err(format!("{} requires the Responses API. Select a Chat Completions model.", a.name));
    }
    let text = Arc::new(Mutex::new(String::new()));
    let pending = Arc::clone(&text);
    let paint = ctx.clone();
    let viewport = ctx.viewport_id();
    let a = a.clone();
    let run = chat(a, messages, tools, move |delta| {
        if let Ok(mut s) = pending.lock() {
            s.push_str(delta);
        }
        paint.request_repaint_of(viewport);
    });
    Ok(Job { task: spawn(ctx, run)?, text })
}

type Listings = (Vec<Agent>, Vec<String>);
pub(in super::super) struct Discovery(Task<Listings>);
impl Discovery {
    pub(in super::super) fn poll(&mut self) -> Option<Result<Listings, String>> {
        self.0.poll()
    }
}
async fn list(h: &Host) -> Result<Vec<String>, String> {
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
        .filter_map(|v| v[if native { "name" } else { "id" }].as_str())
        .filter(|n| !n.is_empty() && n.len() <= 512)
        .map(str::to_string)
        .collect())
}
/// Configuration is the authority; discovery only adds new ids, never replaces
/// declared capabilities, display names, credentials or the configured default.
pub(in super::super) fn discover(ctx: egui::Context) -> Result<Discovery, String> {
    let mut catalog = settings::catalog()?;
    Ok(Discovery(spawn(ctx, async move {
        let mut issues = Vec::new();
        for h in catalog.hosts.iter().filter(|h| h.discover) {
            match list(h).await {
                Ok(models) => {
                    for id in models {
                        if !catalog.declared.iter().any(|a| a.provider == h.template.provider && a.model == id) {
                            let mut a = h.template.clone();
                            a.name.clone_from(&id);
                            a.model = id;
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
