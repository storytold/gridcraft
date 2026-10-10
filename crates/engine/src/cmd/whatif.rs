//! What-If Analysis (Data › Forecast): Goal Seek, Scenario Manager, Data Tables; and the Data
//! Tools that work on whole lists: Advanced Filter and Consolidate.
//!
//! Every what-if evaluation is a *trial*: the workbook is cloned (cheap — sheets and cell bands
//! are copy-on-write), the input cells are overwritten, and only their dependents are
//! recalculated with the document's dependency graph. The document itself only changes when a
//! command writes its final answer (one undo step).

use std::collections::{HashMap, HashSet};

use gridcraft_calc::Calc;
use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{CalcMode, Cell, DefinedName, Sheet, StyleId, Workbook};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.goalSeek",
            "Goal Seek…",
            ["Data", "Forecast", "What-If Analysis"],
            None,
            "{set: \"B5\" (formula cell), to: number, changing: \"B2\" (constant cell)} → {value, result, iterations, converged}. Secant search with bisection once the answer is bracketed; stops within Maximum Change (0.001) or after Maximum Iterations (100) from File › Options › Formulas. Writes the found value into `changing`.",
            has_doc,
            goal_seek
        ),
        cmd!(
            query "data.scenarioManager",
            "Scenario Manager…",
            ["Data", "Forecast", "What-If Analysis"],
            None,
            "{sheet?} → {scenarios: [{name, sheet, changing, values, comment}]} (scenarios of the active sheet; `sheet: \"*\"` lists all)",
            has_doc,
            scenario_list
        ),
        cmd!(
            "data.scenarioAdd",
            "Add Scenario",
            [],
            None,
            "{name, changing: \"B2:B4\" or \"B2,B4\" (≤32 cells, active sheet), values?: [..] (default: the cells' current values), comment?, replace?: bool}",
            has_doc,
            scenario_add
        ),
        cmd!("data.scenarioShow", "Show Scenario", [], None, "{name} writes the scenario's values into its changing cells", has_doc, scenario_show),
        cmd!("data.scenarioDelete", "Delete Scenario", [], None, "{name}", has_doc, scenario_delete),
        cmd!(
            "data.scenarioSummary",
            "Scenario Summary",
            [],
            None,
            "{resultCells?: \"B10\" or \"B10:B12,D4\"} inserts a \"Scenario Summary\" sheet: changing cells and result cells for the current values and each scenario",
            has_doc,
            scenario_summary
        ),
        cmd!(
            "data.dataTable",
            "Data Table…",
            ["Data", "Forecast", "What-If Analysis"],
            None,
            "{range: \"D2:E10\", rowInput?: \"B1\", columnInput?: \"B2\"} one-variable (values down the first column with columnInput, or across the top row with rowInput; formulas in the other edge) or two-variable (formula in the top-left corner). Computes every result by substitution and writes static values (a snapshot, not a live {=TABLE()}); ≤100,000 cells.",
            has_doc,
            data_table
        ),
        cmd!(
            "data.advancedFilter",
            "Advanced…",
            ["Data", "Sort & Filter"],
            None,
            "{range? (list with headers; default current region), criteria: \"H1:I3\" (header row of field names; rows OR'ed, columns AND'ed; \">100\", \"=abc\", \"a*\", \"<>\"; blank-headed formula cells are computed criteria for the first data row), copyTo?: \"K1\" (else filter in place by hiding rows), unique?: bool}",
            has_doc,
            advanced_filter
        ),
        cmd!(
            "data.consolidate",
            "Consolidate…",
            ["Data", "Data Tools"],
            None,
            "{sources: [\"Sheet2!A1:C10\", ..], destination?: \"A1\" (default active cell), function?: sum|count|countNums|average|max|min|product, topRow?: bool, leftColumn?: bool (consolidate by labels; else by position)}",
            has_doc,
            consolidate
        ),
    ]
}

// ---------------------------------------------------------------- parameters

/// Resolves an optional sheet name (else `default`).
fn sheet_named(wb: &Workbook, name: Option<String>, default: usize, cmd: &str) -> Result<usize> {
    match name {
        Some(n) => wb.sheet_index(&n).ok_or_else(|| bad(cmd, format!("no sheet named `{n}`"))),
        None => Ok(default),
    }
}

/// A `"B5"` / `"Sheet2!B5"` parameter.
fn cell_arg(s: &Session, p: &Json, key: &str, cmd: &str) -> Result<(usize, CellRef)> {
    let d = s.doc()?;
    let t = str_param(p, key).ok_or_else(|| bad(cmd, format!("missing `{key}`")))?;
    let (sh, body) = split_sheet(t);
    let si = sheet_named(&d.wb, sh, d.wb.active_sheet, cmd)?;
    let c = CellRef::parse(body.trim()).ok_or_else(|| bad(cmd, format!("`{key}` is not a cell: `{t}`")))?;
    Ok((si, c))
}

/// A range text (`"A1:C9"`, `"Sheet2!A:C"`) resolved against the workbook. Whole rows/columns
/// are trimmed to the used range.
fn parse_range(wb: &Workbook, t: &str, cmd: &str) -> Result<(usize, RangeRef)> {
    let (sh, body) = split_sheet(t);
    let si = sheet_named(wb, sh, wb.active_sheet, cmd)?;
    let r = RangeRef::parse(body.trim()).ok_or_else(|| bad(cmd, format!("not a range: `{t}`")))?;
    Ok((si, trim_to_used(wb, si, r)))
}

fn trim_to_used(wb: &Workbook, si: usize, r: RangeRef) -> RangeRef {
    if !(r.is_full_cols() || r.is_full_rows()) {
        return r;
    }
    match wb.sheet(si).and_then(|s| s.used_range()) {
        Some(u) => r.intersection(&RangeRef::new(CellRef::new(0, 0), u.end)).unwrap_or(RangeRef::cell(r.start)),
        None => RangeRef::cell(r.start),
    }
}

fn range_arg(s: &Session, p: &Json, key: &str, cmd: &str) -> Result<(usize, RangeRef)> {
    let d = s.doc()?;
    let t = str_param(p, key).ok_or_else(|| bad(cmd, format!("missing `{key}`")))?;
    parse_range(&d.wb, t, cmd)
}

/// Cells of `"B2:B4,D1"` (row-major per area, duplicates removed), at most `cap`.
fn cell_list(wb: &Workbook, t: &str, cmd: &str, cap: usize) -> Result<(usize, Vec<CellRef>)> {
    let mut sheet = None;
    let mut out: Vec<CellRef> = Vec::new();
    for part in t.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let (si, r) = parse_range(wb, part, cmd)?;
        if *sheet.get_or_insert(si) != si {
            return Err(bad(cmd, "all cells must be on one sheet"));
        }
        if r.count() > cap as u64 {
            return Err(bad(cmd, format!("too many cells (at most {cap})")));
        }
        for c in r.iter() {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        if out.len() > cap {
            return Err(bad(cmd, format!("too many cells (at most {cap})")));
        }
    }
    Ok((sheet.unwrap_or(wb.active_sheet), out))
}

/// What a JSON value typed into a cell is, as a constant.
fn json_value(v: &Json, wb: &Workbook) -> Value {
    match v {
        Json::Null => Value::Empty,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n.as_f64().map(Value::number).unwrap_or_default(),
        Json::String(s) if s.is_empty() => Value::Empty,
        Json::String(s) => gridcraft_core::parse::parse_input(s, wb.date_system).value,
        other => Value::text(other.to_string()),
    }
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Empty => Json::Null,
        Value::Number(n) => json!(n),
        Value::Bool(b) => json!(b),
        Value::Text(t) => json!(t.as_ref()),
        Value::Error(e) => json!(e.as_str()),
        Value::Array(a) => value_json(&a.data.first().cloned().unwrap_or_default()),
    }
}

