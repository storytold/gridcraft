//! Formulas tab: AutoSum, Insert Function, names, auditing, calculation.

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_model::{CalcMode, DefinedName, Formula};
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
            "{name, refersTo?: \"=Sheet1!$A$1:$A$10\" (default: the selection), scope?: \"Workbook\"|sheet name, comment?, oldName?, oldScope?} (oldName: edit that name (Name Manager › Edit); a new name also renames it in formulas)",
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
        cmd!(noundo "formulas.selectPrecedents", "Select Precedents", ["Formulas", "Formula Auditing"], Some("Ctrl+["), "{cell?, all?: true = every level (Ctrl+Shift+[)} → selects the precedent cells", has_doc, select_precedents),
        cmd!(noundo "formulas.selectDependents", "Select Dependents", ["Formulas", "Formula Auditing"], Some("Ctrl+]"), "{cell?, all?: true = every level (Ctrl+Shift+])}", has_doc, select_dependents),
        cmd!(noundo "formulas.removeArrows", "Remove Arrows", ["Formulas", "Formula Auditing"], None, "{}", has_doc, |_, _| ok()),
        cmd!("formulas.showFormulas", "Show Formulas", ["Formulas", "Formula Auditing"], Some("Ctrl+`"), "{on?}", has_doc, show_formulas),
        cmd!(
            "formulas.convertToNumber",
            "Convert to Number",
            [],
            None,
            "{range?} → {converted} (numbers stored as text in the range or selection become numbers)",
            has_doc,
            convert_to_number
        ),
        cmd!(query "formulas.errorChecking", "Error Checking", ["Formulas", "Formula Auditing"], None, "{} → cells with errors", has_doc, error_checking),
        cmd!(query "formulas.evaluateFormula", "Evaluate Formula", ["Formulas", "Formula Auditing"], None, "{cell?, formula?} → steps", has_doc, evaluate_formula),
        cmd!(query "formulas.evaluate", "Evaluate", [], None, "{formula: \"=SUM(A1:A3)\", cell?} → value without changing the sheet; a formula that couldn't be entered in a cell is an error", has_doc, evaluate),
        cmd!(noundo "formulas.calculateNow", "Calculate Now", ["Formulas", "Calculation"], Some("F9"), "{}", has_doc, calc_now),
        cmd!(noundo "formulas.calculateSheet", "Calculate Sheet", ["Formulas", "Calculation"], Some("Shift+F9"), "{}", has_doc, calc_now),
        cmd!(
            "formulas.calculationOptions",
            "Calculation Options",
            ["Formulas", "Calculation"],
            None,
            "{mode?: automatic|automaticExceptTables|manual, iterative?: bool (Enable iterative calculation), maxIterations?: 1–32767 (100), maxChange?: number (0.001)} → the settings. File › Options › Formulas › Calculation options; stored in the workbook.",
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
        // Columns past the sheet's data get no total (a whole-row selection doesn't write 16,384
        // formulas), and all totals are one edit (one undo step).
        let Some(last_col) = sh.cells.used_range().map(|u| u.end.col.min(sel.end.col)) else { return ok() };
        let last_row_blank = (sel.start.col..=last_col).all(|c| sh.value(CellRef::new(sel.end.row, c)).is_empty());
        let (data_end_row, out_row) = if last_row_blank { (sel.end.row.saturating_sub(1), sel.end.row) } else { (sel.end.row, sel.end.row + 1) };
        if out_row >= gridcraft_core::MAX_ROWS {
            return Err(EngineError::Other("There's no room below the selection for the totals.".into()));
        }
        let writes: Vec<(CellRef, String)> = (sel.start.col..=last_col)
            .map(|c| {
                (CellRef::new(out_row, c), format!("={func}({})", RangeRef::new(CellRef::new(sel.start.row, c), CellRef::new(data_end_row, c)).a1()))
            })
            .collect();
        let sheet = d.wb.active_sheet;
        edit(s, |cx| {
            for (c, f) in &writes {
                let cell = super::edit::input_to_cell(f, sheet, *c, &mut cx.wb)?.unwrap_or_default();
                cx.sheet_mut(sheet)?.set_cell(*c, cell);
                cx.touch(sheet, *c);
            }
            Ok(())
        })?;
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
        ("TRIMRANGE", "Lookup", "TRIMRANGE(range, [trim_rows], [trim_cols])", "A range or array without its blank outer rows and columns."),
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
    if n.chars().count() > 255 || CellRef::parse(n).is_some() || is_r1c1(n) {
        return false;
    }
    n.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\' | '?'))
}

