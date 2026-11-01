use gridcraft_core::{CellRef, Value};
use serde_json::json;

use crate::Session;

fn s() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}
fn n(s: &Session, a: &str) -> f64 {
    let d = s.doc().unwrap();
    d.wb.active().unwrap().value(CellRef::parse(a).unwrap()).as_f64().unwrap_or(f64::NAN)
}
/// The first saved-model name scoped to the active sheet.
fn saved(s: &Session) -> String {
    let d = s.doc().unwrap();
    d.wb.names.iter().find(|n| n.scope == Some(d.wb.active_sheet())).map(|n| n.name.clone()).unwrap_or_default()
}
fn set(s: &mut Session, cells: &[(&str, &str)]) {
    for (c, v) in cells {
        s.execute("cell.set", json!({"cell": c, "input": v})).unwrap();
    }
}

#[test]
fn simplex_solves_the_wyndor_glass_lp() {
    // Hillier & Lieberman's Wyndor Glass problem: max 3x + 5y, x ≤ 4, 2y ≤ 12, 3x + 2y ≤ 18
    // → x = 2, y = 6, Z = 36.
    let mut s = s();
    set(&mut s, &[("A1", "0"), ("A2", "0"), ("B1", "=3*A1+5*A2"), ("B2", "=2*A2"), ("B3", "=3*A1+2*A2")]);
    let r = s
        .execute(
            "data.solver",
            json!({"objective": "B1", "goal": "max", "variables": "A1:A2", "constraints": ["A1 <= 4", "B2 <= 12", {"lhs": "B3", "op": "<=", "rhs": 18}], "method": "simplex"}),
        )
        .unwrap();
    assert_eq!(r["status"], "optimal", "{r}");
    assert!((n(&s, "A1") - 2.0).abs() < 1e-9 && (n(&s, "A2") - 6.0).abs() < 1e-9);
    assert!((n(&s, "B1") - 36.0).abs() < 1e-9);
    // The model is saved as hidden names, survives XLSX and solves again from them.
    let names: Vec<String> = s.doc().unwrap().wb.names.iter().map(|n| n.name.clone()).collect();
    assert!(names.iter().any(|n| n == "solver_opt") && names.iter().any(|n| n == "solver_lhs3"));
    let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "m.xlsx", "base64": b64})).unwrap();
    set(&mut s, &[("A1", "0"), ("A2", "0")]);
    let m = s.execute("data.solver", json!({})).unwrap();
    assert_eq!(m["model"]["objective"], "B1");
    assert_eq!(m["model"]["method"], "simplex");
    let r = s.execute("data.solver", json!({"saved": true})).unwrap();
    assert_eq!(r["status"], "optimal");
    assert!((n(&s, "B1") - 36.0).abs() < 1e-9);
    // Minimise with keep: false leaves the cells alone.
    set(&mut s, &[("A1", "1"), ("A2", "1")]);
    let r = s.execute("data.solver", json!({"saved": true, "goal": "min", "keep": false})).unwrap();
    assert_eq!(r["objective"], 0.0);
    assert_eq!((n(&s, "A1"), n(&s, "A2")), (1.0, 1.0));
}

#[test]
fn integer_and_binary_constraints_use_branch_and_bound() {
    // max 5x + 4y, 6x + 4y ≤ 24, x + 2y ≤ 6: the LP optimum (3, 1.5) = 21; integers give
    // (4, 0) = 20.
    let mut s = s();
    set(&mut s, &[("B1", "=5*A1+4*A2"), ("B2", "=6*A1+4*A2"), ("B3", "=A1+2*A2")]);
    let base = json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 24", "B3 <= 6"], "method": "simplex"});
    let r = s.execute("data.solver", base.clone()).unwrap();
    assert!((r["objective"].as_f64().unwrap() - 21.0).abs() < 1e-9);
    let mut p = base.clone();
    p["constraints"].as_array_mut().unwrap().push(json!("A1:A2 int"));
    let r = s.execute("data.solver", p).unwrap();
    assert_eq!(r["status"], "optimal");
    assert_eq!((n(&s, "A1"), n(&s, "A2"), n(&s, "B1")), (4.0, 0.0, 20.0));
    // 0/1 knapsack: values 10, 13, 7, weights 5, 6, 4, capacity 10 → items 2 and 3 (20).
    set(&mut s, &[("D1", "=10*C1+13*C2+7*C3"), ("D2", "=5*C1+6*C2+4*C3")]);
    let r = s
        .execute("data.solver", json!({"objective": "D1", "variables": "C1:C3", "constraints": ["D2 <= 10", "C1:C3 bin"], "method": "simplex"}))
        .unwrap();
    assert_eq!(r["status"], "optimal");
    assert_eq!((n(&s, "C1"), n(&s, "C2"), n(&s, "C3"), n(&s, "D1")), (0.0, 1.0, 1.0, 20.0));
}