// ---------------------------------------------------------------- trials

/// Evaluates a workbook with some input cells replaced, without touching the document.
pub(crate) struct Trial {
    base: Workbook,
    calc: Calc,
}

impl Trial {
    pub(crate) fn new(d: &DocState) -> Trial {
        let mut base = (*d.wb).clone();
        // What-if analysis always recalculates, even in manual mode.
        base.calc.mode = CalcMode::Automatic;
        Trial { base, calc: d.calc.clone() }
    }

    /// Sets `sets` to constants, recalculates their dependents and reads `reads`.
    pub(crate) fn run(&mut self, sets: &[(usize, CellRef, Value)], reads: &[(usize, CellRef)]) -> Vec<Value> {
        let mut wb = self.base.clone();
        let mut keys = Vec::with_capacity(sets.len());
        for (si, c, v) in sets {
            if let Some(sh) = wb.sheet_mut(*si) {
                let style = sh.style_id(*c);
                sh.set_cell(*c, Cell { value: v.clone(), formula: None, style });
                keys.push((*si, *c));
            }
        }
        self.calc.cells_changed(&mut wb, &keys);
        reads.iter().map(|(si, c)| wb.sheet(*si).map(|s| s.value(*c)).unwrap_or_default()).collect()
    }
}

// ---------------------------------------------------------------- goal seek

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Seek {
    pub x: f64,
    pub fx: f64,
    pub iterations: u32,
    pub converged: bool,
}

/// Finds `x` with `|f(x)| <= tol`: secant steps (with growing steps while nothing is
/// bracketed), switching to secant-or-bisection inside a sign-change bracket. Returns the best
/// point seen. `f` returns NaN where the model errors.
pub(crate) fn seek(x0: f64, f: &mut dyn FnMut(f64) -> f64, max_iter: u32, tol: f64) -> Seek {
    let tol = if tol.is_finite() && tol > 0.0 { tol } else { 0.001 };
    let max_iter = max_iter.clamp(1, 10_000);
    let x0 = if x0.is_finite() { x0 } else { 0.0 };
    let f0 = f(x0);
    let mut best = (x0, f0);
    let consider = |best: &mut (f64, f64), x: f64, fx: f64| {
        if fx.is_finite() && (!best.1.is_finite() || fx.abs() < best.1.abs()) {
            *best = (x, fx);
        }
    };
    let done = |best: &(f64, f64), it: u32| Seek { x: best.0, fx: best.1, iterations: it, converged: best.1.is_finite() && best.1.abs() <= tol };
    if f0.is_finite() && f0.abs() <= tol {
        return done(&best, 0);
    }
    let step0 = if x0 == 0.0 { 0.01 } else { x0.abs() * 0.01 };
    let (mut xa, mut fa) = (x0, f0);
    let mut xb = x0 + step0;
    let mut fb = f(xb);
    let mut it = 1u32;
    consider(&mut best, xb, fb);
    // (lo, f(lo), hi, f(hi)) with a sign change.
    let mut bracket: Option<(f64, f64, f64, f64)> = None;
    let mut slow = 0u32;
    while it < max_iter {
        if fb.is_finite() && fb.abs() <= tol {
            break;
        }
        if bracket.is_none() && fa.is_finite() && fb.is_finite() && (fa < 0.0) != (fb < 0.0) {
            bracket = Some((xa, fa, xb, fb));
        }
        let secant = if fa.is_finite() && fb.is_finite() && fb != fa { xb - fb * (xb - xa) / (fb - fa) } else { f64::NAN };
        let next = match bracket {
            Some((lo, _, hi, _)) => {
                let (a, b) = (lo.min(hi), lo.max(hi));
                if slow >= 2 || !(secant > a && secant < b) { (lo + hi) / 2.0 } else { secant }
            }
            None => {
                let span = (xb - xa).abs().max(f64::MIN_POSITIVE);
                if !fb.is_finite() {
                    // The model errors here: step back towards the last good point.
                    (xa + xb) / 2.0
                } else if !secant.is_finite() {
                    xb + 2.0 * (xb - xa)
                } else if (secant - xb).abs() > 100.0 * span + 1.0 {
                    xb + (secant - xb).signum() * (100.0 * span + 1.0)
                } else {
                    secant
                }
            }
        };
        if !next.is_finite() {
            break;
        }
        let fnext = f(next);
        it += 1;
        consider(&mut best, next, fnext);
        if let Some((lo, flo, hi, fhi)) = bracket {
            let width = (hi - lo).abs();
            if fnext.is_finite() {
                bracket = Some(if (fnext < 0.0) == (flo < 0.0) { (next, fnext, hi, fhi) } else { (lo, flo, next, fnext) });
            } else {
                // An error inside the bracket: shrink towards the better end.
                bracket = Some(if flo.abs() < fhi.abs() { (lo, flo, next, fhi) } else { (next, flo, hi, fhi) });
            }
            if let Some((l, _, h, _)) = bracket {
                let nw = (h - l).abs();
                slow = if nw > width * 0.5 { slow + 1 } else { 0 };
                if nw <= f64::EPSILON * (1.0 + l.abs().max(h.abs())) {
                    break;
                }
            }
        }
        xa = xb;
        fa = fb;
        xb = next;
        fb = fnext;
    }
    done(&best, it)
}

fn goal_seek(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.goalSeek";
    let (ssi, set) = cell_arg(s, p, "set", CMD)?;
    let (csi, chg) = cell_arg(s, p, "changing", CMD)?;
    let target = f64_param(p, "to")
        .or_else(|| str_param(p, "to").and_then(gridcraft_core::parse::parse_number_text))
        .filter(|v| v.is_finite())
        .ok_or_else(|| bad(CMD, "`to` must be a number"))?;
    let d = s.doc()?;
    let has_formula = |si: usize, c: CellRef| d.wb.sheet(si).and_then(|sh| sh.cell(c)).is_some_and(|c| c.formula.is_some());
    if !has_formula(ssi, set) {
        return Err(EngineError::Other("Cell must contain a formula.".into()));
    }
    if has_formula(csi, chg) {
        return Err(EngineError::Other("Cell must contain a value.".into()));
    }
    let x0 = d.wb.sheet(csi).map(|sh| sh.value(chg)).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let (max_iter, tol) = (d.wb.calc.max_iterations, d.wb.calc.max_change);
    let mut trial = Trial::new(d);
    let mut f = |x: f64| -> f64 {
        trial.run(&[(csi, chg, Value::number(x))], &[(ssi, set)]).first().and_then(Value::as_f64).map(|v| v - target).unwrap_or(f64::NAN)
    };
    let r = seek(x0, &mut f, max_iter, tol);
    edit(s, |cx| {
        let sh = cx.sheet_mut(csi)?;
        let style = sh.style_id(chg);
        sh.set_cell(chg, Cell { value: Value::number(r.x), formula: None, style });
        cx.touch(csi, chg);
        Ok(())
    })?;
    let result = s.doc()?.wb.sheet(ssi).map(|sh| sh.value(set)).unwrap_or_default();
    Ok(json!({"value": r.x, "result": value_json(&result), "iterations": r.iterations, "converged": r.converged}))
}

// ---------------------------------------------------------------- scenarios

/// Hidden workbook-scope name holding the scenarios as a quoted JSON string.
pub(crate) const SCENARIO_NAME: &str = "_sc_scenarios";
const MAX_SCENARIO_CELLS: usize = 32;
const MAX_SCENARIOS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Scenario {
    pub name: String,
    pub sheet: String,
    pub changing: String,
    pub values: Vec<Json>,
    #[serde(default)]
    pub comment: String,
}

pub(crate) fn load_scenarios(wb: &Workbook) -> Vec<Scenario> {
    let Some(n) = wb.names.iter().find(|n| n.scope.is_none() && n.name.eq_ignore_ascii_case(SCENARIO_NAME)) else { return vec![] };
    serde_json::from_str(&string_literals(&n.formula)).unwrap_or_default()
}

