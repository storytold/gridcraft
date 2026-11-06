//! Real-time collaboration for GridCraft.
//!
//! This crate is the protocol layer of `docs/collab/rfc.md`: the operation envelope (ADR-0001),
//! session-scoped sheet keys (ADR-0002), the allowlist and conflict class of every command
//! (ADR-0004), the authoritative cell delta, and an in-process authoritative room (the piece the
//! sync server owns). The network transport, JWT and persistence are not here: they live in the
//! server crate and are not needed to test the protocol.
//!
//! A room applies a normalised command through [`gridcraft_engine::Session::apply_remote`], so the
//! command runs with the engine's own `enabled`, protection, array and recalculation rules but
//! leaves the receiving session's journal and undo alone. The cells it changed — including the
//! formulas recalculated because of them — come back as an authoritative [`SheetDelta`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod delta;
mod keys;
mod ops;
mod protocol;
mod room;

pub use delta::delta_for;
pub use keys::SheetKeys;
pub use ops::{Class, classify};
pub use protocol::{Applied, CellChange, ClientOp, CommandEcho, Hint, Kind, Outcome, Payload, RejectCode, Rejected, SheetDelta, Target};
pub use room::{Room, RoomError};

#[cfg(test)]
mod tests;
