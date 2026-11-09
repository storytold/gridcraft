//! The Office Scripts runtime: Bun, found or installed by the user, driven over stdin/stdout.
//!
//! Bun (MIT) is not shipped with GridCraft; it is a separate runtime the user downloads or
//! places by hand, exactly as model weights are. [`BunHost::discover`] looks at
//! `$GRIDCRAFT_BUN`, then `PATH`, then `<model root>/runtimes/bun/bun(.exe)`.
//!
//! A run writes the shim in [`shim`] to a temporary file, starts `bun run <shim>`, and sends the
//! script as the first stdin line. The shim defines the `ExcelScript` globals documented in
//! `docs/office-scripts.md`, and turns each workbook operation into a line on stdout; this crate
//! answers on stdin after the engine has run it as a command. Everything is bounded — script
//! length, calls, output, wall-clock time — and a timeout kills the child. A malformed line, a
//! crashed runtime or a missing Bun is an error the pane shows, never a panic.
//!
//! WebAssembly has no child processes: [`BunHost::discover`] returns `None` there and the engine
//! reports the runtime as unavailable.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod discover;
mod shim;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use gridcraft_script::bridge::{MAX_LINE_BYTES, parse_request};
use gridcraft_script::{BridgeExecutor, Limits, RunReport, RuntimeInfo, ScriptError, ScriptHost};
use serde_json::Value as Json;

/// The most stdout bytes one run may send before it is stopped. Requests are small; this bounds
/// a script that floods the bridge.
pub const MAX_WIRE_BYTES: usize = 16 * 1024 * 1024;
/// How long the runtime gets to answer `--version` when it is discovered.
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
/// The most stderr text kept for an error message.
const MAX_STDERR: usize = 8 * 1024;

/// A Bun runtime at a known path.
pub struct BunHost {
    exe: PathBuf,
    runtime: RuntimeInfo,
    shim: OnceLock<shim::TempShim>,
}

impl BunHost {
    /// A host for the Bun executable at `exe`, if it is a file.
    pub fn at(exe: impl Into<PathBuf>) -> Option<BunHost> {
        let exe = exe.into();
        if !exe.is_file() {
            return None;
        }
        let expected = discover::expected_path().display().to_string();
        let version = discover::version_of(&exe, VERSION_TIMEOUT);
        let path = exe.display().to_string();
        let runtime = RuntimeInfo::found(path, expected, version);
        Some(BunHost { exe, runtime, shim: OnceLock::new() })
    }

    /// Finds Bun: `$GRIDCRAFT_BUN`, then `PATH`, then the model root's `runtimes/bun`. Returns
    /// `None` when there is none (also on wasm, which cannot start processes).
    pub fn discover() -> Option<BunHost> {
        discover::find_exe().and_then(BunHost::at)
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    fn shim_file(&self) -> Result<&Path, ScriptError> {
        if let Some(shim) = self.shim.get() {
            return Ok(shim.path());
        }
        let shim = shim::TempShim::write()?;
        let _ = self.shim.set(shim);
        self.shim.get().map(|s| s.path()).ok_or_else(|| ScriptError::runtime("the Office Scripts bridge could not be prepared"))
    }
}

/// A host for a machine without Bun: it answers "missing" with the place Bun is expected and
/// refuses to run, so the pane can show what to install instead of treating the feature as
/// absent.
pub struct MissingHost {
    runtime: RuntimeInfo,
}

impl MissingHost {
    pub fn new() -> MissingHost {
        let expected = discover::expected_path().display().to_string();
        let message =
            format!("Bun is not installed. Install it from {}, or use Import Bun… to put it at {expected}.", gridcraft_script::BUN_DOWNLOAD_URL);
        MissingHost { runtime: RuntimeInfo::missing(expected, message) }
    }
}

impl Default for MissingHost {
    fn default() -> Self {
        MissingHost::new()
    }
}

impl ScriptHost for MissingHost {
    fn runtime(&self) -> RuntimeInfo {
        self.runtime.clone()
    }

