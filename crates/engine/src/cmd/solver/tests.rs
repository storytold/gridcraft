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
