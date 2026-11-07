//! What a request produced, the limits it ran under, and the pipe to a worker: shared by the
//! client and its web stand-in.

use std::io::{Read, Write};
use std::time::Duration;

use gridcraft_core::CellError;

use crate::protocol::{MAX_LINE_BYTES, Output, PyError};

/// What one worker and its requests may use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Time one request may take, from when it is submitted.
    pub timeout: Duration,
    /// Extra time the first request on a fresh worker gets, for the interpreter to start.
    pub start_timeout: Duration,
    /// Resident memory the worker may use, in bytes; it is killed above that. On Linux the parent
    /// reads `/proc`; on macOS it runs `ps` at most every 500 ms, so the limit is a watch rather
    /// than a hard cap. Elsewhere there is no watch. Each reply also reports the worker's peak, so
    /// that a spike between two reads counts against the request that made it.
    pub memory_bytes: u64,
    /// Bytes in one line from the worker, at most [`MAX_LINE_BYTES`].
    pub line_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            timeout: Duration::from_secs(30),
            start_timeout: Duration::from_secs(10),
            memory_bytes: 1024 * 1024 * 1024,
            line_bytes: MAX_LINE_BYTES,
        }
    }
}

/// The longest time limit accepted; longer ones are cut to it.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(60 * 60);

impl Limits {
    /// The limits brought within range: times from 1 ms to [`MAX_TIMEOUT`], a line cap from 1 byte
    /// to [`MAX_LINE_BYTES`].
    pub fn clamped(&self) -> Limits {
        Limits {
            timeout: self.timeout.clamp(Duration::from_millis(1), MAX_TIMEOUT),
            start_timeout: self.start_timeout.min(MAX_TIMEOUT),
            memory_bytes: self.memory_bytes,
            line_bytes: self.line_bytes.clamp(1, MAX_LINE_BYTES),
        }
    }
}

/// How a request ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The code ran; `output` is the value of its last expression (empty when there is none).
    Ok { output: Output, stdout: String },
    /// The code raised an error (or didn't compile). The worker keeps running.
    PythonError { error: PyError, stdout: String },
    /// The request passed its deadline; the worker was killed.
    TimedOut,
    /// The worker went over its memory limit; it was killed.
    OverMemory { rss_bytes: u64 },
    /// The worker wrote a line over the cap; it was killed.
    OutputTooLarge,
    /// The worker wrote something the protocol doesn't allow (malformed, wrong id, unsolicited,
    /// a second reply); it was killed.
    ProtocolViolation(String),
    /// The worker stopped after it started, before it answered.
    Crashed(String),
    /// The worker could not start, or stopped before it said it was ready.
    FailedToStart(String),
}

impl Outcome {
    /// Whether the worker was killed or stopped (the next request starts a fresh one, with empty
    /// globals).
    pub fn lost_worker(&self) -> bool {
        !matches!(self, Outcome::Ok { .. } | Outcome::PythonError { .. })
    }

    /// The error a cell shows for this outcome, when it shows one: `#CONNECT!` when the worker
    /// could not start, `#TIMEOUT!` at the deadline, `#PYTHON!` for the rest.
    pub fn cell_error(&self) -> Option<CellError> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::TimedOut => Some(CellError::Timeout),
            Outcome::FailedToStart(_) => Some(CellError::Connect),
            Outcome::PythonError { .. }
            | Outcome::OverMemory { .. }
            | Outcome::OutputTooLarge
            | Outcome::ProtocolViolation(_)
            | Outcome::Crashed(_) => Some(CellError::Python),
        }
    }
}

/// A finished request.
#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    pub id: u64,
    pub outcome: Outcome,
}

/// A request was submitted while another was in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy;

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a Python request is already running")
    }
}

impl std::error::Error for Busy {}

/// The pipes of one started worker.
pub struct Pipes {
    /// The worker's standard input: the client writes requests here.
    pub stdin: Box<dyn Write + Send>,
    /// The worker's standard output: the client reads replies here.
    pub stdout: Box<dyn Read + Send>,
    /// Reads the worker's resident memory, when this system can.
    pub memory: Option<Box<dyn MemoryProbe>>,
}

/// Reads one worker's resident memory.
pub trait MemoryProbe: Send {
    /// Bytes in memory now, if it can be read.
    fn rss_bytes(&mut self) -> Option<u64>;
    /// How long to wait between reads.
    fn interval(&self) -> Duration;
}

/// Starts and stops workers. The real one runs the interpreter (`ProcessTransport`); the fake one
/// (`FakeTransport`) runs a script on a thread, so tests run on every system without Python.
pub trait Transport: Send {
    /// Starts a worker (stopping the one before, if any) and returns its pipes.
    fn start(&mut self) -> Result<Pipes, String>;
    /// Stops the current worker now. It never blocks on the worker, and calling it twice is fine.
    fn kill(&mut self);
    /// The end of what the worker wrote to its standard error, for a message when it stopped.
    fn last_words(&mut self) -> String {
        String::new()
    }
}