/// Concatenates the string literals of a `"a"&"b""c"` formula.
fn string_literals(f: &str) -> String {
    let mut out = String::new();
    let mut it = f.chars().peekable();
    let mut in_str = false;
    while let Some(c) = it.next() {
        if !in_str {
            if c == '"' {
                in_str = true;
            }
            continue;
        }
        if c == '"' {
            if it.peek() == Some(&'"') {
                it.next();
                out.push('"');
            } else {
                in_str = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn store_scenarios(wb: &mut Workbook, list: &[Scenario]) {
    wb.names.retain(|n| !(n.scope.is_none() && n.name.eq_ignore_ascii_case(SCENARIO_NAME)));
    if list.is_empty() {
        return;
    }
    let text = serde_json::to_string(list).unwrap_or_else(|_| "[]".into());
    wb.names.push(DefinedName {
        name: SCENARIO_NAME.into(),
        scope: None,
        // Excel caps string literals at 255 characters: store `"…"&"…"` chunks.
        formula: text
            .chars()
            .collect::<Vec<char>>()
            .chunks(250)
            .map(|c| format!("\"{}\"", c.iter().collect::<String>().replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join("&"),
        comment: "GridCraft Scenario Manager data".into(),
        hidden: true,
    });
}

fn scenario_json(sc: &Scenario) -> Json {
    json!({"name": sc.name, "sheet": sc.sheet, "changing": sc.changing, "values": sc.values, "comment": sc.comment})
}

fn scenario_list(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sheet = str_param(p, "sheet").map(str::to_string).unwrap_or_else(|| d.wb.active().map(|s| s.name.clone()).unwrap_or_default());
    let list: Vec<Json> =
        load_scenarios(&d.wb).iter().filter(|sc| sheet == "*" || sc.sheet.eq_ignore_ascii_case(&sheet)).map(scenario_json).collect();
    Ok(json!({"scenarios": list}))
}

fn scenario_add(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.scenarioAdd";
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(CMD, "missing `name`"))?.to_string();
    if name.chars().count() > 255 {
        return Err(bad(CMD, "the name is too long (255 characters max)"));
    }
    let changing = str_param(p, "changing").ok_or_else(|| bad(CMD, "missing `changing`"))?.to_string();
    let d = s.doc()?;
    let (si, cells) = cell_list(&d.wb, &changing, CMD, MAX_SCENARIO_CELLS)?;
    if cells.is_empty() {
        return Err(bad(CMD, "no changing cells"));
    }
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let values: Vec<Json> = match p.get("values").and_then(Json::as_array) {
        Some(a) => {
            if a.len() != cells.len() {
                return Err(bad(CMD, format!("{} values for {} changing cells", a.len(), cells.len())));
            }
            a.clone()
        }
        None => cells.iter().map(|c| value_json(&sh.value(*c))).collect(),
    };
    let changing = cells.iter().map(|c| c.a1()).collect::<Vec<_>>().join(",");
    let sc = Scenario { name: name.clone(), sheet: sh.name.clone(), changing, values, comment: str_param(p, "comment").unwrap_or("").to_string() };
    let replace = bool_param(p, "replace").unwrap_or(false);
    let mut list = load_scenarios(&d.wb);
    match list.iter().position(|x| x.name.eq_ignore_ascii_case(&name) && x.sheet.eq_ignore_ascii_case(&sc.sheet)) {
        Some(i) if replace => {
            if let Some(slot) = list.get_mut(i) {
                *slot = sc.clone();
            }
        }
        Some(_) => return Err(EngineError::Other(format!("A scenario named '{name}' already exists on this sheet."))),
        None => {
            if list.len() >= MAX_SCENARIOS {
                return Err(bad(CMD, "too many scenarios"));
            }
            list.push(sc.clone());
        }
    }
    edit(s, |cx| {
        store_scenarios(&mut cx.wb, &list);
        Ok(())
    })?;
    Ok(scenario_json(&sc))
}

fn find_scenario(wb: &Workbook, p: &Json, cmd: &str) -> Result<(Vec<Scenario>, usize)> {
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "missing `name`"))?;
    let list = load_scenarios(wb);
    let active = wb.active().map(|s| s.name.clone()).unwrap_or_default();
    let i = list
        .iter()
        .position(|x| x.name.eq_ignore_ascii_case(name) && x.sheet.eq_ignore_ascii_case(&active))
        .or_else(|| list.iter().position(|x| x.name.eq_ignore_ascii_case(name)))
        .ok_or_else(|| bad(cmd, format!("no scenario named `{name}`")))?;
    Ok((list, i))
}

fn scenario_show(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.scenarioShow";
    let d = s.doc()?;
    let (list, i) = find_scenario(&d.wb, p, CMD)?;
    let sc = list.get(i).cloned().ok_or_else(|| bad(CMD, "no such scenario"))?;
    let si = d.wb.sheet_index(&sc.sheet).ok_or_else(|| bad(CMD, format!("the scenario's sheet `{}` no longer exists", sc.sheet)))?;
    let (_, cells) = cell_list(&d.wb, &sc.changing, CMD, MAX_SCENARIO_CELLS)?;
    edit(s, |cx| {
        for (c, v) in cells.iter().zip(sc.values.iter()) {
            let input = super::edit::json_to_input(v);
            let cell = super::edit::input_to_cell(&input, si, *c, &mut cx.wb)?;
            let sh = cx.sheet_mut(si)?;
            match cell {
                Some(cell) => sh.set_cell(*c, cell),
                None => {
                    sh.remove_cell(*c);
                }
            }
            cx.touch(si, *c);
        }
        Ok(())
    })?;
    Ok(scenario_json(&sc))
}

fn scenario_delete(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (mut list, i) = find_scenario(&d.wb, p, "data.scenarioDelete")?;
    let removed = list.remove(i);
    edit(s, |cx| {
        store_scenarios(&mut cx.wb, &list);
        Ok(())
    })?;
    Ok(json!({"deleted": removed.name}))
}

fn scenario_summary(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.scenarioSummary";
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let scenarios: Vec<Scenario> = load_scenarios(&d.wb).into_iter().filter(|x| x.sheet.eq_ignore_ascii_case(&sh.name)).collect();
    if scenarios.is_empty() {
        return Err(EngineError::Other("There are no scenarios on this sheet.".into()));
    }
    let results: Vec<CellRef> = match str_param(p, "resultCells").map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            let (rsi, cells) = cell_list(&d.wb, t, CMD, 255)?;
            if rsi != si {
                return Err(bad(CMD, "result cells must be on the scenarios' sheet"));
            }
            cells
        }
        None => vec![],
    };
    // Changing cells of every scenario, first-seen order; per scenario its values.
    let mut changing: Vec<CellRef> = Vec::new();
    let mut per: Vec<HashMap<CellRef, Value>> = Vec::new();
    for sc in &scenarios {
        let (_, cells) = cell_list(&d.wb, &sc.changing, CMD, MAX_SCENARIO_CELLS)?;
        let mut m = HashMap::new();
        for (c, v) in cells.iter().zip(sc.values.iter()) {
            if !changing.contains(c) {
                changing.push(*c);
            }
            m.insert(*c, json_value(v, &d.wb));
        }
        per.push(m);
    }
    let current: Vec<Value> = changing.iter().map(|c| sh.value(*c)).collect();
    let current_results: Vec<Value> = results.iter().map(|c| sh.value(*c)).collect();
    let mut trial = Trial::new(d);
    let reads: Vec<(usize, CellRef)> = results.iter().map(|c| (si, *c)).collect();
    let scenario_results: Vec<Vec<Value>> = per
        .iter()
        .map(|m| {
            let sets: Vec<(usize, CellRef, Value)> = m.iter().map(|(c, v)| (si, *c, v.clone())).collect();
            trial.run(&sets, &reads)
        })
        .collect();
    let fmt_of = |c: CellRef| d.wb.styles.get(sh.style_id(c)).num_fmt.clone();
    let changing_fmts: Vec<_> = changing.iter().map(|c| fmt_of(*c)).collect();
    let result_fmts: Vec<_> = results.iter().map(|c| fmt_of(*c)).collect();
    let name = (1..)
        .map(|n| if n == 1 { "Scenario Summary".to_string() } else { format!("Scenario Summary {n}") })
        .find(|n| d.wb.sheet_index(n).is_none())
        .unwrap_or_else(|| "Scenario Summary".into());
    let names: Vec<String> = scenarios.iter().map(|x| x.name.clone()).collect();
    let new_index = edit(s, |cx| {
        let bold = cx.wb.styles.derive(StyleId::DEFAULT, |st| st.font.bold = true);
        let mut sheet = Sheet::new(name.clone());
        let put = |sheet: &mut Sheet, row: u32, col: u32, v: Value, style: StyleId| {
            sheet.set_cell(CellRef::new(row, col), Cell { value: v, formula: None, style });
        };
        put(&mut sheet, 1, 1, Value::text("Scenario Summary"), bold);
        put(&mut sheet, 2, 2, Value::text("Current Values:"), bold);
        for (k, n) in names.iter().enumerate() {
            put(&mut sheet, 2, 3 + k as u32, Value::text(n.as_str()), bold);
        }
        put(&mut sheet, 3, 1, Value::text("Changing Cells:"), bold);
        let mut row = 4u32;
        for (i, c) in changing.iter().enumerate() {
            let fmt = changing_fmts.get(i).cloned().unwrap_or_default();
            let st = cx.wb.styles.derive(StyleId::DEFAULT, |st| st.num_fmt = fmt);
            put(&mut sheet, row, 1, Value::text(format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1)), StyleId::DEFAULT);
            let cur = current.get(i).cloned().unwrap_or_default();
            put(&mut sheet, row, 2, cur.clone(), st);
            for (k, m) in per.iter().enumerate() {
                put(&mut sheet, row, 3 + k as u32, m.get(c).cloned().unwrap_or_else(|| cur.clone()), st);
            }
            row += 1;
        }
        if !results.is_empty() {
            put(&mut sheet, row, 1, Value::text("Result Cells:"), bold);
            row += 1;
            for (i, c) in results.iter().enumerate() {
                let fmt = result_fmts.get(i).cloned().unwrap_or_default();
                let st = cx.wb.styles.derive(StyleId::DEFAULT, |st| st.num_fmt = fmt);
                put(&mut sheet, row, 1, Value::text(format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1)), StyleId::DEFAULT);
                put(&mut sheet, row, 2, current_results.get(i).cloned().unwrap_or_default(), st);
                for (k, r) in scenario_results.iter().enumerate() {
                    put(&mut sheet, row, 3 + k as u32, r.get(i).cloned().unwrap_or_default(), st);
                }
                row += 1;
            }
        }
        put(
            &mut sheet,
            row + 1,
            1,
            Value::text("Notes: Current Values column represents values of changing cells at time Scenario Summary Report was created."),
            StyleId::DEFAULT,
        );
        sheet.cols.insert(1, gridcraft_model::LineInfo { size: Some(110.0), ..Default::default() });
        sheet.cols.insert(2, gridcraft_model::LineInfo { size: Some(100.0), ..Default::default() });
        cx.wb.sheets.push(std::sync::Arc::new(sheet));
        cx.structural = true;
        Ok(cx.wb.sheets.len() - 1)
    })?;
    s.doc_mut()?.wb_switch_sheet(new_index);
    Ok(json!({"sheet": name, "scenarios": names.len(), "changingCells": changing.len(), "resultCells": results.len()}))
}