/// Looks like an R1C1 reference (`R`, `C`, `RC`, `R2`, `C3`, `R1C1`), which Excel doesn't
/// accept as a name.
pub(crate) fn is_r1c1(n: &str) -> bool {
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
        // As in Excel, relative references are relative to the active cell; names keep them
        // relative to A1, as files do.
        Some(r) => match gridcraft_formula::parse_name(r.trim_start_matches('=')) {
            Ok(e) if gridcraft_formula::adjust::has_relative_refs(&e) => {
                let at = d.selection.active;
                gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative_wrapping(e, -(at.row as i64), -(at.col as i64)))
            }
            _ => r.trim_start_matches('=').to_string(),
        },
        None => {
            let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
            let r = d.selection.current();
            let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1);
            let body = if r.is_single() { abs(r.start) } else { format!("{}:{}", abs(r.start), abs(r.end)) };
            format!("{}!{}", gridcraft_formula::quote_sheet(&sh.name), body)
        }
    };
    if gridcraft_formula::parse_name(&refers).is_err() {
        return Err(EngineError::Other("There's a problem with this formula.".into()));
    }
    let comment = str_param(p, "comment").unwrap_or("").to_string();
    // Editing an existing name (Name Manager › Edit…): it must exist, and its new name must not
    // collide with another name in the target scope.
    let old = match str_param(p, "oldName") {
        Some(old_name) => {
            let old_scope = match str_param(p, "oldScope") {
                None | Some("Workbook") => None,
                Some(sh) => Some(d.wb.sheet_index(sh).ok_or_else(|| bad("formulas.defineName", "no such sheet"))?),
            };
            if !d.wb.names.iter().any(|n| n.name.eq_ignore_ascii_case(old_name) && n.scope == old_scope) {
                return Err(bad("formulas.defineName", "no such name to edit"));
            }
            let same = old_name.eq_ignore_ascii_case(&name) && old_scope == scope;
            if !same && d.wb.names.iter().any(|n| n.name.eq_ignore_ascii_case(&name) && n.scope == scope) {
                return Err(EngineError::Other("The name entered already exists. Enter a unique name.".into()));
            }
            Some((old_name.to_string(), old_scope))
        }
        None => None,
    };
    edit(s, |cx| {
        if let Some((old_name, old_scope)) = &old {
            cx.wb.names.retain(|n| !(n.name.eq_ignore_ascii_case(old_name) && n.scope == *old_scope));
            if !old_name.eq_ignore_ascii_case(&name) {
                rename_name_in_formulas(&mut cx.wb, old_name, *old_scope, &name);
            }
        }
        cx.wb.names.retain(|n| !(n.name.eq_ignore_ascii_case(&name) && n.scope == scope));
        cx.wb.names.push(DefinedName { name: name.clone(), scope, formula: refers.clone(), comment: comment.clone(), hidden: false });
        cx.structural = true;
        Ok(json!({"name": name, "refersTo": format!("={refers}")}))
    })
}

/// Formulas that use a renamed name follow it: cell formulas and other names' definitions, on
/// the sheets where `old` resolves to the renamed name (its own sheet for a sheet-scoped name;
/// every sheet without a same-named sheet-level name for a workbook-level one).
fn rename_name_in_formulas(wb: &mut gridcraft_model::Workbook, old: &str, old_scope: Option<usize>, new: &str) {
    let shadowed: Vec<usize> = wb.names.iter().filter(|n| n.name.eq_ignore_ascii_case(old)).filter_map(|n| n.scope).collect();
    let applies = |sheet: usize| match old_scope {
        Some(s) => s == sheet,
        None => !shadowed.contains(&sheet),
    };
    // The renamed expression, or `None` when the name doesn't occur in it.
    let rename = |e: gridcraft_formula::Expr| {
        let mut hit = false;
        let out = e.map(&mut |x| match x {
            gridcraft_formula::Expr::Name(n) if n.eq_ignore_ascii_case(old) => {
                hit = true;
                gridcraft_formula::Expr::Name(new.to_string())
            }
            o => o,
        });
        hit.then_some(out)
    };
    for i in 0..wb.sheets.len() {
        if !applies(i) {
            continue;
        }
        let Some(sh) = wb.sheet_mut(i) else { continue };
        let formulas: Vec<(CellRef, std::sync::Arc<Formula>)> = sh.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect();
        for (c, f) in formulas {
            let Some(e) = f.expr().and_then(rename) else { continue };
            let mut nf = Formula::from_expr(e);
            nf.array = f.array;
            nf.dynamic = f.dynamic;
            if let Some(cell) = sh.cells.get_mut(c) {
                cell.formula = Some(std::sync::Arc::new(nf));
            }
        }
    }
    for n in wb.names.iter_mut() {
        if n.scope.is_none_or(applies)
            && let Some(e) = gridcraft_formula::parse_name(&n.formula).ok().and_then(rename)
        {
            n.formula = gridcraft_formula::print(&e);
        }
    }
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
            // parse_name, not evaluate's parse: a name's top-level commas are unions.
            let v = gridcraft_formula::parse_name(&n.formula)
                .map_or(Value::Error(CellError::Name), |e| gridcraft_calc::recalc::evaluate_expr(&d.wb, sheet, CellRef::default(), &e));
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
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let areas = d.calc.graph.precedents_of((d.wb.active_sheet, at));
    let v: Vec<Json> = areas.iter().map(|a| json!({"sheet": d.wb.sheet(a.sheet).map(|s| s.name.clone()), "range": a.range.a1()})).collect();
    Ok(Json::Array(v))
}

