//! The parent side: [`Client`] starts a worker through a [`Transport`], sends it one request at a
//! time, and guards it.
//!
//! The parent owns the clock: each request gets an absolute deadline when it is submitted, and
//! nothing the worker writes moves it. Every line from the worker is untrusted. Three threads per
//! worker keep the caller from blocking:
//!
//! - a reader takes the worker's output in bounded chunks, cuts it into lines of at most
//!   [`Limits::line_bytes`], and feeds them to a bounded channel;
//! - a writer writes requests, so a worker that stops reading never blocks the caller;
//! - a watcher reads the worker's memory, when the system can, and reports when it is over the
//!   limit.
//!
//! The worker is killed at the deadline, over the memory limit (as the watcher reads it, or as a
//! reply reports its peak, which catches a spike between two reads), when it crashes, for a line over
//! the cap, for malformed JSON, for a reply with the wrong id, for an unsolicited line or a second
//! reply, and when the client is dropped. The next request starts a fresh worker.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::time::{Duration, Instant};

use gridcraft_core::Value;

use crate::protocol::{Init, Input, Output, Reply, Request, SelfTest};
pub use crate::run::*;

/// Lines (or other events) the reader may queue before it waits for the client.
const EVENT_QUEUE: usize = 2;
/// Requests the writer may queue. One is in flight at a time, so this is never reached by a
/// worker that reads its input.
const WRITE_QUEUE: usize = 2;
/// Bytes the reader takes from the pipe at a time.
const READ_CHUNK: usize = 64 * 1024;
/// How much slower the watcher reads while no request runs (code can still allocate on a thread
/// it started).
const IDLE_WATCH_FACTOR: u32 = 10;
/// Bytes of the worker's last words kept in a message.
const LAST_WORDS_BYTES: usize = 2 * 1024;

#[allow(clippy::disallowed_methods)] // the client only exists off wasm, where the clock works
fn now() -> Instant {
    Instant::now()
}

/// `at + d`, or a day later when that can't be represented, or `at` itself.
fn later(at: Instant, d: Duration) -> Instant {
    at.checked_add(d).or_else(|| at.checked_add(Duration::from_secs(86_400))).unwrap_or(at)
}

/// What the reader and the watcher tell the client.
enum Event {
    Line(Vec<u8>),
    /// A line went over the cap; the reader stopped.
    TooLong,
    /// The worker closed its output (it exited), or reading failed.
    Eof,
    /// The worker's memory went over the limit.
    OverMemory(u64),
}

struct Conn {
    events: Receiver<Event>,
    writer: SyncSender<Vec<u8>>,
    /// Wakes the watcher when a request starts; dropped to stop it.
    watch: Option<mpsc::Sender<()>>,
    /// A request is in flight: the watcher reads at its full rate.
    busy: Arc<AtomicBool>,
    /// The worker said `ready`.
    ready: bool,
}

struct Inflight {
    id: u64,
    deadline: Instant,
}

/// One worker and the request it runs. See the module documentation.
pub struct Client {
    transport: Box<dyn Transport>,
    limits: Limits,
    conn: Option<Conn>,
    inflight: Option<Inflight>,
    /// A result decided without the worker (it could not start, or a reset with no worker).
    local: Option<RunResult>,
    next_id: u64,
    starts: u64,
    self_test: Option<SelfTest>,
    touched: Option<[u32; 3]>,
}

impl Client {
    pub fn new(transport: Box<dyn Transport>, limits: Limits) -> Client {
        Client { transport, limits: limits.clamped(), conn: None, touched: None, inflight: None, local: None, next_id: 1, starts: 0, self_test: None }
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// New limits apply to the next request; a memory or line limit applies from the next worker.
    /// A caller that kills the worker when they change makes a change take effect at once.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits.clamped();
    }

    /// Workers started so far.
    pub fn starts(&self) -> u64 {
        self.starts
    }

