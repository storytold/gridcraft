//! Runs a basic subset of the VBA macros found in an open workbook's `vbaProject.bin` (see
//! `gridcraft-vba`). There's no VBA *editor* here, nor any writer — just enough to run a macro
//! someone already wrote in real Excel.

use gridcraft_core::{CellRef, Value as CoreValue};
use gridcraft_vba::{Host, Macro, Value as VbaValue};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "macro.list", "List Macros", [], None, "{} → names of zero-argument Subs found in the workbook's VBA project", has_doc, list),
        cmd!("macro.run", "Run Macro", ["Automate"], None, "{name} → messages (MsgBox text, in order)", has_doc, run),
    ]
}

fn macros(s: &Session) -> Result<Vec<Macro>> {
    let d = s.doc()?;
    let Some(bytes) = &d.wb.vba else { return Ok(vec![]) };
    let modules = gridcraft_vba::extract(bytes).map_err(|e| bad("macro", e))?;
    Ok(modules.iter().flat_map(|m| Macro::parse_all(&m.source)).collect())
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let names: Vec<String> = macros(s)?.into_iter().map(|m| m.name).collect();
    Ok(json!({ "macros": names }))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("macro.run", "missing `name`"))?;
    let m = macros(s)?.into_iter().find(|m| m.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad("macro.run", format!("no macro named `{name}`")))?;
    let mut host = EngineHost { s, messages: vec![] };
    m.run(&mut host).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(json!({ "messages": host.messages }))
}

/// Adapts [`Session`] to [`gridcraft_vba::Host`]: reads go straight to the sheet, writes go
/// through `cell.set` — the same command the ribbon and formula bar use — so a macro's edits are
/// undoable, journaled and recalculated exactly like a person's.
struct EngineHost<'s> {
    s: &'s mut Session,
    messages: Vec<String>,
}

impl Host for EngineHost<'_> {
    fn get_cell(&self, sheet: Option<&str>, row: u32, col: u32) -> VbaValue {
        let Ok(d) = self.s.doc() else { return VbaValue::Empty };
        let idx = match sheet {
            Some(name) => match d.wb.sheet_index(name) {
                Some(i) => i,
                None => return VbaValue::Empty,
            },
            None => d.wb.active_sheet,
        };
        let Some(sh) = d.wb.sheet(idx) else { return VbaValue::Empty };
        core_to_vba(&sh.value(CellRef::new(row, col)))
    }

    fn set_cell(&mut self, sheet: Option<&str>, row: u32, col: u32, value: VbaValue) {
        let mut params = json!({ "cell": CellRef::new(row, col).a1(), "input": value.display() });
        if let Some(name) = sheet {
            params["sheet"] = json!(name);
        }
        let _ = self.s.execute("cell.set", params);
    }

    fn msg_box(&mut self, text: &str) {
        self.messages.push(text.to_string());
    }
}

fn core_to_vba(v: &CoreValue) -> VbaValue {
    match v {
        CoreValue::Empty => VbaValue::Empty,
        CoreValue::Number(n) => VbaValue::Number(*n),
        CoreValue::Text(t) => VbaValue::Str(t.to_string()),
        CoreValue::Bool(b) => VbaValue::Bool(*b),
        CoreValue::Error(_) | CoreValue::Array(_) => VbaValue::Empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }

    /// Exercises the adapter that wires `gridcraft-vba`'s `Host` trait into a live `Session`
    /// (reads via `Sheet::value`, writes via `cell.set`, undo/journal included) — without needing
    /// a real `vbaProject.bin`, which `gridcraft-vba`'s own tests already cover extracting from.
    #[test]
    fn engine_host_reads_and_writes_cells_through_cell_set() {
        let mut s = session();
        s.execute("cell.set", json!({"cell": "A1", "input": "200"})).unwrap();
        let m = Macro::parse_all(
            r#"
            Sub Classify()
                If Range("A1").Value > 100 Then
                    Range("B1").Value = "High"
                Else
                    Range("B1").Value = "Low"
                End If
            End Sub
            "#,
        )
        .remove(0);
        let mut host = EngineHost { s: &mut s, messages: vec![] };
        m.run(&mut host).unwrap();
        assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B1").unwrap()), CoreValue::text("High"));
        // Went through `cell.set`, so it's undoable like any other edit.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B1").unwrap()), CoreValue::Empty);
    }

    #[test]
    fn list_and_run_without_a_vba_project() {
        let mut s = session();
        assert_eq!(list(&mut s, &json!({})).unwrap(), json!({"macros": Vec::<String>::new()}));
        let err = run(&mut s, &json!({"name": "Anything"})).unwrap_err();
        assert!(err.to_string().contains("no macro named"), "{err}");
    }
}
