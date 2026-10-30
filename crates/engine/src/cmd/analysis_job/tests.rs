use super::*;
use gridcraft_core::CellRef;
use std::sync::Arc;

/// Builds deterministic source data and canonical cross-sheet simulation parameters.
fn fixture() -> (Session, Json) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
    s.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
    let p = json!({"inputs":[{"cell":"Sheet1!A1","min":0,"max":1}],"outputs":["Sheet1!B1"],"iterations":8,"samples":false,"seed":42});
    (s, p)
}

/// Measure real small prefix-sum recalculations, then gate the conservative large bound without
/// visiting 1.25 billion cells. This covers a new logical recalculation rather than many small calls.
#[test]
fn prefix_sum_work_measurement_and_bulk_policy_cover_the_large_model_shape() {
    /// Records the actual evaluator/visitor charges and verifies the resulting sum.
    fn measured(rows: u32) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        let mut s = Session::new();
        s.new_workbook();
        let values = (1..=rows).map(|row| json!([1, format!("=SUM($A$1:A{row})")])).collect::<Vec<_>>();
        s.execute("range.setValues", json!({"range":"A1","values":values})).unwrap();
        let units = Arc::new(AtomicU64::new(0));
        let seen = units.clone();
        let hook = gridcraft_calc::WorkHook::new(move |n| {
            seen.fetch_add(n, Ordering::Relaxed);
            true
        });
        {
            let _scope = hook.enter().unwrap();
            s.execute("formulas.calculateNow", json!({})).unwrap();
        }
        assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(rows - 1, 1)).as_f64(), Some(f64::from(rows)));
        units.load(Ordering::Relaxed)
    }
    for rows in [16u32, 64] {
        let actual = measured(rows);
        let n = u64::from(rows);
        let triangular = n * (n + 1) / 2;
        assert!(actual >= triangular);
        assert!(actual <= triangular + 16 * n, "real SUM charge exceeded the conservative fixture: {rows}/{actual}");
    }
    let rows = 50_000u64;
    let bound = rows * (rows + 1) / 2 + 16 * rows;
    let bulk = crate::workload::Meter::command(true, false).unwrap();
    bulk.charge(bound).unwrap();
    assert!(crate::workload::Meter::command(false, false).unwrap().charge(bound).is_err());
    for id in ["formulas.calculateNow", "formulas.calculateSheet", "file.open", "cell.set"] {
        let spec = crate::find_command(id).unwrap();
        assert!(spec.journal || spec.undoable, "bulk metadata missing for {id}");
    }
    let query = crate::find_command("formulas.evaluate").unwrap();
    assert!(!query.journal && !query.undoable);
}

