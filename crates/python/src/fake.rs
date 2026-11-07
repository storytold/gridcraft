//! A fake worker with no Python, for tests on every system (Windows included): a script runs on a
//! thread, behind real OS pipes, so the client's reader, writer, caps and deadlines work exactly
//! as with the interpreter. The script answers with given values, sends forged lines, stalls,
//! floods, or exits.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gridcraft_core::Value;

use crate::protocol::{Input, Output, PyError, Reply, Request, SelfTest};
use crate::run::{MemoryProbe, Pipes, Transport};

type Script = dyn Fn(FakeWorker) + Send + Sync;

/// Starts [`FakeWorker`]s that run a script.
pub struct FakeTransport {
    script: Arc<Script>,
    starts: Arc<AtomicU64>,
    exited: Arc<AtomicU64>,
    rss: Arc<AtomicU64>,
    /// Every request any worker of this transport read, in order.
    seen: Arc<Mutex<Vec<Request>>>,
    current: Option<Current>,
}

struct Current {
    killed: Arc<AtomicBool>,
    /// Dropped to wake a worker that stalls.
    _kill: Sender<()>,
}

/// What the test keeps after the transport moves into the client.
#[derive(Clone)]
pub struct FakeHandle {
    starts: Arc<AtomicU64>,
    exited: Arc<AtomicU64>,
    rss: Arc<AtomicU64>,
    seen: Arc<Mutex<Vec<Request>>>,
}

impl FakeHandle {
    /// Workers started so far.
    pub fn starts(&self) -> u64 {
        self.starts.load(Ordering::SeqCst)
    }
    /// How many workers have gone, their pipes closed with them.
    pub fn exited(&self) -> u64 {
        self.exited.load(Ordering::SeqCst)
    }
    /// Sets the memory the current worker reports (0: none).
    pub fn set_rss(&self, bytes: u64) {
        self.rss.store(bytes, Ordering::SeqCst);
    }
    /// The requests the workers read so far.
    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl FakeTransport {
    /// A worker that runs `script` each time it starts.
    pub fn new(script: impl Fn(FakeWorker) + Send + Sync + 'static) -> FakeTransport {
        FakeTransport {
            script: Arc::new(script),
            starts: Arc::new(AtomicU64::new(0)),
            exited: Arc::new(AtomicU64::new(0)),
            rss: Arc::new(AtomicU64::new(0)),
            seen: Arc::new(Mutex::new(Vec::new())),
            current: None,
        }
    }

    /// A well-behaved worker: ready, then each run answered by `answer(code, inputs)`, each reset
    /// answered with an empty value.
    pub fn answering(answer: impl Fn(&str, &BTreeMap<String, Input>) -> Result<Output, PyError> + Send + Sync + 'static) -> FakeTransport {
        FakeTransport::new(move |mut w| {
            w.ready();
            while let Some(req) = w.next_request() {
                let result = match &req {
                    Request::Run { code, inputs, .. } => answer(code, inputs),
                    Request::Render { .. } | Request::Reset { .. } => Ok(Output::Value(Value::Empty)),
                };
                if !w.done(req.id(), result) {
                    return;
                }
            }
        })
    }

    pub fn handle(&self) -> FakeHandle {
        FakeHandle { starts: self.starts.clone(), exited: self.exited.clone(), rss: self.rss.clone(), seen: self.seen.clone() }
    }

    /// Another transport with the same script and counters, for a caller that makes one per
    /// workbook.
    pub fn sharing(&self) -> FakeTransport {
        FakeTransport {
            script: self.script.clone(),
            starts: self.starts.clone(),
            exited: self.exited.clone(),
            rss: self.rss.clone(),
            seen: self.seen.clone(),
            current: None,
        }
    }
}

impl Transport for FakeTransport {
    fn start(&mut self) -> Result<Pipes, String> {
        self.kill();
        let (client_out, worker_out) = std::io::pipe().map_err(|e| e.to_string())?;
        let (worker_in, client_in) = std::io::pipe().map_err(|e| e.to_string())?;
        let start = self.starts.fetch_add(1, Ordering::SeqCst) + 1;
        let (kill, kill_rx) = mpsc::channel();
        let killed = Arc::new(AtomicBool::new(false));
        let worker =
            FakeWorker { input: BufReader::new(worker_in), output: worker_out, kill_rx, killed: killed.clone(), start, seen: self.seen.clone() };
        let script = self.script.clone();
        let exited = self.exited.clone();
        std::thread::Builder::new()
            .name("python-fake".into())
            .spawn(move || {
                script(worker);
                exited.fetch_add(1, Ordering::SeqCst);
            })
            .map_err(|e| e.to_string())?;
        self.current = Some(Current { killed, _kill: kill });
        Ok(Pipes { stdin: Box::new(client_in), stdout: Box::new(client_out), memory: Some(Box::new(FakeMemory(self.rss.clone()))) })
    }

