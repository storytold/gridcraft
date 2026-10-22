//! GridCraft's MCP server.
//!
//! [Model Context Protocol](https://modelcontextprotocol.io) over stdio: newline-delimited
//! JSON-RPC 2.0, hand-written (no async runtime). The server exposes GridCraft as MCP tools and
//! resources and forwards everything to a [`Backend`]:
//!
//! - [`Remote`] talks to a running desktop app through its loopback JSON-lines control channel
//!   (`gridcraft --control 7979`): one `{"id","method","params"}` line in, one
//!   `{"id","ok","result"|"error"}` line out, after an `{"auth": token}` handshake
//!   ([`control_auth`]; see `docs/control-protocol.md`).
//! - [`Headless`] hosts an in-process [`gridcraft_engine::Session`] and implements the
//!   engine-level control-channel methods itself, so agents can build workbooks without a window.
//!
//! Entry points: [`Server::serve`] (stdio loop) and [`Server::handle_line`] (one message).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod backend;
pub mod control_auth;
mod headless;
mod server;
mod tools;

pub use backend::{Backend, Remote};
pub use headless::Headless;
pub use server::{COMMANDS_URI, FUNCTIONS_URI, PROTOCOL_VERSION, SUPPORTED_VERSIONS, Server, WORKBOOK_URI};
pub use tools::{ToolResult, call_tool, tool_definitions};

/// Default control-channel port of the desktop app.
pub const DEFAULT_PORT: u16 = 7979;

/// `"7979"` → `"127.0.0.1:7979"`; `"host:port"` is kept as is.
pub fn control_addr(s: &str) -> String {
    if s.parse::<u16>().is_ok() { format!("127.0.0.1:{s}") } else { s.to_string() }
}

#[cfg(test)]
mod tests;
