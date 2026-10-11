//! Formulas tab: AutoSum, Insert Function, names, auditing, calculation.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{CalcMode, DefinedName};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "formulas.autoSum",
            "AutoSum",
            ["Formulas", "Function Library"],
            Some("Cmd+Shift+T"),
            "{function?: SUM|AVERAGE|COUNT|MAX|MIN, range?}",
            has_doc,
            auto_sum
        ),
        cmd!(
            "formulas.insertFunction",
            "Insert Function…",
            ["Formulas", "Function Library"],
            Some("Shift+F3"),
            "{name?: \"VLOOKUP\"} (UI opens the dialog; with a name, starts the formula in the active cell)",
            has_doc,
            insert_function
        ),
        cmd!(query "formulas.functions", "List Functions", [], None, "{category?, search?} → [{name, category, signature, description}]", always, list_functions),
        cmd!(
            "formulas.defineName",
            "Define Name…",
            ["Formulas", "Defined Names"],
            None,
            "{name, refersTo?: \"=Sheet1!$A$1:$A$10\" (default: the selection), scope?: \"Workbook\"|sheet name, comment?}",
            has_doc,
            define_name
        ),
        cmd!("formulas.deleteName", "Delete Name", [], None, "{name, scope?: \"Workbook\"|sheet name (default: every scope)}", has_doc, delete_name),
        cmd!(query "formulas.nameManager", "Name Manager", ["Formulas", "Defined Names"], Some("Cmd+F3"), "{} → names with values", has_doc, name_manager),
        cmd!(
            "formulas.createFromSelection",
            "Create from Selection",
            ["Formulas", "Defined Names"],
            Some("Cmd+Shift+F3"),
            "{range?, top?: true, left?: false, bottom?: false, right?: false, replace?: false (existing names are kept and listed in `skipped`)}",
            has_doc,
            create_from_selection
        ),
        cmd!(query "formulas.tracePrecedents", "Trace Precedents", ["Formulas", "Formula Auditing"], None, "{cell?} → ranges", has_doc, trace_precedents),
        cmd!(query "formulas.traceDependents", "Trace Dependents", ["Formulas", "Formula Auditing"], None, "{cell?} → cells", has_doc, trace_dependents),
        cmd!(noundo "formulas.removeArrows", "Remove Arrows", ["Formulas", "Formula Auditing"], None, "{}", has_doc, |_, _| ok()),
        cmd!("formulas.showFormulas", "Show Formulas", ["Formulas", "Formula Auditing"], Some("Ctrl+`"), "{on?}", has_doc, show_formulas),
        cmd!(query "formulas.errorChecking", "Error Checking", ["Formulas", "Formula Auditing"], None, "{} → cells with errors", has_doc, error_checking),
        cmd!(query "formulas.evaluateFormula", "Evaluate Formula", ["Formulas", "Formula Auditing"], None, "{cell?, formula?} → steps", has_doc, evaluate_formula),
        cmd!(query "formulas.evaluate", "Evaluate", [], None, "{formula: \"=SUM(A1:A3)\", cell?} → value without changing the sheet", has_doc, evaluate),
        cmd!(noundo "formulas.calculateNow", "Calculate Now", ["Formulas", "Calculation"], Some("F9"), "{}", has_doc, calc_now),
        cmd!(noundo "formulas.calculateSheet", "Calculate Sheet", ["Formulas", "Calculation"], Some("Shift+F9"), "{}", has_doc, calc_now),
        cmd!(
            "formulas.calculationOptions",
            "Calculation Options",
            ["Formulas", "Calculation"],
            None,
            "{mode: automatic|automaticExceptTables|manual, iterative?, maxIterations?, maxChange?, multiThreaded?, threads? (0 = every processor)}",
            has_doc,
            calc_options
        ),
    ]
}

