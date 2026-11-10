//! An in-process engine session that answers the engine-level control-channel methods itself.

use std::sync::Arc;

use gridcraft_engine::automation::{self, Automation};
use gridcraft_engine::{Session, Target};
use serde_json::{Value, json};

use crate::backend::Backend;

/// Error for methods that need a window.
pub(crate) const NO_UI: &str = "no UI in headless mode: start the app with `gridcraft --control 7979` and run the MCP server \
with `gridcraft-cli mcp --connect 7979`";

/// Headless backend: a [`Session`] with one blank workbook to start with.
pub struct Headless {
    pub session: Session,
    automation: Option<Arc<Automation>>,
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

impl Headless {
    /// A session with a blank workbook (`Book1`) open.
    pub fn new() -> Self {
        let mut session = Session::new();
        session.new_workbook();
        Self { session, automation: None }
    }

    /// A session wrapping an existing one (e.g. with a file already open).
    pub fn with_session(session: Session) -> Self {
        Self { session, automation: None }
    }

    /// Hold every request to `automation`'s folders and commands
    /// (`gridcraft-cli mcp --automation-read-root … --automation-write-root … --allow-commands …`).
    pub fn with_automation(mut self, automation: Option<Arc<Automation>>) -> Self {
        self.automation = automation;
        self
    }

    /// Answer one control-channel `method`. A command the client names (`engine.execute`, or a
    /// command id as the method, such as `document.inspect`) must be on the command list in force.
    fn answer(&mut self, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "ping" => Ok(json!("pong")),
            "engine.execute" => {
                let id = params.get("command").and_then(Value::as_str).ok_or("engine.execute: missing `command`")?.to_string();
                automation::check_command(&id).map_err(|e| e.to_string())?;
                let p = params.get("params").cloned().unwrap_or_else(|| json!({}));
                if !(p.is_object() || p.is_null()) {
                    return Err(format!("engine.execute: `params` of `{id}` must be an object"));
                }
                let target = Target::from_json(&params).map_err(|e| e.to_string())?;
                self.execute(target, &id, p)
            }
            "engine.commands" => Ok(self.commands(&params)),
            "engine.journal" => Ok(Value::Array(self.session.journal.iter().map(|(id, p)| json!({"command": id, "params": p})).collect())),
            m if m.starts_with("ui.") || m.starts_with("app.screenshot") => Err(format!("{m}: {NO_UI}")),
            // Read-only engine methods (`document.inspect`, `sheet.read`, `cell.get`, …) are
            // commands with the same id, held to --allow-commands like any other.
            // Their params may name a `window` or `doc` to act in, except for the window commands,
            // whose own params those are.
            m if gridcraft_engine::find_command(m).is_some() => {
                automation::check_command(m).map_err(|e| e.to_string())?;
                if gridcraft_engine::windows::is_window_command(m) {
                    return self.execute(Target::Active, m, params);
                }
                let target = Target::from_json(&params).map_err(|e| e.to_string())?;
                let mut params = params;
                if let Some(o) = params.as_object_mut() {
                    o.remove("window");
                    o.remove("doc");
                }
                self.execute(target, m, params)
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }

    /// Run command `id` in `target` with no UI: programmatic calls never open dialogs.
    fn execute(&mut self, target: Target, id: &str, params: Value) -> Result<Value, String> {
        // Programmatic calls never open dialogs; anything a command queued for a UI is dropped.
        self.session.execute_headless_in(target, id, params).map_err(|e| e.to_string())
    }

    /// The commands `engine.commands` lists (filtered by `search`): with a command list in force,
    /// only those on it.
    fn commands(&self, params: &Value) -> Value {
        let q = params.get("search").and_then(Value::as_str).map(str::to_ascii_lowercase);
        let list: Vec<Value> = self
            .session
            .commands()
            .into_iter()
            // With --allow-commands, the list shows only what the client may run.
            .filter(|c| automation::current().is_none_or(|a| a.commands().is_none_or(|l| l.allows(c.id))))
            .filter(|c| {
                q.as_ref().is_none_or(|q| {
                    c.id.to_ascii_lowercase().contains(q.as_str())
                        || c.label.to_ascii_lowercase().contains(q.as_str())
                        || c.menu.iter().any(|m| m.to_ascii_lowercase().contains(q.as_str()))
                })
            })
            .map(|c| serde_json::to_value(c).unwrap_or_default())
            .collect();
        Value::Array(list)
    }
}

impl Backend for Headless {
    /// Answer one request with this server's automation (folders and commands) in force on the
    /// calling thread, and the previous scope restored afterwards.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let params = if params.is_null() { json!({}) } else { params };
        let scope = self.automation.clone();
        automation::confine(scope.as_ref(), || self.answer(method, params))
    }

    fn has_ui(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        "headless (in-process engine, no window)".to_string()
    }
}