#[test]
fn grg_finds_smooth_optima() {
    // Largest rectangle with perimeter at most 20: a 5 × 5 square, area 25.
    let mut s = s();
    set(&mut s, &[("A1", "1"), ("A2", "1"), ("B1", "=A1*A2"), ("B2", "=2*A1+2*A2")]);
    let r = s.execute("data.solver", json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 20"]})).unwrap();
    assert_eq!(r["status"], "converged", "{r}");
    assert!((n(&s, "A1") - 5.0).abs() < 1e-3 && (n(&s, "A2") - 5.0).abs() < 1e-3, "{r}");
    // Unconstrained minimum of (x − 3)² + (y + 1)² at (3, −1) once negatives are allowed.
    set(&mut s, &[("C1", "0"), ("C2", "0"), ("D1", "=(C1-3)^2+(C2+1)^2")]);
    s.execute("data.solver", json!({"objective": "D1", "goal": "min", "variables": "C1:C2", "nonNegative": false})).unwrap();
    assert!((n(&s, "C1") - 3.0).abs() < 1e-4 && (n(&s, "C2") + 1.0).abs() < 1e-4);
    // Value Of: x³ = 8 → x = 2.
    set(&mut s, &[("E1", "1"), ("F1", "=E1^3")]);
    s.execute("data.solver", json!({"objective": "F1", "goal": "value", "value": 8, "variables": "E1"})).unwrap();
    assert!((n(&s, "E1") - 2.0).abs() < 1e-4);
}

#[test]
fn evolutionary_finds_the_global_minimum_of_non_smooth_and_multimodal_models() {
    let mut s = s();
    // |x − 3| + |y + 2| has a kink at its minimum (3, −2).
    set(&mut s, &[("A1", "0"), ("A2", "0"), ("B1", "=ABS(A1-3)+ABS(A2+2)")]);
    let p = json!({"objective": "B1", "goal": "min", "variables": "A1:A2", "constraints": ["A1:A2 >= -10", "A1:A2 <= 10"], "method": "evolutionary"});
    let r = s.execute("data.solver", p.clone()).unwrap();
    assert!(r["status"] == "converged" || r["status"] == "cannotImprove", "{r}");
    assert!((n(&s, "A1") - 3.0).abs() < 1e-6 && (n(&s, "A2") + 2.0).abs() < 1e-6, "{r}");
    // The same seed repeats the run exactly.
    set(&mut s, &[("A1", "0"), ("A2", "0")]);
    let again = s.execute("data.solver", p).unwrap();
    assert_eq!(again["values"], r["values"]);
    assert_eq!(again["evaluations"], r["evaluations"]);
    // Rastrigin's function 20 + Σ (x² − 10 cos 2πx) on [−5.12, 5.12]² has its global minimum 0
    // at the origin among many local minima; GRG from (3.3, −2.7) stops at a local one.
    set(&mut s, &[("C1", "3.3"), ("C2", "-2.7"), ("D1", "=20+C1^2-10*COS(2*PI()*C1)+C2^2-10*COS(2*PI()*C2)")]);
    let base = json!({"objective": "D1", "goal": "min", "variables": "C1:C2", "constraints": ["C1:C2 >= -5.12", "C1:C2 <= 5.12"]});
    s.execute("data.solver", base.clone()).unwrap();
    assert!(n(&s, "D1") > 0.5, "GRG is local: {}", n(&s, "D1"));
    set(&mut s, &[("C1", "3.3"), ("C2", "-2.7")]);
    let mut p = base.clone();
    p["method"] = json!("evolutionary");
    let r = s.execute("data.solver", p).unwrap();
    assert!(n(&s, "D1") < 1e-8 && n(&s, "C1").abs() < 1e-5 && n(&s, "C2").abs() < 1e-5, "{r}");
    // A stepped model: ROUND makes it flat almost everywhere; the minimum is at 7.2.
    set(&mut s, &[("E1", "0"), ("F1", "=ABS(ROUND(E1,0)-7)+ABS(E1-7.2)/100")]);
    s.execute("data.solver", json!({"objective": "F1", "goal": "min", "variables": "E1", "constraints": ["E1 <= 20"], "method": "evolutionary"}))
        .unwrap();
    assert!((n(&s, "E1") - 7.2).abs() < 1e-6, "{}", n(&s, "E1"));
}

#[test]
fn evolutionary_handles_integers_bounds_and_the_saved_model() {
    let mut s = s();
    // The 0/1 knapsack of the branch-and-bound test: items 2 and 3, value 20.
    set(&mut s, &[("D1", "=10*C1+13*C2+7*C3"), ("D2", "=5*C1+6*C2+4*C3")]);
    let p = json!({"objective": "D1", "variables": "C1:C3", "constraints": ["D2 <= 10", "C1:C3 bin"], "method": "evolutionary", "populationSize": 30, "randomSeed": 7});
    let r = s.execute("data.solver", p).unwrap();
    assert!(r["status"] == "converged" || r["status"] == "cannotImprove", "{r}");
    assert_eq!((n(&s, "C1"), n(&s, "C2"), n(&s, "C3"), n(&s, "D1")), (0.0, 1.0, 1.0, 20.0));
    // The method and its options are saved, survive XLSX and solve again.
    let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "e.xlsx", "base64": b64})).unwrap();
    let m = s.execute("data.solver", json!({})).unwrap();
    assert_eq!(m["model"]["method"], "evolutionary");
    assert_eq!((m["model"]["populationSize"].as_f64(), m["model"]["randomSeed"].as_f64()), (Some(30.0), Some(7.0)));
    set(&mut s, &[("C1", "0"), ("C2", "0"), ("C3", "0")]);
    s.execute("data.solver", json!({"saved": true})).unwrap();
    assert_eq!(n(&s, "D1"), 20.0);
    // Integer constraints: max x + y with x + 2y ≤ 7 on integers in [0, 10] → (7, 0).
    set(&mut s, &[("A1", "0"), ("A2", "0"), ("B1", "=A1+A2"), ("B2", "=A1+2*A2")]);
    let r = s
        .execute(
            "data.solver",
            json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 7", "A1:A2 <= 10", "A1:A2 int"], "method": "evolutionary"}),
        )
        .unwrap();
    assert_eq!((n(&s, "A1"), n(&s, "A2")), (7.0, 0.0), "{r}");
    // Every variable needs bounds unless requireBounds is off; then it searches around the start.
    set(&mut s, &[("E1", "1"), ("F1", "=ABS(E1-4)")]);
    let p = json!({"objective": "F1", "goal": "min", "variables": "E1", "method": "evolutionary"});
    assert!(s.execute("data.solver", p.clone()).is_err());
    let mut p = p;
    p["requireBounds"] = json!(false);
    s.execute("data.solver", p).unwrap();
    assert!((n(&s, "E1") - 4.0).abs() < 1e-6);
    // Infeasible constraints and a tiny budget are reported without changing the cells.
    set(&mut s, &[("E1", "1")]);
    let r = s
        .execute("data.solver", json!({"objective": "F1", "variables": "E1", "constraints": ["E1 <= 3", "F1 >= 10"], "method": "evolutionary"}))
        .unwrap();
    assert_eq!(r["status"], "infeasible", "{r}");
    let r = s
        .execute(
            "data.solver",
            json!({"objective": "F1", "variables": "E1", "constraints": ["E1 <= 3"], "method": "evolutionary", "maxEvaluations": 3}),
        )
        .unwrap();
    assert_eq!(r["status"], "iterationLimit", "{r}");
    assert_eq!(n(&s, "E1"), 1.0);
}