    fn run(&self, _code: &str, _limits: &Limits, _exec: &mut dyn BridgeExecutor) -> Result<RunReport, ScriptError> {
        Err(ScriptError::missing_runtime(self.runtime.message.clone()))
    }
}

/// The host the app installs: Bun when it is there, a host that reports what is missing when it
/// is not. Always `Some`, so `script.runtime` can give the expected path either way.
pub fn script_host() -> Arc<dyn ScriptHost> {
    match BunHost::discover() {
        Some(host) => Arc::new(host),
        None => Arc::new(MissingHost::new()),
    }
}

impl ScriptHost for BunHost {
    fn runtime(&self) -> RuntimeInfo {
        self.runtime.clone()
    }

    fn run(&self, code: &str, limits: &Limits, exec: &mut dyn BridgeExecutor) -> Result<RunReport, ScriptError> {
        if code.chars().count() > limits.max_code_chars {
            return Err(ScriptError::limit(format!("the script is longer than the {}-character Office Scripts limit", limits.max_code_chars)));
        }
        let shim = self.shim_file()?.to_path_buf();
        let mut command = Command::new(&self.exe);
        command.arg("run").arg(&shim).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // A minimal environment: the script cannot read the user's tokens or settings out of it,
        // and a parent's `NODE_OPTIONS`/`BUN_*` cannot change how the runtime starts.
        command.env_clear().envs(runtime_env());
        let mut child = command.spawn().map_err(|e| ScriptError::runtime(format!("Bun could not start: {e}")))?;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            return Err(ScriptError::runtime("Bun's input could not be opened"));
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            return Err(ScriptError::runtime("Bun's output could not be opened"));
        };
        let Some(stderr) = child.stderr.take() else {
            let _ = child.kill();
            return Err(ScriptError::runtime("Bun's error output could not be opened"));
        };

        let child = Arc::new(Mutex::new(child));
        let finished = Arc::new((Mutex::new(false), Condvar::new()));
        let timed_out = Arc::new(AtomicBool::new(false));
        let watchdog = watchdog(child.clone(), finished.clone(), timed_out.clone(), limits.timeout);
        let stderr_reader = read_stderr(stderr);

        let init = serde_json::json!({
            "script": code,
            "limits": {
                "maxCalls": limits.max_calls,
                "maxOutputBytes": limits.max_output_bytes,
            }
        });
        let pumped = (|| {
            write_line(&mut stdin, &init.to_string())?;
            pump(BufReader::new(stdout), &mut stdin, exec, limits)
        })();
        // The watchdog killed the child: the empty EOF the pump saw is the timeout, not a
        // finished script.
        let result = if timed_out.load(Ordering::Relaxed) {
            Err(ScriptError::timeout(limits.timeout.as_millis().min(u128::from(u64::MAX)) as u64))
        } else {
            pumped
        };
        // Whatever happened, the child is stopped before its pipes are joined.
        if let Ok(mut done) = finished.0.lock() {
            *done = true;
        }
        finished.1.notify_all();
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = watchdog.join();
        let stderr = stderr_reader.join().unwrap_or_default();
        result.map_err(|e| with_stderr(e, &stderr))
    }
}

