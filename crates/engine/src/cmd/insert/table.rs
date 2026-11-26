use super::{
    Axis, Cell, CellRef, Edit, EngineError, Formula, Json, MAX_ROWS, RangeRef, Result, Session, Sheet, Style, Table, TableColumn, TotalsFn, bad,
    block, bool_param, edit, json, rewrite, shift_cell_features, str_param, target_range, target_sheet, with,
};

#[cfg(test)]
pub(super) fn compiled() -> bool {
    true
}

// ---------------------------------------------------------------- tables

pub(super) fn insert_table(s: &mut Session, p: &Json) -> Result<Json> {
    let r = block(s, p)?;
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    if d.wb.sheet(sheet).is_some_and(|sh| sh.tables.iter().any(|t| t.range.intersects(&r))) {
        return Err(EngineError::Other("A table can't overlap another table.".into()));
    }
    let arrays = |sh: &gridcraft_model::Sheet| {
        sh.cells.iter().any(|(_, x)| x.formula.as_ref().and_then(|f| f.array).is_some_and(|a| !a.is_single() && a.intersects(&r)))
    };
    if d.wb.sheet(sheet).is_some_and(arrays) {
        return Err(EngineError::Other("Multi-cell array formulas aren't allowed in tables.".into()));
    }
    if r.count() > 50_000_000 {
        return Err(bad("insert.table", "range too large"));
    }
    let header = bool_param(p, "header").unwrap_or_else(|| {
        let sh = d.wb.sheet(sheet);
        sh.is_some_and(|sh| (r.start.col..=r.end.col).all(|c| sh.value(CellRef::new(r.start.row, c)).is_text()) && r.height() > 1)
    });
    let name = str_param(p, "name")
        .map(str::to_string)
        .unwrap_or_else(|| (1..).map(|n| format!("Table{n}")).find(|n| d.wb.table(n).is_none()).unwrap_or_else(|| "Table".into()));
    if !valid_table_name(&name) {
        return Err(EngineError::Other("The name that you entered is not valid.".into()));
    }
    if d.wb.table(&name).is_some() || d.wb.names.iter().any(|n| n.name.eq_ignore_ascii_case(&name)) {
        return Err(EngineError::Other("The name entered already exists. Enter a unique name.".into()));
    }
    // Without a header row, the data moves down a row to make room for one: never a table with it.
    let moves_a_table = |sh: &gridcraft_model::Sheet| {
        sh.tables.iter().any(|t| t.range.end.row >= r.start.row && t.range.start.col <= r.end.col && t.range.end.col >= r.start.col)
    };
    if !header && d.wb.sheet(sheet).is_some_and(moves_a_table) {
        return Err(EngineError::Other("This operation isn't allowed: it would shift cells in a table on your worksheet.".into()));
    }
    let style = str_param(p, "style").unwrap_or("TableStyleMedium2").to_string();
    let id = d.wb.next_object_id();
    edit(s, |cx| {
        // Without a header row, insert one with generic names.
        let r = if header {
            r
        } else {
            let sh = cx.sheet_mut(sheet)?;
            // The header row pushes the data down one row: never off the end of the sheet.
            let last = RangeRef::new(CellRef::new(MAX_ROWS - 1, r.start.col), CellRef::new(MAX_ROWS - 1, r.end.col));
            if sh.cells.iter_range(last).any(|(_, x)| x.has_content()) {
                return Err(EngineError::Other("To prevent possible loss of data, cells can't be pushed off the end of the worksheet.".into()));
            }
            sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.start.row, 1);
            // Like Insert Cells > Shift down: move merges, conditional formats, notes, links and pictures, and re-point every formula
            // reference (on any sheet) at the cells that moved.
            let e = Edit::InsertCells { axis: Axis::Rows, range: RangeRef::new(r.start, CellRef::new(r.start.row, r.end.col)) };
            shift_cell_features(sh, &e);
            let name = sh.name.clone();
            rewrite(cx, &name, &e)?;
            let sh = cx.sheet_mut(sheet)?;
            for (i, c) in (r.start.col..=r.end.col).enumerate() {
                sh.set_value(CellRef::new(r.start.row, c), gridcraft_core::Value::text(format!("Column{}", i + 1)));
            }
            cx.structural = true;
            RangeRef::new(r.start, CellRef::new((r.end.row + 1).min(MAX_ROWS - 1), r.end.col))
        };
        let sh = cx.sheet_mut(sheet)?;
        let mut names: Vec<String> = Vec::new();
        // Lower-case names taken, and the next suffix for each header, so that thousands of equal
        // headers don't take cubic time.
        let mut taken = std::collections::HashSet::new();
        let mut next_suffix = std::collections::HashMap::new();
        for c in r.start.col..=r.end.col {
            let mut n = sh.value(CellRef::new(r.start.row, c)).display();
            if n.is_empty() {
                n = format!("Column{}", c - r.start.col + 1);
            }
            let base = n.clone();
            let key = base.to_ascii_lowercase();
            let mut k = next_suffix.get(&key).copied().unwrap_or(2);
            while taken.contains(&n.to_ascii_lowercase()) {
                n = format!("{base}{k}");
                k += 1;
            }
            next_suffix.insert(key, k);
            taken.insert(n.to_ascii_lowercase());
            // Headers are text.
            sh.set_value(CellRef::new(r.start.row, c), gridcraft_core::Value::text(n.as_str()));
            names.push(n);
        }
        sh.tables.push(Table {
            id,
            name: name.clone(),
            range: r,
            header_row: true,
            totals_row: false,
            columns: names
                .into_iter()
                .map(|n| TableColumn { name: n, totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None })
                .collect(),
            style,
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
            filter: vec![],
            table_type: None,
            connection_id: None,
        });
        cx.structural = true;
        Ok(json!({"table": name, "range": r.a1()}))
    })
}