/// Text or number at `a1` on sheet `name`.
fn at(s: &Session, name: &str, a1: &str) -> Value {
    let d = s.doc().unwrap();
    d.wb.sheet(d.wb.sheet_index(name).unwrap()).unwrap().value(CellRef::parse(a1).unwrap())
}

/// The row of the first cell holding `text` on sheet `name`.
fn row_of(s: &Session, name: &str, text: &str) -> u32 {
    let d = s.doc().unwrap();
    let sh = d.wb.sheet(d.wb.sheet_index(name).unwrap()).unwrap();
    (0..60).find(|r| (0..8).any(|c| sh.value(CellRef::new(*r, c)) == Value::text(text))).unwrap()
}

#[test]
fn answer_and_limits_reports_describe_the_wyndor_glass_solution() {
    // Wyndor Glass with labels: doors and windows across, the three plants' hours down.
    let mut s = s();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [[null, "Doors", "Windows", "Used", "Available"], ["Batches", 0, 0, null, null], ["Plant 1", 1, 0, null, 4], ["Plant 2", 0, 2, null, 12], ["Plant 3", 3, 2, null, 18], ["Profit", 3, 5, null, null]]}),
    )
    .unwrap();
    for r in 3..=6 {
        set(&mut s, &[(&format!("D{r}"), &format!("=SUMPRODUCT(B{r}:C{r},$B$2:$C$2)"))]);
    }
    let solved = s
        .execute(
            "data.solver",
            json!({"objective": "D6", "variables": "B2:C2", "constraints": ["D3:D5 <= E3:E5"], "method": "simplex", "reports": ["answer", "limits"]}),
        )
        .unwrap();
    assert_eq!(solved["status"], "optimal", "{solved}");
    assert_eq!(solved["reports"], json!(["Answer Report 1", "Limits Report 1"]));
    assert_eq!(solved["original"], json!({"B2": 0.0, "C2": 0.0}));
    let d = s.doc().unwrap();
    let names: Vec<&str> = d.wb.sheets.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Answer Report 1", "Limits Report 1", "Sheet1"]);
    assert_eq!(d.wb.active().unwrap().name, "Sheet1", "the model's sheet stays active");
    assert_eq!(saved(&s), "solver_opt", "the saved model follows its sheet");
    let a = "Answer Report 1";
    assert_eq!(at(&s, a, "A1"), Value::text("GridCraft Answer Report"));
    // Objective: $D$6 "Profit Used", from 0 to 36.
    let r = row_of(&s, a, "$D$6");
    assert_eq!(at(&s, a, &format!("C{}", r + 1)), Value::text("Profit Used"));
    assert_eq!((at(&s, a, &format!("D{}", r + 1)), at(&s, a, &format!("E{}", r + 1))), (Value::number(0.0), Value::number(36.0)));
    // Variables: doors 0 → 2, windows 0 → 6, continuous.
    let r = row_of(&s, a, "$B$2") + 1;
    assert_eq!(at(&s, a, &format!("C{r}")), Value::text("Batches Doors"));
    assert_eq!(at(&s, a, &format!("E{r}")), Value::number(2.0));
    assert_eq!(at(&s, a, &format!("F{r}")), Value::text("Contin"));
    assert_eq!(at(&s, a, &format!("E{}", r + 1)), Value::number(6.0));
    // Constraints: plant 1 has 2 hours to spare; plants 2 and 3 are binding.
    let r = row_of(&s, a, "$D$3") + 1;
    assert_eq!(at(&s, a, &format!("E{r}")), Value::text("$D$3<=$E$3"));
    assert_eq!((at(&s, a, &format!("F{r}")), at(&s, a, &format!("G{r}"))), (Value::text("Not Binding"), Value::number(2.0)));
    for k in 1..=2 {
        assert_eq!((at(&s, a, &format!("F{}", r + k)), at(&s, a, &format!("G{}", r + k))), (Value::text("Binding"), Value::number(0.0)));
    }
    // Limits with the other variable held: doors 0..2 (profit 30..36), windows 0..6 (6..36).
    let l = "Limits Report 1";
    let r = row_of(&s, l, "$B$2") + 1;
    let row = |r: u32| ["D", "E", "F", "G", "H"].map(|c| at(&s, l, &format!("{c}{r}")).as_f64().unwrap());
    assert_eq!(row(r), [2.0, 0.0, 30.0, 2.0, 36.0]);
    assert_eq!(row(r + 1), [6.0, 0.0, 6.0, 6.0, 36.0]);
    // data.solverReport writes reports from the saved model and a given solution.
    let out = s.execute("data.solverReport", json!({"reports": ["answer"], "values": {"B2": 2, "C2": 6}, "original": {"B2": 1, "C2": 1}})).unwrap();
    assert_eq!(out["sheets"], json!(["Answer Report 2"]));
    let r = row_of(&s, "Answer Report 2", "$D$6") + 1;
    assert_eq!(at(&s, "Answer Report 2", &format!("D{r}")), Value::number(8.0));
    // Undo removes the reports and keeps the solution; refused requests write nothing.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 3);
    for p in [
        json!({}),
        json!({"reports": []}),
        json!({"reports": "answer"}),
        json!({"reports": ["population"]}),
        json!({"reports": ["answer"], "values": {"Z9": 1}}),
        json!({"reports": ["answer"], "values": {"B2": "x"}}),
        json!({"reports": ["limits"], "constraints": ["D3:D5 <= E3:E5", "B2:C2 int"]}),
        json!({"reports": ["limits"], "method": "evolutionary", "constraints": ["D3:D5 <= E3:E5", "B2:C2 <= 10"]}),
    ] {
        assert!(s.execute("data.solverReport", p.clone()).is_err(), "{p}");
    }
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 3);
    // With the solve, its reports and the saved model undone (one step), the result still
    // describes the solution.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!((s.doc().unwrap().wb.sheets.len(), n(&s, "B2")), (1, 0.0));
    let mut p = solved.clone();
    p["reports"] = json!(["answer"]);
    assert_eq!(s.execute("data.solverReport", p).unwrap()["sheets"], json!(["Answer Report 1"]));
    let r = row_of(&s, "Answer Report 1", "$B$2") + 1;
    assert_eq!(at(&s, "Answer Report 1", &format!("E{r}")), Value::number(2.0));
}

