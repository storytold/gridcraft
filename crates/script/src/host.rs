//! The runner seam: an [`ScriptHost`] starts a runtime, feeds it the script, and calls back into
//! GridCraft for every workbook operation ([`BridgeExecutor`]). The native host drives Bun; the
//! [`FakeHost`] here replays a transcript for tests, so no JavaScript runtime is needed in CI.

use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::MAX_CODE_CHARS;
use crate::bridge::{BridgeCall, BridgeError, ScriptError};

/// What one script run did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    /// Bridge calls the script made, including ones that failed.
    pub calls: usize,
    /// `console.log` lines, oldest first, already bounded by [`Limits::max_output_bytes`].
    pub output: Vec<String>,
}

/// The bounds on one run. Every one of them is a hard stop: a script that reaches one fails and
/// its changes are rolled back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_code_chars: usize,
    pub max_calls: usize,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_code_chars: MAX_CODE_CHARS, max_calls: 10_000, timeout: Duration::from_secs(30), max_output_bytes: 1024 * 1024 }
    }
}

impl Limits {
    /// The limits with a caller-chosen wall-clock timeout, clamped to 100 ms … 10 minutes.
    pub fn with_timeout(mut self, ms: Option<u64>) -> Self {
        if let Some(ms) = ms {
            self.timeout = Duration::from_millis(ms.clamp(100, 600_000));
        }
        self
    }
}

/// A JavaScript runtime that can run an Office Script.
pub trait ScriptHost: Send + Sync {
    /// Where the runtime is, or why there is none. Must be cheap: the pane asks often.
    fn runtime(&self) -> RuntimeInfo;

    /// Runs `code` against `exec`, bounded by `limits`. Failing returns the error the pane shows;
    /// anything it changed is rolled back by the caller. Never panics and never leaves the
    /// runtime running.
    fn run(&self, code: &str, limits: &Limits, exec: &mut dyn BridgeExecutor) -> Result<RunReport, ScriptError>;
}

/// The workbook side of the bridge. Implemented by the engine, which turns each call into a
/// normal command.
pub trait BridgeExecutor {
    fn execute(&mut self, call: &BridgeCall) -> Result<Json, BridgeError>;
}

/// What the pane shows about the runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub found: bool,
    /// One sentence for the pane: where it is, or what to do.
    pub message: String,
    /// The executable actually found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The place the app looks last, for the "put it here" instruction.
    pub expected_path: String,
    /// Where the user can download it.
    pub download_url: String,
    /// What the runtime reports for `--version`, when it answered in time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl RuntimeInfo {
    /// No runtime: `expected` is where the app would find one.
    pub fn missing(expected: impl Into<String>, message: impl Into<String>) -> Self {
        RuntimeInfo {
            found: false,
            message: message.into(),
            path: None,
            expected_path: expected.into(),
            download_url: crate::BUN_DOWNLOAD_URL.to_string(),
            version: None,
        }
    }

    /// A runtime at `path`, with its version when it was asked.
    pub fn found(path: impl Into<String>, expected: impl Into<String>, version: Option<String>) -> Self {
        let path = path.into();
        RuntimeInfo {
            found: true,
            message: format!("Bun found at {path}"),
            path: Some(path),
            expected_path: expected.into(),
            download_url: crate::BUN_DOWNLOAD_URL.to_string(),
            version,
        }
    }
}

impl fmt::Display for RuntimeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A host for tests: it runs a fixed transcript of bridge calls, as the shim would send for a
/// script, and can fail after a chosen number of them. It does not read `code`; its point is the
/// workbook side of the protocol — that a call is a command, that a failure rolls back, that a
/// timeout leaves the workbook alone.
#[derive(Debug)]
pub struct FakeHost {
    calls: Vec<BridgeCall>,
    fail_after: Option<usize>,
    failure: Option<ScriptError>,
    runtime: RuntimeInfo,
    last_code: Mutex<Option<String>>,
    executed: Mutex<Vec<BridgeCall>>,
}