fn auto_sum(s: &mut Session, p: &Json) -> Result<Json> {
    let func = str_param(p, "function").unwrap_or("SUM").to_ascii_uppercase();
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let sel = match str_param(p, "range") {
        Some(_) => target_range(s, p)?,
        None => d.selection.current(),
    };
    if !sel.is_single() {
        // A block: totals below each column (and to the right when a blank column is included).
        let num_end = sel.end;
        let below = CellRef::new(num_end.row + 1, 0);
        let mut writes = Vec::new();
        let last_row_blank = (sel.start.col..=sel.end.col).all(|c| sh.value(CellRef::new(sel.end.row, c)).is_empty());
        let (data_end_row, out_row) = if last_row_blank { (sel.end.row.saturating_sub(1), sel.end.row) } else { (sel.end.row, below.row) };
        for c in sel.start.col..=sel.end.col {
            let r = RangeRef::new(CellRef::new(sel.start.row, c), CellRef::new(data_end_row, c));
            writes.push((CellRef::new(out_row, c), format!("={func}({})", r.a1())));
        }
        for (c, f) in writes {
            s.execute("cell.set", json!({"cell": c.a1(), "input": f}))?;
        }
        return ok();
    }
    // Single cell: guess the range above, else to the left.
    let at = sel.start;
    let numeric = |c: CellRef| sh.value(c).is_number();
    let mut range = None;
    if at.row > 0 && (numeric(CellRef::new(at.row - 1, at.col)) || at.col == 0 || !numeric(CellRef::new(at.row, at.col - 1))) {
        let mut top = at.row - 1;
        while top > 0
            && (numeric(CellRef::new(top - 1, at.col))
                || sh.value(CellRef::new(top - 1, at.col)).is_empty() && top > 1 && numeric(CellRef::new(top - 2, at.col)))
        {
            top -= 1;
        }
        if numeric(CellRef::new(at.row - 1, at.col)) {
            range = Some(RangeRef::new(CellRef::new(top, at.col), CellRef::new(at.row - 1, at.col)));
        }
    }
    if range.is_none() && at.col > 0 && numeric(CellRef::new(at.row, at.col - 1)) {
        let mut left = at.col - 1;
        while left > 0 && numeric(CellRef::new(at.row, left - 1)) {
            left -= 1;
        }
        range = Some(RangeRef::new(CellRef::new(at.row, left), CellRef::new(at.row, at.col - 1)));
    }
    let text = match range {
        Some(r) => format!("={func}({})", r.a1()),
        None => {
            s.ui_requests.push(crate::UiRequest::EditCell(Some(format!("={func}()"))));
            return ok();
        }
    };
    if p.get("enter").and_then(Json::as_bool) == Some(false) {
        s.ui_requests.push(crate::UiRequest::EditCell(Some(text.clone())));
        return Ok(json!({"formula": text}));
    }
    s.execute("cell.set", json!({"cell": at.a1(), "input": text}))?;
    Ok(json!({"formula": text}))
}

fn insert_function(s: &mut Session, p: &Json) -> Result<Json> {
    match str_param(p, "name") {
        Some(n) => {
            s.ui_requests.push(crate::UiRequest::EditCell(Some(format!("={}(", n.to_ascii_uppercase()))));
            ok()
        }
        None => {
            s.ui_requests.push(crate::UiRequest::Dialog("insertFunction".into(), json!({})));
            ok()
        }
    }
}