/// Real dispatch preserves the query ceiling for either trust state and when nested in bulk work.
#[test]
fn query_dispatch_caps_local_and_imported_books_and_nested_bulk_calls() {
    const BOUND: u64 = 1_250_825_000;
    /// Models the measured large workload without doing billions of actual visits.
    fn bounded(_: &mut Session, _: &Json) -> Result<Json> {
        let allowed = gridcraft_calc::WorkHook::current().unwrap().charge(BOUND);
        Ok(json!({"allowed":allowed}))
    }
    /// A bulk caller deliberately handles the query error; the enclosing latch must still fail.
    fn nested(s: &mut Session, _: &Json) -> Result<Json> {
        let _ = s.execute("test.work.readOnlyPrefix", json!({}));
        Ok(json!(true))
    }
    crate::register_test_command("test.work.bulkPrefix", bounded);
    crate::register_test_command("test.work.nestedPrefix", nested);
    let spec = Box::leak(Box::new(CommandSpec {
        id: "test.work.readOnlyPrefix",
        label: "query budget probe",
        menu: &[],
        shortcut: None,
        params: "{}",
        enabled: |_| Ok(()),
        run: bounded,
        journal: false,
        undoable: false,
    }));
    crate::cmd::test_commands().push(spec);
    for trust in [crate::Trust::Local, crate::Trust::FromFile] {
        let mut s = Session::new();
        s.new_workbook();
        s.doc_mut().unwrap().trust = trust;
        let before = s.doc().unwrap().wb.clone();
        assert_eq!(s.execute("test.work.bulkPrefix", json!({})).unwrap()["allowed"], true);
        assert!(s.execute("test.work.readOnlyPrefix", json!({})).is_err());
        assert!(s.execute("test.work.nestedPrefix", json!({})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    }
}

/// Analysis exhaustion cannot disable ordinary edits/recalc or bill imports to the old workbook.
#[test]
fn exhausted_analysis_bank_leaves_ordinary_commands_and_file_open_independent() {
    let (mut s, params) = fixture();
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let budget = s.doc().unwrap().analysis_budget.clone();
    for _ in 0..3 {
        crate::workload::Meter::new(budget.clone()).charge(crate::workload::MAX_RUN_WORK).unwrap();
    }
    assert_eq!(budget.remaining().unwrap(), 0);
    s.execute("cell.set", json!({"cell":"A1","input":"11"})).unwrap();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 1)).as_f64(), Some(22.));
    assert!(s.execute("data.monteCarlo", params).is_err());
    s.execute("file.open", json!({"name":"independent.xlsx","base64":crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 1)).as_f64(), Some(20.));
    assert_eq!(budget.remaining().unwrap(), 0);
    assert_eq!(s.doc().unwrap().analysis_budget.remaining().unwrap(), crate::workload::MAX_WINDOW_WORK);
    let mut empty = Session::new();
    empty.execute("file.open", json!({"name":"first.xlsx","base64":crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(empty.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 1)).as_f64(), Some(20.));
    assert_eq!(empty.doc().unwrap().analysis_budget.remaining().unwrap(), crate::workload::MAX_WINDOW_WORK);
}

/// Ordinary commands still have a logical cap and an installed hook even without a document.
#[test]
fn ordinary_work_cap_is_independent_and_also_covers_commands_without_a_document() {
    /// Simulates a hostile kernel; its ignored refusal must still reach the execute barrier.
    fn excessive(_: &mut Session, _: &Json) -> Result<Json> {
        let hook = gridcraft_calc::WorkHook::current().expect("ordinary scope must exist without a doc");
        assert!(!hook.charge(crate::workload::MAX_EDIT_WORK + 1));
        Ok(json!(true))
    }
    crate::register_test_command("test.work.ordinaryCap", excessive);
    let mut s = Session::new();
    assert!(s.execute("test.work.ordinaryCap", json!({})).is_err());
    let (mut s, _) = fixture();
    let original = s.doc().unwrap().wb.clone();
    assert!(s.execute("test.work.ordinaryCap", json!({})).is_err());
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().analysis_budget.remaining().unwrap(), crate::workload::MAX_WINDOW_WORK);
}

/// Discovery exposes the supported budget bound and a small seeded model still fits 1,000 units.
#[test]
fn discovery_documents_max_work_and_small_simulations_keep_their_exact_run_limit() {
    let (mut s, mut params) = fixture();
    assert!(crate::find_command("data.monteCarlo").unwrap().params.contains("maxWork?: positive integer ≤ 100000000"));
    params["iterations"] = json!(2);
    params["samples"] = json!(true);
    params["maxWork"] = json!(1000);
    s.execute("data.monteCarlo", params).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
}

/// Completes a tiny job with explicit batches, avoiding assumptions about elapsed execution time.
fn ready(s: &mut Session, id: &Json) {
    for _ in 0..20 {
        let p = s.execute("analysis.step", json!({"job":id,"trials":2})).unwrap();
        if p["state"] == "ready" {
            return;
        }
    }
    panic!("fixture should finish in bounded steps");
}