/// Reads requests until the runtime finishes, executes them and answers. Every failure closes the
/// bridge: the child is killed by the caller.
fn pump(mut reader: impl BufRead, stdin: &mut impl Write, exec: &mut dyn BridgeExecutor, limits: &Limits) -> Result<RunReport, ScriptError> {
    let mut report = RunReport::default();
    let mut console_bytes = 0usize;
    let mut wire_bytes = 0usize;
    let mut line = Vec::new();
    let mut completed = false;
    loop {
        if !read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? {
            break;
        }
        wire_bytes = wire_bytes.saturating_add(line.len());
        if wire_bytes > MAX_WIRE_BYTES {
            return Err(ScriptError::limit(format!("the script sent more than {MAX_WIRE_BYTES} bytes to GridCraft")));
        }
        let text = std::str::from_utf8(&line).map_err(|_| ScriptError::protocol("the script runtime sent a line that is not UTF-8".to_string()))?;
        let text = text.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            continue;
        }
        if completion_line(text) {
            completed = true;
            break;
        }
        if let Some(fatal) = fatal_line(text) {
            return Err(fatal);
        }
        let request = parse_request(text)
            .map_err(|e| ScriptError::protocol(format!("the script runtime sent a line GridCraft could not read: {}", e.message)))?;
        report.calls = report.calls.saturating_add(1);
        if report.calls > limits.max_calls {
            return Err(ScriptError::limit(format!("the script made more than {} workbook calls", limits.max_calls)));
        }
        let call = request.call();
        let reply = if call.method == "console.log" {
            if let Some(message) = call.params.get("message").and_then(Json::as_str) {
                console_bytes = console_bytes.saturating_add(message.len());
                if console_bytes <= limits.max_output_bytes {
                    report.output.push(message.to_string());
                } else if report.output.last().map(String::as_str) != Some("… output truncated") {
                    report.output.push("… output truncated".into());
                }
            }
            gridcraft_script::Response::ok(request.id, Json::Null)
        } else {
            match exec.execute(&call) {
                Ok(value) => gridcraft_script::Response::ok(request.id, value),
                // The runtime throws this at the call's line; a script may catch it.
                Err(error) => gridcraft_script::Response::failed(request.id, error),
            }
        };
        write_line(stdin, &reply.to_line())?;
    }
    if !completed {
        // End of output without the shim's completion line: the runtime crashed or the script
        // exited early (`process.exit(0)`). Committing the calls so far would leave half a run.
        return Err(ScriptError::runtime("the script runtime stopped before the script finished"));
    }
    Ok(report)
}

/// One line, bounded: `take` stops the read at `max + 1` bytes, so a script that writes a
/// gigabyte without a newline cannot grow GridCraft's memory. `Ok(false)` is end of output.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> Result<bool, ScriptError> {
    buf.clear();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let n = reader.take(limit).read_until(b'\n', buf).map_err(|e| ScriptError::runtime(format!("the script runtime stopped responding: {e}")))?;
    if n == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(true);
    }
    if buf.len() > max {
        return Err(ScriptError::limit(format!("the script sent a line longer than {max} bytes to GridCraft")));
    }
    Ok(true)
}

/// The shim's completion line, written after the script's `main` returns.
fn completion_line(text: &str) -> bool {
    serde_json::from_str::<Json>(text).ok().and_then(|v| v.get("done").and_then(Json::as_bool)) == Some(true)
}

/// A `{"fatal":{"message":"…","line":N}}` line, which the shim writes when the script fails
/// outside a bridge call.
fn fatal_line(text: &str) -> Option<ScriptError> {
    let value: Json = serde_json::from_str(text).ok()?;
    let fatal = value.get("fatal")?;
    let message = fatal.get("message").and_then(Json::as_str).unwrap_or("the script failed");
    let line = fatal.get("line").and_then(Json::as_u64).and_then(|n| u32::try_from(n).ok());
    Some(ScriptError::runtime(message).at(line).with_line_from_message())
}

fn write_line(stdin: &mut impl Write, text: &str) -> Result<(), ScriptError> {
    stdin
        .write_all(text.as_bytes())
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .map_err(|e| ScriptError::runtime(format!("the script runtime stopped reading: {e}")))
}

/// A thread that keeps the runtime's stderr for an error message, bounded. The pipe is drained so
/// a chatty child never blocks on a full buffer, but no more than [`MAX_STDERR`] bytes are kept.
fn read_stderr(stderr: std::process::ChildStderr) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut kept = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let room = MAX_STDERR.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..n.min(room)]);
        }
        String::from_utf8_lossy(&kept).trim().to_string()
    })
}