pub fn function_list() -> Vec<Json> {
    let mut v: Vec<Json> = gridcraft_functions::all()
        .iter()
        .map(|f| json!({"name": f.name, "category": format!("{:?}", f.category), "signature": f.signature, "description": f.description}))
        .collect();
    let specials: &[(&str, &str, &str, &str)] = &[
        (
            "IF",
            "Logical",
            "IF(logical_test, [value_if_true], [value_if_false])",
            "Returns one value if a condition is true and another if it's false.",
        ),
        ("IFS", "Logical", "IFS(test1, value1, ...)", "Returns the value of the first true condition."),
        ("IFERROR", "Logical", "IFERROR(value, value_if_error)", "Returns a fallback when a value is an error."),
        ("IFNA", "Logical", "IFNA(value, value_if_na)", "Returns a fallback when a value is #N/A."),
        ("CHOOSE", "Lookup", "CHOOSE(index_num, value1, [value2], ...)", "Picks a value from a list by position."),
        ("SWITCH", "Logical", "SWITCH(expression, value1, result1, ..., [default])", "Matches a value against a list of cases."),
        ("ROW", "Lookup", "ROW([reference])", "Row number of a reference."),
        ("COLUMN", "Lookup", "COLUMN([reference])", "Column number of a reference."),
        ("ROWS", "Lookup", "ROWS(array)", "Number of rows in a reference or array."),
        ("COLUMNS", "Lookup", "COLUMNS(array)", "Number of columns in a reference or array."),
        ("OFFSET", "Lookup", "OFFSET(reference, rows, cols, [height], [width])", "A reference shifted from a starting point."),
        ("INDIRECT", "Lookup", "INDIRECT(ref_text, [a1])", "The reference named by a text string."),
        ("INDEX", "Lookup", "INDEX(array, row_num, [column_num], [area_num])", "The value or reference at a position."),
        ("SUBTOTAL", "MathTrig", "SUBTOTAL(function_num, ref1, ...)", "A subtotal that can skip hidden rows."),
        ("AGGREGATE", "MathTrig", "AGGREGATE(function_num, options, ref1, ...)", "An aggregate that can skip hidden rows and errors."),
        ("TEXT", "Text", "TEXT(value, format_text)", "Formats a number as text."),
        ("LET", "Logical", "LET(name1, value1, ..., calculation)", "Names intermediate results."),
        ("LAMBDA", "Logical", "LAMBDA([parameter1, ...], calculation)", "Creates a reusable custom function."),
        ("MAP", "Logical", "MAP(array1, ..., lambda)", "Applies a LAMBDA to each value."),
        ("REDUCE", "Logical", "REDUCE(initial_value, array, lambda)", "Accumulates an array into one value."),
        ("SCAN", "Logical", "SCAN(initial_value, array, lambda)", "Running accumulation of an array."),
        ("BYROW", "Logical", "BYROW(array, lambda)", "Applies a LAMBDA to each row."),
        ("BYCOL", "Logical", "BYCOL(array, lambda)", "Applies a LAMBDA to each column."),
        ("MAKEARRAY", "Logical", "MAKEARRAY(rows, cols, lambda)", "Builds an array from a LAMBDA."),
        ("CELL", "Information", "CELL(info_type, [reference])", "Information about a cell."),
        ("ISREF", "Information", "ISREF(value)", "TRUE for references."),
        ("ISFORMULA", "Information", "ISFORMULA(reference)", "TRUE when the cell has a formula."),
        ("FORMULATEXT", "Lookup", "FORMULATEXT(reference)", "The formula of a cell as text."),
        ("SHEET", "Information", "SHEET([value])", "Sheet number."),
        ("SHEETS", "Information", "SHEETS([reference])", "Number of sheets."),
        ("AREAS", "Lookup", "AREAS(reference)", "Number of areas in a reference."),
        ("ISOMITTED", "Information", "ISOMITTED(argument)", "TRUE when a LAMBDA argument was left out."),
    ];
    for (n, c, sig, d) in specials {
        if !v.iter().any(|x| x["name"] == *n) {
            v.push(json!({"name": n, "category": c, "signature": sig, "description": d}));
        }
    }
    v.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    v
}

fn list_functions(_: &mut Session, p: &Json) -> Result<Json> {
    let cat = str_param(p, "category").map(str::to_ascii_lowercase);
    let q = str_param(p, "search").map(str::to_ascii_lowercase);
    let v: Vec<Json> = function_list()
        .into_iter()
        .filter(|f| cat.as_ref().is_none_or(|c| f["category"].as_str().is_some_and(|x| x.to_ascii_lowercase() == *c)))
        .filter(|f| {
            q.as_ref().is_none_or(|q| {
                f["name"].as_str().is_some_and(|x| x.to_ascii_lowercase().contains(q.as_str()))
                    || f["description"].as_str().is_some_and(|x| x.to_ascii_lowercase().contains(q.as_str()))
            })
        })
        .collect();
    Ok(Json::Array(v))
}

