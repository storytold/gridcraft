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
        cmd!(query "formulas.functions", "List Functions", [], None, "{category?: Financial|Logical|Text|\"Date & Time\"|\"Lookup & Reference\"|\"Math & Trig\"|Statistical|Engineering|Information|Database|Compatibility|Web|Cube (Excel labels and engine names both work), search?} → [{name (canonical), localName (session formula language), category, signature, description}]", always, list_functions),
        cmd!(
            "formulas.defineName",
            "Define Name…",
            ["Formulas", "Defined Names"],
            None,
            "{name, refersTo?: \"=Sheet1!$A$1:$A$10\" (canonical) | refersToLocal?: (session language and separators), locale?, scope?: \"Workbook\"|sheet name, comment?}",
            has_doc,
            define_name
        ),
        cmd!("formulas.deleteName", "Delete Name", [], None, "{name, scope?: \"Workbook\"|sheet name (default: every scope)}", has_doc, delete_name),
        cmd!(query "formulas.nameManager", "Name Manager", ["Formulas", "Defined Names"], Some("Cmd+F3"), "{} → [{name, refersTo (canonical), refersToLocal, scope (`Workbook` or the sheet name), value, valueLocal (spelled in the session's language and region), comment}]", has_doc, name_manager),
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
        cmd!(query "formulas.errorChecking", "Error Checking", ["Formulas", "Formula Auditing"], None, "{} → {errors: [{sheet, cell, error, errorLocal, formula (canonical), formulaLocal}], circular}; the Local fields are spelled in the session's language and region", has_doc, error_checking),
        cmd!(query "formulas.evaluateFormula", "Evaluate Formula", ["Formulas", "Formula Auditing"], None, "{cell?, formula?|formulaLocal? (as typed in the session's language and region, or the `locale` tag)} → {formula, formulaLocal, steps: [{expression, expressionLocal, value, valueLocal}], result, resultLocal}", has_doc, evaluate_formula),
        cmd!(query "formulas.evaluate", "Evaluate", [], None, "{formula: \"=SUM(A1:A3)\" | formulaLocal: \"=SOMA(A1:A3)\" (as typed in the session's language and region, or the `locale` tag), cell?} → value without changing the sheet", has_doc, evaluate),
        cmd!(noundo "formulas.calculateNow", "Calculate Now", ["Formulas", "Calculation"], Some("F9"), "{}", has_doc, calc_now),
        cmd!(noundo "formulas.calculateSheet", "Calculate Sheet", ["Formulas", "Calculation"], Some("Shift+F9"), "{}", has_doc, calc_now),
        cmd!(
            "formulas.calculationOptions",
            "Calculation Options",
            ["Formulas", "Calculation"],
            None,
            "{mode: automatic|automaticExceptTables|manual, iterative?, maxIterations?, maxChange?}",
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
            let local =
                crate::locale::to_local_formula(&format!("={func}()"), &s.locale().dialect(), &|n: &str| d.wb.knows_name(n, d.wb.active_sheet));
            s.ui_requests.push(crate::UiRequest::EditCell(Some(local)));
            return ok();
        }
    };
    if p.get("enter").and_then(Json::as_bool) == Some(false) {
        let local = crate::locale::to_local_formula(&text, &s.locale().dialect(), &|n: &str| d.wb.knows_name(n, d.wb.active_sheet));
        s.ui_requests.push(crate::UiRequest::EditCell(Some(local)));
        return Ok(json!({"formula": text}));
    }
    s.execute("cell.set", json!({"cell": at.a1(), "input": text}))?;
    Ok(json!({"formula": text}))
}

fn insert_function(s: &mut Session, p: &Json) -> Result<Json> {
    match str_param(p, "name") {
        Some(n) => {
            // Spelled through an empty call so a workbook name with the function's local
            // spelling keeps it reading as the function (`=_xlfn.SUM(`).
            let call = format!("={}()", n.to_ascii_uppercase());
            let local = match s.doc() {
                Ok(d) => crate::locale::to_local_formula(&call, &s.locale().dialect(), &|n: &str| d.wb.knows_name(n, d.wb.active_sheet)),
                Err(_) => crate::locale::to_local_formula(&call, &s.locale().dialect(), &gridcraft_formula::no_names),
            };
            let text = local.strip_suffix(')').unwrap_or(&local).to_string();
            s.ui_requests.push(crate::UiRequest::EditCell(Some(text)));
            ok()
        }
        None => {
            s.ui_requests.push(crate::UiRequest::Dialog("insertFunction".into(), json!({})));
            ok()
        }
    }
}

/// Every function: canonical `name`, `localName` (as typed in the invariant language), category,
/// signature and description.
pub fn function_list() -> Vec<Json> {
    function_list_in(gridcraft_locale::INVARIANT.formula)
}

/// [`function_list`] with `localName` as `lang` spells it (the session's formula language).
pub fn function_list_in(lang: &gridcraft_locale::Language) -> Vec<Json> {
    gridcraft_functions::docs()
        .iter()
        .map(|f| {
            json!({
                "name": f.name,
                "localName": lang.local_function(f.name).unwrap_or(f.name),
                "category": format!("{:?}", f.category),
                "signature": f.signature,
                "description": f.description,
            })
        })
        .collect()
}

/// A category as a comparison key: Excel's labels (`Math & Trig`, `Lookup & Reference`,
/// `Date & Time`) and the engine's names (`MathTrig`, `Lookup`, `DateTime`) agree.
fn category_key(name: &str) -> String {
    let key: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    match key.as_str() {
        "lookupreference" | "lookupandreference" => "lookup".into(),
        "dateandtime" => "datetime".into(),
        "mathandtrig" | "mathtrigonometry" => "mathtrig".into(),
        _ => key,
    }
}

