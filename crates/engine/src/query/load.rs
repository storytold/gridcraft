//! Close & Load: writing a query result into a worksheet table (a new worksheet named after the
//! query, or a cell of an existing one), and rewriting that table on refresh.

use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS as SHEET_COLS, MAX_ROWS as SHEET_ROWS, RangeRef, Value};
use gridcraft_model::query::{ColType, LoadTo, QueryLoad};
use gridcraft_model::{Cell, Formula, NumFmt, Sheet, StyleId, Table, TableColumn, TotalsFn};

use super::{QResult, QTable};
use crate::cmd::Ctx;

/// Where a result was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub sheet: usize,
    pub table: String,
    pub range: RangeRef,
    /// Index of a worksheet this load added (the caller keeps per-sheet selections in step).
    pub new_sheet: Option<usize>,
}

/// A sheet name from a query name: Excel's forbidden characters dropped, at most 31 characters,
/// made unique (`Sales (2)`).
pub fn sheet_name_for(wb: &gridcraft_model::Workbook, query: &str) -> String {
    let base: String = query.chars().filter(|c| !matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']') && !c.is_control()).collect();
    let base = base.trim().trim_matches('\'').trim();
    let base: String = if base.is_empty() || base.eq_ignore_ascii_case("History") { "Query".into() } else { base.chars().take(31).collect() };
    if wb.check_sheet_name(&base, None).is_ok() {
        return base;
    }
    for n in 2..10_000 {
        let suffix = format!(" ({n})");
        let stem: String = base.chars().take(31usize.saturating_sub(suffix.chars().count())).collect();
        let cand = format!("{}{suffix}", stem.trim_end());
        if wb.check_sheet_name(&cand, None).is_ok() {
            return cand;
        }
    }
    wb.next_sheet_name()
}

/// A table name from a query name (`Sales Data` → `Sales_Data`), valid and unique among tables
/// and defined names.
pub fn table_name_for(wb: &gridcraft_model::Workbook, query: &str) -> String {
    let mut base: String = query.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '.' { c } else { '_' }).take(200).collect();
    if !base.starts_with(|c: char| c.is_alphabetic() || c == '_') || CellRef::parse(&base).is_some() {
        base = format!("_{base}");
    }
    let taken = |n: &str| wb.table(n).is_some() || wb.names.iter().any(|x| x.name.eq_ignore_ascii_case(n));
    if !taken(&base) {
        return base;
    }
    (2..).map(|k| format!("{base}{k}")).find(|n| !taken(n)).unwrap_or(base)
}

/// The number format a loaded column's cells get from its type.
fn type_format(t: ColType) -> Option<&'static str> {
    match t {
        ColType::Date => Some("m/d/yyyy"),
        ColType::DateTime => Some("m/d/yyyy h:mm"),
        ColType::Time => Some("h:mm:ss"),
        ColType::Percentage => Some("0.00%"),
        _ => None,
    }
}

/// Header names made unique ignoring case (a table's columns must be).
fn table_headers(columns: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(columns.len());
    let mut taken = std::collections::HashSet::new();
    for (i, c) in columns.iter().enumerate() {
        let base = if c.trim().is_empty() { format!("Column{}", i + 1) } else { c.chars().take(255).collect() };
        let mut n = base.clone();
        let mut k = 2;
        while !taken.insert(n.to_lowercase()) {
            n = format!("{base}{k}");
            k += 1;
        }
        out.push(n);
    }
    out
}

/// Removes the table `name` and clears its cells (values gone, formatting kept).
pub fn remove_table(cx: &mut Ctx<'_>, name: &str) -> QResult<Option<(usize, RangeRef, Table)>> {
    let Some((si, ti)) = cx.wb.table(name) else { return Ok(None) };
    let sh = cx.wb.sheet_mut(si).ok_or("The table's sheet is missing.")?;
    if ti >= sh.tables.len() {
        return Ok(None);
    }
    let t = sh.tables.remove(ti);
    clear_values(sh, t.range);
    cx.structural = true;
    Ok(Some((si, t.range, t)))
}