fn valid_name(n: &str) -> bool {
    let mut chars = n.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_alphabetic() || first == '_' || first == '\\') {
        return false;
    }
    if n.len() > 255 || CellRef::parse(n).is_some() || is_r1c1(n) {
        return false;
    }
    n.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\' | '?'))
}

/// Looks like an R1C1 reference (`R`, `C`, `RC`, `R2`, `C3`, `R1C1`), which Excel doesn't
/// accept as a name.
fn is_r1c1(n: &str) -> bool {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let mut rest = n;
    if let Some(r) = rest.strip_prefix(['R', 'r']) {
        rest = r.get(digits(r)..).unwrap_or("");
    }
    if let Some(c) = rest.strip_prefix(['C', 'c']) {
        rest = c.get(digits(c)..).unwrap_or("");
    }
    rest.is_empty() && !n.is_empty()
}

fn define_name(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(name) = str_param(p, "name").map(str::to_string) else {
        s.ui_requests.push(crate::UiRequest::Dialog("defineName".into(), json!({})));
        return ok();
    };
    if !valid_name(&name) {
        return Err(EngineError::Other("The name that you entered is not valid.".into()));
    }
    let d = s.doc()?;
    let scope = match str_param(p, "scope") {
        None | Some("Workbook") => None,
        Some(sh) => Some(d.wb.sheet_index(sh).ok_or_else(|| bad("formulas.defineName", "no such sheet"))?),
    };
    let refers = match str_param(p, "refersTo") {
        Some(r) => r.trim_start_matches('=').to_string(),
        None => {
            let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
            let r = d.selection.current();
            let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1);
            let body = if r.is_single() { abs(r.start) } else { format!("{}:{}", abs(r.start), abs(r.end)) };
            format!("{}!{}", gridcraft_formula::quote_sheet(&sh.name), body)
        }
    };
    if gridcraft_formula::parse(&refers).is_err() {
        return Err(EngineError::Other("There's a problem with this formula.".into()));
    }
    let comment = str_param(p, "comment").unwrap_or("").to_string();
    edit(s, |cx| {
        cx.wb.names.retain(|n| !(n.name.eq_ignore_ascii_case(&name) && n.scope == scope));
        cx.wb.names.push(DefinedName { name: name.clone(), scope, formula: refers.clone(), comment: comment.clone(), hidden: false });
        cx.structural = true;
        Ok(json!({"name": name, "refersTo": format!("={refers}")}))
    })
}