/// Format as Table inside a table restyles that table, as in Excel; elsewhere it makes a new one.
pub(super) fn format_as_table(s: &mut Session, p: &Json) -> Result<Json> {
    // Without a range, the active area decides (as in insert_table), not the first one.
    let r = if p.get("range").is_some() { target_range(s, p)? } else { s.doc()?.selection.current() };
    let sheet = target_sheet(s, p)?;
    let inside = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.tables.iter().find(|t| t.range.contains_range(&r))).map(|t| (t.name.clone(), t.range));
    match inside {
        Some((table, range)) => {
            table_style(s, &with(p, "table", json!(table)))?;
            Ok(json!({"table": table, "range": range.a1()}))
        }
        None => insert_table(s, p),
    }
}

fn find_table(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    if let Some(n) = str_param(p, "table") {
        return d.wb.table(n).ok_or_else(|| bad("table", format!("no table named `{n}`")));
    }
    let sheet = d.wb.active_sheet();
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let ti = sh
        .tables
        .iter()
        .position(|t| t.range.contains(d.selection.active))
        .ok_or_else(|| EngineError::Disabled("table".into(), "select a cell in a table".into()))?;
    Ok((sheet, ti))
}

pub(super) fn table_flag(s: &mut Session, p: &Json, which: &str) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let on = bool_param(p, "on");
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let Some(t) = sh.tables.get_mut(ti) else { return Ok(Json::Null) };
        match which {
            "totals" => {
                let v = on.unwrap_or(!t.totals_row);
                if v != t.totals_row {
                    if v && t.range.end.row.saturating_add(1) >= MAX_ROWS {
                        return Err(EngineError::Other("There's no room below the table for the total row.".into()));
                    }
                    t.totals_row = v;
                    if v {
                        t.range.end.row += 1;
                        if let Some(first) = t.columns.first_mut() {
                            first.totals_label = Some("Total".into());
                        }
                        if t.columns.len() > 1
                            && let Some(last) = t.columns.last_mut()
                            && last.totals == TotalsFn::None
                        {
                            last.totals = TotalsFn::Sum;
                        }
                    } else {
                        let row = t.range.end.row;
                        t.range.end.row = t.range.end.row.saturating_sub(1);
                        let (c0, c1) = (t.range.start.col, t.range.end.col);
                        for c in c0..=c1 {
                            sh.remove_cell(CellRef::new(row, c));
                        }
                    }
                    write_totals(sh, ti);
                    cx.structural = true;
                }
            }
            "bandedRows" => t.banded_rows = on.unwrap_or(!t.banded_rows),
            "bandedCols" => t.banded_cols = on.unwrap_or(!t.banded_cols),
            "firstCol" => t.first_col = on.unwrap_or(!t.first_col),
            "lastCol" => t.last_col = on.unwrap_or(!t.last_col),
            "header" => {
                t.filter_button = on.unwrap_or(!t.filter_button);
                if !t.filter_button {
                    t.filter.clear();
                }
            }
            _ => {}
        }
        Ok(Json::Null)
    })
}

