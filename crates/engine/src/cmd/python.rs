//! Python in cells: making and editing Python cells, their return type, turning them back into
//! normal cells, and running them. Running needs a capability the app gives the session
//! ([`crate::Session::set_python_capability`]), which no command can give: without one, these
//! commands report that Python doesn't run here, and a cell with new or changed code shows
//! `#BLOCKED!`. See [`crate::python`].

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::python::{MAX_FORMULA_CHARS, MAX_INIT_CHARS, init_fits, normalize_code};
use gridcraft_model::{Cell, PyCode, PyTable};
use serde_json::{Value as Json, json};

use super::{CommandSpec, always, bad, cell_param, cell_param_on, cmd, edit, has_doc, str_param, target_sheet, u32_param};
use crate::python::{PythonCalculation, diagnostic_entry_json, environment_json, run_json};
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
            ["Python", "Code"],
            Some("Ctrl+Alt+Shift+M"),
            "{cell?, mode?: object|value, lines?: cell|rows} sets the return type of a Python cell (default: the active cell): `object` (a Python object) or `value` (an Excel value). Without `mode` or `lines` it switches from the return type in effect to the other one. A return type asked for is a choice. The cell keeps its code; a cached result of the old return type goes, as with `python.set`, so the cell shows #BLOCKED! until it runs. The result stays in two cases: when the type asked for is already the type in effect, and for a picture with no choice, which behaves as a Python Object, so that asking for `value` moves it into the cell with no new run. `lines` says what a text result with line breaks does: it stays in the cell (`cell`, the default) or spills one line per row (`rows`); the result is kept, so this runs nothing. GridCraft JSON keeps this choice; an xlsx file has no place for it, so there it lasts for the session.",
            has_doc,
            output
        ),
        cmd!(
            "python.clear",
            "Convert to Normal Cell",
            [],
            None,
            "{cell?} turns a Python cell (default: the active cell) into a normal cell that keeps what it shows: the code goes, the cell keeps its value, and each cell its result spills over keeps its value as a plain value. A Python-only error (#BLOCKED!, #BUSY!, #CONNECT!, #PYTHON!, #TIMEOUT!) is no result and can't be an ordinary cell's value, so a cell showing one becomes empty. Formats stay, and what reads the cells is recalculated. One undo step brings the Python cell back with its code, return type and result",
            has_doc,
            clear
        ),
        cmd!(
            query "python.table",
            "Python Table",
            [],
            None,
            "{cell?, sheet?, format?: json|csv|markdown} → the table a Python Object cell (default: the active cell) keeps from its last run: at most 200 rows, 30 columns and 1 MiB of text. {cell, rows, cols, totalRows, totalCols, rowLabels, textCut, cut, columns, data} with `format` json (the default), or {…, text} with csv or markdown. `totalCols` doesn't count the row labels; `cut` is true when anything of the object isn't here, so a copy of this is only part of it. CSV guards a text a spreadsheet would run as a formula (quoted, with a ' before it), and Markdown escapes | and line breaks. An error for a cell with no table: a file doesn't hold one, so a cell opened from a file needs to run first. Read-only",
            has_doc,
            table
        ),
        cmd!(
            query "python.image",
            "Python Image",
            [],
            None,
            "{cell?, sheet?} → {cell, mime, width, height, density, bytes, base64}: the picture a Python cell (default: the active cell) gave in its last run. `width` and `height` are what the card reports, and `density` is how many pixels of the bytes stand for one of those. An error for a cell with no picture.",
            has_doc,
            image
        ),
        cmd!(
            noundo "python.saveImage",
            "Save Python Image",
            [],
            None,
            "{cell?, sheet?, path} → {cell, path, bytes, mime}: writes the picture `python.image` gives, with `.png` or `.jpg` added to `path` when it has no such ending. Refused inside a script, as other file writes are.",
            has_doc,
            save_image
        ),
        cmd!(
            "python.displayOverCells",
            "Display Plot over Cells",
            ["Python", "Plots"],
            Some("Ctrl+Alt+Shift+C"),
            "{cell?, sheet?, at?} → {image, cell}: a floating picture of the picture a Python cell gave (default: the active cell), placed at `at` (default: the cell to its right) on the active sheet. It is linked to the cell: it changes when the cell's result does, and goes when the cell can no longer give a picture.",
            has_doc,
            display_over_cells
        ),
        cmd!(
            "python.createReference",
            "Create Reference",
            ["Python", "Plots"],
            None,
            "{id} → {image, cell}: another picture linked to the same cell as the linked picture `id` on the active sheet, a little below and to the right of it.",
            has_doc,
            create_reference
        ),
        cmd!(
            noundo "python.saveCsv",
            "Save Python Table as CSV",
            [],
            None,
            "{cell?, sheet?, path} → {cell, path, bytes, rows, cols, totalRows, totalCols, cut}: writes the table `python.table` gives as CSV, `.csv` added to a path that lacks it. Only that table: when `cut` is true the file holds only part of the object. It needs a path and opens no dialog. In a macro it needs the macro's permission, like the other commands that write a file; scripts can't write files",
            has_doc,
            save_csv
        ),
        cmd!(
            noundo "python.run",
            "Run Python Cell",
            ["Python", "Run"],
            None,
            "{cell?, sheet?} runs a Python cell (default: the active cell) now, whatever the calculation mode. Cells share variables, so the cells after it run too, or under manual calculation are marked stale. A cell that stopped the worker runs again only this way. Returns {cell, queued}. An error when Python doesn't run here, with the reason; it opens no dialog",
            has_doc,
            run
        ),
        cmd!(
            noundo "python.runAll",
            "Run All Python Cells",
            ["Python", "Run"],
            None,
            "runs every Python cell of the workbook in run order, whatever the calculation mode, in the variables Python has now. Returns {queued}. An error when Python doesn't run here, with the reason",
            has_doc,
            run_all
        ),
        cmd!(
            noundo "python.stop",
            "Stop Python",
            ["Python", "Run"],
            None,
            "stops the workbook's running cell by stopping its worker, which clears the variables: that cell shows #PYTHON! with the diagnostic state.stopped, and the cells waiting keep what they kept, marked stale. Returns {stopped}, false when nothing was running or waiting",
            has_doc,
            stop
        ),
        cmd!(
            noundo "python.reset",
            "Reset Python",
            ["Formulas", "Python"],
            None,
            "starts the workbook's Python again with fresh variables and runs every cell, whatever the calculation mode. Returns {queued}. An error when Python doesn't run here, with the reason",
            has_doc,
            reset
        ),
        cmd!(
            "python.initialization",
            "Initialization",
            ["Formulas", "Python"],
            None,
            "{code?} the code that runs before the workbook's Python cells, in the variables they share, at most 65,536 characters. With `code` it sets it, as one undo step (an empty text clears it), refused while the workbook's structure is protected; every Python cell then runs again from the top, as it does when an undo brings other code back, in a fresh worker. Without it nothing changes. Returns {code, userModified, fits, diagnostic?}: `userModified` is true for code a person gave, and false for what the application that wrote the file put there, whose failing imports don't stop the cells (they are reported as init.skipped); `fits` is false for code from a file cut at the limit, which doesn't run until it is shortened; `diagnostic` is the init.failed every Python cell shows. It opens no dialog",
            has_doc,
            initialization
        ),
        cmd!(
            noundo "python.calculation",
            "Python Calculation",
            ["Python", "Run"],
            None,
            "{mode?: automatic|manual} sets when the workbook's Python cells run: as soon as they are due, or under manual, marked stale until python.run or python.runAll. The setting stays with the open workbook and isn't saved. Without `mode` it reports the current one. Returns {mode}",
            has_doc,
            calculation
        ),
        cmd!(
            noundo "python.refreshEnvironment",
            "Check Python Again",
            ["Python", "Environment"],
            None,
            "checks again whether Python can run here, in the background, when the app has turned Python on: python.environment gives the answer. It can't turn Python on or choose an interpreter. Returns the report as python.environment does",
            always,
            refresh_environment
        ),
        cmd!(
            query "python.environment",
            "Python Environment",
            [],
            None,
            "→ {state: off|notSetUp|ready|running|blocked, diagnostic?, initialization?, sandbox, uv?, interpreter? {path, runAs, version, kind, siteDirs, readPaths}, packages: [{name, version}]}, with `initialization` the init.failed of the active workbook, whose cells can't run while Python itself is ready. Read-only",
            always,
            environment
        ),
        cmd!(
            query "python.diagnostics",
            "Python Diagnostics",
            [],
            None,
            "→ what is wrong or worth saying about Python in this workbook, as a list of {kind, severity, scope, message, action, detail, line?, cell?, install?}, with `cell` as Sheet!A1 where it is about one cell, and `install`, for packages.missing, the name to install. Read-only",
            has_doc,
            diagnostics
        ),
        cmd!(
            query "python.runLog",
            "Python Run Log",
            [],
            None,
            "→ the workbook's last 200 Python runs, oldest first, as a list of {sheet, cell, ms, stdout, stdoutCut, diagnostic?}, with printed output cut to 64 KiB. Read-only",
            has_doc,
            run_log
        ),
        cmd!(
            noundo "python.clearRunLog",
            "Clear Python Run Log",
            ["Python", "Diagnostics"],
            None,
            "forgets the workbook's Python runs. It changes nothing else: the cells keep what they show. Returns {cleared}, how many runs there were",
            has_doc,
            clear_run_log
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
    s.python_after_edit();
    let d = s.doc()?;
    let shown = d.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": crate::cmd::inspect::value_json(&shown)}))
}