fn trace_dependents(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let mut deps = Vec::new();
    d.calc.graph.dependents(d.wb.active_sheet, at, &mut deps);
    deps.sort();
    deps.dedup();
    let v: Vec<Json> = deps.iter().map(|(si, c)| json!({"sheet": d.wb.sheet(*si).map(|s| s.name.clone()), "cell": c.a1()})).collect();
    Ok(Json::Array(v))
}

/// Excel's Ctrl+[ / Ctrl+]: select the precedent (or dependent) cells. When they all live on
/// another sheet, jump there. `all` walks every level (Ctrl+Shift+[ / Ctrl+Shift+]).
fn select_trace_cells(s: &mut Session, p: &Json, precedents: bool) -> Result<Json> {
    use std::collections::BTreeSet;
    let d = s.doc()?;
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let sheet = d.wb.active_sheet;
    let all = bool_param(p, "all").unwrap_or(false);
    // (sheet, a1) pairs to select; direct level keeps whole precedent ranges, transitive cells.
    let mut found: BTreeSet<(usize, String)> = BTreeSet::new();
    let mut seen: BTreeSet<(usize, CellRef)> = BTreeSet::new();
    let mut frontier = vec![(sheet, at)];
    seen.insert((sheet, at));
    while let Some((ks, kc)) = frontier.pop() {
        if precedents {
            for a in d.calc.graph.precedents_of((ks, kc)) {
                if all {
                    // Walk only the stored cells inside the precedent area (sparse
                    // range iterator, not the whole sheet): blanks are never selected
                    // and only formula cells extend the frontier, so a formula late in
                    // a large range is not skipped.
                    let Some(sh) = d.wb.sheet(a.sheet) else { continue };
                    for (c, cell) in sh.cells.iter_range(a.range) {
                        if matches!(cell.value, Value::Empty) {
                            continue;
                        }
                        if seen.insert((a.sheet, c)) {
                            found.insert((a.sheet, c.a1()));
                            if cell.formula.is_some() {
                                frontier.push((a.sheet, c));
                            }
                        }
                    }
                } else {
                    found.insert((a.sheet, a.range.a1()));
                }
            }
        } else {
            let mut deps = Vec::new();
            d.calc.graph.dependents(ks, kc, &mut deps);
            for k2 in deps {
                if all {
                    if seen.insert(k2) {
                        found.insert((k2.0, k2.1.a1()));
                        frontier.push(k2);
                    }
                } else {
                    found.insert((k2.0, k2.1.a1()));
                }
            }
        }
        if !all {
            break;
        }
    }
    found.remove(&(sheet, at.a1()));
    let name = if precedents { "precedent" } else { "dependent" };
    let Some(si) = found.iter().find(|(si, _)| *si == sheet).or_else(|| found.first()).map(|(si, _)| *si) else {
        return Err(bad("cell", format!("No {name} cells.")));
    };
    let list: Vec<String> = found.iter().filter(|(s, _)| *s == si).map(|(_, a)| a.clone()).collect();
    let mut params = json!({ "range": list.join(",") });
    if si != sheet {
        params["sheet"] = json!(si);
    }
    s.execute("selection.set", params)
}