// ---------------------------------------------------------------- data tables

const MAX_TABLE_CELLS: u64 = 100_000;

fn data_table(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.dataTable";
    let (si, r) = range_arg(s, p, "range", CMD)?;
    let row_in = if p.get("rowInput").and_then(Json::as_str).is_some_and(|t| !t.is_empty()) { Some(cell_arg(s, p, "rowInput", CMD)?) } else { None };
    let col_in =
        if p.get("columnInput").and_then(Json::as_str).is_some_and(|t| !t.is_empty()) { Some(cell_arg(s, p, "columnInput", CMD)?) } else { None };
    if row_in.is_none() && col_in.is_none() {
        return Err(bad(CMD, "give `rowInput`, `columnInput` or both"));
    }
    if r.height() < 2 || r.width() < 2 {
        return Err(bad(CMD, "the table needs at least two rows and two columns"));
    }
    let body = RangeRef::new(CellRef::new(r.start.row + 1, r.start.col + 1), r.end);
    if body.count() > MAX_TABLE_CELLS {
        return Err(bad(CMD, format!("the table body is too large ({MAX_TABLE_CELLS} cells max)")));
    }
    for input in [row_in, col_in].into_iter().flatten() {
        if input.0 == si && r.contains(input.1) {
            return Err(EngineError::Other("Input cell reference is not valid.".into()));
        }
    }
    let d = s.doc()?;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let (r0, c0) = (r.start.row, r.start.col);
    let mut trial = Trial::new(d);
    let mut out: Vec<(CellRef, Value)> = Vec::with_capacity(body.count() as usize);
    let mode = match (row_in, col_in) {
        (None, Some((isi, ic))) => {
            let reads: Vec<(usize, CellRef)> = (c0 + 1..=r.end.col).map(|c| (si, CellRef::new(r0, c))).collect();
            for row in r0 + 1..=r.end.row {
                let v = sh.value(CellRef::new(row, c0));
                let vals = trial.run(&[(isi, ic, v)], &reads);
                for (k, val) in vals.into_iter().enumerate() {
                    out.push((CellRef::new(row, c0 + 1 + k as u32), val));
                }
            }
            "column"
        }
        (Some((isi, ic)), None) => {
            let reads: Vec<(usize, CellRef)> = (r0 + 1..=r.end.row).map(|row| (si, CellRef::new(row, c0))).collect();
            for col in c0 + 1..=r.end.col {
                let v = sh.value(CellRef::new(r0, col));
                let vals = trial.run(&[(isi, ic, v)], &reads);
                for (k, val) in vals.into_iter().enumerate() {
                    out.push((CellRef::new(r0 + 1 + k as u32, col), val));
                }
            }
            "row"
        }
        (Some((rsi, rc)), Some((csi, cc))) => {
            let reads = [(si, CellRef::new(r0, c0))];
            for row in r0 + 1..=r.end.row {
                let cv = sh.value(CellRef::new(row, c0));
                for col in c0 + 1..=r.end.col {
                    let rv = sh.value(CellRef::new(r0, col));
                    let vals = trial.run(&[(rsi, rc, rv), (csi, cc, cv.clone())], &reads);
                    out.push((CellRef::new(row, col), vals.into_iter().next().unwrap_or_default()));
                }
            }
            "twoVariable"
        }
        (None, None) => return Err(bad(CMD, "give `rowInput`, `columnInput` or both")),
    };
    let n = out.len();
    edit(s, |cx| {
        for (c, v) in out {
            let sh = cx.sheet_mut(si)?;
            let style = sh.style_id(c);
            sh.set_cell(c, Cell { value: v, formula: None, style });
            cx.touch(si, c);
        }
        Ok(())
    })?;
    Ok(json!({"range": body.a1(), "cells": n, "mode": mode, "static": true}))
}

// ---------------------------------------------------------------- advanced filter