/// A table header is the column's name, never a formula or code (as in Excel): the python.*
/// commands leave a cell in a table's header row alone.
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
    // Unchanged code with the same return type keeps its result (and its value); new code, or a
    // new return type, has none until it runs: a result of the other type would show a value and
    // preview that don't belong to the cell. A picture with no choice is both, so any choice keeps it.
    // The formats its results wrote stay its to take back, whatever the new code gives, and what a
    // text does with its lines is the cell's choice.
    if let Some(o) = &old {
        py.dates.written = o.dates.written.clone();
        py.spill_lines = o.spill_lines;
    }
    let keep = old.filter(|o| o.code == py.code && (o.object == py.object || (!o.output_chosen && o.image().is_some())));
    let value = match &keep {
        Some(o) => {
            py.output = o.output.clone();
            py.dates.result = o.dates.result.clone();
            py.stale = o.stale;
            py.failed = o.failed;
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
fn set_output_choice(s: &mut Session, sheet: usize, at: CellRef, object: bool, spill_lines: bool) -> Result<()> {
    refuse_table_header(s, sheet, at)?;
    if let Some(m) = s.locked_cell_message(sheet, at) {
        return Err(EngineError::Other(m));
    }
    edit(s, |cx| {
        let Some(py) = cx.sheet_mut(sheet)?.cells.get_mut(at).and_then(|c| c.py.as_mut()) else { return Ok(()) };
        let py = Arc::make_mut(py);
        py.object = object;
        py.output_chosen = true;
        py.spill_lines = spill_lines;
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
    let lines = lines_mode(p)?;
    let requested = output_mode("python.output", p, "mode")?;
    let current = old.shows_object();
    let object = requested.unwrap_or(if lines.is_some() { current } else { !current });
    let mode = |object: bool| if object { "object" } else { "value" };
    let spill_lines = lines.unwrap_or(old.spill_lines);
    let chooses = requested.is_some() || lines.is_none();
    if object != old.object && object != current {
        // One edit, so one undo step, when `lines` changes too.
        let py = PyCode { code: old.code.clone(), object, output_chosen: chooses || old.output_chosen, ..PyCode::default() };
        store(s, "python.output", sheet, at, py, Some(PyCode { spill_lines, ..old }))?;
        s.python_after_edit();
    } else if chooses && (object != old.object || !old.output_chosen) {
        set_output_choice(s, sheet, at, object, spill_lines)?;
        s.python_after_edit();
    } else if spill_lines != old.spill_lines {
        set_spill_lines(s, sheet, at, spill_lines)?;
        s.python_after_edit();
    }
    let d = s.doc()?;
    let shown = d.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({
        "cell": at.a1(),
        "output": mode(object),
        "lines": if spill_lines { "rows" } else { "cell" },
        "value": crate::cmd::inspect::value_json(&shown),
    }))
}

fn lines_mode(p: &Json) -> Result<Option<bool>> {
    match p.get("lines") {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(m)) if m.eq_ignore_ascii_case("cell") => Ok(Some(false)),
        Some(Json::String(m)) if m.eq_ignore_ascii_case("rows") => Ok(Some(true)),
        Some(_) => Err(bad("python.output", "`lines` is `cell` or `rows`")),
    }
}

/// What a text result does with its line breaks. The result stays, so nothing runs: the calculation
/// lays it out again, and what reads the cells it spills over is recalculated. A locked cell on a
/// protected sheet is refused here, as the option isn't what the edit itself compares.
fn set_spill_lines(s: &mut Session, sheet: usize, at: CellRef, on: bool) -> Result<()> {
    if let Some(m) = s.locked_cell_message(sheet, at) {
        return Err(EngineError::Other(m));
    }
    edit(s, |cx| {
        let Some(py) = cx.sheet_mut(sheet)?.cells.get_mut(at).and_then(|c| c.py.as_mut()) else { return Ok(()) };
        Arc::make_mut(py).spill_lines = on;
        cx.touch(sheet, at);
        Ok(())
    })
}

/// The picture a Python cell keeps from its last run.
fn kept_image(s: &Session, sheet: usize, at: CellRef) -> Result<gridcraft_model::PyImage> {
    let Some(py) = existing(s, sheet, at)? else {
        return Err(EngineError::Other(format!("{} isn't a Python cell, so it has no picture", at.a1())));
    };
    py.image().cloned().ok_or_else(|| {
        EngineError::Other(format!(
            "{} has no picture: only a Python cell whose last run gave one has it. A file keeps it, and a cell waiting to run has none.",
            at.a1()
        ))
    })
}

/// `python.image`: the picture, for agents and other tools.
fn image(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let i = kept_image(s, sheet, at)?;
    Ok(json!({
        "cell": at.a1(),
        "mime": i.picture.mime,
        "width": i.width,
        "height": i.height,
        "density": i.density,
        "bytes": i.picture.data.len(),
        "base64": crate::io::base64_encode(&i.picture.data),
    }))
}

/// `python.saveImage`: the picture as a file. Refused inside a script, or a macro step without the
/// permission, by [`crate::io::write_file`].
fn save_image(s: &mut Session, p: &Json) -> Result<Json> {
    let path = match p.get("path") {
        Some(Json::String(path)) if !path.trim().is_empty() => path.as_str(),
        _ => return Err(bad("python.saveImage", "missing `path`")),
    };
    let (sheet, at) = target_cell(s, p)?;
    let i = kept_image(s, sheet, at)?;
    let (ending, endings): (&str, &[&str]) = if i.picture.mime == "image/jpeg" { (".jpg", &["jpg", "jpeg"]) } else { (".png", &["png"]) };
    let has_ending = std::path::Path::new(path).extension().is_some_and(|e| endings.iter().any(|x| e.eq_ignore_ascii_case(x)));
    let target = if has_ending { path.to_string() } else { format!("{path}{ending}") };
    crate::io::write_file(&target, &i.picture.data)?;
    Ok(json!({"cell": at.a1(), "path": target, "bytes": i.picture.data.len(), "mime": i.picture.mime}))
}

/// `python.displayOverCells`: the picture, floating, and linked to its cell.
fn display_over_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let image = kept_image(s, sheet, at)?;
    let d = s.doc()?;
    let target = d.wb.active_sheet();
    // Sheet protection locks drawing objects, as for `insert.picture`.
    super::object_clipboard::refuse_when_protected(s, target)?;
    let place = cell_param(s, p, "at")?.unwrap_or_else(|| CellRef::new(at.row, at.col.saturating_add(1).min(gridcraft_core::MAX_COLS - 1)));
    let id = d.wb.next_object_id();
    let picture = crate::python::links::new_picture(&d.wb, sheet, at, &image, id, place).ok_or(EngineError::NoDocument)?;
    let source = picture.python_source.clone();
    edit(s, |cx| {
        cx.sheet_mut(target)?.images.push(picture);
        Ok(json!({"image": id, "cell": at.a1(), "source": source}))
    })
}

/// `python.createReference`: another picture of the same cell, near the first.
fn create_reference(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(id) = u32_param(p, "id") else { return Err(bad("python.createReference", "missing `id`")) };
    super::object_clipboard::refuse_when_protected(s, s.doc()?.wb.active_sheet())?;
    let d = s.doc()?;
    let target = d.wb.active_sheet();
    let sh = d.wb.sheet(target).ok_or(EngineError::NoDocument)?;
    let Some(first) = sh.images.iter().find(|o| o.id == id) else { return Err(bad("python.createReference", "no such picture")) };
    if first.python_source.is_none() {
        return Err(bad("python.createReference", "that picture isn't linked to a Python cell"));
    }
    let mut picture = first.clone();
    picture.id = d.wb.next_object_id();
    picture.anchor.dx += 16.0;
    picture.anchor.dy += 16.0;
    let new = picture.id;
    edit(s, |cx| {
        cx.sheet_mut(target)?.images.push(picture);
        Ok(json!({"image": new}))
    })
}

/// The table a Python Object cell keeps from its last run.
fn kept_table(s: &Session, sheet: usize, at: CellRef) -> Result<Arc<PyTable>> {
    let Some(py) = existing(s, sheet, at)? else {
        return Err(EngineError::Other(format!("{} isn't a Python cell, so it has no table", at.a1())));
    };
    let table = py.output.and_then(|o| o.table).filter(|_| py.object);
    table.ok_or_else(|| {
        EngineError::Other(format!(
            "{} has no table: only a Python Object cell holding a DataFrame, a Series, or a 2-D list or array has one, once it has run. A file doesn't keep it, so a cell opened from a file needs to run first.",
            at.a1()
        ))
    })
}

/// What a table is, without its cells.
pub(crate) fn table_summary(t: &PyTable) -> Json {
    json!({
        "rows": t.rows,
        "cols": t.cols,
        "totalRows": t.total_rows,
        "totalCols": t.total_cols,
        "rowLabels": t.row_labels,
        "textCut": t.text_cut,
        "cut": t.cut(),
    })
}

fn table(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let t = kept_table(s, sheet, at)?;
    let mut out = table_summary(&t);
    let Some(o) = out.as_object_mut() else { return Ok(out) };
    o.insert("cell".into(), json!(at.a1()));
    match p.get("format") {
        None | Some(Json::Null) => json_cells(&t, o),
        Some(Json::String(f)) if f.eq_ignore_ascii_case("json") => json_cells(&t, o),
        Some(Json::String(f)) if f.eq_ignore_ascii_case("csv") => {
            o.insert("text".into(), json!(t.to_csv()));
        }
        Some(Json::String(f)) if f.eq_ignore_ascii_case("markdown") => {
            o.insert("text".into(), json!(t.to_markdown()));
        }
        Some(_) => return Err(bad("python.table", "`format` is `json`, `csv` or `markdown`")),
    }
    Ok(out)
}

fn json_cells(t: &PyTable, o: &mut serde_json::Map<String, Json>) {
    o.insert("columns".into(), json!(t.columns));
    o.insert("data".into(), t.data.iter().map(crate::cmd::inspect::value_json).collect());
}

/// `python.saveCsv`: the table as a file. Refused inside a script, or a macro step without the
/// permission, by [`crate::io::write_file`].
fn save_csv(s: &mut Session, p: &Json) -> Result<Json> {
    let path = match p.get("path") {
        Some(Json::String(path)) if !path.trim().is_empty() => path.as_str(),
        _ => return Err(bad("python.saveCsv", "missing `path`")),
    };
    let (sheet, at) = target_cell(s, p)?;
    let t = kept_table(s, sheet, at)?;
    if t.is_empty() {
        return Err(EngineError::Other(format!("{}'s table has no columns, so there is nothing to save", at.a1())));
    }
    let has_csv = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("csv"));
    let target = if has_csv { path.to_string() } else { format!("{path}.csv") };
    let text = t.to_csv();
    crate::io::write_file(&target, text.as_bytes())?;
    let mut out = table_summary(&t);
    if let Some(o) = out.as_object_mut() {
        o.insert("cell".into(), json!(at.a1()));
        o.insert("path".into(), json!(target));
        o.insert("bytes".into(), json!(text.len()));
    }
    Ok(out)
}

