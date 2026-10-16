//! Table styles: how a cell inside a table looks (our own palettes, keyed by the familiar
//! `TableStyleLight1…21 / Medium1…28 / Dark1…11` names so files round-trip).

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::*;

/// (family, number) from a style name, e.g. `TableStyleMedium2` → ("Medium", 2).
pub fn parse_style_name(name: &str) -> (&'static str, u32) {
    let rest = name.strip_prefix("TableStyle").unwrap_or(name);
    for fam in ["Light", "Medium", "Dark"] {
        if let Some(n) = rest.strip_prefix(fam) {
            let k = n.parse::<u32>().unwrap_or(2).max(1); // the gallery starts at 1; 0 would underflow `n - 1`
            return (fam, k);
        }
    }
    ("Medium", 2)
}

/// Accent theme slot (4..=9) for a style number; 1 and every 7th style are neutral (dk1).
fn accent_for(n: u32) -> Option<u8> {
    let k = (n.saturating_sub(1)) % 7;
    if k == 0 { None } else { Some(3 + k as u8) }
}

/// Header/band/body colours for a table style.
pub struct TableLook {
    pub header_fill: Option<Color>,
    pub header_font: Color,
    pub header_bold: bool,
    pub band_fill: Option<Color>,
    pub body_fill: Option<Color>,
    pub line: Option<Color>,
    pub total_top: Option<Color>,
}

pub fn look(style: &str) -> TableLook {
    let (fam, n) = parse_style_name(style);
    let acc = accent_for(n);
    let base = |tint: i16| match acc {
        Some(a) => Color::Theme(a, tint),
        None => Color::Theme(1, if tint == 0 { 0 } else { tint.clamp(-1000, 1000) }),
    };
    let neutral_light = |tint: i16| match acc {
        Some(a) => Color::Theme(a, tint),
        None => Color::Theme(0, -((1000 - tint).clamp(0, 1000) / 6)),
    };
    match fam {
        "Light" => {
            let group = (n - 1) / 7; // 0: lines only, 1: banded header line, 2: grid
            TableLook {
                header_fill: None,
                header_font: if group == 0 { base(-250) } else { Color::Auto },
                header_bold: true,
                band_fill: Some(neutral_light(800)),
                body_fill: None,
                line: Some(base(0)),
                total_top: Some(base(0)),
            }
        }
        "Dark" => TableLook {
            header_fill: Some(Color::Theme(1, 0)),
            header_font: Color::Theme(0, 0),
            header_bold: true,
            band_fill: Some(base(-250)),
            body_fill: Some(base(0)),
            line: None,
            total_top: Some(Color::Theme(0, 0)),
        },
        _ => TableLook {
            header_fill: Some(base(0)),
            header_font: Color::Theme(0, 0),
            header_bold: true,
            band_fill: Some(neutral_light(800)),
            body_fill: None,
            line: if (n - 1) / 7 == 0 { None } else { Some(base(400)) },
            total_top: Some(base(0)),
        },
    }
}

/// The style a table gives a cell (merged over the cell's own style: explicit fills/fonts win).
pub fn table_cell_style(wb: &Workbook, sheet: usize, t: &Table, c: CellRef) -> Style {
    let own = wb.sheet(sheet).map(|sh| wb.styles.get(sh.style_id(c)).clone()).unwrap_or_default();
    let mut s = own.clone();
    let lk = look(&t.style);
    let is_header = t.header_row && c.row == t.range.start.row;
    let is_total = t.totals_row && c.row == t.range.end.row;
    let data_row = c.row.saturating_sub(t.range.start.row + t.header_row as u32);
    let data_col = c.col - t.range.start.col;
    let explicit_fill = own.fill.pattern != PatternType::None;
    if is_header {
        if let Some(f) = lk.header_fill
            && !explicit_fill
        {
            s.fill = Fill::solid(f);
        }
        if own.font.color == Color::Auto {
            s.font.color = lk.header_font;
        }
        s.font.bold = s.font.bold || lk.header_bold;
        if let Some(l) = lk.line {
            s.borders.bottom = BorderLine { style: BorderStyle::Thin, color: l };
        }
    } else if is_total {
        s.font.bold = true;
        if let Some(l) = lk.total_top {
            s.borders.top = BorderLine { style: BorderStyle::Double, color: l };
        }
    } else {
        let band = (t.banded_rows && data_row.is_multiple_of(2)) || (t.banded_cols && data_col.is_multiple_of(2));
        if !explicit_fill {
            if band {
                if let Some(f) = lk.band_fill {
                    s.fill = Fill::solid(f);
                }
            } else if let Some(f) = lk.body_fill {
                s.fill = Fill::solid(f);
            }
        }
        if let Some(l) = lk.line
            && t.style.contains("Light")
            && c.row == t.range.end.row
        {
            s.borders.bottom = BorderLine { style: BorderStyle::Thin, color: l };
        }
    }
    if (t.first_col && c.col == t.range.start.col) || (t.last_col && c.col == t.range.end.col) {
        s.font.bold = true;
    }
    if t.style.starts_with("TableStyleDark") && !is_header && own.font.color == Color::Auto {
        s.font.color = Color::Theme(0, 0);
    }
    s
}

