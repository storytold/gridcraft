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
        Ok(match call.method.as_str() {
            // The engine answers these with values, so the shim's readers have something to read.
            "Worksheet.getName" => Json::String("Data".into()),
            "RangeFormat.getRangeBorder" => json!({"kind": "border", "sheet": "Data", "range": "A1", "side": "edgetop"}),
            // Far past the shim's 64 KiB read, in three-byte characters: the read boundary falls
            // inside one of them.
            "Range.getValues" => json!([["€".repeat(30_000)]]),
            "Worksheet.getVisibility" => Json::String("visible".into()),
            _ => Json::Null,
        })
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

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn a_worksheet_name_and_the_excel_script_enums_reach_the_bridge() {
    let Some(host) = host() else { return };
    // `activate()` used to store "" (the shim read `.sheet` from a string), and every
    // `ExcelScript.…` enum was a ReferenceError before the namespace existed.
    let script = r##"
function main(workbook: ExcelScript.Workbook) {
  const sheet = workbook.getWorksheet("Data");
  sheet.activate();
  sheet.getRange("A1").setValue(ExcelScript.BorderIndex.edgeTop);
  sheet.getRange("A1").getFormat().getRangeBorder(ExcelScript.BorderIndex.edgeTop).setColor("#ff0000");
  console.log("name", sheet.getName());
}
"##;
    let mut calls = Calls { calls: Vec::new() };
    let report = host.run(script, &Limits::default(), &mut calls).expect("the script runs");
    let methods: Vec<&str> = calls.calls.iter().map(|c| c.method.as_str()).collect();
    // `getRangeBorder` builds the Border in the shim; `console.log` is answered by the host
    // without reaching the engine.
    assert_eq!(methods, ["Worksheet.activate", "Worksheet.getName", "Range.setValue", "RangeBorder.setColor", "Worksheet.getName"]);
    assert_eq!(calls.calls[1].params["sheet"], json!("Data"), "activate kept the worksheet name");
    assert_eq!(calls.calls[2].params["value"], json!("EdgeTop"), "the enum reaches the bridge as its value");
    assert_eq!(calls.calls[3].params["side"], json!("EdgeTop"));
    assert_eq!(report.output, vec!["name Data".to_string()]);
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn an_unknown_excel_script_enum_is_a_typed_error() {
    let Some(host) = host() else { return };
    let mut calls = Calls { calls: Vec::new() };
    let script =
        "function main(workbook: ExcelScript.Workbook) { workbook.getWorksheet(\"Data\").getRange(\"A1\").setValue(ExcelScript.Nope.value); }";
    let error = host.run(script, &Limits::default(), &mut calls).expect_err("an unknown enum is refused");
    assert!(error.message.contains("ExcelScript.Nope"), "{}", error.message);
    assert!(calls.calls.is_empty(), "nothing reaches the bridge");
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn a_script_cannot_reach_process_bun_require_or_fetch() {
    let Some(host) = host() else { return };
    // `typeof require` is folded to "function" by the transpiler, so the test uses the globals:
    // each one is `undefined` in the script's scope and throws on use.
    let script = r#"
function main() {
  const blocked: string[] = [];
  try { require("node:fs"); } catch { blocked.push("require"); }
  try { process.exit(1); } catch { blocked.push("process"); }
  try { Bun.spawn(["echo"]); } catch { blocked.push("Bun"); }
  try { fetch("http://127.0.0.1:1/"); } catch { blocked.push("fetch"); }
  console.log(blocked.join(","));
}
"#;
    let mut calls = Calls { calls: Vec::new() };
    let report = host.run(script, &Limits::default(), &mut calls).expect("the script runs");
    assert_eq!(report.output, vec!["require,process,Bun,fetch".to_string()]);
    assert!(calls.calls.is_empty(), "no workbook call is made");
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn a_reply_longer_than_a_read_keeps_its_multibyte_characters() {
    let Some(host) = host() else { return };
    let script = r#"
function main(workbook: ExcelScript.Workbook) {
  const values = workbook.getWorksheet("Data").getRange("A1:A30000").getValues();
  const text = String(values[0][0]);
  console.log("len", text.length, "replacement", text.includes("\uFFFD"));
}
"#;
    let mut calls = Calls { calls: Vec::new() };
    let report = host.run(script, &Limits::default(), &mut calls).expect("the script runs");
    assert_eq!(report.output, vec!["len 30000 replacement false".to_string()]);
}

#[test]
#[ignore = "needs Bun; set GRIDCRAFT_BUN"]
fn worksheet_visibility_reads_back_in_the_excel_script_casing() {
    let Some(host) = host() else { return };
    // The engine answers `visible`; ExcelScript compares against `SheetVisibility.visible`.
    let script = r#"
function main(workbook: ExcelScript.Workbook) {
  const sheet = workbook.getWorksheet("Data");
  console.log(sheet.getVisibility(), sheet.getVisible(), sheet.getVisibility() === ExcelScript.SheetVisibility.visible);
}
"#;
    let mut calls = Calls { calls: Vec::new() };
    let report = host.run(script, &Limits::default(), &mut calls).expect("the script runs");
    assert_eq!(report.output, vec!["Visible true true".to_string()]);
}
