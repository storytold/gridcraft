//! Python in cells: making and editing Python cells. This build stores the code and the last
//! result a file cached; it doesn't run Python, so a cell with new or changed code shows
//! `#BLOCKED!`.

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Cell, PyCode, python::MAX_FORMULA_CHARS};
use serde_json::{Value as Json, json};

use super::{CommandSpec, bad, cell_param_on, cmd, edit, has_doc, str_param, target_sheet};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "python.set",
        "Set Python Code",
        [],
        None,
        "{cell?, code, output?: object|value} makes the cell (default: the active cell) a Python cell with this code; the code must fit in Excel's 8,192-character formula once written as PY(…). `output` is the return type: `value` (an Excel value, the default) or `object` (a Python object)",
        has_doc,
        set
    )]
}

fn target_cell(s: &Session, p: &Json) -> Result<(usize, CellRef)> {
    let sheet = target_sheet(s, p)?;
    let at = match cell_param_on(s, p, "cell", sheet)? {
        Some(c) => c,
        None => s.doc()?.selection.active,
    };
    Ok((sheet, at))
}

fn existing(s: &Session, sheet: usize, at: CellRef) -> Result<Option<PyCode>> {
    Ok(s.doc()?.wb.sheet(sheet).and_then(|sh| sh.cell(at)).and_then(|c| c.py.as_deref()).cloned())
}

fn output_mode(p: &Json, key: &str) -> Result<Option<bool>> {
    match str_param(p, key) {
        None => Ok(None),
        Some(m) if m.eq_ignore_ascii_case("object") => Ok(Some(true)),
        Some(m) if m.eq_ignore_ascii_case("value") => Ok(Some(false)),
        Some(m) => Err(bad("python.set", format!("`{key}` is `object` or `value`, not `{m}`"))),
    }
}

fn set(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let code = match p.get("code") {
        Some(Json::String(c)) => c.as_str(),
        Some(_) => return Err(bad("python.set", "`code` must be text")),
        None => return Err(bad("python.set", "missing `code`")),
    };
    let mode = output_mode(p, "output")?;
    let old = existing(s, sheet, at)?;
    let object = mode.or(old.as_ref().map(|o| o.object)).unwrap_or(false);
    let mut py = PyCode { object, ..PyCode::new(code) };
    if !py.fits() {
        return Err(bad(
            "python.set",
            format!("the code doesn't fit in one cell: written as PY(…) it would be longer than Excel's {MAX_FORMULA_CHARS}-character formula limit"),
        ));
    }
    // A table header is the column's name, never a formula or code (as in Excel).
    let header =
        s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.tables.iter().any(|t| t.header_row && t.range.start.row == at.row && t.range.contains(at)));
    if header {
        return Err(EngineError::Other(format!("{} is a table header; Python cells can't go in a table's header row", at.a1())));
    }
    // Unchanged code keeps its result (and its value); new code has none until it runs.
    let keep = old.filter(|o| o.code == py.code);
    let value = match &keep {
        Some(o) => {
            py.output = o.output.clone();
            py.stale = o.stale;
            None
        }
        None => Some(gridcraft_model::python::BLOCKED),
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let style = sh.style_id(at);
        let value = value.unwrap_or_else(|| sh.cell(at).map(|c| c.value.clone()).unwrap_or(Value::Empty));
        // A spill the cell had (a formula's, or the old result's) is left to the recalculation,
        // which removes it and recalculates what read it.
        sh.set_cell(at, Cell { value, formula: None, style, py: Some(Arc::new(py)) });
        cx.touch(sheet, at);
        Ok(())
    })?;
    let d = s.doc()?;
    let shown = d.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": crate::cmd::inspect::value_json(&shown)}))
}