#[test]
fn refused_solver_reports_preserve_the_document_and_history() {
    for (protected, method, constraints, reports, error) in [
        (true, "simplex", json!(["A1 <= 4"]), json!(["answer"]), "workbook is protected"),
        (false, "simplex", json!(["A1 <= 4", "A1 int"]), json!(["answer", "limits"]), "integer or binary"),
        (false, "evolutionary", json!(["A1 <= 4"]), json!(["answer", "limits"]), "Simplex LP and GRG"),
    ] {
        for (keep, save) in [(true, true), (false, true), (true, false), (false, false)] {
            let mut s = s();
            set(&mut s, &[("A1", "1"), ("B1", "=2*A1")]);
            s.execute("data.solver", json!({"objective": "B1", "variables": "A1", "constraints": ["A1 <= 2"], "method": "simplex", "keep": false}))
                .unwrap();
            set(&mut s, &[("C1", "redo")]);
            s.execute("edit.undo", json!({})).unwrap();
            let d = s.doc_mut().unwrap();
            std::sync::Arc::make_mut(&mut d.wb).protected_structure = protected;
            d.saved = d.wb.clone();
            let before = d.clone();
            let journal = s.journal.clone();
            let err = s
                .execute(
                    "data.solver",
                    json!({"objective": "B1", "variables": "A1", "constraints": constraints, "method": method,
                        "reports": reports, "keep": keep, "saveModel": save}),
                )
                .unwrap_err();
            assert!(err.to_string().contains(error), "{err}");
            let d = s.doc().unwrap();
            assert!(std::sync::Arc::ptr_eq(&d.wb, &before.wb), "{method}: keep={keep}, save={save}: {err}");
            assert!(!d.is_dirty());
            assert_eq!(d.revision, before.revision);
            assert_eq!(d.selection, before.selection);
            assert_eq!(d.sheet_selections, before.sheet_selections);
            for (after, before) in [(&d.undo, &before.undo), (&d.redo, &before.redo)] {
                assert_eq!(after.len(), before.len());
                for (a, b) in after.iter().zip(before) {
                    assert!(std::sync::Arc::ptr_eq(&a.wb, &b.wb));
                    assert_eq!((&a.label, &a.selection), (&b.label, &b.selection));
                }
            }
            assert_eq!(s.journal, journal);
        }
    }
}

#[test]
fn solver_reports_and_optional_solution_and_model_share_one_undo() {
    for (keep, save) in [(true, true), (false, true), (true, false), (false, false)] {
        let mut s = s();
        set(&mut s, &[("A1", "1"), ("B1", "=2*A1")]);
        let before = s.doc().unwrap().clone();
        let out = s
            .execute(
                "data.solver",
                json!({"objective": "B1", "variables": "A1", "constraints": ["A1 <= 4"], "method": "simplex",
                    "reports": ["answer"], "keep": keep, "saveModel": save}),
            )
            .unwrap();
        assert_eq!(out["status"], "optimal");
        assert_eq!(out["reports"], json!(["Answer Report 1"]));
        assert_eq!(out["model"]["objective"], "Sheet1!B1");
        assert_eq!(n(&s, "A1"), if keep { 4.0 } else { 1.0 });
        assert_eq!(saved(&s), if save { "solver_opt" } else { "" });
        assert_eq!(s.doc().unwrap().undo.len(), before.undo.len() + 1);
        assert_eq!(s.doc().unwrap().wb.active().unwrap().name, "Sheet1");
        let r = row_of(&s, "Answer Report 1", "$A$1") + 1;
        assert_eq!(at(&s, "Answer Report 1", &format!("E{r}")), Value::number(4.0));
        s.execute("edit.undo", json!({})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().wb, &before.wb));
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
        assert_eq!(n(&s, "A1"), if keep { 4.0 } else { 1.0 });
    }
}

#[test]
fn solver_reports_keep_row_autofit_on_the_model_sheet() {
    let mut s = s();
    set(&mut s, &[("A1", "1"), ("B1", "=2*A1")]);
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 30})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let before = s.doc().unwrap().wb.active().unwrap().row_height(0);
    s.execute(
        "data.solver",
        json!({"objective": "B1", "variables": "A1", "constraints": ["A1 <= 1000000"], "method": "simplex", "reports": ["answer"]}),
    )
    .unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.name, "Sheet1");
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::number(1_000_000.0));
    assert!(sh.row_height(0) > before, "the solved wrapped number must re-fit its row after the report shifts its sheet");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().row_height(0), before);
}

#[test]
fn solver_report_insertion_keeps_variable_edit_guards() {
    for array in [false, true] {
        let mut s = s();
        set(&mut s, &[("A2", "1"), ("B1", "=2*A2")]);
        let error = if array {
            s.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
            s.execute("cell.set", json!({"input": "=1", "array": true})).unwrap();
            "part of an array"
        } else {
            s.execute("review.protectSheet", json!({})).unwrap();
            "protected sheet"
        };
        let before = s.doc().unwrap().clone();
        let err = s
            .execute(
                "data.solver",
                json!({"objective": "B1", "variables": "A2", "constraints": ["A2 <= 4"], "method": "simplex", "reports": ["answer"]}),
            )
            .unwrap_err();
        assert!(err.to_string().contains(error), "{err}");
        let d = s.doc().unwrap();
        assert!(std::sync::Arc::ptr_eq(&d.wb, &before.wb));
        assert_eq!(d.undo.len(), before.undo.len());
        assert_eq!(d.sheet_selections, before.sheet_selections);
    }
}

fn formula(s: &Session, a1: &str) -> String {
    let d = s.doc().unwrap();
    d.wb.active().unwrap().cell(CellRef::parse(a1).unwrap()).and_then(|c| c.formula.as_ref()).map(|f| f.text.clone()).unwrap_or_default()
}