fn list_functions(s: &mut Session, p: &Json) -> Result<Json> {
    let cat = str_param(p, "category").map(category_key);
    let q = str_param(p, "search").map(str::to_lowercase);
    let v: Vec<Json> = function_list_in(s.locale().formula)
        .into_iter()
        .filter(|f| cat.as_ref().is_none_or(|c| f["category"].as_str().is_some_and(|x| category_key(x) == *c)))
        .filter(|f| {
            q.as_ref().is_none_or(|q| {
                ["name", "localName", "description"].iter().any(|k| f[*k].as_str().is_some_and(|x| x.to_lowercase().contains(q.as_str())))
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
    let call_loc = crate::locale::call_locale(s, p)?;
    // The name being defined is not known yet: `SUMA` refers to SUM until it exists.
    let known = |n: &str| d.wb.knows_name(n, scope.unwrap_or(d.wb.active_sheet));
    let refers = match crate::locale::canonical_formula_param(p, "refersTo", &call_loc, &known)? {
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
    let sheet = scope.unwrap_or(d.wb.active_sheet);
    edit(s, |cx| {
        cx.wb.names.retain(|n| !(n.name.eq_ignore_ascii_case(&name) && n.scope == scope));
        cx.wb.names.push(DefinedName { name: name.clone(), scope, formula: refers.clone(), comment: comment.clone(), hidden: false });
        cx.structural = true;
        let local = crate::locale::to_local_body(&refers, &call_loc.dialect(), &|n: &str| cx.wb.knows_name(n, sheet));
        Ok(json!({"name": name, "refersTo": format!("={refers}"), "refersToLocal": format!("={local}")}))
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
            let local = crate::locale::to_local_body(&n.formula, &d.wb.locale.dialect(), &|x: &str| d.wb.knows_name(x, sheet));
            json!({"name": n.name, "refersTo": format!("={}", n.formula), "refersToLocal": format!("={local}"), "scope": n.scope.and_then(|i| d.wb.sheet(i)).map(|s| s.name.clone()).unwrap_or_else(|| "Workbook".into()), "value": crate::cmd::inspect::value_json(&v), "valueLocal": crate::locale::value_local(&v, &d.wb.locale), "comment": n.comment})
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
    let loc = &d.wb.locale;
    let dialect = loc.dialect();
    let mut out = Vec::new();
    for (si, sh) in d.wb.sheets.iter().enumerate() {
        let known = |n: &str| d.wb.knows_name(n, si);
        for (c, cell) in sh.cells.iter() {
            if let Some(e) = cell.value.as_error() {
                let formula = cell.formula.as_ref().filter(|_| !crate::display::formula_hidden(&d.wb, sh, c)).map(|f| format!("={}", f.text));
                let formula_local = formula.as_deref().map(|f| crate::locale::to_local_formula(f, &dialect, &known));
                out.push(json!({
                    "sheet": sh.name,
                    "cell": c.a1(),
                    "error": e.as_str(),
                    "errorLocal": loc.formula.local_error(e.as_str()),
                    "formula": formula,
                    "formulaLocal": formula_local,
                }));
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
    let loc = crate::locale::call_locale(s, p)?;
    let d = s.doc()?;
    let at = cell_param(p, "cell").unwrap_or(d.selection.active);
    let sheet = d.wb.active_sheet;
    let known = |n: &str| d.wb.knows_name(n, sheet);
    let text = match crate::locale::canonical_formula_param(p, "formula", &loc, &known)? {
        Some(f) => f.trim_start_matches('=').to_string(),
        None => {
            let sh = d.wb.sheet(sheet);
            if sh.is_some_and(|sh| crate::display::formula_hidden(&d.wb, sh, at)) {
                return Err(EngineError::Protected);
            }
            sh.and_then(|sh| sh.cell(at))
                .and_then(|c| c.formula.as_ref())
                .map(|f| f.text.clone())
                .ok_or_else(|| EngineError::Other("The cell has no formula.".into()))?
        }
    };
    let expr = gridcraft_formula::parse(&text).map_err(|e| EngineError::Other(e.to_string()))?;
    let dialect = loc.dialect();
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
        steps.push(json!({
            "expression": gridcraft_formula::print(&e),
            "expressionLocal": gridcraft_formula::print_local_with(&e, &dialect, &known),
            "value": crate::cmd::inspect::value_json(&v),
            "valueLocal": crate::locale::value_local(&v, &loc),
        }));
    }
    let result = gridcraft_calc::recalc::evaluate_expr(&d.wb, sheet, at, &expr);
    Ok(json!({
        "formula": format!("={text}"),
        "formulaLocal": format!("={}", gridcraft_formula::print_local_with(&expr, &dialect, &known)),
        "steps": steps,
        "result": crate::cmd::inspect::value_json(&result),
        "resultLocal": crate::locale::value_local(&result, &loc),
    }))
}

fn evaluate(s: &mut Session, p: &Json) -> Result<Json> {
    let loc = crate::locale::call_locale(s, p)?;
    let d = s.doc()?;
    let f = crate::locale::canonical_formula_param(p, "formula", &loc, &|n: &str| d.wb.knows_name(n, d.wb.active_sheet))?
        .ok_or_else(|| bad("formulas.evaluate", "missing `formula` or `formulaLocal`"))?;
    let at = cell_param(p, "cell").unwrap_or(d.selection.active);
    let v = gridcraft_calc::evaluate(&d.wb, d.wb.active_sheet, at, &f);
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
        if mode == Some(CalcMode::Automatic) {
            cx.structural = true;
        }
        Ok(json!({"mode": format!("{:?}", cx.wb.calc.mode)}))
    })
}