/// Why `python.clear` would refuse the cell `at` on `sheet`, or `None` when it would convert it:
/// a cell without Python code, a table header, or a locked cell on a protected sheet (the cell, or
/// a cell its result spills over). Front ends ask before offering Convert to Normal Cell. The
/// message never holds the code.
pub fn clear_refusal(s: &Session, sheet: usize, at: CellRef) -> Option<String> {
    let Some(sh) = s.active().and_then(|d| d.wb.sheet(sheet)) else {
        return Some("There is no such sheet.".into());
    };
    if sh.cell(at).is_none_or(|c| c.py.is_none()) {
        return Some(format!("{} isn't a Python cell, so there is no Python code to remove", at.a1()));
    }
    if let Err(e) = refuse_table_header(s, sheet, at) {
        return Some(e.to_string());
    }
    if let Some(m) = s.locked_cell_message(sheet, at) {
        return Some(m);
    }
    let r = sh.spill_ranges.get(&at)?;
    sh.spill.range(r.start..=r.end).filter(|(c, v)| **c != at && r.contains(**c) && !v.is_empty()).find_map(|(c, _)| s.locked_cell_message(sheet, *c))
}

/// Whether `v` is an error only a Python cell shows: not a result, and not something an ordinary
/// cell can hold (a file keeps it as `#VALUE!`).
fn python_only_error(v: &Value) -> bool {
    matches!(v, Value::Error(CellError::Python | CellError::Busy | CellError::Timeout | CellError::Blocked | CellError::Connect))
}

