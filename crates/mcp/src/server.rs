//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources methods, backed by
//! `craft_mcp_server`.

use std::io::{BufRead, Write};

use craft_mcp_server::{Contents, Info, Options, Tools};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{call_tool, check_arguments, tool_definitions};

/// The MCP revision we implement and the revisions we can speak.
pub use craft_mcp_server::{PROTOCOL_VERSION, SUPPORTED_VERSIONS};

/// Longest accepted message line (16 MB): bigger payloads belong in files.
pub(crate) const MAX_LINE: usize = 16 * 1024 * 1024;

const INSTRUCTIONS: &str = "GridCraft is a spreadsheet app (an Excel clone). Cells use A1 notation (\"B2\", \"A1:D10\", \
\"Sheet2!A1:B3\"). Every action is a command: find ids and parameters with list_commands and run any of them with \
execute_command. Convenience tools cover the common flow: write_range / set_cell to enter values and formulas (strings \
starting with = are formulas), read_range / get_cell to read computed values back, format_range, create_table, \
insert_chart, sort_range, filter, evaluate_formula (try a formula without changing the sheet), undo / redo, \
open_workbook / save_workbook. inspect_workbook summarizes sheets, used ranges, tables, charts and names. \
list_functions lists the worksheet functions.";

/// Resource URIs.
pub const WORKBOOK_URI: &str = "gridcraft://workbook";
pub const COMMANDS_URI: &str = "gridcraft://commands";
pub const FUNCTIONS_URI: &str = "gridcraft://functions";

struct GridcraftTools {
    backend: Box<dyn Backend>,
}

impl Tools for GridcraftTools {
    fn instructions(&self) -> String {
        format!("{INSTRUCTIONS} Backend: {}.", self.backend.describe())
    }

    fn list(&mut self) -> Vec<Value> {
        tool_definitions()
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        if let Err(e) = check_arguments(name, args) {
            return craft_mcp_server::text_result(e, true);
        }
        call_tool(self.backend.as_mut(), name, args).to_value()
    }

    fn resources(&self) -> Vec<Value> {
        vec![
            json!({"uri": WORKBOOK_URI, "name": "workbook", "title": "Active workbook", "description": "Sheets, used ranges, tables, charts, names, selection and undo history of the active workbook (document.inspect)", "mimeType": "application/json"}),
            json!({"uri": COMMANDS_URI, "name": "commands", "title": "Commands", "description": "Every command: id, label, ribbon path, shortcut, params, enabled (engine.commands)", "mimeType": "application/json"}),
            json!({"uri": FUNCTIONS_URI, "name": "functions", "title": "Worksheet functions", "description": "Every worksheet function: name, category, signature, description (formulas.functions)", "mimeType": "application/json"}),
        ]
    }

    fn read_resource(&mut self, uri: &str) -> Option<Result<Contents, String>> {
        let result = match uri {
            WORKBOOK_URI => self.backend.call("document.inspect", json!({})),
            COMMANDS_URI => self.backend.call("engine.commands", json!({})),
            FUNCTIONS_URI => self.backend.call("engine.execute", json!({"command": "formulas.functions", "params": {}})),
            _ => return None,
        };
        Some(result.map(|v| Contents::Text { mime_type: "application/json".into(), text: serde_json::to_string_pretty(&v).unwrap_or_default() }))
    }
}

/// An MCP server bound to one backend.
pub struct Server {
    inner: craft_mcp_server::Server<GridcraftTools>,
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let info = Info { name: "gridcraft", title: "GridCraft", version: env!("CARGO_PKG_VERSION"), max_line: MAX_LINE };
        let options = Options { website_url: None, tool_errors_as_results: false, strict_request_ids: true };
        Self { inner: craft_mcp_server::Server::new(info, GridcraftTools { backend }).with_options(options) }
    }

    pub fn backend(&mut self) -> &mut dyn Backend {
        self.inner.tools().backend.as_mut()
    }

    /// Whether the client has sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.inner.is_initialized()
    }

    /// Serve newline-delimited JSON-RPC until `input` closes. Logs go to stderr only (stdout is
    /// the protocol stream).
    pub fn serve(&mut self, input: impl BufRead, output: impl Write) -> std::io::Result<()> {
        self.inner.serve(input, output)
    }

    /// Handle one line; returns the reply line (None for notifications and blank lines).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.len() > MAX_LINE {
            return self.inner.handle_line(line);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(trimmed)
            && let Some(err) = precheck_request(&o)
        {
            return Some(err.to_string());
        }
        self.inner.handle_line(line)
    }

    /// Handle one JSON-RPC message; `None` for notifications and responses.
    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        if let Value::Object(o) = &msg
            && let Some(err) = precheck_request(o)
        {
            return Some(err);
        }
        self.inner.handle(msg)
    }
}

fn invalid_params(id: &Value, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": message.into()}})
}

fn precheck_request(o: &serde_json::Map<String, Value>) -> Option<Value> {
    // Shared envelope validation takes priority over app-specific argument checks.
    if o.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return None;
    }
    let id = o.get("id")?;
    let method = o.get("method").and_then(Value::as_str)?;
    if !(id.is_string() || id.is_number()) {
        return None;
    }
    let params = o.get("params");
    if params.is_some_and(|p| !(p.is_object() || p.is_null())) {
        return Some(invalid_params(id, "`params` must be an object"));
    }
    let params = params.filter(|p| p.is_object());
    match method {
        "resources/read" if params.and_then(|p| p.get("uri")).and_then(Value::as_str).is_none() => Some(invalid_params(id, "missing `uri`")),
        "tools/call" => {
            let name = params.and_then(|p| p.get("name")).and_then(Value::as_str)?;
            let args = match params.and_then(|p| p.get("arguments")) {
                None | Some(Value::Null) => json!({}),
                Some(v @ Value::Object(_)) => v.clone(),
                Some(_) => return None,
            };
            check_arguments(name, &args).err().map(|e| invalid_params(id, e))
        }
        _ => None,
    }
}