    fn kill(&mut self) {
        if let Some(c) = self.current.take() {
            c.killed.store(true, Ordering::SeqCst);
        }
    }

    fn last_words(&mut self) -> String {
        "fake worker".into()
    }
}

impl Drop for FakeTransport {
    fn drop(&mut self) {
        self.kill();
    }
}

struct FakeMemory(Arc<AtomicU64>);

impl MemoryProbe for FakeMemory {
    fn rss_bytes(&mut self) -> Option<u64> {
        let v = self.0.load(Ordering::SeqCst);
        (v > 0).then_some(v)
    }
    fn interval(&self) -> Duration {
        Duration::from_millis(10)
    }
}

/// The worker side of a fake: what a script uses.
pub struct FakeWorker {
    input: BufReader<PipeReader>,
    output: PipeWriter,
    kill_rx: Receiver<()>,
    killed: Arc<AtomicBool>,
    start: u64,
    seen: Arc<Mutex<Vec<Request>>>,
}

impl FakeWorker {
    /// Which start of the transport this is (1 for the first worker).
    pub fn start_number(&self) -> u64 {
        self.start
    }

    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    /// Says it is ready (self-test not checked), as the shim does.
    pub fn ready(&mut self) -> bool {
        self.send(&Reply::Ready { self_test: SelfTest::NotChecked })
    }

    /// The next request, or `None` once the client closed the pipe.
    pub fn next_request(&mut self) -> Option<Request> {
        let mut line = Vec::new();
        match self.input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let req = Request::parse(line.strip_suffix(b"\n").unwrap_or(&line)).ok()?;
                if let Ok(mut s) = self.seen.lock() {
                    s.push(req.clone());
                }
                Some(req)
            }
        }
    }

    pub fn send(&mut self, reply: &Reply) -> bool {
        self.write_raw(&reply.to_line())
    }

    pub fn done(&mut self, id: u64, result: Result<Output, PyError>) -> bool {
        self.send(&Reply::Done { id, result, stdout: String::new(), peak: None, touched: None })
    }

    /// Writes bytes as they are (forged or broken lines).
    pub fn write_raw(&mut self, bytes: &[u8]) -> bool {
        !self.killed() && self.output.write_all(bytes).and_then(|()| self.output.flush()).is_ok()
    }

    /// Writes `total` bytes of `byte` with no newline, in chunks, until done or the pipe closes.
    /// Returns how many bytes went into the pipe.
    pub fn flood(&mut self, total: usize, byte: u8) -> usize {
        let chunk = vec![byte; 64 * 1024];
        let mut sent = 0;
        while sent < total && !self.killed() {
            let n = chunk.len().min(total - sent);
            if self.output.write_all(chunk.get(..n).unwrap_or_default()).is_err() {
                break;
            }
            sent += n;
        }
        sent
    }

    /// Does nothing until killed.
    pub fn stall(&mut self) {
        let _ = self.kill_rx.recv();
    }
}