#[test]
fn save_and_load_model_round_trip_through_cells() {
    let mut s = s();
    set(&mut s, &[("B1", "=5*A1+4*A2"), ("B2", "=6*A1+4*A2"), ("B3", "=A1+2*A2"), ("C2", "24")]);
    let model = json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= C2", "B3 <= 6", "A1:A2 int", "A2 bin", "A1 >= 1"], "method": "simplex"});
    s.execute("data.solver", model.clone()).unwrap();
    let out = s.execute("data.solverSaveModel", json!({"to": "H1"})).unwrap();
    assert_eq!(out["range"], "H1:H8");
    let rows: Vec<String> = (1..=8).map(|r| formula(&s, &format!("H{r}"))).collect();
    assert_eq!(
        rows,
        [
            "MAX($B$1)",
            "COUNT($A$1:$A$2)",
            "AND($B$2<=$C$2)",
            "AND($B$3<=6)",
            "AND($A$1:$A$2=INT($A$1:$A$2))",
            "AND(($A$2=0)+($A$2=1))",
            "AND($A$1>=1)",
            "{2,1,100,0.075,0,1,0.0001,50}",
        ]
    );
    // The constraint cells show whether each constraint holds at the solution (4, 0).
    assert_eq!(at(&s, "Sheet1", "H3"), Value::Bool(true));
    assert_eq!(at(&s, "Sheet1", "H2"), Value::number(2.0));
    // Reset All removes the model; Load Model restores it from the range or its first cell.
    assert_eq!(s.execute("data.solverReset", json!({})).unwrap()["removed"].as_u64().map(|n| n > 0), Some(true));
    assert!(s.execute("data.solver", json!({})).unwrap()["model"].is_null());
    assert_eq!(s.execute("data.solverReset", json!({})).unwrap()["removed"], 0);
    for from in ["H1:H8", "H1"] {
        let loaded = s.execute("data.solverLoadModel", json!({"from": from})).unwrap();
        assert_eq!(loaded["model"]["objective"], "B1", "{loaded}");
        assert_eq!(loaded["model"]["constraints"], json!(["B2 <= C2", "B3 <= 6", "A1:A2 int", "A2 bin", "A1 >= 1"]));
        assert_eq!(loaded["model"]["method"], "simplex");
    }
    set(&mut s, &[("A1", "0"), ("A2", "0")]);
    let r = s.execute("data.solver", json!({"saved": true})).unwrap();
    assert_eq!((r["status"].as_str(), n(&s, "A1"), n(&s, "A2")), (Some("optimal"), 4.0, 0.0), "{r}");
    // A Value Of goal and the Evolutionary options survive too; an explicit model is saved
    // as given.
    let evo = json!({"objective": "B1", "goal": "value", "value": -2.5, "variables": "A1:A2", "constraints": ["A1:A2 <= 10"], "nonNegative": false, "method": "evolutionary", "populationSize": 40, "randomSeed": 3, "requireBounds": false});
    assert_eq!(s.execute("data.solverSaveModel", json!({"to": "J10", "model": evo})).unwrap()["range"], "J10:J13");
    assert_eq!(formula(&s, "J10"), "$B$1=-2.5");
    let m = s.execute("data.solverLoadModel", json!({"from": "J10:J13"})).unwrap()["model"].clone();
    assert_eq!((m["goal"].as_str(), m["value"].as_f64(), m["nonNegative"].as_bool()), (Some("value"), Some(-2.5), Some(false)), "{m}");
    assert_eq!((m["method"].as_str(), m["populationSize"].as_f64(), m["randomSeed"].as_f64()), (Some("evolutionary"), Some(40.0), Some(3.0)));
    assert_eq!(m["requireBounds"], false);
    // Each command is one undo step.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("data.solver", json!({})).unwrap()["model"]["method"], "simplex");
    // Refused: no model or no target, overlapping the model, off the sheet, ranges that
    // hold no model, other sheets and oversized ranges.
    set(&mut s, &[("L1", "=1+1"), ("L2", "=SUM(A1:A2)")]);
    for (cmd, p) in [
        ("data.solverSaveModel", json!({})),
        ("data.solverSaveModel", json!({"to": "A1"})),
        ("data.solverSaveModel", json!({"to": "B3"})),
        ("data.solverSaveModel", json!({"to": "A1048575"})),
        ("data.solverSaveModel", json!({"to": "H1", "model": {"objective": "B1", "variables": "A1,A2", "constraints": ["A1,A2 <= 3"]}})),
        ("data.solverSaveModel", json!({"to": "H1", "model": "B1"})),
        ("data.solverLoadModel", json!({})),
        ("data.solverLoadModel", json!({"from": "L1:L2"})),
        ("data.solverLoadModel", json!({"from": "Z1"})),
        ("data.solverLoadModel", json!({"from": "H2:H8"})),
        ("data.solverLoadModel", json!({"from": "H1:H1048576"})),
        ("data.solverLoadModel", json!({"from": "Nope!H1"})),
    ] {
        assert!(s.execute(cmd, p.clone()).is_err(), "{cmd} {p}");
    }
    s.execute("data.solverReset", json!({})).unwrap();
    assert!(s.execute("data.solverSaveModel", json!({"to": "H1"})).is_err(), "no model to save");
}