fn delete_name(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("formulas.deleteName", "missing `name`"))?.to_string();
    let scope = match str_param(p, "scope") {
        None => None,
        Some("Workbook") => Some(None),
        Some(sh) => Some(Some(s.doc()?.wb.sheet_index(sh).ok_or_else(|| bad("formulas.deleteName", "no such sheet"))?)),
    };
    edit(s, |cx| {
        let before = cx.wb.names.len();
        cx.wb.names.retain(|n| !(n.name.eq_ignore_ascii_case(&name) && scope.is_none_or(|sc| n.scope == sc)));
        if cx.wb.names.len() == before {
            return Err(bad("formulas.deleteName", "no such name"));
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn name_manager(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let list: Vec<Json> = d
        .wb
        .names
        .iter()
        .filter(|n| !n.hidden)
        .map(|n| {
            let sheet = n.scope.unwrap_or(d.wb.active_sheet);
            let v = gridcraft_calc::evaluate(&d.wb, sheet, CellRef::default(), &n.formula);
            json!({"name": n.name, "refersTo": format!("={}", n.formula), "scope": n.scope.and_then(|i| d.wb.sheet(i)).map(|s| s.name.clone()).unwrap_or_else(|| "Workbook".into()), "value": crate::cmd::inspect::value_json(&v), "comment": n.comment})
        })
        .collect();
    Ok(Json::Array(list))
}

fn create_from_selection(s: &mut Session, p: &Json) -> Result<Json> {
    let r = target_range(s, p)?;
    let top = bool_param(p, "top").unwrap_or(true);
    let left = bool_param(p, "left").unwrap_or(false);
    let bottom = bool_param(p, "bottom").unwrap_or(false);
    let right = bool_param(p, "right").unwrap_or(false);
    let replace = bool_param(p, "replace").unwrap_or(false);
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let q = gridcraft_formula::quote_sheet(&sh.name);
    let abs = |r: RangeRef| {
        format!(
            "{q}!${}${}:${}${}",
            gridcraft_core::col_to_letters(r.start.col),
            r.start.row + 1,
            gridcraft_core::col_to_letters(r.end.col),
            r.end.row + 1
        )
    };
    let clean = |t: String| -> String {
        let mut n: String = t.trim().chars().map(|c| if c.is_alphanumeric() || c == '.' { c } else { '_' }).collect();
        if !n.is_empty() && !valid_name(&n) {
            n.insert(0, '_');
        }
        if valid_name(&n) { n } else { String::new() }
    };
    // The labels in the chosen edge rows/columns name the cells between them.
    let (r0, r1) = (r.start.row + top as u32, r.end.row.saturating_sub(bottom as u32));
    let (c0, c1) = (r.start.col + left as u32, r.end.col.saturating_sub(right as u32));
    let mut made = Vec::new();
    if r0 <= r1 && c0 <= c1 {
        for (on, row) in [(top, r.start.row), (bottom, r.end.row)] {
            for c in (c0..=c1).filter(|_| on) {
                let name = clean(sh.value(CellRef::new(row, c)).display());
                if !name.is_empty() {
                    made.push((name, abs(RangeRef::new(CellRef::new(r0, c), CellRef::new(r1, c)))));
                }
            }
        }
        for (on, col) in [(left, r.start.col), (right, r.end.col)] {
            for row in (r0..=r1).filter(|_| on) {
                let name = clean(sh.value(CellRef::new(row, col)).display());
                if !name.is_empty() {
                    made.push((name, abs(RangeRef::new(CellRef::new(row, c0), CellRef::new(row, c1)))));
                }
            }
        }
    }
    // Excel asks before replacing an existing name; here it's kept unless `replace` is set, and
    // reported.
    let exists = |n: &str| d.wb.names.iter().any(|x| x.name.eq_ignore_ascii_case(n) && x.scope.is_none());
    let (made, skipped): (Vec<_>, Vec<_>) = made.into_iter().partition(|(n, _)| replace || !exists(n));
    let skipped: Vec<String> = skipped.into_iter().map(|(n, _)| n).collect();
    let n = made.len();
    edit(s, |cx| {
        for (name, f) in made {
            cx.wb.names.retain(|x| !(x.name.eq_ignore_ascii_case(&name) && x.scope.is_none()));
            cx.wb.names.push(DefinedName { name, scope: None, formula: f, comment: String::new(), hidden: false });
        }
        cx.structural = true;
        Ok(json!({"created": n, "skipped": skipped}))
    })
}

fn trace_precedents(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(p, "cell").unwrap_or(d.selection.active);
    let areas = d.calc.graph.precedents_of((d.wb.active_sheet, at));
    let v: Vec<Json> = areas.iter().map(|a| json!({"sheet": d.wb.sheet(a.sheet).map(|s| s.name.clone()), "range": a.range.a1()})).collect();
    Ok(Json::Array(v))
}

fn trace_dependents(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(p, "cell").unwrap_or(d.selection.active);
    let mut deps = Vec::new();
    d.calc.graph.dependents(d.wb.active_sheet, at, &mut deps);
    deps.sort();
    deps.dedup();
    let v: Vec<Json> = deps.iter().map(|(si, c)| json!({"sheet": d.wb.sheet(*si).map(|s| s.name.clone()), "cell": c.a1()})).collect();
    Ok(Json::Array(v))
}

fn show_formulas(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet;
    let cur = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.show_formulas);
    let on = bool_param(p, "on").unwrap_or(!cur);
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.show_formulas = on;
        Ok(json!({"on": on}))
    })
}