#[derive(Clone, Debug)]
enum Cond {
    /// Column offset in the list, comparison operator and operand text.
    Compare(u32, Value),
    /// A computed criterion: formula relative to the first data row, evaluated at `at`.
    Computed(gridcraft_formula::Expr, CellRef),
}

/// Splits `">=10"` into (`">="`, `"10"`); text without an operator means "begins with".
fn split_op(t: &str) -> (&str, &str) {
    for op in ["<>", ">=", "<=", "=", ">", "<"] {
        if let Some(rest) = t.strip_prefix(op) {
            return (op, rest);
        }
    }
    ("begins", t)
}

/// Does a list value match one criteria cell's value (Excel's advanced-filter semantics)?
pub(crate) fn criterion_matches(v: &Value, text: &str, crit: &Value) -> bool {
    let crit_text = match crit {
        Value::Empty => return true,
        Value::Number(n) => return v.as_f64().is_some_and(|x| (x - n).abs() <= 1e-12 * n.abs().max(1.0)),
        Value::Bool(b) => return matches!(v, Value::Bool(x) if x == b),
        Value::Error(e) => return v.as_error() == Some(*e),
        Value::Text(t) => t.to_string(),
        Value::Array(_) => return false,
    };
    let (op, operand) = split_op(&crit_text);
    if operand.is_empty() {
        return match op {
            "=" => v.is_empty() || text.is_empty(),
            "<>" => !(v.is_empty() || text.is_empty()),
            _ => true,
        };
    }
    let num = gridcraft_core::parse::parse_number_text(operand);
    let lower_text = text.to_lowercase();
    let lower_op = operand.to_lowercase();
    use std::cmp::Ordering;
    let cmp: Option<Ordering> = match (v, num) {
        (Value::Number(a), Some(b)) => a.partial_cmp(&b),
        (Value::Number(_), None) => None,
        (Value::Empty, _) => None,
        (_, Some(_)) if op != "begins" => None,
        _ => Some(gridcraft_core::compare_text(text, operand)),
    };
    let wild = |pat: &str| super::edit::wildcard(&lower_text, pat);
    match op {
        "=" => {
            if num.is_none() && operand.contains(['*', '?']) {
                wild(&lower_op)
            } else if num.is_none() {
                lower_text == lower_op
            } else {
                cmp == Some(Ordering::Equal)
            }
        }
        "<>" => {
            if num.is_none() && operand.contains(['*', '?']) {
                !wild(&lower_op)
            } else if num.is_none() {
                lower_text != lower_op
            } else {
                cmp != Some(Ordering::Equal)
            }
        }
        ">" => cmp == Some(Ordering::Greater),
        "<" => cmp == Some(Ordering::Less),
        ">=" => matches!(cmp, Some(Ordering::Greater | Ordering::Equal)),
        "<=" => matches!(cmp, Some(Ordering::Less | Ordering::Equal)),
        _ => {
            // Plain text criterion: begins with (wildcards allowed); a plain number equals.
            match (v, num) {
                (Value::Number(a), Some(b)) => (a - b).abs() <= 1e-12 * b.abs().max(1.0),
                _ => !v.is_empty() && wild(&format!("{lower_op}*")),
            }
        }
    }
}

fn advanced_filter(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.advancedFilter";
    let d = s.doc()?;
    let (si, list) = match str_param(p, "range") {
        Some(t) => parse_range(&d.wb, t, CMD)?,
        None => {
            let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
            (d.wb.active_sheet, crate::selection::current_region(sh, d.selection.active))
        }
    };
    let (csi, crit) = range_arg(s, p, "criteria", CMD)?;
    if list.height() < 1 || list.count() > 50_000_000 {
        return Err(bad(CMD, "the list range is too large"));
    }
    if crit.width() > 1024 || crit.height() > 10_000 {
        return Err(bad(CMD, "the criteria range is too large"));
    }
    let wb = &d.wb;
    let sh = wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let csh = wb.sheet(csi).ok_or(EngineError::NoDocument)?;
    let headers: Vec<String> =
        (list.start.col..=list.end.col).map(|c| crate::display::cell_text(wb, sh, CellRef::new(list.start.row, c)).trim().to_lowercase()).collect();
    let first_data = list.start.row + 1;
    // Criteria rows → conditions.
    let mut rows: Vec<Vec<Cond>> = Vec::new();
    for cr in crit.start.row + 1..=crit.end.row {
        let mut conds = Vec::new();
        for cc in crit.start.col..=crit.end.col {
            let at = CellRef::new(cr, cc);
            let cell = csh.cell(at);
            let v = csh.value(at);
            let formula = cell.and_then(|c| c.formula.as_ref());
            if v.is_empty() && formula.is_none() {
                continue;
            }
            let head = crate::display::cell_text(wb, csh, CellRef::new(crit.start.row, cc)).trim().to_lowercase();
            match headers.iter().position(|h| !head.is_empty() && *h == head) {
                Some(off) => conds.push(Cond::Compare(off as u32, v)),
                None => match formula.and_then(|f| f.expr()) {
                    Some(e) => conds.push(Cond::Computed(e, at)),
                    None => return Err(EngineError::Other(format!("The criteria label `{head}` doesn't match a column of the list."))),
                },
            }
        }
        rows.push(conds);
    }
    let matches_row = |row: u32| -> bool {
        if rows.is_empty() {
            return true;
        }
        rows.iter().any(|conds| {
            conds.iter().all(|c| match c {
                Cond::Compare(off, crit) => {
                    let at = CellRef::new(row, list.start.col + off);
                    let v = sh.value(at);
                    let text = crate::display::cell_text(wb, sh, at);
                    criterion_matches(&v, &text, crit)
                }
                Cond::Computed(e, at) => {
                    let shifted = gridcraft_formula::adjust::shift_relative(e.clone(), row as i64 - first_data as i64, 0);
                    let v = gridcraft_calc::recalc::evaluate_expr(wb, csi, *at, &shifted);
                    matches!(v.scalar().to_bool(), Ok(true))
                }
            })
        })
    };
    let unique = bool_param(p, "unique").unwrap_or(false);
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    let mut keep: Vec<(u32, bool)> = Vec::new();
    for row in first_data..=list.end.row {
        let mut ok = matches_row(row);
        if ok && unique {
            let key: Vec<String> =
                (list.start.col..=list.end.col).map(|c| crate::display::cell_text(wb, sh, CellRef::new(row, c)).to_lowercase()).collect();
            ok = seen.insert(key);
        }
        keep.push((row, ok));
    }
    let matched = keep.iter().filter(|(_, k)| *k).count();
    if let Some(t) = str_param(p, "copyTo") {
        let (dsi, dest) = parse_range(wb, t, CMD)?;
        let dest = dest.start;
        let extract = RangeRef::new(dest, dest.offset_clamped(list.height() as i64 - 1, list.width() as i64 - 1));
        if dsi == si && extract.intersects(&list) {
            return Err(EngineError::Other("The extract range can't overlap the list range.".into()));
        }
        let mut rows_out: Vec<u32> = vec![list.start.row];
        rows_out.extend(keep.iter().filter(|(_, k)| *k).map(|(r, _)| *r));
        let cells: Vec<(CellRef, Cell)> = rows_out
            .iter()
            .enumerate()
            .flat_map(|(i, row)| {
                (list.start.col..=list.end.col).filter_map(move |c| {
                    let src = CellRef::new(*row, c);
                    let to = dest.offset(i as i64, (c - list.start.col) as i64)?;
                    let style = sh.style_id(src);
                    Some((to, Cell { value: sh.value(src), formula: None, style }))
                })
            })
            .collect();
        let out_range = RangeRef::new(dest, dest.offset_clamped(rows_out.len() as i64 - 1, list.width() as i64 - 1));
        edit(s, |cx| {
            let shm = cx.sheet_mut(dsi)?;
            let old: Vec<CellRef> = shm.cells.iter_range(extract).map(|(c, _)| c).collect();
            for c in &old {
                shm.remove_cell(*c);
            }
            for (c, cell) in cells {
                shm.set_cell(c, cell);
            }
            for c in old {
                cx.touch(dsi, c);
            }
            for c in out_range.iter() {
                cx.touch(dsi, c);
            }
            Ok(())
        })?;
        return Ok(json!({"matched": matched, "range": out_range.a1()}));
    }
    let hidden = keep.iter().filter(|(_, k)| !*k).count();
    edit(s, |cx| {
        let shm = cx.sheet_mut(si)?;
        for (row, k) in &keep {
            if *k {
                if let Some(e) = shm.rows.get_mut(row) {
                    e.hidden = false;
                    if *e == gridcraft_model::LineInfo::default() {
                        shm.rows.remove(row);
                    }
                }
            } else {
                shm.rows.entry(*row).or_default().hidden = true;
            }
        }
        cx.structural = true;
        Ok(())
    })?;
    Ok(json!({"matched": matched, "hiddenRows": hidden}))
}

