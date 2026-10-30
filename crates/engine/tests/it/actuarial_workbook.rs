//! The shipped command-script template is exercised through recalculation, XLSX and MC.
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

fn template() -> Session {
    replay(include_str!("../../../../docs/examples/actuarial.jsonl"))
}

fn replay(script: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    for line in script.lines() {
        let command: serde_json::Value = serde_json::from_str(line).unwrap();
        s.execute(command["command"].as_str().unwrap(), command["params"].clone()).unwrap();
    }
    s
}
fn numeric(s: &Session, sheet: &str, cell: &str) -> f64 {
    let sh = s.doc().unwrap().wb.sheets.iter().find(|s| s.name == sheet).unwrap();
    match sh.value(CellRef::parse(cell).unwrap()) {
        Value::Number(x) => x,
        other => panic!("{sheet}!{cell}: expected numeric, got {other:?}"),
    }
}
fn close(s: &Session, sheet: &str, cell: &str, expected: f64) {
    let actual = numeric(s, sheet, cell);
    assert!((actual - expected).abs() < 1e-9 * (1.0 + expected.abs()), "{sheet}!{cell}: {actual} != {expected}");
}
fn check(s: &Session) {
    close(s, "Loss", "B6", 10_000.0);
    close(s, "Loss", "B7", 20_000_000.0);
    close(s, "Loss", "E6", 0.0);
    close(s, "Loss", "E7", 100.0);
    close(s, "Loss", "E15", 500.0);
    close(s, "Life", "B11", 0.0);
    close(s, "Reserves", "G8", 148.0);
    close(s, "Reserves", "G9", 148.0);
    close(s, "Markov", "G8", 1.0);
    close(s, "Markov", "G9", 1.0);
    close(s, "Markov", "F14", (-0.2_f64).exp());
    close(s, "Pricing", "F3", 2.0_f64.ln());
    close(s, "Pricing", "F4", 2.0_f64.ln());
    close(s, "Pricing", "H3", 2.0);
    close(s, "Pricing", "H6", 4.0);
    close(s, "Pricing", "B9", 0.0);
    // No worksheet error is permitted anywhere in the teaching template, including spills.
    for sh in &s.doc().unwrap().wb.sheets {
        for (cell, data) in sh.cells.iter() {
            assert!(!matches!(data.value, Value::Error(_)), "{}!{cell:?}: {:?}", sh.name, data.value);
        }
    }
}
#[test]
fn actuarial_template_calculates_and_roundtrips_xlsx() {
    let mut s = template();
    check(&s);
    let saved = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name":"actuarial.xlsx","base64":saved["base64"]})).unwrap();
    check(&reopened);
    reopened.execute("cell.set", json!({"sheet":"Loss","cell":"B2","input":"12"})).unwrap();
    close(&reopened, "Loss", "B6", 12_000.0);
    reopened.execute("edit.undo", json!({})).unwrap();
    close(&reopened, "Loss", "B6", 10_000.0);
}
#[test]
fn actuarial_formula_models_use_existing_seeded_monte_carlo_without_changing_sources() {
    let mut s = template();
    let report = s
        .execute(
            "data.monteCarlo",
            json!({
                "inputs":[{"cell":"Loss!B2","distribution":"uniform","min":8,"max":12}],
                "outputs":["Loss!B6","Loss!B10"],"iterations":32,"seed":42,
                "method":"latinHypercube","replicates":4,"samples":true,"newSheet":"Actuarial uncertainty"
            }),
        )
        .unwrap();
    assert!(!report.is_null());
    close(&s, "Loss", "B2", 10.0);
    close(&s, "Loss", "B6", 10_000.0);
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 6);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 5);
    check(&s);
}

#[test]
fn scientific_template_has_calibrated_models_and_validated_predictions_after_xlsx_reopen() {
    fn check_science(s: &Session) {
        close(s, "Inference", "F16", 1.2);
        close(s, "Inference", "G16", 0.24);
        close(s, "Inference", "K16", 1.44);
        close(s, "Inference", "I3", 0.75);
        close(s, "Inference", "I4", 0.5);
        close(s, "Inference", "J12", 0.75);
        close(s, "Inference", "K12", 0.25);
        close(s, "Inference", "E26", 0.0);
        close(s, "Inference", "G26", 2.0 / 3.0);
        close(s, "Extremes", "E14", 100.0 + 20.0 * 10.0_f64.ln());
        close(s, "Extremes", "E15", 120.0 + 20.0 * 10.0_f64.ln());
        close(s, "Decision", "D19", 5.0);
        close(s, "Decision", "D20", 2.5);
        close(s, "Decision", "E28", 200.0);
        close(s, "Decision", "H28", 19.0);
        close(s, "Proxy", "M3", 4.75);
        close(s, "Proxy", "M6", 10.75);
        close(s, "Duration", "D12", 0.4);
        close(s, "Duration", "E12", 0.6);
        close(s, "Bootstrap", "H3", 940.0);
        close(s, "Bootstrap", "M3", 110.0 / 7.0);
        for sh in &s.doc().unwrap().wb.sheets {
            for (cell, data) in sh.cells.iter() {
                assert!(!matches!(data.value, Value::Error(_)), "{}!{cell:?}: {:?}", sh.name, data.value);
            }
        }
    }
    let mut s = replay(include_str!("../../../../docs/examples/actuarial-scientific.jsonl"));
    check_science(&s);
    let saved = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name":"scientific-actuarial.xlsx","base64":saved["base64"]})).unwrap();
    check_science(&reopened);
}

