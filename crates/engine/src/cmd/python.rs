//! Python in cells: making and editing Python cells, and their return type. This build stores
//! the code and the last result a file cached; it doesn't run Python, so a cell with new or
//! changed code shows `#BLOCKED!`.

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Cell, PyCode, python::MAX_FORMULA_CHARS};
use serde_json::{Value as Json, json};

use super::{CommandSpec, bad, cell_param_on, cmd, edit, has_doc, str_param, target_sheet};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "python.set",
            "Set Python Code",
            [],
            None,
            "{cell?, code, output?: object|value} makes the cell (default: the active cell) a Python cell with this code; the code must fit in Excel's 8,192-character formula once written as PY(…). `output` is the return type: `value` (an Excel value, the default) or `object` (a Python object). `output`, when given, is a choice, which new code without `output` keeps; a picture with no choice behaves as a Python Object.",
            has_doc,
            set
        ),
        cmd!(
            "python.output",
            "Python Output",
            [],
            Some("Ctrl+Alt+Shift+M"),
            "{cell?, mode?: object|value} sets the return type of a Python cell (default: the active cell): `object` (a Python object) or `value` (an Excel value). Without `mode` it switches from the return type in effect to the other one. A return type asked for is a choice. The cell keeps its code; a cached result of the old return type goes, as with `python.set`, so the cell shows #BLOCKED! until it runs. The result stays in two cases: when the type asked for is already the type in effect, and for a picture with no choice, which behaves as a Python Object, so that asking for `value` moves it into the cell with no new run.",
            has_doc,
            output
        ),
    ]
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

fn output_mode(cmd: &str, p: &Json, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Json::Null) => return Ok(None),
        Some(Json::String(_)) => {}
        Some(_) => return Err(bad(cmd, format!("`{key}` is `object` or `value`"))),
    }
    match str_param(p, key) {
        None => Ok(None),
        Some(m) if m.eq_ignore_ascii_case("object") => Ok(Some(true)),
        Some(m) if m.eq_ignore_ascii_case("value") => Ok(Some(false)),
        Some(m) => Err(bad(cmd, format!("`{key}` is `object` or `value`, not `{m}`"))),
    }
}

fn set(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let code = match p.get("code") {
        Some(Json::String(c)) => c.as_str(),
        Some(_) => return Err(bad("python.set", "`code` must be text")),
        None => return Err(bad("python.set", "missing `code`")),
    };
    let mode = output_mode("python.set", p, "output")?;
    let old = existing(s, sheet, at)?;
    let object = mode.or(old.as_ref().map(|o| o.object)).unwrap_or(false);
    let output_chosen = mode.is_some() || old.as_ref().is_some_and(|o| o.output_chosen);
    store(s, "python.set", sheet, at, PyCode { object, output_chosen, ..PyCode::new(code) }, old)?;
    let d = s.doc()?;
    let shown = d.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": crate::cmd::inspect::value_json(&shown)}))
}

/// A table header is the column's name, never a formula or code (as in Excel), and the header
/// row would take it over.
fn refuse_table_header(s: &Session, sheet: usize, at: CellRef) -> Result<()> {
    let header =
        s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.tables.iter().any(|t| t.header_row && t.range.start.row == at.row && t.range.contains(at)));
    if header {
        return Err(EngineError::Other(format!("{} is a table header; Python cells can't go in a table's header row", at.a1())));
    }
    Ok(())
}

/// Writes `py` into the cell, which held `old`: what `python.set` and `python.output` share. The
/// code must fit in one cell and a table header takes none; a locked cell on a protected sheet is
/// refused by the edit itself, and here for the choice alone, which the edit doesn't compare.
fn store(s: &mut Session, cmd: &str, sheet: usize, at: CellRef, mut py: PyCode, old: Option<PyCode>) -> Result<()> {
    if !py.fits() {
        return Err(bad(
            cmd,
            format!("the code doesn't fit in one cell: written as PY(…) it would be longer than Excel's {MAX_FORMULA_CHARS}-character formula limit"),
        ));
    }
    refuse_table_header(s, sheet, at)?;
    if old.as_ref().is_some_and(|o| o.output_chosen != py.output_chosen)
        && let Some(m) = s.locked_cell_message(sheet, at)
    {
        return Err(EngineError::Other(m));
    }
    // What a text does with its lines is the cell's choice, whatever the code gives.
    if let Some(o) = &old {
        py.spill_lines = o.spill_lines;
    }
    // Unchanged code with the same return type keeps its result (and its value); new code, or a
    // new return type, has none until it runs: a result of the other type would show a value and
    // preview that don't belong to the cell. A picture with no choice is both, so any choice keeps it.
    let keep = old.filter(|o| o.code == py.code && (o.object == py.object || (!o.output_chosen && o.image().is_some())));
    let value = match &keep {
        Some(o) => {
            py.output = o.output.clone();
            py.stale = o.stale;
            py.busy = o.busy;
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
    })
}

/// The return type in effect stays, with its result, and is now a choice: a picture with no choice
/// behaves as a Python Object, and needs no new run to move into the cell. A locked cell on a
/// protected sheet is refused here, as the choice isn't what the edit itself compares.
fn set_output_choice(s: &mut Session, sheet: usize, at: CellRef, object: bool) -> Result<()> {
    refuse_table_header(s, sheet, at)?;
    if let Some(m) = s.locked_cell_message(sheet, at) {
        return Err(EngineError::Other(m));
    }
    edit(s, |cx| {
        let Some(py) = cx.sheet_mut(sheet)?.cells.get_mut(at).and_then(|c| c.py.as_mut()) else { return Ok(()) };
        let py = Arc::make_mut(py);
        py.object = object;
        py.output_chosen = true;
        cx.touch(sheet, at);
        Ok(())
    })
}

/// `python.output`: the cell's return type. The code stays; a cached result belongs to the old
/// return type, so it goes, as with `python.set` and a new return type: the cell shows
/// `#BLOCKED!` until it runs, and its old spill goes with its readers recalculated. It stays when the
/// type asked for is already in effect, and for a picture with no choice.
fn output(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let Some(old) = existing(s, sheet, at)? else {
        return Err(EngineError::Other(format!("{} isn't a Python cell, so it has no Python output to change", at.a1())));
    };
    let current = old.shows_object();
    let object = output_mode("python.output", p, "mode")?.unwrap_or(!current);
    let mode = |object: bool| if object { "object" } else { "value" };
    if object != old.object && object != current {
        let py = PyCode { code: old.code.clone(), object, output_chosen: true, ..PyCode::default() };
        store(s, "python.output", sheet, at, py, Some(old))?;
    } else if object != old.object || !old.output_chosen {
        set_output_choice(s, sheet, at, object)?;
    }
    let d = s.doc()?;
    let shown = d.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "output": mode(object), "value": crate::cmd::inspect::value_json(&shown)}))
}