/// `python.clear` (Convert to Normal Cell): the code goes and nothing shown is lost. The cell keeps
/// its value and format, and the values its result spilled become plain values
/// ([`gridcraft_model::Sheet::settle_spill`]), except a Python-only error, which leaves its cell
/// empty (format kept); every cell that changed is touched, so what reads
/// them is recalculated. A locked cell on a protected sheet (the cell, or a cell its result
/// spills over) is refused by the edit itself, as with `python.set`. The result never holds the
/// code, which sheet protection may hide.
fn clear(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    if let Some(why) = clear_refusal(s, sheet, at) {
        return Err(EngineError::Other(why));
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        // The cells the result spills over (their values, not the whole range: a range a file
        // claims can be far larger than what it holds).
        let spilled: Vec<CellRef> = match sh.spill_ranges.get(&at) {
            Some(r) => sh.spill.range(r.start..=r.end).map(|(c, _)| *c).filter(|c| *c != at && r.contains(*c)).collect(),
            None => Vec::new(),
        };
        // What it shows is kept: a picture drawn in the cell stays there as an ordinary cell picture.
        let in_cell = sh.cell(at).and_then(|c| c.py.as_deref()).and_then(|py| py.image_in_cell().cloned());
        if let Some(cell) = sh.cells.get_mut(at) {
            cell.py = None;
        }
        if let Some(picture) = in_cell {
            sh.set_picture(at, picture);
        }
        sh.settle_spill(at);
        for c in std::iter::once(at).chain(spilled.iter().copied()) {
            if let Some(cell) = sh.cells.get_mut(c).filter(|x| x.formula.is_none() && x.py.is_none() && python_only_error(&x.value)) {
                cell.value = Value::Empty;
            }
        }
        cx.touch(sheet, at);
        for c in spilled {
            cx.touch(sheet, c);
        }
        Ok(())
    })?;
    let d = s.doc()?;
    let shown = d.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": crate::cmd::inspect::value_json(&shown)}))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, at) = target_cell(s, p)?;
    let queued = s.python_run_cell(sheet, at)?;
    Ok(json!({"cell": at.a1(), "queued": queued}))
}

