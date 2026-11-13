//! Formula-based actuarial reports from registered worksheet functions.
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_formula::{Expr, SheetSel};
use gridcraft_model::{Cell, Formula, Sheet, StyleId};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, cmd, edit, has_doc};

const CMD: &str = "data.actuarialReport";

/// Registers a report command usable from the UI, CLI and MCP.
pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "data.actuarialReport",
        "Actuarial report…",
        ["Data", "Analysis"],
        None,
        "{function: ACT.* function name, arguments: [cell/range reference or literal, ...], newSheet?: string}; creates a live formula-based report.",
        has_doc,
        run
    )]
}

/// A safe report argument: literals and cell/range references are kept live, not evaluated here.
fn argument(wb: &gridcraft_model::Workbook, text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let text = text.strip_prefix('=').unwrap_or(text).trim();
    if text.is_empty() {
        return Err(bad(CMD, "invalid argument syntax"));
    }
    if text.len() > 4096 {
        return Err(bad(CMD, "argument is too long"));
    }
    let expr = gridcraft_formula::parse(text).map_err(|_| bad(CMD, "invalid argument syntax"))?;
    if let Expr::Ref(r) = &expr {
        return match &r.sheet {
            SheetSel::Current => {
                let sheet = wb.active().ok_or(EngineError::NoDocument)?;
                Ok(format!("{}!{text}", gridcraft_formula::quote_sheet(&sheet.name)))
            }
            SheetSel::Named(name) if wb.sheet_index(name).is_some() => Ok(text.to_string()),
            _ => Err(bad(CMD, "argument refers to an unavailable sheet")),
        };
    }
    let mut literal = true;
    expr.walk(&mut |e| {
        if !matches!(e, Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Array(_) | Expr::Unary(_, _) | Expr::Paren(_)) {
            literal = false;
        }
    });
    if !literal {
        return Err(bad(CMD, "use a cell/range reference or a literal for each argument"));
    }
    Ok(text.to_string())
}

/// Creates one undoable report sheet whose formula retains links to the source workbook.
fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    if d.wb.protected_structure {
        return Err(bad(CMD, "the workbook structure is protected"));
    }
    let name = p.get("function").and_then(Json::as_str).unwrap_or("").trim().to_ascii_uppercase();
    let spec = gridcraft_functions::lookup(&name)
        .filter(|f| f.name.starts_with("ACT."))
        .ok_or_else(|| bad(CMD, "choose a registered actuarial function"))?;
    let raw = p.get("arguments").and_then(Json::as_array).ok_or_else(|| bad(CMD, "arguments must be a list"))?;
    if raw.len() > spec.max_args.min(32) || raw.len() < spec.min_args {
        return Err(bad(CMD, "incorrect number of arguments"));
    }
    let args = raw
        .iter()
        .map(|v| v.as_str().ok_or_else(|| bad(CMD, "each argument must be text")).and_then(|v| argument(&d.wb, v)))
        .collect::<Result<Vec<_>>>()?;
    if args.iter().take(spec.min_args).any(String::is_empty) {
        return Err(bad(CMD, "required arguments cannot be empty"));
    }
    let formula = format!("{}({})", spec.name, args.join(","));
    let expr = gridcraft_formula::parse(&formula).map_err(|_| bad(CMD, "invalid formula"))?;
    let name = match p.get("newSheet").and_then(Json::as_str).filter(|s| !s.trim().is_empty()) {
        Some(s) => s.trim().to_string(),
        None => (1..=100_000)
            .map(|i| if i == 1 { "Actuarial Report".to_string() } else { format!("Actuarial Report {i}") })
            .find(|n| d.wb.sheet_index(n).is_none())
            .ok_or_else(|| bad(CMD, "no report sheet name available"))?,
    };
    d.wb.check_sheet_name(&name, None).map_err(EngineError::Other)?;
    crate::workload::report_capacity(&d.wb, 16)?;
    let index = edit(s, |cx| {
        let mut sh = Sheet::new(name.clone());
        let bold = cx.wb.styles.derive(StyleId::DEFAULT, |style| style.font.bold = true);
        for (row, value, style) in [(0, spec.name, bold), (1, spec.signature, StyleId::DEFAULT), (2, spec.description, StyleId::DEFAULT)] {
            sh.set_cell(CellRef::new(row, 0), Cell { value: Value::text(value), formula: None, style, py: None });
        }
        sh.set_cell(CellRef::new(4, 0), Cell::formula(Formula::from_expr(expr)));
        let i = cx.wb.sheets.len();
        cx.wb.sheets.push(Arc::new(sh));
        cx.wb.set_active_sheet(i);
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(i)
    })?;
    if let Ok(doc) = s.doc_mut() {
        doc.sheet_selections.insert(index.min(doc.sheet_selections.len()), crate::Selection::default());
    }
    Ok(json!({"sheet":name,"cell":"A5","formula":format!("={formula}")}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reports keep live links to the original sheet and are one undoable edit.
    #[test]
    fn actuarial_report_is_formula_based_and_tracks_inputs() {
        // The shared dialog is also built without the optional actuarial pack.
        if gridcraft_functions::lookup("ACT.ANNUITY").is_none() {
            return;
        }
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range":"A1","values":[[0.1,0.05,1],[0.1,null,null]]})).unwrap();
        let p = json!({"function":"ACT.ANNUITY","arguments":["A1:A2","B1","C1"]});
        let result = s.execute(CMD, p).unwrap();
        assert_eq!(result["sheet"], "Actuarial Report");
        assert_eq!(result["formula"], "=ACT.ANNUITY(Sheet1!A1:A2,Sheet1!B1,Sheet1!C1)");
        let report = s.doc().unwrap().wb.sheet(1).unwrap();
        assert_eq!(report.value(CellRef::new(4, 0)), Value::Number(1.0));
        assert!(report.cell(CellRef::new(4, 0)).unwrap().formula.is_some());
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
        s.execute("sheet.activate", json!({"sheet":0})).unwrap();
        s.execute("cell.set", json!({"cell":"C1","input":"2"})).unwrap();
        assert!(s.doc().unwrap().wb.sheet(1).unwrap().value(CellRef::new(4, 0)).as_f64().unwrap() > 1.0);
    }

    /// Leading equals signs are accepted for references and literal values alike.
    #[test]
    fn actuarial_report_arguments_accept_leading_equals() {
        let mut s = Session::new();
        s.new_workbook();
        let wb = &s.doc().unwrap().wb;
        assert_eq!(argument(wb, " =A1 ").unwrap(), "Sheet1!A1");
        assert_eq!(argument(wb, " =0.05 ").unwrap(), "0.05");
        assert!(argument(wb, "=").is_err());
    }

    /// Invalid functions and incomplete inputs must not create a sheet.
    #[test]
    fn actuarial_report_rejects_invalid_parameters_atomically() {
        let mut s = Session::new();
        s.new_workbook();
        for p in [
            json!({"function":"SUM","arguments":["1"]}),
            json!({"function":"ACT.ANNUITY","arguments":["A1"]}),
            json!({"function":"ACT.ANNUITY","arguments":["A1+1","1"]}),
        ] {
            assert!(s.execute(CMD, p).is_err());
            assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
        }
    }
}