/// Gallery list of style names.
pub fn gallery() -> Vec<String> {
    let mut v = Vec::new();
    for n in 1..=21 {
        v.push(format!("TableStyleLight{n}"));
    }
    for n in 1..=28 {
        v.push(format!("TableStyleMedium{n}"));
    }
    for n in 1..=11 {
        v.push(format!("TableStyleDark{n}"));
    }
    v
}

/// Editing a table's header row renames the column, as in Excel: an empty or repeated header
/// gets a unique name written back into the cell, and structured references follow the rename.
pub(crate) fn sync_headers(cx: &mut crate::cmd::Ctx) {
    if cx.wb.sheets.iter().all(|s| s.tables.is_empty()) {
        return;
    }
    // (sheet, table name, table range, old column name, new column name)
    let mut renames: Vec<(usize, String, RangeRef, String, String)> = Vec::new();
    let heads: Vec<(usize, RangeRef)> = (0..cx.wb.sheets.len())
        .flat_map(|si| cx.wb.sheet(si).into_iter().flat_map(move |sh| sh.tables.iter().filter(|t| t.header_row).map(move |t| (si, t.range))))
        .map(|(si, r)| (si, RangeRef::new(r.start, CellRef::new(r.start.row, r.end.col))))
        .collect();
    // A structural edit can move anything into a header row: then every header is checked.
    let changed: std::collections::BTreeSet<(usize, CellRef)> = if cx.structural {
        heads.iter().flat_map(|(s, r)| r.iter().map(move |c| (*s, c))).collect()
    } else {
        cx.changed.iter().filter(|(si, c)| heads.iter().any(|(s, r)| s == si && r.contains(*c))).copied().collect()
    };
    // Per table: lower-case column name → column, and the next number to try for a repeated name
    // (both kept up to date, so a wide header row stays linear).
    type Names = (std::collections::HashMap<String, usize>, std::collections::HashMap<String, usize>);
    let mut names: std::collections::HashMap<(usize, usize), Names> = std::collections::HashMap::new();
    for (si, c) in changed {
        let Some(sh) = cx.wb.sheet(si) else { continue };
        let Some(ti) = sh.tables.iter().position(|t| t.header_row && c.row == t.range.start.row && t.range.contains(c)) else { continue };
        let Some(t) = sh.tables.get(ti) else { continue };
        let col = (c.col - t.range.start.col) as usize;
        let Some(old) = t.columns.get(col).map(|x| x.name.clone()) else { continue };
        // A text header names its column as it is: a number format only changes how it looks (an
        // accounting format pads text with spaces), and the file writes the name into the cell.
        let value = sh.value(c);
        let text = match value.as_text() {
            Some(t) => t.to_string(),
            None => crate::display::cell_text(&cx.wb, sh, c),
        };
        // Headers hold constant text: a formula (or a spilled value) there becomes its text.
        let is_text = value.is_text() && sh.cell(c).is_some_and(|x| x.formula.is_none());
        if text == old && is_text {
            continue;
        }
        let (taken, next) = names
            .entry((si, ti))
            .or_insert_with(|| (t.columns.iter().enumerate().map(|(i, x)| (x.name.to_lowercase(), i)).collect(), Default::default()));
        if taken.get(&old.to_lowercase()) == Some(&col) {
            taken.remove(&old.to_lowercase());
        }
        let base = if text.trim().is_empty() { format!("Column{}", col + 1) } else { text.clone() };
        let k = next.entry(base.to_lowercase()).or_insert(2);
        let mut name = base.clone();
        while taken.contains_key(&name.to_lowercase()) {
            name = format!("{base}{k}");
            *k += 1;
        }
        taken.insert(name.to_lowercase(), col);
        let (table, range) = (t.name.clone(), t.range);
        let Some(sh) = cx.wb.sheet_mut(si) else { continue };
        if let Some(x) = sh.tables.get_mut(ti).and_then(|t| t.columns.get_mut(col)) {
            x.name = name.clone();
        }
        // Headers are text: a number typed there, or a generated name, is written back as text.
        if name != text || !is_text {
            let style = sh.style_id(c);
            sh.cells.set(c, Cell { value: gridcraft_core::Value::text(name.as_str()), style, ..Default::default() });
        }
        if !old.eq_ignore_ascii_case(&name) {
            renames.push((si, table, range, old, name));
        }
    }
    if renames.is_empty() {
        return;
    }
    // Structured references follow the new names.
    for i in 0..cx.wb.sheets.len() {
        let formulas: Vec<(CellRef, std::sync::Arc<Formula>)> =
            cx.wb.sheets.get(i).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
        for (c, f) in formulas {
            let Some(e) = f.expr() else { continue };
            let mut hit = false;
            let ne = e.map(&mut |x| match x {
                gridcraft_formula::Expr::Struct(mut st) => {
                    for (si, table, range, old, name) in &renames {
                        let this = if st.table.is_empty() { i == *si && range.contains(c) } else { st.table.eq_ignore_ascii_case(table) };
                        if this {
                            for end in [&mut st.col_start, &mut st.col_end].into_iter().flatten() {
                                if end.eq_ignore_ascii_case(old) {
                                    *end = name.clone();
                                    hit = true;
                                }
                            }
                        }
                    }
                    gridcraft_formula::Expr::Struct(st)
                }
                o => o,
            });
            if hit && let Some(cell) = cx.wb.sheet_mut(i).and_then(|s| s.cells.get_mut(c)) {
                cell.formula = Some(std::sync::Arc::new(Formula::from_expr(ne)));
            }
        }
    }
    cx.structural = true;
}

