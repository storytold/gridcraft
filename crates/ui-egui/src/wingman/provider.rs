//! Native Wingman provider settings and I/O. Reading settings never contacts a server.
//! Settings vocabulary: https://zed.dev/docs/ai/use-a-local-model and
//! https://zed.dev/docs/ai/use-api-access#openai-compatible (public documentation).
#[path = "provider_settings.rs"]
mod settings;
#[path = "provider_stream.rs"]
mod stream;
#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
#[path = "provider_thinking.rs"]
mod thinking;
#[path = "provider_transport.rs"]
mod transport;

pub use settings::set_config_dir;
pub(super) use settings::{Agent, load, mcp_servers, open_config};
pub(super) use stream::ToolCall;
pub(super) use transport::{Discovery, Event, Job, discover, start, summarize};