    /// Whether a request is waiting for its result.
    pub fn busy(&self) -> bool {
        self.inflight.is_some() || self.local.is_some()
    }

    /// Whether a worker is running.
    pub fn alive(&self) -> bool {
        self.conn.is_some()
    }

    /// What the current worker reported about its confinement, once it is ready.
    /// The first earlier cell whose figure the last finished request changed, as sheet, row and
    /// column. Asking forgets it.
    pub fn take_touched(&mut self) -> Option<[u32; 3]> {
        self.touched.take()
    }

    pub fn self_test(&self) -> Option<&SelfTest> {
        self.self_test.as_ref()
    }

    /// Stops the worker. The request in flight, if any, is dropped without a result; its id is
    /// returned. The next request starts a fresh worker.
    pub fn stop(&mut self) -> Option<u64> {
        self.kill_worker();
        self.local.take().map(|r| r.id).or_else(|| self.inflight.take().map(|r| r.id))
    }

    /// Runs `code` with the `xl()` inputs, in the globals the earlier runs left.
    pub fn submit_run(&mut self, code: &str, inputs: std::collections::BTreeMap<String, Input>) -> Result<u64, Busy> {
        self.submit_cell(code, inputs, false, None)
    }

    /// [`Client::submit_run`] for a cell with its return type: with `object`, a result with no
    /// single-cell form comes back as a Python object rather than cells to spill. `at` is the cell's
    /// place in the run order, as sheet, row and column.
    pub fn submit_cell(
        &mut self,
        code: &str,
        inputs: std::collections::BTreeMap<String, Input>,
        object: bool,
        at: Option<[u32; 3]>,
    ) -> Result<u64, Busy> {
        let id = self.take_id()?;
        self.send(Request::Run { id, code: code.to_string(), inputs, object, init: None, at });
        Ok(id)
    }

    /// Runs a workbook's initialization code, under the rules of `init`. A result's value is empty,
    /// or a row of `module, line` for each package it skipped.
    pub fn submit_init(&mut self, code: &str, init: Init) -> Result<u64, Busy> {
        let id = self.take_id()?;
        self.send(Request::Run { id, code: code.to_string(), inputs: std::collections::BTreeMap::new(), object: false, init: Some(init), at: None });
        Ok(id)
    }

    /// Draws the figure of the cell at `at` again at this size, without running the cell. With no
    /// worker there is no figure: `None`, and nothing starts one.
    pub fn submit_render(&mut self, at: [u32; 3], width: u32, height: u32) -> Option<u64> {
        if !self.alive() {
            return None;
        }
        let id = self.take_id().ok()?;
        self.deliver(Request::Render { id, at, width, height }, false).then_some(id)
    }

    /// Clears the globals. With no worker running there is nothing to clear, and the result is
    /// ready at once.
    pub fn submit_reset(&mut self) -> Result<u64, Busy> {
        let id = self.take_id()?;
        self.drain_idle();
        if self.conn.is_none() {
            self.local = Some(RunResult { id, outcome: Outcome::Ok { output: Output::Value(Value::Empty), stdout: String::new() } });
        } else {
            self.send(Request::Reset { id });
        }
        Ok(id)
    }

    /// Waits up to `wait` for the request in flight to finish, and returns its result when it has.
    /// The deadline and the memory limit are checked here, so a caller that polls with
    /// `Duration::ZERO` every frame doesn't block, but for one moment: when a worker stops, its last
    /// words are read for the message. Its exit closes that pipe, so this takes no time, and at most
    /// 200 ms if something else still holds the pipe open. With nothing in flight, any line the worker
    /// writes within `wait` kills it.
    pub fn poll(&mut self, wait: Duration) -> Option<RunResult> {
        if let Some(r) = self.local.take() {
            return Some(r);
        }
        let until = later(now(), wait);
        let Some(deadline) = self.inflight.as_ref().map(|r| r.deadline) else {
            self.watch_idle(until);
            return None;
        };
        loop {
            let at = now();
            if at >= deadline {
                return Some(self.finish(Outcome::TimedOut));
            }
            let Some(conn) = self.conn.as_mut() else {
                return Some(self.finish(Outcome::Crashed("Python stopped".into())));
            };
            let event = match conn.events.recv_timeout(until.min(deadline).saturating_duration_since(at)) {
                Ok(e) => e,
                Err(RecvTimeoutError::Timeout) => {
                    if now() >= deadline {
                        return Some(self.finish(Outcome::TimedOut));
                    }
                    return None;
                }
                Err(RecvTimeoutError::Disconnected) => Event::Eof,
            };
            if let Some(outcome) = self.handle(event) {
                return Some(self.finish(outcome));
            }
        }
    }

