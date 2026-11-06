//! Python in cells: the worker that runs the user's own Python, and the client that drives it.
//!
//! - [`interp`] finds an interpreter (Python 3.11 or newer), uv's first.
//! - `describe` works out what to start and what the sandbox must let it read.
//! - `shim.py` is what that interpreter runs: it reads requests, keeps the globals the cells
//!   share, gives each cell its `xl()` inputs and converts the result.
//! - [`diagnostics`] is the catalog of what can be wrong, in the words the interface uses.
//! - [`protocol`] is the JSON-lines protocol between them, with a cap on every field.
//! - [`client::Client`] runs one request at a time with an absolute deadline, and kills the
//!   worker at any limit or at anything the protocol doesn't allow.
//! - [`process::ProcessTransport`] starts the interpreter; [`fake::FakeTransport`] stands in for
//!   it in tests on every system.
//!
//! The web build has no processes: its [`client::Client`] reports every request as failed to
//! start, and detection finds nothing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod diagnostics;
pub mod interp;
mod interpreter;
pub mod protocol;
pub mod run;

#[cfg(not(target_arch = "wasm32"))]
pub mod client;
#[cfg(target_arch = "wasm32")]
#[path = "client_web.rs"]
pub mod client;

#[cfg(not(target_arch = "wasm32"))]
pub mod describe;
/// The web build finds no interpreter, and only has the types.
#[cfg(target_arch = "wasm32")]
pub mod describe {
    pub use crate::interpreter::{Interpreter, Kind, STARTER_PACKAGES, UV_INSTALL, pip_install_command, uv_install_commands, uv_setup_commands};
}
#[cfg(not(target_arch = "wasm32"))]
pub mod fake;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;

pub use client::Client;
pub use protocol::{Clock, Dated, Day, Input, Output, Picture, PictureType, PyError, SelfTest, Stamp};
pub use run::{Busy, Limits, Outcome, RunResult, Transport};

/// Whether this build can run Python at all (the web build can't).
pub const AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

/// Whether this system can confine a worker: what provides the sandbox, or why there is none.
pub fn sandbox_support() -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match gridcraft_sandbox::support() {
            gridcraft_sandbox::Support::Available(how) => Ok(how),
            gridcraft_sandbox::Support::Unsupported(why) => Err(why),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("the web version has no sandbox".into())
    }
}