// ---------------------------------------------------------------- consolidate

#[derive(Clone, Copy, Debug, Default)]
struct Agg {
    count: u64,
    nums: u64,
    sum: f64,
    product: f64,
    min: f64,
    max: f64,
}

impl Agg {
    fn add(&mut self, v: &Value) {
        if v.is_empty() {
            return;
        }
        self.count += 1;
        if let Value::Number(n) = v {
            if self.nums == 0 {
                self.min = *n;
                self.max = *n;
                self.product = 1.0;
            }
            self.nums += 1;
            self.sum += n;
            self.product *= n;
            self.min = self.min.min(*n);
            self.max = self.max.max(*n);
        }
    }
    fn result(&self, func: &str) -> Value {
        match func {
            "count" => Value::number(self.count as f64),
            "countNums" | "countnums" => Value::number(self.nums as f64),
            "average" => {
                if self.nums == 0 {
                    Value::Error(gridcraft_core::CellError::Div0)
                } else {
                    Value::number(self.sum / self.nums as f64)
                }
            }
            "max" => Value::number(if self.nums == 0 { 0.0 } else { self.max }),
            "min" => Value::number(if self.nums == 0 { 0.0 } else { self.min }),
            "product" => Value::number(if self.nums == 0 { 0.0 } else { self.product }),
            _ => Value::number(self.sum),
        }
    }
}

/// A row/column key: a position or a label (matched case-insensitively).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Pos(u32),
    Label(String),
}

const MAX_CONSOLIDATE_CELLS: u64 = 5_000_000;

