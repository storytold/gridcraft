//! Runs a real Bun against the bridge. Ignored by default: CI has no runtime, and the maintainer
//! decision is that Bun is user-downloaded. Run it with
//! `GRIDCRAFT_BUN=/path/to/bun cargo test -p gridcraft-script-bun -- --ignored`.

use std::time::Duration;

use gridcraft_script::bridge::FailureKind;
use gridcraft_script::{BridgeCall, BridgeError, BridgeExecutor, Limits, ScriptError, ScriptHost};
use gridcraft_script_bun::BunHost;
use serde_json::{Value as Json, json};

struct Calls {
    calls: Vec<BridgeCall>,
}

impl BridgeExecutor for Calls {
    fn execute(&mut self, call: &BridgeCall) -> Result<Json, BridgeError> {
        self.calls.push(call.clone());
        Ok(Json::Null)
    }
}

fn host() -> Option<BunHost> {
    let Ok(path) = std::env::var("GRIDCRAFT_BUN") else {
        eprintln!("GRIDCRAFT_BUN is not set; skipping the Bun integration test");
        return None;
    };
    match BunHost::at(path) {
        Some(host) => Some(host),
        None => {
            eprintln!("GRIDCRAFT_BUN does not point at a file; skipping");
            None
        }
    }
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn a_typescript_script_calls_the_bridge_and_logs() {
    let Some(host) = host() else { return };
    let script = r#"
function main(workbook: ExcelScript.Workbook) {
  const sheet = workbook.getWorksheet("Data");
  sheet.getRange("A1:B2").setValues([[1, "two"], [true, null]]);
  sheet.getRange("A1").setValue(42);
  sheet.getRange("C1").setNumberFormat("0.00");
  sheet.getRange("D1").getFormat().getFont().setBold(true);
  console.log("done", 42);
}
"#;
    let mut calls = Calls { calls: Vec::new() };
    let report = host.run(script, &Limits::default(), &mut calls).expect("the script runs");
    let methods: Vec<&str> = calls.calls.iter().map(|c| c.method.as_str()).collect();
    assert_eq!(methods, ["Range.setValues", "Range.setValue", "Range.setNumberFormat", "RangeFont.setBold"]);
    assert_eq!(calls.calls[1].params["value"], json!(42));
    assert_eq!(calls.calls[0].params["sheet"], json!("Data"));
    assert_eq!(report.output, vec!["done 42".to_string()]);
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn an_endless_script_is_killed_at_the_timeout() {
    let Some(host) = host() else { return };
    let limits = Limits { timeout: Duration::from_millis(500), ..Limits::default() };
    let mut calls = Calls { calls: Vec::new() };
    let error: ScriptError = host.run("function main() { while (true) {} }", &limits, &mut calls).expect_err("an endless script is stopped");
    assert_eq!(error.kind, FailureKind::Timeout);
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn a_script_without_main_is_reported() {
    let Some(host) = host() else { return };
    let mut calls = Calls { calls: Vec::new() };
    let error = host.run("const x = 1;", &Limits::default(), &mut calls).expect_err("a script needs main");
    assert_eq!(error.kind, FailureKind::Runtime);
    assert!(error.message.contains("main"), "{}", error.message);
}
