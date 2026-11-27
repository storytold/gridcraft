//! Optional MCP tool host. Only a user action in the MCP profile opens a configured server;
//! model and workbook content cannot create a server or grant an external tool permission.
mod connection;

use super::provider;
use crate::i18n::Language;
use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};
use tokio::sync::oneshot;

#[derive(Clone, Deserialize)]
pub(crate) struct ServerConfig {
    pub command: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    header_env: BTreeMap<String, String>,
    pub allowed_tools: Option<BTreeSet<String>>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "enabled")]
    enabled: bool,
}
fn default_timeout() -> u64 {
    120
}
fn enabled() -> bool {
    true
}
impl ServerConfig {
    fn validate(&self) -> Result<(), String> {
        if !(1..=3600).contains(&self.timeout_seconds) {
            return Err("Set timeout_seconds between 1 and 3600.".into());
        }
        match (&self.command, &self.url) {
            (Some(cmd), None) if !cmd.trim().is_empty() && self.headers.is_empty() && self.header_env.is_empty() => {
                if self.env.iter().any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
                    || cmd.contains('\0')
                    || self.args.iter().any(|s| s.contains('\0'))
                {
                    return Err("Invalid MCP program arguments or environment.".into());
                }
                Ok(())
            }
            (None, Some(s)) if self.args.is_empty() && self.env.is_empty() => {
                let url = reqwest::Url::parse(s).map_err(|_| "Invalid MCP URL.")?;
                let local = url
                    .host_str()
                    .is_some_and(|s| s == "localhost" || s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().is_ok_and(|a| a.is_loopback()));
                if !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !(url.scheme() == "https" || (url.scheme() == "http" && local))
                {
                    return Err("Use an HTTPS MCP endpoint or loopback HTTP, without URL credentials, query or fragment.".into());
                }
                Ok(())
            }
            _ => Err("Set either command (args/env) or url (headers/header_env) for each MCP server.".into()),
        }
    }
    fn http_headers(&self) -> Result<HashMap<HeaderName, HeaderValue>, String> {
        let mut h = HashMap::new();
        let mut insert = |name: &str, value: &str| -> Result<(), String> {
            let key = HeaderName::from_bytes(name.as_bytes()).map_err(|_| "Invalid MCP header name.")?;
            if matches!(
                key.as_str(),
                "host"
                    | "connection"
                    | "content-length"
                    | "transfer-encoding"
                    | "content-type"
                    | "accept"
                    | "origin"
                    | "cookie"
                    | "proxy-authorization"
            ) || key.as_str().starts_with("mcp-")
            {
                return Err("Cannot override transport-owned MCP headers.".into());
            }
            let mut value = HeaderValue::from_str(value).map_err(|_| "Invalid MCP header value.")?;
            value.set_sensitive(true);
            if h.insert(key, value).is_some() {
                return Err("Duplicate MCP header.".into());
            }
            Ok(())
        };
        for (k, v) in &self.headers {
            insert(k, v)?;
        }
        for (k, n) in &self.header_env {
            if n.is_empty() || !n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                return Err("Invalid MCP header environment variable.".into());
            }
            insert(k, &std::env::var(n).map_err(|_| "A configured MCP header environment variable is missing.")?)?;
        }
        Ok(h)
    }
}

