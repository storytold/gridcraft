//! Real-time collaboration for GridCraft, client side (RFC §10).
//!
//! [`Client`] owns the desktop WebSocket and the reconnect loop; [`Synced`] keeps one engine
//! [`Session`] in step with the authoritative room: it previews merge commands, re-runs other
//! participants' commands through `Session::apply_remote`, patches the authoritative values, rolls
//! a rejected preview back and replaces the document from a snapshot.
//!
//! [`Synced`] speaks to its connection through [`Link`], so the same state machine serves the
//! desktop (tokio + tungstenite, native only) and the browser build, whose `WebSocket` lives in
//! `gridcraft-collab-web` behind the host bridge. A wasm build carries no network code from this
//! crate.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
mod client;
mod link;
mod synced;

#[cfg(not(target_arch = "wasm32"))]
pub use client::{Client, Config, ConnectError};
pub use link::{Event, Link, State, Welcome};
pub use synced::{EditError, Synced};