    /// Waits for the request in flight to finish (never past its deadline).
    pub fn wait(&mut self) -> Option<RunResult> {
        loop {
            if let Some(r) = self.poll(Duration::from_millis(250)) {
                return Some(r);
            }
            if !self.busy() {
                return None;
            }
        }
    }

    /// Runs `code` and waits for the result (tests and headless callers).
    pub fn run(&mut self, code: &str, inputs: std::collections::BTreeMap<String, Input>) -> Result<Outcome, Busy> {
        let id = self.submit_run(code, inputs)?;
        Ok(self.wait_for(id))
    }

    /// Clears the globals and waits for it.
    pub fn reset(&mut self) -> Result<Outcome, Busy> {
        let id = self.submit_reset()?;
        Ok(self.wait_for(id))
    }

    fn wait_for(&mut self, id: u64) -> Outcome {
        match self.wait() {
            Some(r) if r.id == id => r.outcome,
            _ => Outcome::Crashed("Python stopped".into()),
        }
    }

    fn take_id(&mut self) -> Result<u64, Busy> {
        if self.busy() {
            return Err(Busy);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        Ok(id)
    }

    /// Sends a request, starting a worker first if none runs. Failures become the request's
    /// result.
    fn send(&mut self, req: Request) {
        self.deliver(req, true);
    }

    /// Sends `req`. Without `may_start` it goes only to the worker already running, which may have
    /// stopped since anyone last looked: then nothing starts, and the answer is false.
    fn deliver(&mut self, req: Request, may_start: bool) -> bool {
        let id = req.id();
        self.drain_idle();
        let fresh = self.conn.is_none();
        if fresh && !may_start {
            return false;
        }
        let start = now();
        let mut allowance = self.limits.timeout;
        if fresh {
            if let Err(why) = self.start_worker() {
                self.local = Some(RunResult { id, outcome: Outcome::FailedToStart(why) });
                return true;
            }
            allowance = allowance.saturating_add(self.limits.start_timeout);
        }
        let line = req.to_line();
        let sent = match self.conn.as_ref().map(|c| c.writer.try_send(line)) {
            Some(Ok(())) => true,
            // The writer stopped (the worker exited) or is stuck behind a request the worker never
            // read; the worker is no use either way.
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_))) | None => false,
        };
        if !sent {
            self.kill_worker();
            if fresh {
                self.local = Some(RunResult { id, outcome: Outcome::FailedToStart("Python stopped as it started".into()) });
                return true;
            }
            // An idle worker that died: start a fresh one for this request, if it may have one.
            return may_start && self.deliver(req, may_start);
        }
        self.inflight = Some(Inflight { id, deadline: later(start, allowance) });
        if let Some(c) = self.conn.as_ref() {
            c.busy.store(true, Ordering::SeqCst);
            if let Some(w) = c.watch.as_ref() {
                let _ = w.send(());
            }
        }
        true
    }

    fn start_worker(&mut self) -> Result<(), String> {
        self.kill_worker();
        let pipes = self.transport.start()?;
        self.starts += 1;
        let (tx, events) = mpsc::sync_channel(EVENT_QUEUE);
        let (writer, requests) = mpsc::sync_channel::<Vec<u8>>(WRITE_QUEUE);
        let cap = self.limits.line_bytes;
        let reader_tx = tx.clone();
        let stdout = pipes.stdout;
        let spawned = std::thread::Builder::new().name("python-reader".into()).spawn(move || read_lines(stdout, cap, &reader_tx));
        if let Err(e) = spawned {
            self.transport.kill();
            return Err(format!("couldn't start a thread: {e}"));
        }
        let stdin = pipes.stdin;
        let spawned = std::thread::Builder::new().name("python-writer".into()).spawn(move || write_lines(stdin, &requests));
        if let Err(e) = spawned {
            self.transport.kill();
            return Err(format!("couldn't start a thread: {e}"));
        }
        let busy = Arc::new(AtomicBool::new(false));
        let watch = pipes.memory.and_then(|probe| {
            let (watch, nudges) = mpsc::channel::<()>();
            let limit = self.limits.memory_bytes;
            let (tx, busy) = (tx.clone(), busy.clone());
            // Without the watcher the worker still runs, only without a memory limit.
            std::thread::Builder::new()
                .name("python-memory".into())
                .spawn(move || watch_memory(probe, limit, &busy, &nudges, &tx))
                .ok()
                .map(|_| watch)
        });
        self.conn = Some(Conn { events, writer, watch, busy, ready: false });
        self.self_test = None;
        Ok(())
    }

    fn kill_worker(&mut self) {
        // Dropping the connection stops the writer and the watcher, and the reader at its next
        // line; killing the worker ends the reader's read.
        if self.conn.take().is_some() {
            self.transport.kill();
        }
    }

    /// Ends the request in flight with `outcome`; kills the worker unless the outcome keeps it.
    fn finish(&mut self, outcome: Outcome) -> RunResult {
        if outcome.lost_worker() {
            self.kill_worker();
        } else if let Some(c) = self.conn.as_ref() {
            c.busy.store(false, Ordering::SeqCst);
        }
        let id = self.inflight.take().map(|r| r.id).unwrap_or(0);
        RunResult { id, outcome }
    }

    /// One event while a request is in flight: the outcome when it ends the request.
    fn handle(&mut self, event: Event) -> Option<Outcome> {
        let ready = self.conn.as_ref().is_some_and(|c| c.ready);
        match event {
            Event::TooLong => Some(Outcome::OutputTooLarge),
            Event::OverMemory(rss_bytes) => Some(Outcome::OverMemory { rss_bytes }),
            Event::Eof if ready => Some(Outcome::Crashed(self.stopped_message("Python stopped"))),
            Event::Eof => Some(Outcome::FailedToStart(self.stopped_message("Python stopped before it was ready"))),
            Event::Line(line) => match Reply::parse(&line) {
                Err(e) => Some(Outcome::ProtocolViolation(format!("a malformed line from Python: {e}"))),
                Ok(Reply::Ready { .. }) if ready => Some(Outcome::ProtocolViolation("Python said it was ready twice".into())),
                Ok(Reply::Ready { self_test }) => {
                    if let Some(c) = self.conn.as_mut() {
                        c.ready = true;
                    }
                    self.self_test = Some(self_test);
                    None
                }
                Ok(Reply::Done { .. }) if !ready => Some(Outcome::ProtocolViolation("an answer before Python was ready".into())),
                Ok(Reply::Done { id, result, stdout, peak, touched }) => {
                    let expected = self.inflight.as_ref().map(|r| r.id);
                    if Some(id) != expected {
                        return Some(Outcome::ProtocolViolation(format!("an answer for request {id} while request {} ran", expected.unwrap_or(0))));
                    }
                    // Over the limit at any moment of this request, even if the watcher missed it:
                    // it counts against this request, not the next one.
                    if let Some(rss_bytes) = peak.filter(|p| *p > self.limits.memory_bytes) {
                        return Some(Outcome::OverMemory { rss_bytes });
                    }
                    self.touched = touched;
                    Some(match result {
                        Ok(output) => Outcome::Ok { output, stdout },
                        Err(error) => Outcome::PythonError { error, stdout },
                    })
                }
            },
        }
    }

    /// With nothing in flight the worker has nothing to say: any line kills it. A worker that
    /// stopped is noted as gone. Waits up to `until` for one event.
    fn watch_idle(&mut self, until: Instant) {
        let Some(conn) = self.conn.as_ref() else { return };
        match conn.events.recv_timeout(until.saturating_duration_since(now())) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(_) | Err(RecvTimeoutError::Disconnected) => self.kill_worker(),
        }
    }

    /// Kills an idle worker that wrote anything, or that stopped, before a new request goes out.
    fn drain_idle(&mut self) {
        let Some(conn) = self.conn.as_ref() else { return };
        match conn.events.try_recv() {
            Err(TryRecvError::Empty) => {}
            Ok(_) | Err(TryRecvError::Disconnected) => self.kill_worker(),
        }
    }

    fn stopped_message(&mut self, what: &str) -> String {
        let words = self.transport.last_words();
        let words = words.trim();
        if words.is_empty() {
            return what.to_string();
        }
        // The end is where the reason is.
        let mut start = words.len().saturating_sub(LAST_WORDS_BYTES);
        while !words.is_char_boundary(start) {
            start += 1;
        }
        format!("{what}: {}", words.get(start..).unwrap_or_default())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.kill_worker();
    }
}