/// Writes the totals row cells (labels and SUBTOTAL formulas).
fn write_totals(sh: &mut Sheet, ti: usize) {
    let Some(t) = sh.tables.get(ti).cloned() else { return };
    if !t.totals_row {
        return;
    }
    let row = t.range.end.row;
    for (i, col) in t.columns.iter().enumerate() {
        let c = CellRef::new(row, t.range.start.col + i as u32);
        if let Some(code) = col.totals.subtotal_code() {
            let f = format!("=SUBTOTAL({code},{}[{}])", t.name, col.name);
            sh.set_cell(
                c,
                Cell { formula: Some(std::sync::Arc::new(Formula::new(&f))), value: gridcraft_core::Value::Empty, style: sh.style_id(c), py: None },
            );
        } else if let Some(l) = &col.totals_label {
            sh.set_value(c, gridcraft_core::Value::text(l.as_str()));
        } else {
            sh.remove_cell(c);
        }
    }
}

pub(super) fn table_total_fn(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let col = str_param(p, "column").ok_or_else(|| bad("table.totalFunction", "missing `column`"))?.to_string();
    let f = match str_param(p, "function").unwrap_or("sum") {
        "average" => TotalsFn::Average,
        "count" => TotalsFn::Count,
        "countNums" => TotalsFn::CountNums,
        "max" => TotalsFn::Max,
        "min" => TotalsFn::Min,
        "stdDev" => TotalsFn::StdDev,
        "var" => TotalsFn::Var,
        "none" => TotalsFn::None,
        _ => TotalsFn::Sum,
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        if let Some(t) = sh.tables.get_mut(ti)
            && let Some(i) = t.column_index(&col)
            && let Some(c) = t.columns.get_mut(i)
        {
            c.totals = f;
        }
        write_totals(sh, ti);
        cx.structural = true;
        Ok(Json::Null)
    })
}

pub(super) fn table_style(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let style = str_param(p, "style").unwrap_or("TableStyleMedium2").to_string();
    edit(s, |cx| {
        if let Some(t) = cx.sheet_mut(si)?.tables.get_mut(ti) {
            t.style = style.clone();
        }
        Ok(Json::Null)
    })
}

/// Table names follow the Microsoft name rules, as the xlsx writer does: letters, digits, `_` and `.`,
/// starting with a letter, `_` or `\`, and at most 255 characters. Anything else (a hyphen, say) would
/// read as an operator inside a structured reference, and the writer would rename the table on save.
fn valid_table_name(name: &str) -> bool {
    name.chars().count() <= 255
        && name.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '\\')
        && CellRef::parse(name).is_none()
        && !crate::cmd::formulas::is_r1c1(name)
        && name.chars().enumerate().all(|(i, c)| c.is_alphanumeric() || c == '_' || c == '.' || (i == 0 && c == '\\'))
}

