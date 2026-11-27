//! User-initiated MCP connections. The official Rust client owns negotiation, framing,
//! cancellation and transport shutdown. See https://modelcontextprotocol.io/specification/.
use std::{collections::BTreeSet, process::Stdio, sync::Arc, time::Duration};

use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResponse, ClientRequest, ListToolsRequest, PaginatedRequestParams, ServerResult},
    service::{Peer, RunningService},
    transport::{StreamableHttpClientTransport, TokioChildProcess, streamable_http_client::StreamableHttpClientTransportConfig},
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::{Entry, ServerConfig};

type Session = RunningService<RoleClient, ()>;
type Sender = oneshot::Sender<Result<Value, String>>;
type Catalog = Result<Vec<Arc<Entry>>, String>;
pub(super) enum Request {
    Call(Call),
    Refresh(oneshot::Sender<Catalog>),
}
pub(super) struct Call {
    pub entry: Arc<Entry>,
    pub arguments: serde_json::Map<String, Value>,
    pub reply: Sender,
}
pub(super) struct Connection {
    stop: Option<oneshot::Sender<()>>,
    pub calls: mpsc::Sender<Request>,
    pub catalog: Option<oneshot::Receiver<Catalog>>,
}
impl Connection {
    pub fn refresh(&mut self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.calls.try_send(Request::Refresh(tx)).map_err(|_| "MCP worker is busy or disconnected.")?;
        self.catalog = Some(rx);
        Ok(())
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

async fn connect(cfg: &ServerConfig) -> Result<Session, String> {
    if let Some(exe) = &cfg.command {
        let mut cmd = tokio::process::Command::new(exe);
        // Only the operator's settings can select a program/arguments. No shell expansion,
        // workbook working directory, or wholesale inheritance of API keys.
        cmd.env_clear().args(&cfg.args).kill_on_drop(true);
        for var in [
            "PATH",
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "SYSTEMROOT",
            "WINDIR",
            "PATHEXT",
            "TEMP",
            "TMP",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
        ] {
            if let Some(v) = std::env::var_os(var) {
                cmd.env(var, v);
            }
        }
        cmd.envs(&cfg.env);
        if let Some(dir) = std::env::home_dir() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let (transport, _) = TokioChildProcess::builder(cmd)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start the configured MCP executable ({:?}). Check its path and arguments.", e.kind()))?;
        return ().serve(transport).await.map_err(|_| "MCP initialization failed. Check that the program speaks MCP on stdout.".into());
    }
    let url = cfg.url.as_deref().ok_or("Missing MCP endpoint.")?;
    let mut config = StreamableHttpClientTransportConfig::with_uri(url).reinit_on_expired_session(false);
    config.max_json_response_size = 4 * 1024 * 1024;
    config.max_sse_event_size = config.max_json_response_size;
    config.custom_headers = cfg.http_headers()?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "Cannot create MCP HTTP client.")?;
    ().serve(StreamableHttpClientTransport::with_client(client, config))
        .await
        .map_err(|_| "MCP initialization failed. Check the endpoint, authorization headers and TLS configuration.".into())
}

async fn list(session: &Peer<RoleClient>, cfg: &ServerConfig, server: &str) -> Result<Vec<Arc<Entry>>, String> {
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut out = Vec::new();
    let mut bytes = 0;
    for _ in 0..16 {
        // A manual refresh asks the running server, even if its previous catalog had a TTL.
        let request = ClientRequest::ListToolsRequest(ListToolsRequest {
            method: Default::default(),
            params: cursor.take().map(|c| PaginatedRequestParams::default().with_cursor(Some(c))),
            extensions: Default::default(),
        });
        let ServerResult::ListToolsResult(page) = session.send_request(request).await.map_err(|_| "MCP tools/list failed.")? else {
            return Err("Unexpected MCP catalog response.".into());
        };
        for t in page.tools {
            if cfg.allowed_tools.as_ref().is_some_and(|set| !set.contains(t.name.as_ref())) {
                continue;
            }
            if t.name.is_empty() || t.name.len() > 256 || !names.insert(t.name.to_string()) {
                return Err("Invalid or duplicate MCP tool name.".into());
            }
            let schema = Value::Object((*t.input_schema).clone());
            if schema["type"] != "object" {
                return Err("MCP tool input schema must describe an object.".into());
            }
            let description: String = t.description.as_deref().unwrap_or("").chars().take(1024).collect();
            bytes += schema.to_string().len() + description.len();
            if bytes > 64 * 1024 || out.len() >= 64 {
                return Err("MCP catalog is too large. Set allowed_tools to a smaller list.".into());
            }
            out.push(Arc::new(Entry { server: server.to_owned(), tool: t.name.into_owned(), description, schema }));
        }
        let Some(c) = page.next_cursor else { return Ok(out) };
        if !seen.insert(c.clone()) {
            return Err("MCP server repeated its catalog cursor.".into());
        }
        cursor = Some(c);
    }
    Err("MCP tool catalog exceeded the pagination limit.".into())
}

async fn invoke(session: &Peer<RoleClient>, tool: String, arguments: serde_json::Map<String, Value>) -> Result<Value, String> {
    let result = session
        .call_tool_once(CallToolRequestParams::new(tool).with_arguments(arguments))
        .await
        .map_err(|_| "MCP request failed. External side effects may already have occurred; the call was not retried.")?;
    let CallToolResponse::Complete(r) = result else {
        return Err("The MCP server requested additional client input; this tool call was not continued.".into());
    };
    let mut remaining = 32 * 1024;
    let mut truncated = false;
    let mut take = |s: &str| {
        let end = s.floor_char_boundary(s.len().min(remaining));
        truncated |= end < s.len();
        remaining -= end;
        s[..end].to_owned()
    };
    let structured = r.structured_content.as_ref().map(|v| take(&v.to_string()));
    let text: Vec<_> = r.content.iter().filter_map(|c| c.as_text()).take(32).map(|c| take(&c.text)).collect();
    Ok(json!({"isError":r.is_error.unwrap_or(false),"structuredJsonText":structured,"text":text,"truncated":truncated || r.content.len()>32,
        "omittedNonText":r.content.iter().any(|c|c.as_text().is_none())}))
}

async fn catalog(s: &Peer<RoleClient>, cfg: &ServerConfig, name: &str) -> Catalog {
    tokio::time::timeout(Duration::from_secs(20), list(s, cfg, name)).await.unwrap_or_else(|_| Err("MCP discovery timed out.".into()))
}

async fn serve(
    name: String,
    cfg: ServerConfig,
    ctx: &egui::Context,
    view: egui::ViewportId,
    catalog_tx: oneshot::Sender<Catalog>,
    mut rx: mpsc::Receiver<Request>,
    mut stop: oneshot::Receiver<()>,
) {
    let result = tokio::select! {
        biased;
        _ = &mut stop => return,
        r = tokio::time::timeout(Duration::from_secs(20),connect(&cfg)) => r.unwrap_or_else(|_|Err("MCP initialization timed out.".into())),
    };
    let session = match result {
        Ok(session) => session,
        Err(err) => {
            let _ = catalog_tx.send(Err(err));
            return;
        }
    };
    let peer = session.peer().clone();
    let cancel = session.cancellation_token();
    let ended = session.waiting();
    tokio::pin!(ended);
    let handle_requests = async {
        let initial = catalog(&peer, &cfg, &name).await;
        let mut confirmed = initial.as_ref().ok().cloned();
        let _ = catalog_tx.send(initial);
        ctx.request_repaint_of(view);
        if confirmed.is_some() {
            while let Some(request) = rx.recv().await {
                match request {
                    Request::Refresh(tx) => {
                        let listed = catalog(&peer, &cfg, &name).await;
                        confirmed = listed.as_ref().ok().cloned();
                        let _ = tx.send(listed);
                    }
                    Request::Call(mut c) => {
                        // A click can enqueue an approval while refresh is already queued.
                        // Check the actual catalog again in worker order, not just UI order.
                        if !confirmed.as_ref().is_some_and(|items| items.iter().any(|e| Arc::ptr_eq(e, &c.entry))) {
                            let _ = c.reply.send(Err("MCP catalog changed; request this tool again.".into()));
                            ctx.request_repaint_of(view);
                            continue;
                        }
                        let result = tokio::select! {
                            biased;
                            () = c.reply.closed() => continue,
                            r = tokio::time::timeout(Duration::from_secs(cfg.timeout_seconds),invoke(&peer,c.entry.tool.clone(),c.arguments)) =>
                                r.unwrap_or_else(|_| Err("MCP tool timed out; not retried.".into())),
                        };
                        let _ = c.reply.send(result);
                    }
                }
                ctx.request_repaint_of(view);
            }
        }
    };
    // Observing transport exit here also revokes a disconnected server's catalog while idle.
    tokio::select! { biased; _ = &mut ended => return, _ = stop => {}, () = handle_requests => {} }
    // The UI never waits for shutdown. Let the transport close the session/reap the child
    // even when the operator disconnects in the middle of discovery or a tool invocation.
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(2), ended).await;
}

pub(super) fn start(name: String, cfg: ServerConfig, ctx: egui::Context) -> Result<Connection, String> {
    cfg.validate()?;
    let viewport = ctx.viewport_id();
    let (catalog_tx, catalog) = oneshot::channel();
    let (calls, rx) = mpsc::channel(1);
    let (stop, stopped) = oneshot::channel();
    std::thread::Builder::new()
        .name("gridcraft-wingman-mcp".into())
        .spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(serve(name, cfg, &ctx, viewport, catalog_tx, rx, stopped));
                rt.shutdown_timeout(Duration::from_millis(200));
            }
            ctx.request_repaint_of(viewport);
        })
        .map_err(|_| "Cannot start MCP worker.")?;
    Ok(Connection { stop: Some(stop), calls, catalog: Some(catalog) })
}