#[test]
fn independent_scientific_reference_oracles_are_preserved_at_engine_boundary() {
    // These expectations come from independent closed-form/Decimal oracles, not from
    // recorded outputs of this engine. Tiny nonzero tails must never pass as false zero.
    fn compare(actual: &serde_json::Value, expected: &serde_json::Value, relative: f64, label: &str) {
        if let Some(expected_number) = expected.as_f64() {
            let actual_number = actual.as_f64().unwrap_or_else(|| panic!("{label}: {actual:?}"));
            let tolerance = if expected_number == 0.0 { 1e-12 } else { relative * expected_number.abs() };
            assert!((actual_number - expected_number).abs() <= tolerance, "{label}: {actual_number} != {expected_number}, tol {tolerance}");
        } else if let Some(expected_array) = expected.as_array() {
            let actual_array = actual.as_array().unwrap_or_else(|| panic!("{label}: {actual:?}"));
            assert_eq!(actual_array.len(), expected_array.len(), "{label}");
            for (actual, expected) in actual_array.iter().zip(expected_array) {
                compare(actual, expected, relative, label);
            }
        } else {
            assert_eq!(actual, expected, "{label}");
        }
    }
    let data: serde_json::Value = serde_json::from_str(include_str!("../fixtures/actuarial-reference.json")).unwrap();
    let mut s = Session::new();
    s.new_workbook();
    for case in data["cases"].as_array().unwrap() {
        let actual = s.execute("formulas.evaluate", json!({"formula":case["formula"]})).unwrap();
        compare(&actual, &case["expected"], case["relative_tolerance"].as_f64().unwrap(), case["label"].as_str().unwrap());
    }
}

/// Uses a supplied mortality model through the same command path as CLI/MCP clients.
fn operational_fixture() -> (Session, serde_json::Value) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"A1","input":"0.05"})).unwrap();
    s.execute("cell.set", json!({"cell":"B1","input":"=ACT.ANNUITY({0.2;0.5;1},A1,,0)"})).unwrap();
    let p = json!({"inputs":[{"cell":"Sheet1!A1","distribution":"uniform","min":0.01,"max":0.1}],"outputs":["Sheet1!B1"],"iterations":16,"seed":42,"samples":true});
    (s, p)
}

/// Allows bounded preparation steps without assuming that one step always completes a trial.
fn advance_actuarial_job(s: &mut Session, id: &serde_json::Value, target: u64) -> serde_json::Value {
    for _ in 0..64 {
        let progress = s.execute("analysis.step", json!({"job":id,"trials":1})).unwrap();
        if progress["completed"].as_u64().unwrap() >= target {
            return progress;
        }
    }
    panic!("small actuarial job did not reach its bounded target");
}