pub(crate) fn auto_expand_tables(cx: &mut crate::cmd::Ctx) {
    if cx.wb.sheets.iter().all(|s| s.tables.is_empty()) || cx.changed.is_empty() {
        return;
    }
    let mut changed = cx.changed.clone();
    // Sort changed cells by sheet, column, then row so header rows are processed before data rows
    changed.sort_by_key(|&(si, c)| (si, c.col, c.row));
    changed.dedup();

    for (si, c) in changed {
        let is_empty = match cx.wb.sheet(si) {
            Some(sh) => sh.cell(c).is_none_or(|cell| cell.value.is_empty() && cell.formula.is_none()),
            None => continue,
        };
        if is_empty {
            continue;
        }

        let num_tables = match cx.wb.sheet(si) {
            Some(sh) => sh.tables.len(),
            None => 0,
        };

        for ti in 0..num_tables {
            // Read fresh table metadata directly from the sheet without cloning the tables collection
            let (t_range, t_totals, num_columns, other_ranges) = match cx.wb.sheet(si) {
                Some(sh) => match sh.tables.get(ti) {
                    Some(t) => {
                        let others: Vec<RangeRef> = sh.tables.iter().enumerate().filter(|(oi, _)| *oi != ti).map(|(_, ot)| ot.range).collect();
                        (t.range, t.totals_row, t.columns.len(), others)
                    }
                    None => continue,
                },
                None => continue,
            };

            // Downward expansion: row immediately below the table, within table columns
            let is_row_below = c.row == t_range.end.row.saturating_add(1) && c.col >= t_range.start.col && c.col <= t_range.end.col && !t_totals;

            if is_row_below {
                let new_range = RangeRef::new(t_range.start, CellRef::new(c.row, t_range.end.col));
                if other_ranges.iter().any(|r| r.intersects(&new_range)) {
                    continue;
                }
                let old_end_row = t_range.end.row;
                let new_end_row = c.row;
                let mut touched = Vec::new();
                if let Some(sh_mut) = cx.wb.sheet_mut(si) {
                    if let Some(tab_mut) = sh_mut.tables.get_mut(ti) {
                        tab_mut.range = new_range;
                    }

                    for val in &mut sh_mut.validations {
                        for vr in &mut val.ranges {
                            if vr.end.row == old_end_row && vr.start.col >= t_range.start.col && vr.end.col <= t_range.end.col {
                                vr.end.row = new_end_row;
                            }
                        }
                    }

                    for r in (old_end_row + 1)..=new_end_row {
                        for col_offset in 0..num_columns {
                            let col = t_range.start.col.saturating_add(col_offset as u32);
                            let target_cell = CellRef::new(r, col);
                            if target_cell == c {
                                let prev_cell = CellRef::new(r.saturating_sub(1), col);
                                let prev_style = sh_mut.style_id(prev_cell);
                                if prev_style != gridcraft_model::StyleId::DEFAULT
                                    && let Some(cell) = sh_mut.cells.get_mut(c)
                                {
                                    cell.style = prev_style;
                                }
                                continue;
                            }
                            if sh_mut.cell(target_cell).is_some_and(|cl| !cl.is_blank()) {
                                continue;
                            }
                            let prev_cell = CellRef::new(r.saturating_sub(1), col);
                            let prev_info = sh_mut.cell(prev_cell).map(|cl| (cl.formula.clone(), cl.style));
                            if let Some((formula, style)) = prev_info {
                                if let Some(f) = formula {
                                    if let Some(expr) = f.expr() {
                                        let shifted = gridcraft_formula::adjust::shift_relative(expr, 1, 0);
                                        let cell = Cell {
                                            value: gridcraft_core::Value::Empty,
                                            formula: Some(std::sync::Arc::new(Formula::from_expr(shifted))),
                                            style,
                                        };
                                        sh_mut.set_cell(target_cell, cell);
                                        touched.push(target_cell);
                                    }
                                } else if style != gridcraft_model::StyleId::DEFAULT {
                                    let cell = Cell { value: gridcraft_core::Value::Empty, formula: None, style };
                                    sh_mut.set_cell(target_cell, cell);
                                }
                            }
                        }
                    }
                }
                cx.structural = true;
                for tc in touched {
                    cx.touch(si, tc);
                }
            }

            // Rightward expansion: column immediately to the right of the table, within table rows
            let is_col_right = c.col == t_range.end.col.saturating_add(1) && c.row >= t_range.start.row && c.row <= t_range.end.row;

            if is_col_right {
                let new_range = RangeRef::new(t_range.start, CellRef::new(t_range.end.row, c.col));
                if other_ranges.iter().any(|r| r.intersects(&new_range)) {
                    continue;
                }

                // Determine header name: if the header cell already has content (entered in this edit
                // or earlier), keep it without overwriting. Otherwise generate "Column{N}".
                let header_cell = CellRef::new(t_range.start.row, c.col);
                let entered = cx.wb.sheet(si).map(|sh| sh.value(header_cell).display()).unwrap_or_default();
                let has_entered = !entered.trim().is_empty();
                let (header_name, write_header) = if has_entered { (entered, false) } else { (format!("Column{}", num_columns + 1), true) };

                if let Some(sh_mut) = cx.wb.sheet_mut(si) {
                    if write_header {
                        sh_mut.set_value(header_cell, gridcraft_core::Value::text(header_name.as_str()));
                    }
                    if let Some(tab_mut) = sh_mut.tables.get_mut(ti) {
                        tab_mut.range = new_range;
                        tab_mut.columns.push(TableColumn {
                            name: header_name,
                            totals: TotalsFn::None,
                            totals_label: None,
                            formula: None,
                            unique_name: None,
                            xml: None,
                        });
                        cx.structural = true;
                    }
                }
                if write_header {
                    cx.touch(si, header_cell);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use crate::Session;

    #[test]
    fn editing_a_header_renames_the_column() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["a", "b", "c"], [1, 2, 3]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "E1", "input": "=SUM(Table1[a])"})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": "zz"})).unwrap();
        s.execute("edit.clearContents", json!({"range": "B1"})).unwrap();
        s.execute("cell.set", json!({"cell": "C1", "input": "ZZ"})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "E1")["formula"], "=SUM(Table1[zz])");
        assert_eq!(get(&mut s, "E1")["value"], json!(1.0));
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(0).unwrap();
        let names: Vec<&str> = sh.tables[0].columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["zz", "Column2", "ZZ2"]);
        assert_eq!(sh.value(gridcraft_core::CellRef::new(0, 1)), Value::from("Column2"));
        assert_eq!(sh.value(gridcraft_core::CellRef::new(0, 2)), Value::from("ZZ2"));
        s.execute("range.setValues", json!({"range": "A1", "values": [["x", "x", 5]]})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        let names: Vec<&str> = sh.tables[0].columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "x2", "5"]);
        assert_eq!(sh.value(gridcraft_core::CellRef::new(0, 2)), Value::from("5"));
    }

    #[test]
    fn go_to_a_table_selects_its_data() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["a", "b"], [1, 2], [3, 4]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
        assert_eq!(s.execute("edit.goTo", json!({"reference": "Table1"})).unwrap()["selection"], "A2:B3");
    }

    #[test]
    fn table_auto_expands_row_when_typing_below() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Age"], ["Alice", 30]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // Type in row immediately below (row 3, index 2 in 0-index / A3)
        s.execute("cell.set", json!({"cell": "A3", "input": "Bob"})).unwrap();

        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables.len(), 1);
        assert_eq!(sh.tables[0].range.a1(), "A1:B3");
    }

    #[test]
    fn table_auto_expand_row_propagates_formulas_and_styles() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Qty", "Price", "Total"], [2, 10, null]]})).unwrap();
        s.execute("cell.set", json!({"cell": "C2", "input": "=A2*B2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true})).unwrap();

        // Add new item in row 3 (A3=3, B3=20)
        s.execute("cell.set", json!({"cell": "A3", "input": 3})).unwrap();
        s.execute("cell.set", json!({"cell": "B3", "input": 20})).unwrap();

        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "C3")["formula"], "=A3*B3");
        assert_eq!(get(&mut s, "C3")["value"], json!(60.0));
    }

    #[test]
    fn table_auto_expand_row_extends_data_validation() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Status"], ["Active"]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("data.validation", json!({"range": "A2", "type": "list", "formula1": "Active,Inactive"})).unwrap();

        // Type in A3
        s.execute("cell.set", json!({"cell": "A3", "input": "Inactive"})).unwrap();

        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        let val = sh.validations.iter().find(|v| v.ranges.iter().any(|r| r.contains(gridcraft_core::CellRef::new(2, 0))));
        assert!(val.is_some(), "Validation must cover A3");
    }

    #[test]
    fn table_auto_expands_column_when_typing_header_right() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // Type new header in C1
        s.execute("cell.set", json!({"cell": "C1", "input": "C"})).unwrap();

        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:C2");
        assert_eq!(sh.tables[0].columns.len(), 3);
        assert_eq!(sh.tables[0].columns[2].name, "C");
    }

    #[test]
    fn table_auto_expands_column_when_typing_data_right_generates_header() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // Type directly in data row C2 without entering header
        s.execute("cell.set", json!({"cell": "C2", "input": 99})).unwrap();

        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:C2");
        assert_eq!(sh.tables[0].columns.len(), 3);
        assert_eq!(sh.tables[0].columns[2].name, "Column3");
        assert_eq!(sh.value(CellRef::new(0, 2)), gridcraft_core::Value::from("Column3"));
    }

    #[test]
    fn table_does_not_auto_expand_below_totals_row() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Num"], [10]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("table.totalRow", json!({"on": true})).unwrap();

        // Table now spans A1:A3 (row 3 is totals row)
        // Typing in A4 (immediately below totals row) must NOT expand table
        s.execute("cell.set", json!({"cell": "A4", "input": 999})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:A3");
    }

    #[test]
    fn table_does_not_auto_expand_on_diagonal_edit() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // C3 is bottom-right diagonal
        s.execute("cell.set", json!({"cell": "C3", "input": "diag"})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:B2");
    }

    #[test]
    fn table_does_not_auto_expand_if_overlapping_another_table() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["T1"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();

        // Second table in A4:A5
        s.execute("range.setValues", json!({"range": "A4", "values": [["T2"], [2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A4:A5", "header": true})).unwrap();

        // Typing in A3 between T1 and T2 would expand T1 to A1:A3 if no overlap.
        // If T2 starts at A3, expansion cannot overlap. Test collision with T2 at A3:A4:
        s.execute("edit.undo", json!({})).unwrap(); // undo T2
        s.execute("insert.table", json!({"range": "A3:A4", "header": true})).unwrap();
        // Now T1 is A1:A2 and T2 is A3:A4.
        // Typing in A3 would replace T2's cell, but T1 cannot expand over T2:
        s.execute("cell.set", json!({"cell": "A3", "input": 50})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:A2");
    }

    #[test]
    fn table_auto_expand_undo_reverts_cleanly() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();

        s.execute("cell.set", json!({"cell": "A3", "input": 2})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().tables[0].range.a1(), "A1:A3");

        // Undo (Ctrl+Z)
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().tables[0].range.a1(), "A1:A2");
        assert!(s.doc().unwrap().wb.sheet(0).unwrap().cell(gridcraft_core::CellRef::new(2, 0)).is_none());
    }

    #[test]
    fn table_does_not_auto_expand_on_formatting_empty_cell() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();

        // Apply only bold or format on empty cell A3
        s.execute("home.bold", json!({"range": "A3"})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:A2");
    }

    #[test]
    fn table_auto_expand_row_inherits_date_and_number_formatting_on_expanding_cell() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Date", "Amount"]]})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "2026-10-10"})).unwrap();
        s.execute("home.numberFormat", json!({"range": "A2", "code": "dd/mm/yyyy"})).unwrap();
        s.execute("cell.set", json!({"cell": "B2", "input": 50})).unwrap();
        s.execute("home.numberFormat", json!({"range": "B2", "format": "Currency"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "A2")["text"], "10/10/2026");
        assert_eq!(get(&mut s, "B2")["text"], "$50.00");

        // Type in A3 (date in ISO format 2026-10-15; inherits dd/mm/yyyy display format)
        s.execute("cell.set", json!({"cell": "A3", "input": "2026-10-15"})).unwrap();
        assert_eq!(get(&mut s, "A3")["text"], "15/10/2026");
        assert_eq!(get(&mut s, "A3")["type"], "number");

        // Type in B3 (number in currency column)
        s.execute("cell.set", json!({"cell": "B3", "input": 75})).unwrap();
        assert_eq!(get(&mut s, "B3")["text"], "$75.00");
    }

    #[test]
    fn table_auto_expands_column_on_multi_cell_write_with_header() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // Multi-cell write into column right (C1:C2 via setValues)
        s.execute("range.setValues", json!({"range": "C1", "values": [["Status"], ["Active"]]})).unwrap();

        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:C2");
        assert_eq!(sh.tables[0].columns.len(), 3);
        assert_eq!(sh.tables[0].columns[2].name, "Status");
        assert_eq!(sh.value(CellRef::new(0, 2)), gridcraft_core::Value::from("Status"));
        assert_eq!(sh.value(CellRef::new(1, 2)), gridcraft_core::Value::from("Active"));
    }

    #[test]
    fn table_auto_expands_column_on_multi_cell_write_without_header() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2], [3, 4]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();

        // Multi-cell write into data rows only (C2:C3)
        s.execute("range.setValues", json!({"range": "C2", "values": [[100], [200]]})).unwrap();

        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:C3");
        assert_eq!(sh.tables[0].columns.len(), 3);
        assert_eq!(sh.tables[0].columns[2].name, "Column3");
        assert_eq!(sh.value(CellRef::new(0, 2)), gridcraft_core::Value::from("Column3"));
        assert_eq!(sh.value(CellRef::new(1, 2)), gridcraft_core::Value::number(100.0));
        assert_eq!(sh.value(CellRef::new(2, 2)), gridcraft_core::Value::number(200.0));
    }
}

#[cfg(test)]
mod style_tests {
    use super::*;

    #[test]
    fn zero_numbered_styles_do_not_underflow() {
        assert_eq!(parse_style_name("TableStyleLight0"), ("Light", 1));
        assert_eq!(parse_style_name("TableStyleMedium0"), ("Medium", 1));
        for name in ["TableStyleLight0", "TableStyleMedium0", "TableStyleDark0"] {
            let _ = look(name);
        }
    }
}