/// Real trial progress can be cancelled without source/history changes; stale ids are harmless.
#[test]
fn cancellation_after_trials_is_atomic_and_ids_do_not_reuse() {
    let (mut s, p) = fixture();
    let original = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let journal = s.journal.len();
    let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    s.execute("analysis.step", json!({"job":id,"trials":1})).unwrap();
    assert_eq!(s.execute("analysis.step", json!({"job":id,"trials":1})).unwrap()["completed"], 1);
    assert!(s.execute("cell.set", json!({"cell":"A1","input":"99"})).is_err());
    assert!(s.execute("analysis.finish", json!({"job":id})).is_err());
    assert!(s.analysis_active());
    s.execute("analysis.cancel", json!({"job":id})).unwrap();
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    assert_eq!(s.journal.len(), journal);
    let newer = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    assert_ne!(id, newer);
    assert!(s.execute("analysis.cancel", json!({"job":id})).is_err());
    assert!(s.analysis_active());
    s.execute("analysis.cancel", json!({"job":newer})).unwrap();
}

/// Publication matches synchronous statistics and creates exactly one replayable undo action.
#[test]
fn finish_matches_synchronous_results_and_undo() {
    let (mut sync, p) = fixture();
    let expected = sync.execute("data.monteCarlo", p.clone()).unwrap();
    let (mut s, _) = fixture();
    let original = s.doc().unwrap().wb.clone();
    let before = s.doc().unwrap().undo.len();
    let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    ready(&mut s, &id);
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb));
    let actual = s.execute("analysis.finish", json!({"job":id})).unwrap();
    assert_eq!(actual["outputs"], expected["outputs"]);
    assert_eq!(actual["tornado"], expected["tornado"]);
    assert_eq!(s.doc().unwrap().undo.len(), before + 1);
    assert_eq!(s.journal.last().unwrap().0, "data.monteCarlo");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
}

/// An unrelated source formula is cheap to leave clean during trials, but costs more than the
/// run limit when structural publication recalculates it. That work belongs to the command.
#[test]
fn publication_recalculation_uses_command_credit_in_both_execution_modes() {
    use std::sync::atomic::{AtomicU64, Ordering};
    for incremental in [false, true] {
        let (mut s, mut p) = fixture();
        s.execute("cell.set", json!({"cell":"C1","input":"=SUM(SEQUENCE(2000))"})).unwrap();
        p["maxWork"] = json!(1000);
        p["iterations"] = json!(2);
        let before = s.doc().unwrap().undo.len();
        let units = Arc::new(AtomicU64::new(0));
        let seen = units.clone();
        let parent = gridcraft_calc::WorkHook::new(move |n| {
            seen.fetch_add(n, Ordering::Relaxed);
            true
        });
        let _scope = parent.enter().unwrap();
        let result = if incremental {
            let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
            ready(&mut s, &id);
            s.execute("analysis.finish", json!({"job":id}))
        } else {
            s.execute("data.monteCarlo", p)
        };
        assert!(result.is_ok(), "publication should use enclosing command credit: {result:?}");
        assert!(units.load(Ordering::Relaxed) > 1000, "source recalculation must reach the enclosing policy");
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 2)).as_f64(), Some(2_001_000.));
        assert_eq!(s.doc().unwrap().undo.len(), before + 1);
        assert_eq!(s.journal.last().unwrap().0, "data.monteCarlo");
    }
}

/// Releasing analysis credit must not bypass the enclosing command's own refusal/rollback.
#[test]
fn publication_recalculation_still_obeys_parent_refusal() {
    let (mut s, mut p) = fixture();
    s.execute("cell.set", json!({"cell":"C1","input":"=SUM(SEQUENCE(2000))"})).unwrap();
    p["maxWork"] = json!(1000);
    p["iterations"] = json!(2);
    let source = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let journal = s.journal.len();
    let parent = crate::workload::Meter::ordinary();
    parent.set_limit(1000).unwrap();
    {
        let _scope = parent.hook().enter().unwrap();
        assert!(s.execute("data.monteCarlo", p).is_err());
    }
    assert!(parent.check().is_err());
    assert!(Arc::ptr_eq(&source, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    assert_eq!(s.journal.len(), journal);
}

/// External cancellation and edits through an embedding API cannot be silently overwritten.
#[test]
fn cancellation_handles_and_source_conflicts_are_checked() {
    let (mut s, p) = fixture();
    let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    assert!(s.analysis_cancellation().unwrap().cancel());
    assert!(s.execute("analysis.step", json!({"job":id})).is_err());
    assert!(!s.analysis_active());
    let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    let mut changed = (*s.doc().unwrap().wb).clone();
    changed.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::Number(99.));
    s.doc_mut().unwrap().wb = Arc::new(changed);
    assert!(s.execute("analysis.step", json!({"job":id})).is_err());
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), gridcraft_core::Value::Number(99.));
}