#[test]
fn reset_all_masks_a_workbook_level_model_and_keeps_other_sheets_models() {
    let mut s = s();
    set(&mut s, &[("A1", "1"), ("B1", "=2*A1")]);
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    let (one, two) = {
        let wb = &s.doc().unwrap().wb;
        (wb.sheet_index("Sheet1").unwrap(), wb.sheet_index("Two").unwrap())
    };
    // A model saved as workbook-level names, which every sheet reads.
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        for (name, formula) in [
            ("solver_opt", "Sheet1!$B$1"),
            ("solver_adj", "Sheet1!$A$1"),
            ("solver_typ", "1"),
            ("solver_num", "0"),
            ("solver_eng", "2"),
            ("solver_ssz", "500"),
        ] {
            wb.names.push(gridcraft_model::DefinedName {
                name: name.into(),
                scope: None,
                formula: formula.into(),
                comment: String::new(),
                hidden: true,
            });
        }
    }
    let model = |s: &Session, sheet: usize| super::saved_model(&s.doc().unwrap().wb, sheet);
    assert_eq!(model(&s, one).map(|m| m["objective"].clone()), Some(json!("B1")));
    // Reset All empties this sheet's model, which the dialog then shows, and no other sheet's.
    let r = s.execute("data.solverReset", json!({})).unwrap();
    assert_eq!(model(&s, one), None, "{r}");
    assert_eq!(r["masked"], true, "{r}");
    assert!(s.execute("data.solver", json!({})).unwrap()["model"].is_null());
    assert_eq!(model(&s, two).map(|m| m["objective"].clone()), Some(json!("B1")));
    assert_eq!(s.doc().unwrap().wb.names.iter().filter(|n| n.scope.is_none()).count(), 6);
    // Undo brings the model back; the mask survives XLSX.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(model(&s, one).is_some());
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(model(&s, one), None);
    let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "r.xlsx", "base64": b64})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert_eq!(model(&s, one), None);
    assert!(model(&s, two).is_some());
    // A model saved on the sheet replaces the mask and is read alone: the workbook-level
    // population size doesn't leak into it.
    s.execute("data.solver", json!({"objective": "B1", "variables": "A1", "constraints": ["A1 <= 3"], "method": "simplex"})).unwrap();
    let m = model(&s, one).unwrap();
    assert_eq!((m["objective"].as_str(), m["populationSize"].is_null()), (Some("B1"), true), "{m}");
    assert_eq!(s.execute("data.solverReset", json!({})).unwrap()["masked"], true);
    assert_eq!(model(&s, one), None);
}

#[test]
fn saving_a_shorter_model_over_a_longer_one_clears_the_rest() {
    let mut s = s();
    set(&mut s, &[("B1", "=5*A1+4*A2"), ("B2", "=6*A1+4*A2"), ("B3", "=A1+2*A2"), ("H9", "=1+1")]);
    let long = json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 24", "B3 <= 6", "A1 <= 3", "A2 <= 3"]});
    let short = json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 24"]});
    assert_eq!(s.execute("data.solverSaveModel", json!({"to": "H1", "model": long})).unwrap()["range"], "H1:H7");
    assert_eq!(s.execute("data.solverSaveModel", json!({"to": "H1", "model": short, "overwrite": true})).unwrap()["range"], "H1:H4");
    // The old model's last rows are cleared; a formula below it stays.
    for r in 5..=7 {
        assert_eq!(formula(&s, &format!("H{r}")), "", "H{r}");
    }
    assert_eq!(formula(&s, "H9"), "1+1");
    let loaded = s.execute("data.solverLoadModel", json!({"from": "H1"})).unwrap();
    assert_eq!(loaded["model"]["constraints"], json!(["B2 <= 24"]), "{loaded}");
    // Reading down from one cell stops at the options row even with formulas right below it.
    set(&mut s, &[("H5", "=2+2")]);
    let loaded = s.execute("data.solverLoadModel", json!({"from": "H1"})).unwrap();
    assert_eq!(loaded["model"]["constraints"], json!(["B2 <= 24"]), "{loaded}");
    // Saving over cells that hold no model leaves the cells below them as they are.
    assert_eq!(s.execute("data.solverSaveModel", json!({"to": "H8", "model": short, "overwrite": true})).unwrap()["range"], "H8:H11");
    assert_eq!(formula(&s, "H5"), "2+2");
}

#[test]
fn save_model_asks_before_it_replaces_data_and_is_one_undo_step() {
    let mut s = s();
    set(&mut s, &[("B1", "=5*A1+4*A2"), ("B2", "=6*A1+4*A2"), ("H2", "kept"), ("H3", "=1+1")]);
    let model = json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 24"]});
    // A value or a formula in the column: refused, the sheet unchanged, the cell named.
    let e = s.execute("data.solverSaveModel", json!({"to": "H1", "model": model.clone()})).unwrap_err().to_string();
    assert!(e.contains(crate::cmd::analysis::OVERWRITE_ERROR) && e.contains("H1:H4") && e.contains("H2"), "{e}");
    assert_eq!((at(&s, "Sheet1", "H2"), formula(&s, "H3"), formula(&s, "H1")), (Value::text("kept"), "1+1".into(), String::new()));
    // So is a model saved there before, and `overwrite` only as a true boolean.
    assert!(s.execute("data.solverSaveModel", json!({"to": "J10", "model": model.clone()})).is_ok());
    for overwrite in [json!(null), json!(false), json!("true"), json!(1)] {
        let e = s.execute("data.solverSaveModel", json!({"to": "J10", "model": model.clone(), "overwrite": overwrite})).unwrap_err();
        assert!(e.to_string().contains(crate::cmd::analysis::OVERWRITE_ERROR), "{overwrite}: {e}");
    }
    // overwrite: true replaces the data in one undo step, which brings it all back.
    let out = s.execute("data.solverSaveModel", json!({"to": "H1", "model": model, "overwrite": true})).unwrap();
    assert_eq!(out["range"], "H1:H4");
    assert_eq!((formula(&s, "H1"), formula(&s, "H2"), formula(&s, "H3")), ("MAX($B$1)".into(), "COUNT($A$1:$A$2)".into(), "AND($B$2<=24)".into()));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(
        (at(&s, "Sheet1", "H2"), formula(&s, "H3"), formula(&s, "H1"), formula(&s, "H4")),
        (Value::text("kept"), "1+1".into(), String::new(), String::new())
    );
    assert_eq!(formula(&s, "J10"), "MAX($B$1)");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(formula(&s, "H2"), "COUNT($A$1:$A$2)");
}