fn clear_values(sh: &mut Sheet, r: RangeRef) {
    let cells: Vec<(CellRef, StyleId)> = sh.cells.iter_range(r).map(|(c, cell)| (c, cell.style)).collect();
    for (c, style) in cells {
        if style == StyleId::DEFAULT {
            sh.remove_cell(c);
        } else {
            sh.set_cell(c, Cell { value: Value::Empty, formula: None, style });
        }
    }
}

/// Writes query `qi`'s result `t` as a worksheet table: over its previous table (same place,
/// name and style), else at its Load To target (an existing sheet's cell, or a new worksheet).
pub fn write(cx: &mut Ctx<'_>, qi: usize, t: &QTable) -> QResult<Loaded> {
    let q = cx.wb.queries.get(qi).cloned().ok_or("The query is missing.")?;
    let l = write_load(cx, &q.name, &q.load, t)?;
    if let Some(q) = cx.wb.queries.get_mut(qi) {
        q.load.table = Some(l.table.clone());
    }
    Ok(l)
}

/// Writes the result `t` of the query `query` to the Load To target `load`: over the table
/// `load.table` when it exists (same place, name, style and column ids), else at `load.sheet`'s
/// cell or on a new worksheet.
pub fn write_load(cx: &mut Ctx<'_>, query: &str, load: &QueryLoad, t: &QTable) -> QResult<Loaded> {
    if load.to == LoadTo::ConnectionOnly {
        return Err("The query only creates a connection.".into());
    }
    let headers = if t.columns.is_empty() { vec!["Column1".to_string()] } else { table_headers(&t.columns) };
    // Calculated columns added to the old table stay, right of the result's columns, their formula
    // in every row (as Excel keeps them on refresh): the column and its first data cell.
    let calc: Vec<(TableColumn, Option<(CellRef, Cell)>)> = match load.table.as_deref().and_then(|n| cx.wb.table(n)) {
        Some((si, ti)) => match cx.wb.sheet(si).and_then(|sh| sh.tables.get(ti).map(|t| (sh, t))) {
            Some((sh, old)) => old
                .columns
                .iter()
                .enumerate()
                .filter(|(_, c)| c.formula.is_some() && !headers.contains(&c.name))
                .map(|(j, c)| {
                    let first = old.data_range().map(|d| CellRef::new(d.start.row, old.range.start.col + j as u32));
                    (c.clone(), first.and_then(|f| sh.cell(f).cloned().map(|cell| (f, cell))))
                })
                .collect(),
            None => vec![],
        },
        None => vec![],
    };
    // The old table's filter, by column name (a refresh keeps it and filters the new rows).
    let old_filter: Vec<(String, gridcraft_model::FilterCriterion)> = match load.table.as_deref().and_then(|n| cx.wb.table(n)) {
        Some((si, ti)) => cx
            .wb
            .sheet(si)
            .and_then(|sh| sh.tables.get(ti))
            .map(|t| t.filter.iter().filter_map(|(off, c)| t.columns.get(*off as usize).map(|col| (col.name.clone(), c.clone()))).collect())
            .unwrap_or_default(),
        None => vec![],
    };
    let previous = match load.table.as_deref() {
        Some(name) => remove_table(cx, name)?,
        None => None,
    };
    let mut new_sheet = None;
    let (si, anchor) = match &previous {
        Some((si, range, _)) => (*si, range.start),
        None => match load.sheet.as_deref().and_then(|n| cx.wb.sheet_index(n)) {
            Some(si) => {
                let at = match load.cell.as_deref() {
                    Some(c) => CellRef::parse(&c.replace('$', "")).ok_or_else(|| format!("'{c}' isn't a cell reference."))?,
                    None => CellRef::new(0, 0),
                };
                (si, at)
            }
            None => {
                if cx.wb.protected_structure {
                    return Err("The workbook is protected and cannot be changed.".into());
                }
                let name = sheet_name_for(&cx.wb, query);
                let at = (cx.wb.active_sheet + 1).min(cx.wb.sheets.len());
                cx.wb.sheets.insert(at, Arc::new(Sheet::new(name)));
                for n in cx.wb.names.iter_mut() {
                    if let Some(sc) = n.scope.as_mut()
                        && *sc >= at
                    {
                        *sc += 1;
                    }
                }
                cx.wb.active_sheet = at;
                *cx.sel = crate::Selection::default();
                new_sheet = Some(at);
                (at, CellRef::new(0, 0))
            }
        },
    };
    let width = (headers.len() + calc.len()) as u32;
    let data_rows = t.rows.len().max(1) as u32;
    let end_row = anchor.row.checked_add(data_rows).filter(|r| *r < SHEET_ROWS);
    let end_col = anchor.col.checked_add(width - 1).filter(|c| *c < SHEET_COLS);
    let (Some(end_row), Some(end_col)) = (end_row, end_col) else {
        return Err("The query result doesn't fit on the worksheet from that cell.".into());
    };
    let range = RangeRef::new(anchor, CellRef::new(end_row, end_col));
    let sh = cx.wb.sheet(si).ok_or("The sheet is missing.")?;
    if sh.is_protected() {
        return Err(crate::cmd::PROTECTED.into());
    }
    if sh.tables.iter().any(|x| x.range.intersects(&range)) {
        return Err("The query's table would overlap another table. Load it somewhere else.".into());
    }
    if sh.merges.iter().any(|m| m.intersects(&range)) {
        return Err("The query's table would overlap merged cells. Load it somewhere else.".into());
    }
    let old = previous.as_ref().map(|(_, r, _)| *r);
    let occupied =
        sh.cells.iter_range(range).any(|(c, cell)| !old.is_some_and(|o| o.contains(c)) && (cell.formula.is_some() || !cell.value.is_empty()))
            || sh.pivots.iter().any(|p| p.last_range.is_some_and(|r| r.intersects(&range)));
    if occupied {
        return Err("The query's data would overwrite cells that hold data. Clear them or load the query somewhere else.".into());
    }
    let id = cx.wb.next_object_id();
    // Number formats per column type, interned once.
    let mut formats: Vec<Option<StyleId>> = Vec::with_capacity(t.types.len());
    for ty in &t.types {
        formats.push(type_format(*ty).map(|f| cx.wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(f))));
    }
    let sh = cx.wb.sheet_mut(si).ok_or("The sheet is missing.")?;
    for (j, h) in headers.iter().enumerate() {
        let c = CellRef::new(anchor.row, anchor.col + j as u32);
        sh.set_value(c, Value::text(h.as_str()));
    }
    for (i, row) in t.rows.iter().enumerate() {
        let r = anchor.row + 1 + i as u32;
        for (j, v) in row.iter().enumerate().take(width as usize) {
            let c = CellRef::new(r, anchor.col + j as u32);
            let v = match v {
                Value::Text(s) if s.chars().count() > 32_767 => Value::text(s.chars().take(32_767).collect::<String>()),
                Value::Array(_) => Value::Error(gridcraft_core::CellError::Value),
                v => v.clone(),
            };
            let style = match formats.get(j).copied().flatten() {
                Some(f) if sh.style_id(c) == StyleId::DEFAULT => f,
                _ => sh.style_id(c),
            };
            if v.is_empty() && style == StyleId::DEFAULT {
                continue;
            }
            sh.set_cell(c, Cell { value: v, formula: None, style });
        }
    }
    for (k, (col, first)) in calc.iter().enumerate() {
        let c = anchor.col + (headers.len() + k) as u32;
        sh.set_value(CellRef::new(anchor.row, c), Value::text(col.name.as_str()));
        for i in 0..t.rows.len() {
            let at = CellRef::new(anchor.row + 1 + i as u32, c);
            let cell = match first {
                Some((from, cell)) => {
                    let formula = cell.formula.as_ref().and_then(|f| f.expr()).map(|e| {
                        Arc::new(Formula::from_expr(gridcraft_formula::adjust::shift_relative(e, i64::from(at.row) - i64::from(from.row), 0)))
                    });
                    Cell { value: Value::Empty, formula, style: cell.style }
                }
                None => Cell {
                    value: Value::Empty,
                    formula: col.formula.as_ref().map(|f| Arc::new(Formula::new(&format!("={f}")))),
                    style: StyleId::DEFAULT,
                },
            };
            sh.set_cell(at, cell);
        }
    }
    // Columns that stay keep the ids the file knows them by.
    let kept: Vec<TableColumn> = previous.as_ref().map(|(_, _, old)| old.columns.clone()).unwrap_or_default();
    let (name, style, flags) = match previous {
        Some((_, _, old)) => (old.name.clone(), old.style.clone(), Some(old)),
        None => (load.table.clone().filter(|n| crate::cmd::insert::valid_table_name(n)).unwrap_or_default(), "TableStyleMedium2".to_string(), None),
    };
    let name = if name.is_empty() || cx.wb.table(&name).is_some() { table_name_for(&cx.wb, query) } else { name };
    let sh = cx.wb.sheet_mut(si).ok_or("The sheet is missing.")?;
    let columns: Vec<TableColumn> = headers
        .iter()
        .map(|n| TableColumn {
            name: n.clone(),
            totals: TotalsFn::None,
            totals_label: None,
            formula: None,
            unique_name: kept.iter().find(|c| c.name == *n).and_then(|c| c.unique_name.clone()),
            xml: None,
        })
        .chain(calc.iter().map(|(c, _)| c.clone()))
        .collect();
    let table = match flags {
        Some(old) => {
            let filter = old_filter
                .iter()
                .filter_map(|(n, c)| columns.iter().position(|x: &TableColumn| x.name == *n).map(|i| (i as u32, c.clone())))
                .collect();
            Table { id: old.id, name: name.clone(), range, columns, style, totals_row: false, filter, ..old }
        }
        None => Table {
            id,
            name: name.clone(),
            range,
            header_row: true,
            totals_row: false,
            columns,
            style,
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
            filter: vec![],
            table_type: None,
            connection_id: None,
        },
    };
    let filter = table.filter.clone();
    sh.tables.push(table);
    // Rows the old table's filter hid are shown again; the filter then applies to the new rows.
    if let Some(o) = old {
        for row in o.start.row..=o.end.row.max(range.end.row) {
            if let Some(e) = sh.rows.get_mut(&row) {
                e.hidden = false;
            }
        }
    }
    if !filter.is_empty() {
        crate::cmd::data::apply_criteria(&mut cx.wb, si, range, &filter);
    }
    let Some(sh) = cx.wb.sheet_mut(si) else { return Err("The sheet is missing.".into()) };
    // Fit new columns to their content (a refresh keeps the widths the user set).
    if new_sheet.is_some() || old.is_none() {
        for (j, h) in headers.iter().enumerate() {
            let col = anchor.col + j as u32;
            let mut w = crate::cmd::format::approx_text_width(h, gridcraft_model::DEFAULT_FONT_SIZE, false) + 6.0 + 17.0;
            for row in t.rows.iter().take(200) {
                if let Some(v) = row.get(j) {
                    let text = match (v, t.types.get(j)) {
                        (Value::Number(_), Some(ColType::Date | ColType::DateTime)) => "00/00/0000 00:00".to_string(),
                        (v, _) => v.display(),
                    };
                    w = w.max(crate::cmd::format::approx_text_width(&text, gridcraft_model::DEFAULT_FONT_SIZE, false) + 6.0);
                }
            }
            let e = sh.cols.entry(col).or_default();
            if e.size.is_none() {
                e.size = Some(w.clamp(24.0, 400.0).ceil());
            }
        }
    }
    cx.structural = true;
    Ok(Loaded { sheet: si, table: name, range, new_sheet })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_for_sheets_and_tables() {
        let mut wb = gridcraft_model::Workbook::new();
        assert_eq!(sheet_name_for(&wb, "Sales: Q1/Q2"), "Sales Q1Q2");
        assert_eq!(sheet_name_for(&wb, "Sheet1"), "Sheet1 (2)");
        assert_eq!(sheet_name_for(&wb, "''"), "Query");
        assert_eq!(sheet_name_for(&wb, &"x".repeat(40)).chars().count(), 31);
        assert_eq!(table_name_for(&wb, "Sales Data"), "Sales_Data");
        assert_eq!(table_name_for(&wb, "2026 sales"), "_2026_sales");
        assert_eq!(table_name_for(&wb, "A1"), "_A1");
        wb.names.push(gridcraft_model::DefinedName { name: "Taken".into(), scope: None, formula: "1".into(), comment: String::new(), hidden: false });
        assert_eq!(table_name_for(&wb, "Taken"), "Taken2");
        assert_eq!(table_headers(&["a".into(), "A".into(), String::new()]), vec!["a", "A2", "Column3"]);
    }
}