/// Planning rejects excess work; repeated small reports cannot evade the workbook growth cap.
#[test]
fn work_and_cumulative_report_limits_are_atomic() {
    let (mut s, mut p) = fixture();
    p["maxWork"] = json!(1);
    assert!(s.execute("data.monteCarlo", p.clone()).is_err());
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    p.as_object_mut().unwrap().remove("maxWork");
    p["iterations"] = json!(2);
    for _ in 1..crate::workload::MAX_REPORT_SHEETS {
        s.execute("data.monteCarlo", p.clone()).unwrap();
    }
    let before = s.doc().unwrap().wb.clone();
    assert!(s.execute("data.monteCarlo", p).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    assert!(s.doc().unwrap().undo.len() <= 100);
}

/// Volatile trials and unavailable external references never leak trial state into the source.
#[test]
fn volatile_and_external_formula_trials_do_not_leak() {
    let (mut s, p) = fixture();
    s.execute("cell.set", json!({"cell":"B1","input":"=A1+RAND()"})).unwrap();
    let original = s.doc().unwrap().wb.clone();
    let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    s.execute("analysis.step", json!({"job":id})).unwrap();
    s.execute("analysis.step", json!({"job":id,"trials":1})).unwrap();
    s.execute("analysis.cancel", json!({"job":id})).unwrap();
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb));
    s.execute("cell.set", json!({"cell":"B1","input":"='[Unavailable.xlsx]Sheet1'!A1"})).unwrap();
    let original = s.doc().unwrap().wb.clone();
    let id = s.execute("analysis.start", json!({"params":p})).unwrap()["job"].clone();
    s.execute("analysis.step", json!({"job":id})).unwrap();
    assert!(s.execute("analysis.step", json!({"job":id})).is_err());
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb));
}

/// A latched parent is rejected before handlers can retain new runtime state.
#[test]
fn latched_parent_cannot_start_or_retain_a_job() {
    let (mut s, params) = fixture();
    let hook = gridcraft_calc::WorkHook::new(|_| false);
    let _scope = hook.enter().unwrap();
    assert!(!gridcraft_calc::WorkHook::allowed());
    assert!(s.execute("analysis.start", json!({"params":params})).is_err());
    assert!(!s.analysis_active());
}

/// A policy may refuse only after preparation; failed starts must still discard their new job.
#[test]
fn post_handler_refusal_does_not_retain_a_new_job() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let (mut s, params) = fixture();
    let calls = Arc::new(AtomicU64::new(0));
    let seen = calls.clone();
    let hook = gridcraft_calc::WorkHook::new(move |_| seen.fetch_add(1, Ordering::Relaxed) == 0);
    let _scope = hook.enter().unwrap();
    assert!(s.execute("analysis.start", json!({"params":params})).is_err());
    assert!(!s.analysis_active());
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