/// Progress is observable while cancellation preserves source data, journal and undo history.
#[test]
fn actuarial_incremental_progress_and_cancel_preserve_the_workbook() {
    let (mut s, params) = operational_fixture();
    let source = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let journal = s.journal.len();
    let before = s.execute("analysis.status", json!({})).unwrap()["sharedWorkRemaining"].as_u64().unwrap();
    let id = s.execute("analysis.start", json!({"params":params.clone()})).unwrap()["job"].clone();
    let progress = advance_actuarial_job(&mut s, &id, 1);
    assert_eq!(progress["total"], 16);
    assert!(progress["progress"].as_f64().unwrap() > 0.0);
    assert!(progress["progress"].as_f64().unwrap() < 1.0);
    assert!(progress["workUsed"].as_u64().unwrap() > 0);
    assert_eq!(s.execute("analysis.status", json!({"job":id})).unwrap()["completed"], progress["completed"]);
    let cancelled = s.execute("analysis.cancel", json!({"job":id})).unwrap();
    assert_eq!(cancelled["state"], "cancelled");
    assert!(std::sync::Arc::ptr_eq(&source, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    assert_eq!(s.journal.len(), journal);
    assert!(s.execute("analysis.finish", json!({"job":id})).is_err());
    let after = s.execute("analysis.status", json!({})).unwrap()["sharedWorkRemaining"].as_u64().unwrap();
    assert!(after < before, "cancelled work must remain charged to the workbook bank");
    let newer = s.execute("analysis.start", json!({"params":params})).unwrap()["job"].clone();
    advance_actuarial_job(&mut s, &newer, 1);
    s.execute("analysis.cancel", json!({"job":newer})).unwrap();
    let remaining = s.execute("analysis.status", json!({})).unwrap()["sharedWorkRemaining"].as_u64().unwrap();
    assert!(remaining < after, "starting a second job must not reset the shared bank");
    assert!(std::sync::Arc::ptr_eq(&source, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    assert_eq!(s.journal.len(), journal);
}

/// Cooperative completion and the synchronous command preserve the same seeded estimands.
#[test]
fn actuarial_incremental_finish_matches_synchronous_statistics_and_one_undo() {
    let (mut synchronous, params) = operational_fixture();
    let expected = synchronous.execute("data.monteCarlo", params.clone()).unwrap();
    let (mut s, _) = operational_fixture();
    let source = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let id = s.execute("analysis.start", json!({"params":params})).unwrap()["job"].clone();
    advance_actuarial_job(&mut s, &id, 16);
    assert!(std::sync::Arc::ptr_eq(&source, &s.doc().unwrap().wb));
    let actual = s.execute("analysis.finish", json!({"job":id})).unwrap();
    assert_eq!(actual["outputs"], expected["outputs"]);
    assert_eq!(actual["tornado"], expected["tornado"]);
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    close(&s, "Sheet1", "A1", 0.05);
    close(&s, "Sheet1", "B1", 0.8 / 1.05 + 0.4 / 1.05_f64.powi(2));
}

/// A caught worksheet error cannot turn a refused lifted ACT calculation into a report.
#[test]
fn lifted_actuarial_work_exhaustion_is_terminal_even_when_iferror_catches_num() {
    let (mut control, mut params) = operational_fixture();
    params["iterations"] = json!(2);
    params["maxWork"] = json!(1000);
    control.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
    assert!(control.execute("data.monteCarlo", params.clone()).is_ok(), "small trial must fit the same budget");
    let (mut s, _) = operational_fixture();
    s.execute("cell.set", json!({"cell":"B1","input":"=IFERROR(SUM(ACT.ANNUITY({0.2;0.5;1},A1+SEQUENCE(1,64,0,0),3,0)),7)"})).unwrap();
    assert!(numeric(&s, "Sheet1", "B1") > 7.0);
    let source = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let journal = s.journal.len();
    let error = s.execute("data.monteCarlo", params).expect_err("native/lifted work must exhaust the host bank");
    assert!(error.to_string().to_lowercase().contains("work"), "{error}");
    assert!(std::sync::Arc::ptr_eq(&source, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    assert_eq!(s.journal.len(), journal);
}

/// ACT-specific paint formulas retain their quota oracles when generic MC tests use normal CDFs.
#[test]
fn actuarial_conditional_formulas_do_not_style_refused_gpd_results() {
    let mut s = Session::new();
    s.new_workbook();
    let values = (1..=2048).map(|n| json!([n])).collect::<Vec<_>>();
    s.execute("range.setValues", json!({"range":"A1:A2048","values":values})).unwrap();
    for rule in [
        json!({"type":"expression","formula":"=IFERROR(INDEX(ACT.GPD.FIT.MLE($A$1:$A$2048),1,1),TRUE)","preset":"lightRedFill"}),
        json!({"type":"cellIs","operator":"notEqual","value":"=ACT.GPD.FIT.MLE($A$1:$A$2048)","preset":"lightRedFill"}),
    ] {
        s.execute("home.conditionalFormat", json!({"range":"B1:B2","rule":rule})).unwrap();
        let d = s.doc().unwrap();
        let before = d.analysis_budget.remaining().unwrap();
        for _ in 0..3 {
            let mut paint = gridcraft_engine::cf::CfCache::with_budget(d.render_budget.clone());
            assert!(paint.look(&d.wb, 0, CellRef::parse("B1").unwrap()).is_none());
            assert!(paint.look(&d.wb, 0, CellRef::parse("B2").unwrap()).is_none());
        }
        assert_eq!(d.analysis_budget.remaining().unwrap(), before);
    }
}

/// A costly ACT formula cannot consume its frame's bank before the local cell cap rejects it.
#[test]
fn actuarial_render_refusal_keeps_credit_for_inexpensive_neighbouring_cells() {
    let mut s = Session::new();
    s.new_workbook();
    let values = (1..=2048).map(|n| json!([n])).collect::<Vec<_>>();
    s.execute("range.setValues", json!({"range":"A1:A2048","values":values})).unwrap();
    s.execute("home.conditionalFormat", json!({"range":"B1","rule":{"type":"expression","formula":"=IFERROR(INDEX(ACT.GPD.FIT.MLE($A$1:$A$2048),1,1),TRUE)","preset":"lightRedFill"}})).unwrap();
    s.execute("home.conditionalFormat", json!({"range":"B2","rule":{"type":"expression","formula":"=TRUE","preset":"lightRedFill"}})).unwrap();
    let d = s.doc().unwrap();
    let mut paint = gridcraft_engine::cf::CfCache::with_budget(d.render_budget.clone());
    assert!(paint.look(&d.wb, 0, CellRef::parse("B1").unwrap()).is_none());
    assert!(d.render_budget.remaining().unwrap() > 990_000);
    assert!(paint.look(&d.wb, 0, CellRef::parse("B2").unwrap()).is_some());
}