#[derive(Clone)]
pub(super) struct Entry {
    server: String,
    tool: String,
    description: String,
    schema: Value,
}
impl Entry {
    pub fn label(&self) -> String {
        format!("{} → {}", self.server, self.tool)
    }
    // Return stable names across refreshes, independent of the server's listing order.
    fn wire_name(&self) -> String {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let part = |s: &str, n| s.chars().take(n).map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>();
        let mut h = DefaultHasher::new();
        (&self.server, &self.tool).hash(&mut h);
        format!("mcp_{}_{}_{:016x}", part(&self.server, 12), part(&self.tool, 29), h.finish())
    }
}
pub(super) type Offered = BTreeMap<String, Arc<Entry>>;
pub(super) struct Pending {
    pub id: String,
    pub label: String,
    rx: oneshot::Receiver<Result<Value, String>>,
}
impl Pending {
    pub fn poll(&mut self) -> Option<Result<Value, String>> {
        poll(&mut self.rx)
    }
}
fn poll<T>(rx: &mut oneshot::Receiver<Result<T, String>>) -> Option<Result<T, String>> {
    match rx.try_recv() {
        Ok(msg) => Some(msg),
        Err(oneshot::error::TryRecvError::Empty) => None,
        Err(oneshot::error::TryRecvError::Closed) => Some(Err("MCP worker disconnected.".into())),
    }
}
struct Live {
    conn: connection::Connection,
    tools: Option<Vec<Arc<Entry>>>,
}
#[derive(Default)]
pub(super) struct Hub {
    settings: BTreeMap<String, ServerConfig>,
    live: BTreeMap<String, Live>,
    error: String,
}
impl Hub {
    pub fn reload(&mut self) {
        self.disconnect();
        self.settings.clear();
        self.error.clear();
        match provider::mcp_servers() {
            Ok(mut s) => {
                s.retain(|_, c| c.enabled);
                if s.len() > 32 || s.keys().any(|k| k.trim().is_empty() || k.len() > 80 || k.chars().any(char::is_control)) {
                    self.error = "Invalid MCP catalog: use up to 32 named servers.".into();
                } else {
                    self.settings = s;
                }
            }
            Err(e) => self.error = e,
        }
    }
    pub fn disconnect(&mut self) {
        self.live.clear();
    }
    pub fn tick(&mut self) {
        self.live.retain(|name, l| {
            if l.tools.is_none()
                && let Some(msg) = poll(&mut l.conn.catalog)
            {
                match msg {
                    Ok(v) => l.tools = Some(v),
                    Err(e) => {
                        self.error = format!("{name}: {e}");
                        return false;
                    }
                }
            }
            if l.conn.calls.is_closed() {
                self.error = format!("{name}: MCP server disconnected.");
                return false;
            }
            true
        });
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, l: Language, can_connect: bool) {
        if self.settings.is_empty() {
            ui.weak(l.tr("Add context_servers to wingman.json to connect MCP tools."));
        }
        for (name, cfg) in &self.settings {
            let active = self.live.get(name);
            let state =
                active.map(|a| a.tools.as_ref().map_or_else(|| l.tr("Connecting…").to_string(), |v| format!("{} {}", v.len(), l.tr("tools"))));
            let is_live = active.is_some();
            ui.push_id(name, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(name).on_hover_text(cfg.command.as_ref().or(cfg.url.as_ref()).map(String::as_str).unwrap_or(""));
                    if let Some(s) = state {
                        ui.weak(s);
                    }
                    if is_live {
                        if let Some(live) = self.live.get_mut(name)
                            && let Some(tools) = &live.tools
                        {
                            let refresh = ui
                                .menu_button(l.tr("Tools"), |ui| {
                                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                        for t in tools {
                                            ui.label(&t.tool).on_hover_text(&t.description);
                                        }
                                        if tools.is_empty() {
                                            ui.weak(l.tr("No matching tools."));
                                        }
                                    });
                                    let clicked = ui.button(l.tr("Refresh tools")).clicked();
                                    if clicked {
                                        ui.close();
                                    }
                                    clicked
                                })
                                .inner
                                == Some(true);
                            if refresh {
                                match live.conn.refresh() {
                                    Ok(()) => live.tools = None,
                                    Err(e) => self.error = e,
                                }
                            }
                        }
                        if ui.button(l.tr("Disconnect")).clicked() {
                            self.live.remove(name);
                        }
                    } else if ui
                        .add_enabled(can_connect, egui::Button::new(l.tr("Connect")))
                        .on_hover_text(l.tr("Start this configured server and list its tools"))
                        .clicked()
                    {
                        match connection::start(name.clone(), cfg.clone(), ui.ctx().clone()) {
                            Ok(conn) => {
                                self.live.insert(name.clone(), Live { conn, tools: None });
                                self.error.clear();
                            }
                            Err(e) => self.error = format!("{name}: {e}"),
                        }
                    }
                })
            });
        }
        if !self.error.is_empty() {
            ui.label(&self.error);
        }
    }
    pub fn offer(&self, schemas: &mut Vec<Value>) -> Result<Offered, String> {
        if self.live.values().any(|v| v.tools.is_none()) {
            return Err("MCP discovery is still in progress. Send after the tool list is ready.".into());
        }
        let mut offered = Offered::new();
        let mut size = 0;
        for server in self.settings.keys() {
            let Some(entries) = self.live.get(server).and_then(|l| l.tools.as_ref()) else {
                continue;
            };
            for e in entries {
                let name = e.wire_name();
                let s = json!({"type":"function","function":{"name":name,"description":format!("External tool {}. {}",e.label(),e.description),"parameters":e.schema}});
                size += s.to_string().len();
                if size > 128 * 1024 || offered.len() >= 64 {
                    return Err("Too many MCP tools for this turn; disconnect a server or narrow allowed_tools.".into());
                }
                schemas.push(s);
                if offered.insert(name, Arc::clone(e)).is_some() {
                    return Err("External tool names collided.".into());
                }
            }
        }
        Ok(offered)
    }
    pub fn call(&self, entry: &Arc<Entry>, id: String, arguments: &str) -> Result<Pending, String> {
        let live = self.live.get(&entry.server).ok_or("MCP server is disconnected.")?;
        if !live.tools.as_ref().is_some_and(|v| v.iter().any(|e| Arc::ptr_eq(e, entry))) {
            return Err("MCP catalog changed; send a new request.".into());
        }
        if arguments.len() > 64 * 1024 {
            return Err("MCP arguments exceed 64 KiB.".into());
        }
        let arguments = serde_json::from_str(arguments).map_err(|_| "Expected MCP tool arguments as a JSON object.")?;
        let (reply, rx) = oneshot::channel();
        live.conn
            .calls
            .try_send(connection::Request::Call(connection::Call { tool: entry.tool.clone(), arguments, reply }))
            .map_err(|_| "The MCP server is busy or disconnected.")?;
        Ok(Pending { id, label: entry.label(), rx })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_settings_do_not_cross_between_local_programs_and_network_endpoints() {
        let valid = |v| serde_json::from_value::<ServerConfig>(v).unwrap().validate().is_ok();
        assert!(valid(json!({"url":"http://[::1]:9000/mcp"})));
        assert!(valid(json!({"command":"bun","args":["server.js"],"env":{"PORT":"3002"}})));
        assert!(!valid(json!({"command":"bun","url":"https://mcp.example/"})));
        assert!(!valid(json!({"command":"bun","headers":{"Authorization":"fixture"}})));
        assert!(!valid(json!({"url":"https://mcp.example/","env":{"PORT":"3002"}})));
        assert!(!valid(json!({"url":"http://mcp.example/"})));
        assert!(!valid(json!({"url":"https://name@example.org/mcp"})));
    }

    #[test]
    fn similarly_named_tools_keep_distinct_provider_names_and_disconnect_revokes_calls() {
        let entry = |server: &str| {
            Arc::new(Entry { server: server.into(), tool: "search/v2".into(), description: String::new(), schema: json!({"type":"object"}) })
        };
        let a = entry("web.search");
        let b = entry("web-search");
        assert_ne!(a.wire_name(), b.wire_name());
        assert_eq!(a.wire_name(), entry("web.search").wire_name());
        assert!(Hub::default().call(&a, "call-1".into(), "{}").is_err());
    }
}