/// Direct recalc handlers bypass Ctx::commit, so terminal work errors need the global barrier.
#[test]
fn direct_recalculation_failure_restores_source_history_revision_and_journal() {
    let (mut s, _) = fixture();
    s.execute("cell.set", json!({"cell":"D1","input":"99"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    let original = s.doc().unwrap().wb.clone();
    let selection = s.doc().unwrap().selection.clone();
    let revision = s.doc().unwrap().revision;
    let undo = s.doc().unwrap().undo.iter().map(|entry| (entry.label.clone(), entry.wb.clone())).collect::<Vec<_>>();
    let redo = s.doc().unwrap().redo.iter().map(|entry| (entry.label.clone(), entry.wb.clone())).collect::<Vec<_>>();
    let journal = s.journal.clone();
    let meter = crate::workload::Meter::new(s.doc().unwrap().analysis_budget.clone());
    meter.set_limit(1).unwrap();
    let _scope = meter.hook().enter().unwrap();
    assert!(s.execute("formulas.calculateNow", json!({})).is_err());
    let d = s.doc().unwrap();
    assert!(Arc::ptr_eq(&original, &d.wb));
    assert_eq!(d.selection, selection);
    assert_eq!(d.revision, revision);
    assert_eq!(d.undo.len(), undo.len());
    assert_eq!(d.redo.len(), redo.len());
    for (actual, (label, wb)) in d.undo.iter().zip(&undo).chain(d.redo.iter().zip(&redo)) {
        assert_eq!(&actual.label, label);
        assert!(Arc::ptr_eq(&actual.wb, wb));
    }
    assert_eq!(s.journal, journal);
}

/// Nested calls inherit one run meter and cannot hide refusal or lose a trimmed journal prefix.
#[test]
fn nested_handlers_share_work_and_restore_even_a_trimmed_journal() {
    /// Models a native kernel whose caller ignores its refusal after a nested edit.
    fn child(s: &mut Session, _: &Json) -> Result<Json> {
        s.execute("cell.set", json!({"cell":"C1","input":"99"}))?;
        let _ = gridcraft_calc::WorkHook::current().unwrap().charge(60);
        Ok(json!(true))
    }
    /// Intentionally absorbs a child error to test the persistent logical-operation latch.
    fn parent(s: &mut Session, _: &Json) -> Result<Json> {
        let _ = s.execute("test.work.child", json!({}));
        let _ = s.execute("test.work.child", json!({}));
        Ok(json!(true))
    }
    crate::register_test_command("test.work.child", child);
    crate::register_test_command("test.work.parent", parent);
    let (mut s, _) = fixture();
    s.journal = vec![("old".into(), json!({"kept":true})); 10_000];
    let original = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let revision = s.doc().unwrap().revision;
    let journal = s.journal.clone();
    let meter = crate::workload::Meter::new(s.doc().unwrap().analysis_budget.clone());
    meter.set_limit(100).unwrap();
    let _scope = meter.hook().enter().unwrap();
    assert!(s.execute("test.work.parent", json!({})).is_err());
    assert_eq!(meter.spent().unwrap(), 60);
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    assert_eq!(s.doc().unwrap().revision, revision);
    assert_eq!(s.journal, journal);
}
/// Ordinary dependency recalculation never consumes the analysis bank, even across a large window.
#[test]
fn running_total_edits_do_not_spend_analysis_allowance() {
    let mut s = Session::new();
    s.new_workbook();
    let rows = (1..=10_000).map(|n| json!([1, format!("=SUM($A$1:A{n})")])).collect::<Vec<_>>();
    s.execute("range.setValues", json!({"range":"A1:B10000","values":rows})).unwrap();
    let before = s.doc().unwrap().analysis_budget.remaining().unwrap();
    for n in 2..=8 {
        s.execute("cell.set", json!({"cell":"A1","input":n.to_string()})).unwrap();
        assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B10000").unwrap()).as_f64(), Some(9999. + f64::from(n)));
    }
    assert_eq!(s.doc().unwrap().analysis_budget.remaining().unwrap(), before);
}

/// Analysis identities belong to the session, not to workbook snapshots restored by macro previews.
#[test]
fn macros_deny_the_analysis_lifecycle_for_local_and_imported_workbooks() {
    for trust in [crate::Trust::Local, crate::Trust::FromFile] {
        for id in ["analysis.start", "analysis.status", "analysis.step", "analysis.cancel", "analysis.finish", "analysis.futureOperation"] {
            assert!(
                matches!(crate::macros::policy::macro_policy(id, &json!({"job":1}), trust), crate::macros::policy::Policy::Deny(_)),
                "{trust:?}: {id}"
            );
        }
    }
}

/// A dry run must refuse lifecycle commands before they can cancel or advance the live job.
#[test]
fn checking_a_macro_preserves_the_active_analysis_identity_progress_and_source() {
    for trust in [crate::Trust::Local, crate::Trust::FromFile] {
        for id in ["analysis.cancel", "analysis.step"] {
            let (mut s, params) = fixture();
            let next_job = s.next_analysis_job + 1;
            s.execute("macro.save", json!({"name":"AnalysisProbe","steps":[{"type":"command","id":id,"params":{"job":next_job,"trials":1}}]}))
                .unwrap();
            s.doc_mut().unwrap().trust = trust;
            let started = s.execute("analysis.start", json!({"params":params})).unwrap();
            assert_eq!(started["job"], next_job);
            let source = s.doc().unwrap().wb.clone();
            let revision = s.doc().unwrap().revision;
            let undo = s.doc().unwrap().undo.len();
            let journal = s.journal.clone();
            let checked = s.execute("macro.check", json!({"name":"AnalysisProbe"})).unwrap();
            assert_eq!(checked["ok"], false, "{trust:?}: {id}: {checked}");
            assert!(checked["errors"].as_array().is_some_and(|errors| {
                errors.iter().any(|error| error.as_str().is_some_and(|message| message.contains(id) && message.contains("can't run in a macro")))
            }));
            assert_eq!(s.analysis_progress(), Some(started.clone()));
            assert_eq!(s.execute("analysis.status", json!({"job":next_job})).unwrap(), started);
            assert!(Arc::ptr_eq(&source, &s.doc().unwrap().wb));
            assert_eq!(s.doc().unwrap().revision, revision);
            assert_eq!(s.doc().unwrap().undo.len(), undo);
            assert_eq!(s.journal, journal);
            s.execute("analysis.cancel", json!({"job":next_job})).unwrap();
        }
    }
}

/// Upstream macro steps must inherit the terminal work hook and retain normal rollback/history.
#[test]
fn macro_run_inherits_work_refusal_and_preserves_source_history_and_journal() {
    let (mut s, _) = fixture();
    s.execute("macro.save", json!({"macro":{"name":"PaidSteps","steps":[{"type":"command","id":"cell.set","params":{"cell":"A1","input":"99"}}]}}))
        .unwrap();
    s.execute("cell.set", json!({"cell":"D1","input":"temporary"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    let d = s.doc().unwrap();
    let (source, selection, revision, undo, redo) = (d.wb.clone(), d.selection.clone(), d.revision, d.undo.clone(), d.redo.clone());
    let journal = s.journal.clone();
    let hook = gridcraft_calc::WorkHook::new(|units| units == 0);
    {
        let _scope = hook.enter().unwrap();
        assert!(s.execute("macro.run", json!({"name":"PaidSteps","onConflict":"overwrite"})).is_err());
        assert!(!hook.charge(0));
    }
    let d = s.doc().unwrap();
    assert!(Arc::ptr_eq(&source, &d.wb));
    assert_eq!(d.selection, selection);
    assert_eq!(d.revision, revision);
    assert_eq!(s.journal, journal);
    for (expected, actual) in [(&undo, &d.undo), (&redo, &d.redo)] {
        assert_eq!(expected.len(), actual.len());
        for (before, after) in expected.iter().zip(actual) {
            assert!(Arc::ptr_eq(&before.wb, &after.wb));
            assert_eq!(before.label, after.label);
            assert_eq!(before.selection, after.selection);
        }
    }
    s.execute("macro.run", json!({"name":"PaidSteps","onConflict":"overwrite"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), gridcraft_core::Value::Number(99.0));
}