impl FakeHost {
    /// A host that runs `calls` in order.
    pub fn new(calls: Vec<BridgeCall>) -> Self {
        FakeHost {
            calls,
            fail_after: None,
            failure: None,
            runtime: RuntimeInfo::found("fake-bun", "fake-bun", Some("test".into())),
            last_code: Mutex::new(None),
            executed: Mutex::new(Vec::new()),
        }
    }

    /// A host that executes `calls[..after]` and then fails with `error`.
    pub fn failing_after(after: usize, calls: Vec<BridgeCall>, error: ScriptError) -> Self {
        let mut host = FakeHost::new(calls);
        host.fail_after = Some(after);
        host.failure = Some(error);
        host
    }

    /// A host with no runtime, for the "everything is disabled" path.
    pub fn with_runtime(runtime: RuntimeInfo) -> Self {
        let mut host = FakeHost::new(Vec::new());
        host.runtime = runtime;
        host
    }

    /// The script text of the last run, for tests that check what the pane passed in.
    pub fn last_code(&self) -> Option<String> {
        self.last_code.lock().ok().and_then(|c| c.clone())
    }

    /// The calls that reached the workbook side.
    pub fn executed(&self) -> Vec<BridgeCall> {
        self.executed.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl ScriptHost for FakeHost {
    fn runtime(&self) -> RuntimeInfo {
        self.runtime.clone()
    }

    fn run(&self, code: &str, _limits: &Limits, exec: &mut dyn BridgeExecutor) -> Result<RunReport, ScriptError> {
        if let Ok(mut last) = self.last_code.lock() {
            *last = Some(code.to_string());
        }
        let mut report = RunReport::default();
        let mut used = 0usize;
        for call in &self.calls {
            if let Some(after) = self.fail_after
                && used >= after
            {
                return Err(self.failure.clone().unwrap_or_else(|| ScriptError::runtime("the fake host failed")));
            }
            used = used.saturating_add(1);
            report.calls = report.calls.saturating_add(1);
            if call.method == "console.log" {
                if let Some(message) = call.params.get("message").and_then(Json::as_str) {
                    report.output.push(message.to_string());
                }
                continue;
            }
            if let Ok(mut executed) = self.executed.lock() {
                executed.push(call.clone());
            }
            exec.execute(call)?;
        }
        if let Some(after) = self.fail_after
            && used >= after
        {
            return Err(self.failure.clone().unwrap_or_else(|| ScriptError::runtime("the fake host failed")));
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct Recorder {
        calls: Vec<BridgeCall>,
    }

    impl BridgeExecutor for Recorder {
        fn execute(&mut self, call: &BridgeCall) -> Result<Json, BridgeError> {
            self.calls.push(call.clone());
            Ok(Json::Null)
        }
    }

    #[test]
    fn the_fake_host_replays_its_transcript_through_the_executor() {
        let host = FakeHost::new(vec![
            BridgeCall::new("range.setValue", json!({"range": "A1", "value": 42})),
            BridgeCall::new("console.log", json!({"message": "done"})),
        ]);
        let mut exec = Recorder::default();
        let report = host.run("ignored", &Limits::default(), &mut exec).unwrap();
        assert_eq!(report.calls, 2);
        assert_eq!(report.output, vec!["done".to_string()]);
        assert_eq!(exec.calls.len(), 1);
        assert_eq!(host.last_code().as_deref(), Some("ignored"));
    }

    #[test]
    fn a_failure_after_a_call_reports_the_error() {
        let host = FakeHost::failing_after(1, vec![BridgeCall::new("range.setValue", json!({"range": "A1", "value": 1}))], ScriptError::timeout(50));
        let mut exec = Recorder::default();
        let error = host.run("ignored", &Limits::default(), &mut exec).unwrap_err();
        assert_eq!(error.kind, crate::bridge::FailureKind::Timeout);
        assert_eq!(exec.calls.len(), 1);
    }

    #[test]
    fn limits_clamp_the_timeout() {
        assert_eq!(Limits::default().with_timeout(Some(1)).timeout, Duration::from_millis(100));
        assert_eq!(Limits::default().with_timeout(Some(u64::MAX)).timeout, Duration::from_secs(600));
        assert_eq!(Limits::default().with_timeout(None).timeout, Duration::from_secs(30));
    }
}