#[test]
fn saving_and_resetting_a_model_keep_the_users_own_solver_names() {
    let mut s = s();
    set(&mut s, &[("A1", "0"), ("B1", "=ABS(A1-3)"), ("C1", "100")]);
    let name = |name: &str, scope: Option<usize>, formula: &str, hidden: bool| gridcraft_model::DefinedName {
        name: name.into(),
        scope,
        formula: formula.into(),
        comment: String::new(),
        hidden,
    };
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        // The user's own names, on the sheet and the workbook, and Excel's Solver names the
        // model replaces.
        wb.names.push(name("solver_budget", Some(0), "Sheet1!$C$1", false));
        wb.names.push(name("Solver_Lhs_Total", Some(0), "Sheet1!$C$1", false));
        wb.names.push(name("solver_budget", None, "Sheet1!$C$1", false));
        wb.names.push(name("solver_lin", Some(0), "2", true));
        wb.names.push(name("solver_lhs12", Some(0), "Sheet1!$A$1", true));
    }
    let names = |s: &Session| -> Vec<String> {
        let mut v: Vec<String> = s.doc().unwrap().wb.names.iter().map(|n| format!("{}@{:?}", n.name, n.scope)).collect();
        v.sort();
        v
    };
    let mine = ["Solver_Lhs_Total@Some(0)", "solver_budget@None", "solver_budget@Some(0)"];
    let model = json!({"objective": "B1", "goal": "min", "variables": "A1", "constraints": ["A1 <= 4"], "method": "simplex"});
    s.execute("data.solver", model.clone()).unwrap();
    let after = names(&s);
    assert!(mine.iter().all(|n| after.contains(&n.to_string())), "{after:?}");
    assert!(!after.iter().any(|n| n.starts_with("solver_lin") || n.starts_with("solver_lhs12")), "{after:?}");
    // Load Model saves the model again; Reset All removes it and nothing else.
    s.execute("data.solverSaveModel", json!({"to": "H1"})).unwrap();
    s.execute("data.solverLoadModel", json!({"from": "H1"})).unwrap();
    assert!(mine.iter().all(|n| names(&s).contains(&n.to_string())), "{:?}", names(&s));
    let model_names = names(&s).len() - mine.len();
    let r = s.execute("data.solverReset", json!({})).unwrap();
    assert_eq!((r["removed"].as_u64(), r["masked"].as_bool()), (Some(model_names as u64), Some(false)), "{r}");
    assert_eq!(names(&s), mine);
    assert!(s.execute("data.solver", json!({})).unwrap()["model"].is_null());
}

#[test]
fn save_model_refuses_a_place_where_the_options_cannot_spill() {
    let mut s = s();
    set(&mut s, &[("B1", "=ABS(A1-3)"), ("H24", "=SEQUENCE(1,3)"), ("K30", "=SEQUENCE(3)"), ("M50", "=SEQUENCE(3)")]);
    // Tables whose empty data rows meet an options row: one in its way, one holding the
    // options cell.
    for range in ["T59:U62", "W69:W71"] {
        s.execute("insert.table", json!({"range": range, "header": true})).unwrap();
    }
    let model = json!({"objective": "B1", "goal": "min", "variables": "A1", "constraints": ["F4 <= 5"]});
    let save = |s: &mut Session, to: &str| s.execute("data.solverSaveModel", json!({"to": to, "model": model.clone()}));
    // The options row (H4) spills into I4:O4: a value in its last cell is in the way.
    set(&mut s, &[("O4", "x")]);
    let e = save(&mut s, "H1").unwrap_err().to_string();
    assert!(e.contains("I4:O4") && e.contains("O4 isn't empty"), "{e}");
    assert_eq!((formula(&s, "H1"), formula(&s, "H4")), (String::new(), String::new()));
    assert!(s.execute("data.solver", json!({})).unwrap()["model"].is_null());
    // With the row clear the options spill, and saving again at the same cell replaces them.
    set(&mut s, &[("O4", "")]);
    for overwrite in [false, true] {
        let p = json!({"to": "H1", "model": model.clone(), "overwrite": overwrite});
        assert_eq!(s.execute("data.solverSaveModel", p).unwrap()["range"], "H1:H4");
        assert_eq!(
            (at(&s, "Sheet1", "H4"), at(&s, "Sheet1", "I4"), at(&s, "Sheet1", "O4")),
            (Value::number(1.0), Value::number(1.0), Value::number(50.0))
        );
    }
    // Refused: in the options row, a formula (H24), another formula's spilled values (K31,
    // from K30), a cell of the model (F4), merged cells, a table and the sheet's last columns;
    // a column over another formula's spilled values (I24 of H24:J24, M51 of M50:M52) or with
    // its options cell in a table.
    s.execute("home.mergeCells", json!({"range": "S8:T9"})).unwrap();
    for (to, why) in [
        ("G21", "H24 isn't empty"),
        ("J28", "K31 holds the values of another formula"),
        ("E1", "F4 is a cell of the model"),
        ("R5", "merged"),
        ("S57", "T60:Z60, but a table is in the way"),
        ("XFA1", "need 8 cells"),
        ("I21", "cover I24, which holds the values of another formula"),
        ("M51", "cover M51, which holds the values of another formula"),
        ("W67", "X70:AD70, but a table is in the way"),
    ] {
        let e = save(&mut s, to).unwrap_err().to_string();
        assert!(e.contains(why), "{to}: {e}");
        assert_eq!(formula(&s, to), "", "{to}");
    }
    assert_eq!(
        (at(&s, "Sheet1", "J24"), at(&s, "Sheet1", "K32"), at(&s, "Sheet1", "M52")),
        (Value::number(3.0), Value::number(3.0), Value::number(3.0))
    );
}

#[test]
fn load_model_refuses_a_range_with_a_cell_without_a_formula() {
    let mut s = s();
    set(&mut s, &[("B1", "=5*A1+4*A2"), ("B2", "=6*A1+4*A2"), ("B3", "=A1+2*A2")]);
    let model = json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 24", "B3 <= 6", "A1 <= 3"], "method": "simplex"});
    assert_eq!(s.execute("data.solverSaveModel", json!({"to": "H1", "model": model})).unwrap()["range"], "H1:H6");
    // A range with empty cells after the model loads it.
    let loaded = s.execute("data.solverLoadModel", json!({"from": "H1:H9"})).unwrap();
    assert_eq!(loaded["model"]["constraints"], json!(["B2 <= 24", "B3 <= 6", "A1 <= 3"]), "{loaded}");
    s.execute("data.solverReset", json!({})).unwrap();
    // A constraint overwritten by a constant would drop that constraint; a gap before the
    // options row would drop the options. Both are refused, naming the cell.
    for (cell, input) in [("H4", "7"), ("H5", "")] {
        let before = formula(&s, cell);
        set(&mut s, &[(cell, input)]);
        let e = s.execute("data.solverLoadModel", json!({"from": "H1:H6"})).unwrap_err().to_string();
        assert!(e.contains(cell), "{e}");
        assert!(s.execute("data.solver", json!({})).unwrap()["model"].is_null(), "{cell}");
        set(&mut s, &[(cell, &format!("={before}"))]);
    }
    assert!(s.execute("data.solverLoadModel", json!({"from": "H1:H6"})).is_ok());
}