/// The environment a run gets: `NO_COLOR`, the user's home folder (Bun puts its cache under it)
/// and, on Windows, only the directories a child process needs to start. Nothing else from the
/// parent leaks in, so a script cannot read the user's tokens or settings out of `env`.
fn runtime_env() -> Vec<(String, std::ffi::OsString)> {
    let mut env = vec![("NO_COLOR".to_string(), std::ffi::OsString::from("1"))];
    let mut keys: Vec<&str> = vec!["HOME"];
    if cfg!(windows) {
        keys.extend(["SystemRoot", "SystemDrive", "windir", "TEMP", "TMP", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "PATH"]);
    }
    for key in keys {
        if let Some(value) = std::env::var_os(key) {
            env.push((key.to_string(), value));
        }
    }
    env
}

/// Kills the child once `timeout` passes without the run finishing. The main thread signals
/// `finished` when it is done so the watchdog leaves without a kill.
fn watchdog(
    child: Arc<Mutex<Child>>,
    finished: Arc<(Mutex<bool>, Condvar)>,
    timed_out: Arc<AtomicBool>,
    timeout: Duration,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Ok(done) = finished.0.lock() else { return };
        let Ok((guard, wait)) = finished.1.wait_timeout_while(done, timeout, |done| !*done) else { return };
        let unfinished = !*guard;
        drop(guard);
        if wait.timed_out() && unfinished {
            timed_out.store(true, Ordering::Relaxed);
            if let Ok(mut child) = child.lock() {
                let _ = child.kill();
            }
        }
    })
}

fn with_stderr(error: ScriptError, stderr: &str) -> ScriptError {
    if stderr.is_empty() || error.kind == gridcraft_script::bridge::FailureKind::Limit {
        return error;
    }
    let message = format!("{}\n{}", error.message, tail(stderr, 2000));
    ScriptError { message, ..error }.with_line_from_message()
}

fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let start = text.len().saturating_sub(max);
    let start = text.get(start..).map(|_| start).unwrap_or(0);
    text.get(start..).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fatal_line_becomes_a_runtime_error_with_its_line() {
        let error = fatal_line("{\"fatal\":{\"message\":\"x is not defined\",\"line\":4}}").unwrap();
        assert_eq!(error.kind, gridcraft_script::bridge::FailureKind::Runtime);
        assert_eq!(error.line, Some(4));
    }

    #[test]
    fn other_lines_are_not_fatal() {
        assert!(fatal_line("{\"id\":1,\"method\":\"range.setValue\"}").is_none());
        assert!(fatal_line("not json").is_none());
    }

    #[test]
    fn stderr_is_only_kept_for_errors() {
        let base = ScriptError::runtime("the script failed");
        assert_eq!(with_stderr(base.clone(), "").message, "the script failed");
        assert!(with_stderr(base, "boom").message.contains("boom"));
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
    }

    #[test]
    fn discovery_is_quiet_without_bun() {
        // Only checks that looking does not fail; whether Bun is installed does not matter.
        let _ = discover::find_exe();
        assert!(!discover::expected_path().as_os_str().is_empty());
    }

    #[test]
    fn the_missing_host_names_the_expected_place_and_refuses_to_run() {
        let host = MissingHost::new();
        let info = host.runtime();
        assert!(!info.found);
        assert!(!info.expected_path.is_empty());
        assert!(info.message.contains("Bun"));
        let mut exec = NoCalls;
        assert!(host.run("x", &Limits::default(), &mut exec).is_err());
    }

    struct NoCalls;
    impl BridgeExecutor for NoCalls {
        fn execute(&mut self, _call: &gridcraft_script::BridgeCall) -> Result<Json, gridcraft_script::BridgeError> {
            Ok(Json::Null)
        }
    }

    #[test]
    fn the_shim_is_not_empty() {
        assert!(shim::BOOTSTRAP.contains("ExcelScript"));
        assert!(shim::BOOTSTRAP.contains("Range.setValue"));
        // The script gets the enum namespace and a completion line the parent waits for.
        assert!(shim::BOOTSTRAP.contains("ExcelScriptNs"));
        assert!(shim::BOOTSTRAP.contains("done: true"));
        // The dangerous globals are shadowed as parameters, not left in the script's scope.
        for name in ["\"process\"", "\"Bun\"", "\"require\"", "\"fetch\""] {
            assert!(shim::BOOTSTRAP.contains(name), "{name} is not shadowed");
        }
    }

    /// Records the calls the bridge is asked to run.
    struct Recording {
        calls: Vec<String>,
    }

    impl BridgeExecutor for Recording {
        fn execute(&mut self, call: &gridcraft_script::BridgeCall) -> Result<Json, gridcraft_script::BridgeError> {
            self.calls.push(call.method.clone());
            Ok(Json::Null)
        }
    }

    fn run_pump(input: &[u8]) -> (Result<RunReport, ScriptError>, Vec<u8>, Vec<String>) {
        let mut out = Vec::new();
        let mut exec = Recording { calls: Vec::new() };
        let result = pump(std::io::Cursor::new(input.to_vec()), &mut out, &mut exec, &Limits::default());
        (result, out, exec.calls)
    }

    #[test]
    fn a_run_ends_with_the_shims_completion_line() {
        let input = b"{\"id\":1,\"method\":\"Range.setValue\",\"params\":{}}\n{\"done\":true}\n";
        let (result, out, calls) = run_pump(input);
        let report = result.unwrap();
        assert_eq!(report.calls, 1);
        assert_eq!(calls, vec!["Range.setValue"]);
        assert!(out.ends_with(b"\n"), "{out:?}");
    }

    #[test]
    fn output_that_stops_without_the_completion_line_is_an_error() {
        // A crash, or a script that calls `process.exit(0)` after editing the workbook: the
        // calls so far must not be committed as a finished run.
        let input = b"{\"id\":1,\"method\":\"Range.setValue\",\"params\":{}}\n";
        let (result, _, calls) = run_pump(input);
        let error = result.unwrap_err();
        assert_eq!(error.kind, gridcraft_script::bridge::FailureKind::Runtime);
        assert!(error.message.contains("before the script finished"), "{}", error.message);
        assert_eq!(calls, vec!["Range.setValue"], "the call ran, the run did not finish");
    }

    #[test]
    fn a_line_at_the_limit_is_read_and_one_over_is_refused() {
        let mut buf = Vec::new();
        assert!(read_bounded_line(&mut std::io::Cursor::new(vec![b'x'; 16]), &mut buf, 16).unwrap());
        assert_eq!(buf.len(), 16);
        let error = read_bounded_line(&mut std::io::Cursor::new(vec![b'x'; 17]), &mut buf, 16).unwrap_err();
        assert_eq!(error.kind, gridcraft_script::bridge::FailureKind::Limit);
    }

    #[test]
    fn a_very_long_line_fails_the_run_instead_of_growing_memory() {
        // `read_line` would buffer until the timeout; the bounded read stops at the limit.
        let input = vec![b'x'; MAX_LINE_BYTES + 1];
        let (result, _, _) = run_pump(&input);
        assert_eq!(result.unwrap_err().kind, gridcraft_script::bridge::FailureKind::Limit);
    }

    #[test]
    fn a_run_gets_a_minimal_environment() {
        let env = runtime_env();
        assert!(env.iter().any(|(key, value)| key == "NO_COLOR" && value.to_string_lossy() == "1"));
        let allowed = ["NO_COLOR", "HOME", "SystemRoot", "SystemDrive", "windir", "TEMP", "TMP", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "PATH"];
        let unix_allowed = ["NO_COLOR", "HOME"];
        for (key, _) in &env {
            assert!(allowed.contains(&key.as_str()), "{key} leaks in");
            assert!(cfg!(windows) || unix_allowed.contains(&key.as_str()), "{key} is not passed on unix");
        }
    }
}