fn error_checking(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let mut out = Vec::new();
    for sh in &d.wb.sheets {
        for (c, cell) in sh.cells.iter() {
            if let Some(e) = cell.value.as_error() {
                out.push(
                    json!({"sheet": sh.name, "cell": c.a1(), "error": e.as_str(), "formula": cell.formula.as_ref().map(|f| format!("={}", f.text))}),
                );
            }
            if out.len() >= 10_000 {
                break;
            }
        }
    }
    let circ: Vec<String> =
        d.calc.circular.iter().map(|(si, c)| format!("{}!{}", d.wb.sheet(*si).map(|s| s.name.clone()).unwrap_or_default(), c.a1())).collect();
    Ok(json!({"errors": out, "circular": circ}))
}

fn evaluate_formula(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(p, "cell").unwrap_or(d.selection.active);
    let sheet = d.wb.active_sheet;
    let text = match str_param(p, "formula") {
        Some(f) => f.trim_start_matches('=').to_string(),
        None => {
            d.wb.sheet(sheet)
                .and_then(|sh| sh.cell(at))
                .and_then(|c| c.formula.as_ref())
                .map(|f| f.text.clone())
                .ok_or_else(|| EngineError::Other("The cell has no formula.".into()))?
        }
    };
    let expr = gridcraft_formula::parse(&text).map_err(|e| EngineError::Other(e.to_string()))?;
    // Steps: each sub-expression (innermost first) with its value.
    let mut steps = Vec::new();
    let mut nodes = Vec::new();
    expr.walk(&mut |e| {
        if matches!(e, gridcraft_formula::Expr::Call(..) | gridcraft_formula::Expr::Binary(..) | gridcraft_formula::Expr::Ref(_)) {
            nodes.push(e.clone());
        }
    });
    for e in nodes.into_iter().rev().take(200) {
        let v = gridcraft_calc::recalc::evaluate_expr(&d.wb, sheet, at, &e);
        steps.push(json!({"expression": gridcraft_formula::print(&e), "value": crate::cmd::inspect::value_json(&v)}));
    }
    let result = gridcraft_calc::recalc::evaluate_expr(&d.wb, sheet, at, &expr);
    Ok(json!({"formula": format!("={text}"), "steps": steps, "result": crate::cmd::inspect::value_json(&result)}))
}

fn evaluate(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let f = str_param(p, "formula").ok_or_else(|| bad("formulas.evaluate", "missing `formula`"))?;
    let at = cell_param(p, "cell").unwrap_or(d.selection.active);
    let v = gridcraft_calc::evaluate(&d.wb, d.wb.active_sheet, at, f);
    Ok(crate::cmd::inspect::value_json(&v))
}

fn calc_now(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let mut wb = (*d.wb).clone();
    let dirty = d.calc.prepare_all(&mut wb);
    let n = dirty.len();
    super::recalc(d, &mut wb, dirty, &mut Vec::new());
    d.wb = std::sync::Arc::new(wb);
    Ok(json!({"cells": n}))
}

fn calc_options(s: &mut Session, p: &Json) -> Result<Json> {
    let mode = match str_param(p, "mode") {
        Some("manual") => Some(CalcMode::Manual),
        Some("automaticExceptTables") => Some(CalcMode::AutomaticExceptTables),
        Some("automatic") => Some(CalcMode::Automatic),
        _ => None,
    };
    edit(s, |cx| {
        if let Some(m) = mode {
            cx.wb.calc.mode = m;
        }
        if let Some(v) = bool_param(p, "iterative") {
            cx.wb.calc.iterative = v;
        }
        if let Some(v) = u32_param(p, "maxIterations") {
            cx.wb.calc.max_iterations = v.clamp(1, 32767);
        }
        if let Some(v) = f64_param(p, "maxChange") {
            cx.wb.calc.max_change = v.abs();
        }
        if let Some(v) = bool_param(p, "multiThreaded") {
            cx.wb.calc.multi_threaded = v;
        }
        if let Some(v) = u32_param(p, "threads") {
            cx.wb.calc.threads = v.min(1024);
        }
        if mode == Some(CalcMode::Automatic) {
            cx.structural = true;
        }
        Ok(json!({"mode": format!("{:?}", cx.wb.calc.mode), "multiThreaded": cx.wb.calc.multi_threaded, "threads": cx.wb.calc.threads}))
    })
}