fn select_precedents(s: &mut Session, p: &Json) -> Result<Json> {
    select_trace_cells(s, p, true)
}

fn select_dependents(s: &mut Session, p: &Json) -> Result<Json> {
    select_trace_cells(s, p, false)
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

/// The number a cell's text stands for when Excel's background check flags it as a number stored
/// as text: a constant (not a formula's result) that would be a number if typed.
pub fn number_stored_as_text(cell: &gridcraft_model::Cell) -> Option<f64> {
    match &cell.value {
        Value::Text(t) if cell.formula.is_none() => gridcraft_core::parse::number_stored_as_text(t),
        _ => None,
    }
}

/// Convert to Number: each number stored as text in the range becomes that number, keeping the
/// cell's format.
fn convert_to_number(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    edit(s, |cx| {
        let mut converted = 0;
        for r in &ranges {
            let Some(r) = cx.wb.sheet(sheet).and_then(|sh| sh.used_range()).and_then(|u| r.intersection(&u)) else { continue };
            let found: Vec<(CellRef, f64)> = cx
                .wb
                .sheet(sheet)
                .map(|sh| sh.cells.iter_range(r).filter_map(|(c, x)| number_stored_as_text(x).map(|n| (c, n))).collect())
                .unwrap_or_default();
            let sh = cx.sheet_mut(sheet)?;
            for (c, n) in &found {
                sh.set_value(*c, Value::number(*n));
            }
            cx.changed.extend(found.iter().map(|(c, _)| (sheet, *c)));
            converted += found.len();
        }
        Ok(json!({"converted": converted}))
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
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
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
        // Borrowed, not cloned: copying every operator of a long chain A1+A2+… copies the
        // rest of the chain each time (quadratic memory).
        if matches!(e, gridcraft_formula::Expr::Call(..) | gridcraft_formula::Expr::Binary(..) | gridcraft_formula::Expr::Ref(_)) {
            nodes.push(e);
        }
    });
    for e in nodes.into_iter().rev().take(200) {
        let v = gridcraft_calc::recalc::evaluate_expr(&d.wb, sheet, at, e);
        steps.push(json!({"expression": gridcraft_formula::print(e), "value": crate::cmd::inspect::value_json(&v)}));
    }
    let result = gridcraft_calc::recalc::evaluate_expr(&d.wb, sheet, at, &expr);
    Ok(json!({"formula": format!("={text}"), "steps": steps, "result": crate::cmd::inspect::value_json(&result)}))
}

/// Evaluates what typing `formula` into a cell would show. A formula `cell.set` would refuse (it
/// doesn't parse, or calls a function with the wrong number of arguments) is an error that says
/// why, not `#NAME?`; an unknown function name still evaluates to `#NAME?`, as in Excel.
fn evaluate(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let f = str_param(p, "formula").ok_or_else(|| bad("formulas.evaluate", "missing `formula`"))?;
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let expr = crate::cmd::edit::parse_typed_formula(f.strip_prefix('=').unwrap_or(f))?;
    crate::cmd::edit::check_arg_counts(&expr)?;
    let v = gridcraft_calc::recalc::evaluate_expr(&d.wb, d.wb.active_sheet, at, &expr);
    Ok(crate::cmd::inspect::value_json(&v))
}

fn calc_now(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let mut wb = (*d.wb).clone();
    d.calc.recalc_all(&mut wb);
    d.wb = std::sync::Arc::new(wb);
    Ok(json!({"cells": d.calc.last_recalc_cells}))
}

fn calc_options(s: &mut Session, p: &Json) -> Result<Json> {
    let mode = match str_param(p, "mode") {
        Some("manual") => Some(CalcMode::Manual),
        Some("automaticExceptTables") => Some(CalcMode::AutomaticExceptTables),
        Some("automatic") => Some(CalcMode::Automatic),
        _ => None,
    };
    edit(s, |cx| {
        let before = cx.wb.calc.clone();
        if let Some(m) = mode {
            cx.wb.calc.mode = m;
        }
        if let Some(v) = bool_param(p, "iterative") {
            cx.wb.calc.iterative = v;
        }
        if let Some(v) = u32_param(p, "maxIterations") {
            cx.wb.calc.set_max_iterations(v);
        }
        if let Some(v) = f64_param(p, "maxChange") {
            cx.wb.calc.set_max_change(v);
        }
        let c = &cx.wb.calc;
        // Switching to automatic, or changing how circular references are calculated outside
        // manual mode, recalculates the workbook.
        let iteration_changed = (c.iterative, c.max_iterations, c.max_change) != (before.iterative, before.max_iterations, before.max_change);
        if mode == Some(CalcMode::Automatic) || (iteration_changed && c.mode != CalcMode::Manual) {
            cx.structural = true;
        }
        Ok(json!({"mode": format!("{:?}", c.mode), "iterative": c.iterative, "maxIterations": c.max_iterations, "maxChange": c.max_change}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_with_relative_references() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
        // Defined from B2, `=Sheet1!A2` is "the cell to the left".
        s.execute("selection.set", json!({"range": "B2"})).unwrap();
        let r = s.execute("formulas.defineName", json!({"name": "LeftCell", "refersTo": "=Sheet1!A2"})).unwrap();
        assert_eq!(r["refersTo"], "=Sheet1!XFD1");
        s.execute("cell.set", json!({"cell": "B1", "input": "=LeftCell"})).unwrap();
        s.execute("cell.set", json!({"cell": "B3", "input": "=LeftCell*10"})).unwrap();
        let v = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!(v(&mut s, "B1"), json!(1.0));
        assert_eq!(v(&mut s, "B3"), json!(30.0));
        // Absolute references are kept as they are.
        let r = s.execute("formulas.defineName", json!({"name": "First", "refersTo": "=Sheet1!$A$1"})).unwrap();
        assert_eq!(r["refersTo"], "=Sheet1!$A$1");
    }

    #[test]
    fn arabic_defined_names_count_characters_not_bytes() {
        let mut s = Session::new();
        s.new_workbook();
        let arabic = |n: usize| "س".repeat(n);
        for ok in [200, 255] {
            s.execute("formulas.defineName", json!({"name": arabic(ok), "refersTo": "=1"})).unwrap();
        }
        assert!(s.execute("formulas.defineName", json!({"name": arabic(256), "refersTo": "=1"})).is_err());
        s.execute("range.setValues", json!({"range": "A1", "values": [[format!("ع{}", arabic(199))], [1]]})).unwrap();
        let r = s.execute("formulas.createFromSelection", json!({"range": "A1:A2", "top": true})).unwrap();
        assert_eq!(r["created"], 1);
    }

    #[test]
    fn names_referring_to_several_ranges() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1:C2", "values": [[1, 0, 5], [2, 0, 7]]})).unwrap();
        // Excel writes and accepts a union in a name without parentheses.
        let r = s.execute("formulas.defineName", json!({"name": "u", "refersTo": "=Sheet1!$A$1:$A$2,Sheet1!$C$1:$C$2"})).unwrap();
        assert_eq!(r["refersTo"], "=Sheet1!$A$1:$A$2,Sheet1!$C$1:$C$2");
        for (cell, input) in [("E1", "=SUM(u)"), ("E2", "=AREAS(u)"), ("E3", "=MIN(u)"), ("E4", "=INDEX(u,2,1,2)")] {
            s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
        }
        let v = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!(v(&mut s, "E1"), json!(15.0));
        assert_eq!(v(&mut s, "E2"), json!(2.0));
        assert_eq!(v(&mut s, "E3"), json!(1.0));
        assert_eq!(v(&mut s, "E4"), json!(7.0));
        // Renaming the sheet follows every area, and the definition keeps Excel's form.
        s.execute("sheet.rename", json!({"name": "Data"})).unwrap();
        let name = s.doc().unwrap().wb.names.iter().find(|n| n.name == "u").map(|n| n.formula.clone());
        assert_eq!(name.as_deref(), Some("Data!$A$1:$A$2,Data!$C$1:$C$2"));
        assert_eq!(v(&mut s, "E1"), json!(15.0));
    }

    #[test]
    fn name_manager_previews_a_name_over_several_ranges() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1:C2", "values": [[1, 0, 5], [2, 0, 7]]})).unwrap();
        s.execute("formulas.defineName", json!({"name": "u", "refersTo": "=Sheet1!$A$1:$A$2,Sheet1!$C$1:$C$2"})).unwrap();
        let list = s.execute("formulas.nameManager", json!({})).unwrap();
        let u = list.as_array().and_then(|l| l.iter().find(|n| n["name"] == "u")).cloned().unwrap();
        // The union parses as one, so the preview is what the union gives in a cell, not #NAME?.
        assert_eq!(u["value"], json!({"error": "#VALUE!"}));
    }
}