/// The reader thread: lines of at most `cap` bytes (without the newline) to `tx`, then
/// [`Event::TooLong`] or [`Event::Eof`].
fn read_lines(mut input: Box<dyn Read + Send>, cap: usize, tx: &SyncSender<Event>) {
    let mut line: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let n = match input.read(&mut chunk) {
            Ok(0) | Err(_) => {
                let _ = tx.send(Event::Eof);
                return;
            }
            Ok(n) => n,
        };
        let mut rest = chunk.get(..n).unwrap_or_default();
        while let Some(at) = rest.iter().position(|b| *b == b'\n') {
            let (head, tail) = rest.split_at(at);
            if line.len().saturating_add(head.len()) > cap {
                let _ = tx.send(Event::TooLong);
                return;
            }
            line.extend_from_slice(head);
            if tx.send(Event::Line(std::mem::take(&mut line))).is_err() {
                return;
            }
            rest = tail.get(1..).unwrap_or_default();
        }
        if line.len().saturating_add(rest.len()) > cap {
            let _ = tx.send(Event::TooLong);
            return;
        }
        line.extend_from_slice(rest);
    }
}

/// The writer thread: each request to the worker's input, until the client drops its end or the
/// worker stops reading for good.
fn write_lines(mut output: Box<dyn Write + Send>, requests: &Receiver<Vec<u8>>) {
    while let Ok(line) = requests.recv() {
        if output.write_all(&line).and_then(|()| output.flush()).is_err() {
            return;
        }
    }
}

/// The watcher thread: reads the worker's memory until the client drops its end, and reports
/// once when it is over `limit`. It reads at the probe's rate while a request runs, ten times
/// slower otherwise, and at once when a request starts.
fn watch_memory(mut probe: Box<dyn MemoryProbe>, limit: u64, busy: &AtomicBool, nudges: &Receiver<()>, tx: &SyncSender<Event>) {
    loop {
        let every = probe.interval();
        let wait = if busy.load(Ordering::SeqCst) { every } else { every.saturating_mul(IDLE_WATCH_FACTOR) };
        match nudges.recv_timeout(wait) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let Some(rss) = probe.rss_bytes() else { continue };
        if rss <= limit {
            continue;
        }
        match tx.try_send(Event::OverMemory(rss)) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => return,
            // The queue is full of lines; the client reads them and this tries again.
            Err(TrySendError::Full(_)) => {}
        }
    }
}

#[cfg(test)]
mod tests;