#[test]
fn load_model_keeps_the_evolutionary_convergence_and_generations() {
    let mut s = s();
    set(&mut s, &[("B1", "=ABS(A1-3)")]);
    let evo = json!({"objective": "B1", "goal": "min", "variables": "A1", "constraints": ["A1 <= 4"], "method": "evolutionary", "convergence": 0.001, "generationsWithoutImprovement": 20});
    s.execute("data.solverSaveModel", json!({"to": "H1", "model": evo})).unwrap();
    let m = s.execute("data.solverLoadModel", json!({"from": "H1"})).unwrap()["model"].clone();
    assert_eq!((m["convergence"].as_f64(), m["generationsWithoutImprovement"].as_f64()), (Some(0.001), Some(20.0)), "{m}");
    // They survive XLSX; Excel's time limit (solver_mni, in seconds) isn't written or read.
    let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "c.xlsx", "base64": b64})).unwrap();
    let m = s.execute("data.solver", json!({})).unwrap()["model"].clone();
    assert_eq!((m["convergence"].as_f64(), m["generationsWithoutImprovement"].as_f64()), (Some(0.001), Some(20.0)), "{m}");
    assert!(!s.doc().unwrap().wb.names.iter().any(|n| n.name == "solver_mni"));
    // Reset All removes the GridCraft name with the rest.
    s.execute("data.solverReset", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.names.iter().all(|n| n.scope != Some(0)));
}

#[test]
fn limits_report_of_a_nonlinear_model() {
    // The largest rectangle with perimeter at most 20 (5 × 5): with the height held at 5, the
    // width ranges over 0..5, the area over 0..25.
    let mut s = s();
    set(&mut s, &[("A1", "1"), ("A2", "1"), ("B1", "=A1*A2"), ("B2", "=2*A1+2*A2")]);
    let r = s.execute("data.solver", json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 20"], "reports": ["limits"]})).unwrap();
    assert_eq!(r["reports"], json!(["Limits Report 1"]), "{r}");
    let l = "Limits Report 1";
    let r = row_of(&s, l, "$A$1") + 1;
    let got: Vec<f64> = ["E", "F", "G", "H"].iter().map(|c| at(&s, l, &format!("{c}{r}")).as_f64().unwrap()).collect();
    for (g, want) in got.iter().zip([0.0, 0.0, 5.0, 25.0]) {
        assert!((g - want).abs() < 1e-3, "{got:?}");
    }
}

#[test]
fn infeasible_unbounded_and_nonlinear_models_are_reported() {
    let mut s = s();
    set(&mut s, &[("A1", "1"), ("B1", "=2*A1")]);
    for method in ["simplex", "grg"] {
        let r =
            s.execute("data.solver", json!({"objective": "B1", "variables": "A1", "constraints": ["A1 >= 5", "B1 <= 6"], "method": method})).unwrap();
        assert_eq!(r["status"], "infeasible", "{method}: {r}");
        assert_eq!(r["changed"], false);
        assert_eq!(n(&s, "A1"), 1.0, "an infeasible model leaves the cells alone");
    }
    let r = s.execute("data.solver", json!({"objective": "B1", "variables": "A1", "method": "simplex"})).unwrap();
    assert_eq!(r["status"], "unbounded");
    let r = s.execute("data.solver", json!({"objective": "B1", "variables": "A1", "method": "grg", "maxEvaluations": 5000})).unwrap();
    assert!(r["status"] == "unbounded" || r["status"] == "iterationLimit", "{r}");
    set(&mut s, &[("A1", "1"), ("C1", "=A1^2")]);
    let r = s.execute("data.solver", json!({"objective": "C1", "goal": "min", "variables": "A1", "method": "simplex"})).unwrap();
    assert_eq!(r["status"], "notLinear");
}

#[test]
fn hostile_models_are_refused_without_panicking() {
    let mut s = s();
    let r = s.execute("data.solver", json!({})).unwrap();
    assert!(r["model"].is_null());
    assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "solver")));
    set(&mut s, &[("B1", "=SUM(A1:A1000)"), ("C1", "5")]);
    for p in [
        json!({"objective": "B1", "variables": "A1:A1000"}),
        json!({"objective": "B1", "variables": "A1:XFD1048576"}),
        json!({"objective": "C1", "variables": "A1"}),
        json!({"objective": "B1:B2", "variables": "A1"}),
        json!({"objective": "B1", "variables": "B1"}),
        json!({"objective": "B1", "variables": "A1", "goal": "sideways"}),
        json!({"objective": "B1", "variables": "A1", "goal": "value"}),
        json!({"objective": "B1", "variables": "A1", "method": "evolutionary"}),
        json!({"objective": "B1", "variables": "A1", "method": "annealing"}),
        json!({"objective": "B1", "variables": "A1", "constraints": ["A1 ?? 3"]}),
        json!({"objective": "B1", "variables": "A1", "constraints": "A1 <= 3"}),
        json!({"objective": "B1", "variables": "A1", "constraints": ["A1:A3 <= C1:C2"]}),
        json!({"objective": "B1", "variables": "A1", "constraints": ["C1 int"]}),
        json!({"objective": "B1", "variables": "A1", "constraints": ["A1:A1048576 <= 1"]}),
        json!({"objective": "B1", "variables": "A1", "constraints": vec![json!("A1 <= 1"); 1001]}),
        json!({"objective": "Nope!B1", "variables": "A1"}),
    ] {
        assert!(s.execute("data.solver", p.clone()).is_err(), "{p}");
    }
    // Huge evaluation requests are capped, and a model erroring everywhere doesn't panic.
    set(&mut s, &[("D1", "=1/0+A1")]);
    let r = s.execute("data.solver", json!({"objective": "D1", "variables": "A1", "maxEvaluations": 1e300})).unwrap();
    assert_ne!(r["status"], "converged");
}