fn run_all(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!({"queued": s.python_run_all(false)?}))
}

fn reset(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!({"queued": s.python_run_all(true)?}))
}

fn stop(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!({"stopped": s.python_stop()}))
}

fn initialization(s: &mut Session, p: &Json) -> Result<Json> {
    match p.get("code") {
        None | Some(Json::Null) => {}
        Some(Json::String(code)) => {
            if s.doc()?.wb.protected_structure {
                return Err(EngineError::Other("The workbook is protected, so its initialization code can't be changed.".into()));
            }
            let code = normalize_code(code);
            if !init_fits(&code) {
                return Err(bad("python.initialization", format!("the code is longer than the limit of {MAX_INIT_CHARS} characters")));
            }
            s.python_set_initialization(&code)?;
            s.python_after_edit();
        }
        Some(_) => return Err(bad("python.initialization", "`code` must be text")),
    }
    let env = s.doc()?.wb.python_env.clone();
    let code = env.as_ref().map_or("", |e| e.code.as_str());
    Ok(json!({
        "code": code,
        "userModified": env.as_ref().is_some_and(|e| e.user_modified),
        "fits": init_fits(code),
        "diagnostic": s.python_init_failure().map(crate::python::diagnostic_json),
    }))
}

fn calculation(s: &mut Session, p: &Json) -> Result<Json> {
    let mode = match p.get("mode") {
        None | Some(Json::Null) => None,
        Some(Json::String(m)) if m.eq_ignore_ascii_case("automatic") => Some(PythonCalculation::Automatic),
        Some(Json::String(m)) if m.eq_ignore_ascii_case("manual") => Some(PythonCalculation::Manual),
        Some(_) => return Err(bad("python.calculation", "`mode` is `automatic` or `manual`")),
    };
    if let Some(mode) = mode {
        s.python_set_calculation(mode);
    }
    Ok(json!({"mode": s.python_calculation()}))
}

/// The report, with what is about the active workbook's environment.
fn environment_report(s: &Session) -> Json {
    let mut report = environment_json(&s.python_environment());
    if let Some(o) = report.as_object_mut() {
        o.insert("initialization".into(), json!(s.python_init_failure().map(crate::python::diagnostic_json)));
    }
    report
}

fn refresh_environment(s: &mut Session, _: &Json) -> Result<Json> {
    s.python_refresh_environment();
    Ok(environment_report(s))
}

fn environment(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(environment_report(s))
}

fn diagnostics(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(s.python_diagnostics().iter().map(|e| diagnostic_entry_json(&d.wb, e)).collect()))
}

fn run_log(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(Json::Array(s.python_run_log().map(run_json).collect()))
}

fn clear_run_log(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!({"cleared": s.python_clear_run_log()}))
}