pub(super) fn table_rename(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let name = str_param(p, "name").ok_or_else(|| bad("table.rename", "missing `name`"))?.to_string();
    if !valid_table_name(&name) {
        return Err(EngineError::Other("The name that you entered is not valid.".into()));
    }
    let wb = &s.doc()?.wb;
    if wb.table(&name).is_some_and(|x| x != (si, ti)) || wb.names.iter().any(|n| n.name.eq_ignore_ascii_case(&name)) {
        return Err(EngineError::Other("The name entered already exists. Enter a unique name.".into()));
    }
    edit(s, |cx| {
        let old = cx.wb.sheet(si).and_then(|sh| sh.tables.get(ti)).map(|t| t.name.clone()).unwrap_or_default();
        if let Some(t) = cx.sheet_mut(si)?.tables.get_mut(ti) {
            t.name = name.clone();
        }
        // Every reference to the table follows the rename. A bare name that is also a defined
        // name where it is held resolves to the defined name, so it is not the table; in a
        // workbook name, that is a workbook-level defined name.
        let shadowed: Vec<bool> = (0..cx.wb.sheets.len()).map(|i| cx.wb.name(&old, i).is_some()).collect();
        let shadowed_in_book = cx.wb.names.iter().any(|n| n.scope.is_none() && n.name.eq_ignore_ascii_case(&old));
        let sheet_of: std::collections::HashMap<String, usize> =
            cx.wb.sheets.iter().enumerate().map(|(i, s)| (s.name.to_ascii_lowercase(), i)).collect();
        let rename = |sheet: Option<usize>, e: gridcraft_formula::Expr| {
            let shadow = sheet.map_or(shadowed_in_book, |i| shadowed.get(i).copied().unwrap_or(false));
            e.map(&mut |x| match x {
                gridcraft_formula::Expr::Struct(mut st) if st.table.eq_ignore_ascii_case(&old) => {
                    st.table = name.clone();
                    gridcraft_formula::Expr::Struct(st)
                }
                gridcraft_formula::Expr::Name(n) if !shadow && n.eq_ignore_ascii_case(&old) => gridcraft_formula::Expr::Name(name.clone()),
                o => o,
            })
        };
        cx.wb.rewrite_references(&|host, e| rename(host.sheet, e));
        // A bare PivotTable source binds to a table before a defined name (`pivot::resolve_source`),
        // so a defined name with the old name doesn't shadow it: it follows the rename regardless.
        for i in 0..cx.wb.sheets.len() {
            let names_old = |src: &str| src.trim().trim_start_matches('=').trim().eq_ignore_ascii_case(&old);
            if !cx.wb.sheet(i).is_some_and(|sh| sh.pivots.iter().any(|pt| names_old(&pt.source))) {
                continue;
            }
            for pt in &mut cx.sheet_mut(i)?.pivots {
                if names_old(&pt.source) {
                    pt.source = name.clone();
                }
            }
        }
        crate::cmd::edit::rewrite_python_refs(&mut cx.wb, &|host, e| rename(sheet_of.get(&host.to_ascii_lowercase()).copied(), e))?;
        cx.structural = true;
        Ok(Json::Null)
    })
}