fn consolidate(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.consolidate";
    let func = str_param(p, "function").unwrap_or("sum").to_string();
    if !["sum", "count", "countNums", "countnums", "average", "max", "min", "product"].contains(&func.as_str()) {
        return Err(bad(CMD, format!("unknown function `{func}`")));
    }
    let top = bool_param(p, "topRow").unwrap_or(false);
    let left = bool_param(p, "leftColumn").unwrap_or(false);
    let d = s.doc()?;
    let srcs: Vec<String> = p
        .get("sources")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .or_else(|| str_param(p, "sources").map(|t| t.split(';').map(str::to_string).collect()))
        .unwrap_or_default();
    if srcs.is_empty() {
        return Err(bad(CMD, "give at least one source range in `sources`"));
    }
    if srcs.len() > 255 {
        return Err(bad(CMD, "too many sources (255 max)"));
    }
    let (dsi, dest) = match str_param(p, "destination") {
        Some(t) => {
            let (si, r) = parse_range(&d.wb, t, CMD)?;
            (si, r.start)
        }
        None => (d.wb.active_sheet, d.selection.active),
    };
    let mut row_keys: Vec<Key> = Vec::new();
    let mut col_keys: Vec<Key> = Vec::new();
    let mut row_label: HashMap<Key, String> = HashMap::new();
    let mut col_label: HashMap<Key, String> = HashMap::new();
    let mut aggs: HashMap<(usize, usize), Agg> = HashMap::new();
    let mut total: u64 = 0;
    let index_of = |keys: &mut Vec<Key>, k: Key| -> usize {
        match keys.iter().position(|x| *x == k) {
            Some(i) => i,
            None => {
                keys.push(k);
                keys.len() - 1
            }
        }
    };
    for t in &srcs {
        let (si, r) = parse_range(&d.wb, t, CMD)?;
        total = total.saturating_add(r.count());
        if total > MAX_CONSOLIDATE_CELLS {
            return Err(bad(CMD, "the sources are too large"));
        }
        let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
        let r0 = r.start.row + top as u32;
        let c0 = r.start.col + left as u32;
        if r0 > r.end.row || c0 > r.end.col {
            continue;
        }
        let label = |c: CellRef| crate::display::cell_text(&d.wb, sh, c).trim().to_string();
        // Column keys of this source.
        let mut cols: Vec<(u32, usize)> = Vec::new();
        for col in c0..=r.end.col {
            let k = if top {
                let l = label(CellRef::new(r.start.row, col));
                if l.is_empty() {
                    continue;
                }
                let k = Key::Label(l.to_lowercase());
                col_label.entry(k.clone()).or_insert(l);
                k
            } else {
                Key::Pos(col - c0)
            };
            cols.push((col, index_of(&mut col_keys, k)));
        }
        for row in r0..=r.end.row {
            let k = if left {
                let l = label(CellRef::new(row, r.start.col));
                if l.is_empty() {
                    continue;
                }
                let k = Key::Label(l.to_lowercase());
                row_label.entry(k.clone()).or_insert(l);
                k
            } else {
                Key::Pos(row - r0)
            };
            let ri = index_of(&mut row_keys, k);
            for (col, ci) in &cols {
                aggs.entry((ri, *ci)).or_default().add(&sh.value(CellRef::new(row, *col)));
            }
        }
    }
    let (out_r0, out_c0) = (dest.row + top as u32, dest.col + left as u32);
    if out_r0 as u64 + row_keys.len() as u64 > gridcraft_core::MAX_ROWS as u64
        || out_c0 as u64 + col_keys.len() as u64 > gridcraft_core::MAX_COLS as u64
    {
        return Err(EngineError::Other("The consolidated data doesn't fit below and right of the destination.".into()));
    }
    let mut cells: Vec<(CellRef, Value)> = Vec::new();
    if top {
        for (ci, k) in col_keys.iter().enumerate() {
            cells.push((CellRef::new(dest.row, out_c0 + ci as u32), Value::text(col_label.get(k).cloned().unwrap_or_default())));
        }
    }
    if left {
        for (ri, k) in row_keys.iter().enumerate() {
            cells.push((CellRef::new(out_r0 + ri as u32, dest.col), Value::text(row_label.get(k).cloned().unwrap_or_default())));
        }
    }
    for ri in 0..row_keys.len() {
        for ci in 0..col_keys.len() {
            let v = aggs.get(&(ri, ci)).map(|a| if a.count == 0 { Value::Empty } else { a.result(&func) }).unwrap_or_default();
            cells.push((CellRef::new(out_r0 + ri as u32, out_c0 + ci as u32), v));
        }
    }
    let end = CellRef::new(
        (out_r0 + row_keys.len() as u32).saturating_sub(1).max(dest.row),
        (out_c0 + col_keys.len() as u32).saturating_sub(1).max(dest.col),
    );
    edit(s, |cx| {
        for (c, v) in cells {
            let sh = cx.sheet_mut(dsi)?;
            let style = sh.style_id(c);
            if v.is_empty() && style == StyleId::DEFAULT {
                sh.remove_cell(c);
            } else {
                sh.set_cell(c, Cell { value: v, formula: None, style });
            }
            cx.touch(dsi, c);
        }
        Ok(())
    })?;
    Ok(json!({"range": RangeRef::new(dest, end).a1(), "rows": row_keys.len(), "columns": col_keys.len()}))
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use crate::Session;

    fn s() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }
    fn v(s: &Session, a: &str) -> Value {
        let d = s.doc().unwrap();
        let (sh, body) = super::split_sheet(a);
        let si = sh.map(|n| d.wb.sheet_index(&n).unwrap()).unwrap_or(d.wb.active_sheet);
        d.wb.sheet(si).unwrap().value(CellRef::parse(body).unwrap())
    }
    fn n(s: &Session, a: &str) -> f64 {
        v(s, a).as_f64().unwrap_or(f64::NAN)
    }

    #[test]
    fn goal_seek_linear_and_nonlinear() {
        let mut s = s();
        s.execute("cell.set", json!({"cell": "B2", "input": "1"})).unwrap();
        s.execute("cell.set", json!({"cell": "B5", "input": "=B2*3+4"})).unwrap();
        let r = s.execute("data.goalSeek", json!({"set": "B5", "to": 25, "changing": "B2"})).unwrap();
        assert_eq!(r["converged"], true);
        assert!((n(&s, "B2") - 7.0).abs() < 0.001, "{r}");
        assert!((n(&s, "B5") - 25.0).abs() < 0.001);
        // One undo step restores the input.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(n(&s, "B2"), 1.0);
        // Nonlinear: x^2 = 2 from x = 1.
        s.execute("cell.set", json!({"cell": "C1", "input": "=B2^2"})).unwrap();
        let r = s.execute("data.goalSeek", json!({"set": "C1", "to": 2, "changing": "B2"})).unwrap();
        assert_eq!(r["converged"], true);
        assert!((n(&s, "C1") - 2.0).abs() <= 0.001);
        // Loan payment: what rate makes PMT = -500?
        s.execute("range.setValues", json!({"range": "E1", "values": [[0.01], [10000], [24]]})).unwrap();
        s.execute("cell.set", json!({"cell": "E4", "input": "=PMT(E1/12,E3,E2)"})).unwrap();
        let r = s.execute("data.goalSeek", json!({"set": "E4", "to": -500, "changing": "E1"})).unwrap();
        assert_eq!(r["converged"], true, "{r}");
        assert!((n(&s, "E4") + 500.0).abs() <= 0.001);
        // Starting at zero.
        s.execute("cell.set", json!({"cell": "G1", "input": "0"})).unwrap();
        s.execute("cell.set", json!({"cell": "G2", "input": "=G1*G1*G1-27"})).unwrap();
        let r = s.execute("data.goalSeek", json!({"set": "G2", "to": 0, "changing": "G1"})).unwrap();
        assert_eq!(r["converged"], true, "{r}");
        assert!((n(&s, "G1") - 3.0).abs() < 0.01);
    }

    #[test]
    fn goal_seek_errors() {
        let mut s = s();
        s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "=A1"})).unwrap();
        assert!(s.execute("data.goalSeek", json!({"set": "A1", "to": 1, "changing": "A1"})).is_err());
        assert!(s.execute("data.goalSeek", json!({"set": "A2", "to": 1, "changing": "A2"})).is_err());
        assert!(s.execute("data.goalSeek", json!({"set": "A2", "changing": "A1"})).is_err());
        // Unreachable target: not converged, but no error and no crash.
        s.execute("cell.set", json!({"cell": "A3", "input": "=A1^2"})).unwrap();
        let r = s.execute("data.goalSeek", json!({"set": "A3", "to": -4, "changing": "A1"})).unwrap();
        assert_eq!(r["converged"], false);
        assert!(r["iterations"].as_u64().unwrap() <= 100);
    }

    #[test]
    fn seek_unit() {
        let mut f = |x: f64| (x - 1e6) / 3.0;
        let r = super::seek(0.0, &mut f, 100, 0.001);
        assert!(r.converged, "{r:?}");
        let mut g = |x: f64| if x < 0.0 { f64::NAN } else { x.sqrt() - 5.0 };
        let r = super::seek(1.0, &mut g, 100, 0.001);
        assert!(r.converged, "{r:?}");
        let mut h = |_: f64| f64::NAN;
        let r = super::seek(1.0, &mut h, 100, 0.001);
        assert!(!r.converged);
    }

    #[test]
    fn scenarios() {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "B2", "values": [[10], [20]]})).unwrap();
        s.execute("cell.set", json!({"cell": "B10", "input": "=B2*B3"})).unwrap();
        s.execute("data.scenarioAdd", json!({"name": "Base", "changing": "B2:B3"})).unwrap();
        s.execute("data.scenarioAdd", json!({"name": "Best", "changing": "B2:B3", "values": [100, 3], "comment": "optimistic"})).unwrap();
        assert!(s.execute("data.scenarioAdd", json!({"name": "best", "changing": "B2:B3", "values": [1, 2]})).is_err());
        assert!(s.execute("data.scenarioAdd", json!({"name": "Bad", "changing": "B2:B3", "values": [1]})).is_err());
        let l = s.execute("data.scenarioManager", json!({})).unwrap();
        assert_eq!(l["scenarios"].as_array().unwrap().len(), 2);
        assert_eq!(l["scenarios"][1]["comment"], "optimistic");
        s.execute("data.scenarioShow", json!({"name": "Best"})).unwrap();
        assert_eq!(n(&s, "B10"), 300.0);
        s.execute("data.scenarioShow", json!({"name": "Base"})).unwrap();
        assert_eq!(n(&s, "B10"), 200.0);
        // Stored in a hidden workbook name.
        let d = s.doc().unwrap();
        assert!(d.wb.names.iter().any(|n| n.name == super::SCENARIO_NAME && n.hidden));
        assert_eq!(super::load_scenarios(&d.wb).len(), 2);
        // Long data is split into 250-character literals and survives an xlsx round trip.
        s.execute("data.scenarioAdd", json!({"name": "Long", "changing": "B2", "values": [1], "comment": "x".repeat(600)})).unwrap();
        let f = s.doc().unwrap().wb.names.iter().find(|n| n.name == super::SCENARIO_NAME).unwrap().formula.clone();
        assert!(f.matches("\"&\"").count() >= 2, "{f}");
        let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
        s.execute("file.open", json!({"name": "sc.xlsx", "base64": b64})).unwrap();
        let l = s.execute("data.scenarioManager", json!({})).unwrap();
        assert_eq!(l["scenarios"].as_array().unwrap().len(), 3, "{l}");
        assert_eq!(l["scenarios"][2]["comment"].as_str().unwrap().len(), 600);
        s.execute("data.scenarioDelete", json!({"name": "Long"})).unwrap();
        let r = s.execute("data.scenarioSummary", json!({"resultCells": "B10"})).unwrap();
        assert_eq!(r["sheet"], "Scenario Summary");
        assert_eq!(s.doc().unwrap().wb.active().unwrap().name, "Scenario Summary");
        assert_eq!(v(&s, "'Scenario Summary'!D3"), Value::from("Base"));
        assert_eq!(v(&s, "'Scenario Summary'!B5"), Value::from("$B$2"));
        assert_eq!(n(&s, "'Scenario Summary'!E5"), 100.0);
        // Result row: current, Base, Best.
        assert_eq!(n(&s, "'Scenario Summary'!C8"), 200.0);
        assert_eq!(n(&s, "'Scenario Summary'!E8"), 300.0);
        s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
        s.execute("data.scenarioDelete", json!({"name": "Best"})).unwrap();
        assert_eq!(s.execute("data.scenarioManager", json!({})).unwrap()["scenarios"].as_array().unwrap().len(), 1);
        s.execute("data.scenarioDelete", json!({"name": "Base"})).unwrap();
        assert!(s.doc().unwrap().wb.names.iter().all(|n| n.name != super::SCENARIO_NAME));
        assert!(s.execute("data.scenarioSummary", json!({})).is_err());
    }

    #[test]
    fn data_tables() {
        let mut s = s();
        // Loan: rate B1, amount B2; payment formula.
        s.execute("range.setValues", json!({"range": "B1", "values": [[0.05], [1000]]})).unwrap();
        // One-variable, column oriented: amounts down D3:D5, formula in E2.
        s.execute("range.setValues", json!({"range": "D3", "values": [[100], [200], [300]]})).unwrap();
        s.execute("cell.set", json!({"cell": "E2", "input": "=B2*(1+B1)"})).unwrap();
        s.execute("cell.set", json!({"cell": "F2", "input": "=B2*2"})).unwrap();
        let r = s.execute("data.dataTable", json!({"range": "D2:F5", "columnInput": "B2"})).unwrap();
        assert_eq!(r["cells"], 6);
        assert!((n(&s, "E3") - 105.0).abs() < 1e-9);
        assert_eq!(n(&s, "F5"), 600.0);
        // The input cell is untouched.
        assert_eq!(n(&s, "B2"), 1000.0);
        // Row oriented: rates across H2:J2, formula in G3.
        s.execute("range.setValues", json!({"range": "H2", "values": [[0.1, 0.2, 0.3]]})).unwrap();
        s.execute("cell.set", json!({"cell": "G3", "input": "=B2*(1+B1)"})).unwrap();
        s.execute("data.dataTable", json!({"range": "G2:J3", "rowInput": "B1"})).unwrap();
        assert!((n(&s, "J3") - 1300.0).abs() < 1e-9);
        // Two-variable: formula at L1, rates across, amounts down.
        s.execute("cell.set", json!({"cell": "L1", "input": "=B2*(1+B1)"})).unwrap();
        s.execute("range.setValues", json!({"range": "M1", "values": [[0.5, 1]]})).unwrap();
        s.execute("range.setValues", json!({"range": "L2", "values": [[10], [20]]})).unwrap();
        let r = s.execute("data.dataTable", json!({"range": "L1:N3", "rowInput": "B1", "columnInput": "B2"})).unwrap();
        assert_eq!(r["mode"], "twoVariable");
        assert_eq!(n(&s, "M2"), 15.0);
        assert_eq!(n(&s, "N3"), 40.0);
        assert!(s.execute("data.dataTable", json!({"range": "L1:N3"})).is_err());
        assert!(s.execute("data.dataTable", json!({"range": "A1:XFD100000", "columnInput": "B2"})).is_err());
        assert!(s.execute("data.dataTable", json!({"range": "L1:N3", "columnInput": "M2"})).is_err());
    }

    #[test]
    fn advanced_filter_in_place_and_copy() {
        let mut s = s();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [["Region", "Sales"], ["East", 50], ["West", 150], ["East", 200], ["North", 120], ["East", 200]]}),
        )
        .unwrap();
        // East AND Sales > 100, OR North.
        s.execute("range.setValues", json!({"range": "H1", "values": [["Region", "Sales"], ["East", ">100"], ["North", null]]})).unwrap();
        let r = s.execute("data.advancedFilter", json!({"range": "A1:B6", "criteria": "H1:I3"})).unwrap();
        assert_eq!(r["matched"], 3);
        let sh = || s.doc().unwrap().wb.active().unwrap().clone();
        assert!(sh().is_row_hidden(1));
        assert!(sh().is_row_hidden(2));
        assert!(!sh().is_row_hidden(3));
        assert!(!sh().is_row_hidden(4));
        // Unique, copied.
        let r = s.execute("data.advancedFilter", json!({"range": "A1:B6", "criteria": "H1:I3", "copyTo": "K1", "unique": true})).unwrap();
        assert_eq!(r["matched"], 2);
        assert_eq!(r["range"], "K1:L3");
        assert_eq!(v(&s, "K1"), Value::from("Region"));
        assert_eq!(v(&s, "K2"), Value::from("East"));
        assert_eq!(n(&s, "L2"), 200.0);
        assert_eq!(v(&s, "K3"), Value::from("North"));
        assert_eq!(v(&s, "K4"), Value::Empty);
        // Wildcards and exact text.
        s.execute("range.setValues", json!({"range": "H5", "values": [["Region"], ["*st"]]})).unwrap();
        let r = s.execute("data.advancedFilter", json!({"range": "A1:B6", "criteria": "H5:H6"})).unwrap();
        assert_eq!(r["matched"], 4);
        // Computed criterion (blank header): Sales above 100.
        s.execute("cell.set", json!({"cell": "J8", "input": "=B2>100"})).unwrap();
        let r = s.execute("data.advancedFilter", json!({"range": "A1:B6", "criteria": "J7:J8"})).unwrap();
        assert_eq!(r["matched"], 4);
        // Overlap is rejected.
        assert!(s.execute("data.advancedFilter", json!({"range": "A1:B6", "criteria": "H1:I3", "copyTo": "B2"})).is_err());
    }

    #[test]
    fn criteria_semantics() {
        use super::criterion_matches as m;
        let t = |x: &str| Value::from(x);
        assert!(m(&Value::Number(150.0), "150", &t(">100")));
        assert!(!m(&Value::Number(50.0), "50", &t(">100")));
        assert!(!m(&t("abc"), "abc", &t(">100")));
        assert!(m(&t("Apple"), "Apple", &t("a")));
        assert!(m(&t("apple pie"), "apple pie", &t("=apple*")));
        assert!(!m(&t("apple pie"), "apple pie", &t("=apple")));
        assert!(m(&Value::Empty, "", &t("=")));
        assert!(m(&t("x"), "x", &t("<>")));
        assert!(m(&Value::Number(5.0), "5", &Value::Number(5.0)));
        assert!(m(&t("b"), "b", &t(">a")));
        assert!(m(&t("x"), "x", &t("<>y")));
    }

    #[test]
    fn consolidate_by_position_and_label() {
        let mut s = s();
        s.execute("home.insertSheet", json!({})).unwrap();
        s.execute("sheet.rename", json!({"name": "East"})).unwrap();
        s.execute("range.setValues", json!({"range": "A1", "values": [["", "Q1", "Q2"], ["Apples", 10, 20], ["Pears", 5, 6]]})).unwrap();
        s.execute("home.insertSheet", json!({})).unwrap();
        s.execute("sheet.rename", json!({"name": "West"})).unwrap();
        s.execute("range.setValues", json!({"range": "A1", "values": [["", "Q2", "Q3"], ["Pears", 1, 2], ["Plums", 7, 8]]})).unwrap();
        s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
        let r = s
            .execute("data.consolidate", json!({"sources": ["East!A1:C3", "West!A1:C3"], "destination": "A1", "topRow": true, "leftColumn": true}))
            .unwrap();
        assert_eq!(r["range"], "A1:D4");
        assert_eq!(v(&s, "Sheet1!B1"), Value::from("Q1"));
        assert_eq!(v(&s, "Sheet1!D1"), Value::from("Q3"));
        assert_eq!(v(&s, "Sheet1!A3"), Value::from("Pears"));
        assert_eq!(n(&s, "Sheet1!C3"), 7.0);
        assert_eq!(n(&s, "Sheet1!D4"), 8.0);
        assert_eq!(v(&s, "Sheet1!B4"), Value::Empty);
        // By position, average.
        let r = s.execute("data.consolidate", json!({"sources": ["East!B2:C3", "West!B2:C3"], "destination": "F1", "function": "average"})).unwrap();
        assert_eq!(r["range"], "F1:G2");
        assert_eq!(n(&s, "Sheet1!F1"), 5.5);
        assert_eq!(n(&s, "Sheet1!G2"), 7.0);
        assert!(s.execute("data.consolidate", json!({"sources": []})).is_err());
        assert!(s.execute("data.consolidate", json!({"sources": ["A1"], "function": "median"})).is_err());
    }
}
