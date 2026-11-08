//! Real-time collaboration for GridCraft, client side (RFC §10).
//!
//! [`Client`] owns the WebSocket and the reconnect loop; [`Synced`] keeps one engine [`Session`]
//! in step with the authoritative room: it previews merge commands, re-runs other participants'
//! commands through `Session::apply_remote`, patches the authoritative values, rolls a rejected
//! preview back and replaces the document from a snapshot.
//!
//! The crate is native-only (tokio + tungstenite). The browser speaks the same protocol through
//! its host; nothing here is compiled into the WASM build.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod client;
mod synced;

pub use client::{Client, Config, ConnectError, Event, State, Welcome};
pub use synced::{EditError, Synced};