pub(super) fn table_convert(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    edit(s, |cx| {
        let Some(t) = cx.wb.sheet(si).and_then(|sh| sh.tables.get(ti)).cloned() else { return Ok(Json::Null) };
        // Structured references to the table become ordinary cell references, as in Excel.
        let rewrites = plain_table_refs(&cx.wb, si, &t);
        let rewrites: Vec<_> = rewrites.into_iter().map(|(i, c, e)| (i, c, Formula::from_expr(e))).collect();
        // Qualified references are longer than structured ones. Text past the parser's limit would
        // not parse after saving and reopening, leaving a formula that never recalculates.
        if rewrites.iter().any(|(_, _, f)| f.text.chars().count() > gridcraft_formula::MAX_FORMULA_LEN) {
            return Err(EngineError::Other(format!(
                "Converting to a range would make a formula longer than {} characters.",
                gridcraft_formula::MAX_FORMULA_LEN
            )));
        }
        for (i, c, f) in rewrites {
            if let Some(cell) = cx.wb.sheet_mut(i).and_then(|sh| sh.cells.get_mut(c)) {
                // Keep a legacy array's entered range and a dynamic-array mark.
                let (array, dynamic) = cell.formula.as_ref().map_or((None, false), |old| (old.array, old.dynamic));
                cell.formula = Some(std::sync::Arc::new(Formula { array, dynamic, ..f }));
            }
        }
        // Apply the table look as plain formatting, then drop the table.
        let wb = &mut cx.wb;
        let looks: Vec<(CellRef, Style)> = t.range.iter().map(|c| (c, crate::tables::table_cell_style(wb, si, &t, c))).collect();
        for (c, st) in looks {
            let id = wb.styles.intern(st);
            if let Some(sh) = wb.sheet_mut(si) {
                sh.set_style(c, id);
            }
        }
        if let Some(sh) = wb.sheet_mut(si) {
            sh.tables.remove(ti);
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Every formula that refers to table `t` on sheet `si`, rewritten with ordinary references to the
/// cells it reads: absolute, except that a this-row reference keeps a relative row.
fn plain_table_refs(wb: &gridcraft_model::Workbook, si: usize, t: &Table) -> Vec<(usize, CellRef, gridcraft_formula::Expr)> {
    use gridcraft_formula::{Expr, RefKind, Reference, SheetSel, StructItem};
    let plain = |from: usize, a: gridcraft_calc::Area, relative_row: bool| {
        let sheet = match wb.sheet(a.sheet) {
            Some(sh) if a.sheet != from => SheetSel::Named(sh.name.clone()),
            _ => SheetSel::Current,
        };
        let end = |c: CellRef| gridcraft_formula::Anchor { row: c.row, col: c.col, row_abs: !relative_row, col_abs: true };
        let kind = if a.range.is_single() { RefKind::Cell(end(a.range.start)) } else { RefKind::Range(end(a.range.start), end(a.range.end)) };
        Expr::Ref(Reference { sheet, kind })
    };
    let mut out = Vec::new();
    for (i, sh) in wb.sheets.iter().enumerate() {
        for (c, x) in sh.cells.iter() {
            let Some(e) = x.formula.as_ref().and_then(|f| f.expr()) else { continue };
            let mut hit = false;
            let ne = map_scoped(e, &mut Vec::new(), &mut |x, bound| match x {
                Expr::Struct(st) if st.table.eq_ignore_ascii_case(&t.name) || (st.table.is_empty() && i == si && t.range.contains(c)) => {
                    hit = true;
                    match gridcraft_calc::eval::resolve_struct(wb, i, c, &st) {
                        Ok(a) => plain(i, a, st.specifiers.contains(&StructItem::ThisRow)),
                        Err(e) => Expr::Error(e),
                    }
                }
                // A name an enclosing LET or LAMBDA binds is that local variable, not the table.
                Expr::Name(n) if !bound.iter().any(|b| gridcraft_formula::same_name(b, &n)) && names_table(wb, i, &n, &t.name) => {
                    hit = true;
                    match t.data_range() {
                        Some(range) => plain(i, gridcraft_calc::Area { sheet: si, range }, false),
                        None => Expr::Error(gridcraft_core::CellError::Ref),
                    }
                }
                o => o,
            });
            if hit {
                out.push((i, c, ne));
            }
        }
    }
    out
}

/// Rewrites every node of `e` with `f`, bottom-up as [`gridcraft_formula::Expr::map`] does, and
/// tells `f` which names an enclosing `LET` or `LAMBDA` binds: such a name is a local variable,
/// not a reference to the table of the same name.
fn map_scoped(
    e: gridcraft_formula::Expr,
    bound: &mut Vec<String>,
    f: &mut dyn FnMut(gridcraft_formula::Expr, &[String]) -> gridcraft_formula::Expr,
) -> gridcraft_formula::Expr {
    use gridcraft_formula::Expr;
    // A long operator chain nests one `Binary` per operator, thousands deep, so the left spine is
    // followed in a loop and only real nesting recurses, which the parser bounds.
    let mut spine = Vec::new();
    let mut node = e;
    while let Expr::Binary(op, a, b) = node {
        spine.push((op, b));
        node = *a;
    }
    let e = match node {
        Expr::Array(rows) => Expr::Array(rows.into_iter().map(|r| r.into_iter().map(|e| map_scoped(e, bound, f)).collect()).collect()),
        Expr::Unary(op, e) => Expr::Unary(op, Box::new(map_scoped(*e, bound, f))),
        Expr::Paren(e) => Expr::Paren(Box::new(map_scoped(*e, bound, f))),
        Expr::Invoke(callee, args) => {
            Expr::Invoke(Box::new(map_scoped(*callee, bound, f)), args.into_iter().map(|a| map_scoped(a, bound, f)).collect())
        }
        // A LET's declarations are not references, and a value sees only the bindings before it:
        // in `LET(x, x, x)` the first value is the outer `x`.
        Expr::Call(name, args) if name == "LET" && args.len() >= 3 && !args.len().is_multiple_of(2) => {
            let mut out = Vec::with_capacity(args.len());
            let outer = bound.len();
            let mut it = args.into_iter();
            while let Some(first) = it.next() {
                let Some(value) = it.next() else {
                    // The last argument is the body, with every binding in scope.
                    out.push(map_scoped(first, bound, f));
                    break;
                };
                let value = map_scoped(value, bound, f);
                if let Expr::Name(x) = &first {
                    bound.push(x.clone());
                }
                out.push(first);
                out.push(value);
            }
            bound.truncate(outer);
            Expr::Call(name, out)
        }
        // A LAMBDA's parameters, every argument but its body, are in scope in the body.
        Expr::Call(name, args) if name == "LAMBDA" && !args.is_empty() => {
            let mut args = args;
            let body = args.pop().unwrap_or(Expr::Missing);
            let outer = bound.len();
            let mut out = Vec::with_capacity(args.len() + 1);
            for p in args {
                if let Expr::Name(x) = &p {
                    bound.push(x.clone());
                }
                out.push(p);
            }
            out.push(map_scoped(body, bound, f));
            bound.truncate(outer);
            Expr::Call(name, out)
        }
        Expr::Call(name, args) => Expr::Call(name, args.into_iter().map(|a| map_scoped(a, bound, f)).collect()),
        other => other,
    };
    // Rebuild the chain innermost first, so every parent sees its rewritten children.
    let mut e = f(e, bound);
    while let Some((op, b)) = spine.pop() {
        let b = map_scoped(*b, bound, f);
        e = f(Expr::Binary(op, Box::new(e), Box::new(b)), bound);
    }
    e
}

/// Whether the name `n`, used on sheet `from`, denotes the table `table`. A sheet-qualified
/// `Sheet1!Orders` is split at its last `!` as the calculator does. A defined name of the same
/// name in scope wins over the table, so it is not the table.
fn names_table(wb: &gridcraft_model::Workbook, from: usize, n: &str, table: &str) -> bool {
    let (scope, bare) = match n.rsplit_once('!') {
        Some((s, b)) => match wb.sheet_index(s) {
            Some(i) => (i, b),
            None => return false,
        },
        None => (from, n),
    };
    bare.eq_ignore_ascii_case(table) && wb.name(bare, scope).is_none()
}
