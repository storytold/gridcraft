//! Table styles: how a cell inside a table looks (our own palettes, keyed by the familiar
//! `TableStyleLight1…21 / Medium1…28 / Dark1…11` names so files round-trip).

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{
    BorderLine, BorderStyle, Borders, Cell, Color, Fill, Formula, PatternType, Sheet, Style, StyleId, Table, TableColumn, TotalsFn, Workbook,
};

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

/// Fills, text colours and thin lines of a table style, per part of the table (the recipes in
/// plan/excel/table-style-spec.json). Band 1 is the first data row; unbanded rows use band 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableLook {
    pub header_fill: Option<Color>,
    pub header_font: Color,
    pub band1: Option<Color>,
    pub band2: Option<Color>,
    pub body_font: Color,
    pub top: Option<Color>,
    pub header_bottom: Option<Color>,
    pub inner_h: Option<Color>,
    pub inner_v: Option<Color>,
    pub sides: Option<Color>,
    pub bottom: Option<Color>,
    /// The totals row's double top rule (the gallery doesn't show totals).
    pub total_top: Color,
}

pub fn look(style: &str) -> TableLook {
    let (fam, n) = parse_style_name(style);
    let acc = accent_for(n);
    // An accent tint, or the neutral column's colour.
    let a = |tint: i16, neutral: Color| acc.map_or(neutral, |a| Color::Theme(a, tint));
    let (black, white) = (Color::Theme(1, 0), Color::Theme(0, 0));
    let (grey, mid_grey) = (Color::Theme(0, -150), Color::Theme(0, -350));
    let main = a(0, black);
    let plain = TableLook {
        header_fill: None,
        header_font: black,
        band1: None,
        band2: None,
        body_font: black,
        top: None,
        header_bottom: None,
        inner_h: None,
        inner_v: None,
        sides: None,
        bottom: None,
        total_top: main,
    };
    match (fam, n.saturating_sub(1) / 7) {
        ("Light", 0) => {
            let text = a(-250, black);
            let line = Some(main);
            TableLook { header_font: text, body_font: text, band1: Some(a(800, grey)), top: line, header_bottom: line, bottom: line, ..plain }
        }
        ("Light", 1) => {
            let line = Some(main);
            TableLook { header_fill: line, header_font: white, top: line, inner_h: line, sides: line, bottom: line, ..plain }
        }
        ("Light", _) => {
            let line = Some(main);
            TableLook { band1: Some(a(800, grey)), top: line, header_bottom: line, inner_v: line, sides: line, bottom: line, ..plain }
        }
        ("Medium", 0) => {
            let line = Some(a(400, black));
            TableLook { header_fill: Some(main), header_font: white, band1: Some(a(800, grey)), top: line, sides: line, bottom: line, ..plain }
        }
        ("Medium", 1) => TableLook {
            header_fill: Some(main),
            header_font: white,
            band1: Some(a(600, mid_grey)),
            band2: Some(a(800, grey)),
            header_bottom: Some(white),
            inner_v: Some(white),
            ..plain
        },
        ("Medium", 2) => {
            // Accent header over neutral grey bands and black rules; the neutral style adds a grid.
            let line = Some(black);
            let grid = if acc.is_none() { line } else { None };
            let header_fill = Some(main);
            TableLook {
                header_fill,
                header_font: white,
                band1: Some(grey),
                top: line,
                header_bottom: line,
                inner_v: grid,
                sides: grid,
                bottom: line,
                total_top: black,
                ..plain
            }
        }
        ("Medium", _) => {
            let line = Some(a(400, black));
            let pale = Some(a(800, grey));
            TableLook { header_fill: pale, band1: Some(a(600, mid_grey)), band2: pale, top: line, inner_v: line, sides: line, bottom: line, ..plain }
        }
        ("Dark", 0) => TableLook {
            header_fill: Some(black),
            header_font: white,
            band1: Some(a(-250, Color::Theme(1, 250))),
            band2: Some(a(0, Color::Theme(1, 450))),
            body_font: white,
            header_bottom: Some(white),
            total_top: white,
            ..plain
        },
        _ if n == 8 => TableLook { header_fill: Some(black), header_font: white, band1: Some(mid_grey), band2: Some(grey), ..plain },
        _ => {
            // Dark 9-11 pair accents 1+2, 3+4 and 5+6: the header takes the even one.
            let k = n.clamp(9, 11) as u8 - 8;
            let (head, body) = (Color::Theme(3 + 2 * k, 0), 2 + 2 * k);
            let band1 = Some(Color::Theme(body, 600));
            TableLook { header_fill: Some(head), header_font: white, band1, band2: Some(Color::Theme(body, 800)), total_top: head, ..plain }
        }
    }
}

/// Which parts of a table are on (the Table Style Options checkboxes).
#[derive(Clone, Copy, Debug)]
pub struct TableParts {
    pub header_row: bool,
    pub totals_row: bool,
    pub banded_rows: bool,
    pub banded_cols: bool,
    pub first_col: bool,
    pub last_col: bool,
}

impl TableParts {
    pub fn of(t: &Table) -> TableParts {
        TableParts {
            header_row: t.header_row,
            totals_row: t.totals_row,
            banded_rows: t.banded_rows,
            banded_cols: t.banded_cols,
            first_col: t.first_col,
            last_col: t.last_col,
        }
    }
}

/// What a table style paints on one cell of a `rows` x `cols` table (`row`, `col` from its
/// top-left). Each line has one owner: a horizontal line is a cell's bottom (top only on the
/// first row and the totals row), a vertical one a cell's right (left only on the first column).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellLook {
    pub fill: Option<Color>,
    pub font: Color,
    pub bold: bool,
    pub borders: Borders,
}

pub fn cell_look(lk: &TableLook, p: TableParts, row: u32, col: u32, rows: u32, cols: u32) -> CellLook {
    let last_row = rows.saturating_sub(1);
    let last_col = cols.saturating_sub(1);
    let is_header = p.header_row && row == 0;
    let is_total = p.totals_row && row == last_row && !is_header;
    let thin = |c: Option<Color>| c.map(|color| BorderLine { style: BorderStyle::Thin, color }).unwrap_or_default();
    let mut b = Borders::default();
    if row == 0 {
        b.top = thin(lk.top);
    }
    b.bottom = thin(if row == last_row {
        lk.bottom
    } else if is_header {
        lk.header_bottom
    } else if p.totals_row && row.saturating_add(1) == last_row {
        None // the totals row draws its own top rule
    } else {
        lk.inner_h
    });
    if col == 0 {
        b.left = thin(lk.sides);
    }
    b.right = thin(if col == last_col { lk.sides } else { lk.inner_v });
    let mut out = CellLook { borders: b, font: lk.body_font, ..Default::default() };
    if is_header {
        out.fill = lk.header_fill;
        out.font = lk.header_font;
        out.bold = true;
    } else if is_total {
        out.fill = lk.band2;
        out.bold = true;
        out.borders.top = BorderLine { style: BorderStyle::Double, color: lk.total_top };
    } else {
        let data_row = row.saturating_sub(p.header_row as u32);
        let band1 = (p.banded_rows && data_row.is_multiple_of(2)) || (p.banded_cols && col.is_multiple_of(2));
        out.fill = if band1 { lk.band1 } else { lk.band2 };
    }
    if (p.first_col && col == 0) || (p.last_col && col == last_col) {
        out.bold = true;
    }
    out
}

/// The style a table gives a cell (merged over the cell's own style: explicit fills, fonts and
/// borders win).
pub fn table_cell_style(wb: &Workbook, sheet: usize, t: &Table, c: CellRef) -> Style {
    let own = wb.sheet(sheet).map(|sh| wb.styles.get(sh.style_id(c)).clone()).unwrap_or_default();
    let (start, end) = (t.range.start, t.range.end);
    let rows = end.row.saturating_sub(start.row).saturating_add(1);
    let cols = end.col.saturating_sub(start.col).saturating_add(1);
    let lk = cell_look(&look(&t.style), TableParts::of(t), c.row.saturating_sub(start.row), c.col.saturating_sub(start.col), rows, cols);
    let mut s = own.clone();
    if let Some(f) = lk.fill
        && own.fill.pattern == PatternType::None
    {
        s.fill = Fill::solid(f);
    }
    if own.font.color == Color::Auto {
        s.font.color = lk.font;
    }
    s.font.bold = s.font.bold || lk.bold;
    for (side, line) in [
        (&mut s.borders.top, lk.borders.top),
        (&mut s.borders.bottom, lk.borders.bottom),
        (&mut s.borders.left, lk.borders.left),
        (&mut s.borders.right, lk.borders.right),
    ] {
        if side.is_none() {
            *side = line;
        }
    }
    s
}

/// A table's first and last visible row and column (hidden or filtered lines skipped).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShownEdges {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

/// Finds [`ShownEdges`]. It scans the hidden lines at each edge, so the grid and print call it
/// once per table per paint (through [`crate::cf::CfCache::table_edges`]), not once per cell.
pub fn shown_edges(sh: &Sheet, t: &Table) -> ShownEdges {
    let (start, end) = (t.range.start, t.range.end);
    let row_shown = |r: &u32| sh.row_height(*r) > 0.0;
    let col_shown = |k: &u32| sh.col_width(*k) > 0.0;
    // With every row (or column) hidden, each cell takes both edges' lines; nothing is drawn.
    ShownEdges {
        top: (start.row..=end.row).find(row_shown).unwrap_or(end.row),
        bottom: (start.row..=end.row).rev().find(row_shown).unwrap_or(start.row),
        left: (start.col..=end.col).find(col_shown).unwrap_or(end.col),
        right: (start.col..=end.col).rev().find(col_shown).unwrap_or(start.col),
    }
}

/// The style the grid and print draw for a table cell. Like [`table_cell_style`], but the
/// table's outer frame follows hidden rows and columns: the last visible row takes the table's
/// bottom line, the last visible column its right side, and the first visible row and column its
/// top line and left side. (Each frame line is owned by the edge cell, which is never drawn while
/// it is hidden or filtered out.) Convert to Range uses [`table_cell_style`], the table as it is.
pub fn table_cell_shown_style(wb: &Workbook, sheet: usize, t: &Table, e: ShownEdges, c: CellRef) -> Style {
    let mut s = table_cell_style(wb, sheet, t, c);
    let Some(sh) = wb.sheet(sheet) else { return s };
    let (start, end) = (t.range.start, t.range.end);
    let below = c.row < end.row && c.row >= e.bottom;
    let above = c.row > start.row && c.row <= e.top;
    let right = c.col < end.col && c.col >= e.right;
    let left = c.col > start.col && c.col <= e.left;
    if !(below || above || right || left) {
        return s;
    }
    let own = wb.styles.get(sh.style_id(c)).borders;
    if below && own.bottom.is_none() {
        s.borders.bottom = table_cell_style(wb, sheet, t, CellRef::new(end.row, c.col)).borders.bottom;
    }
    if above && own.top.is_none() {
        s.borders.top = table_cell_style(wb, sheet, t, CellRef::new(start.row, c.col)).borders.top;
    }
    if right && own.right.is_none() {
        s.borders.right = table_cell_style(wb, sheet, t, CellRef::new(c.row, end.col)).borders.right;
    }
    if left && own.left.is_none() {
        s.borders.left = table_cell_style(wb, sheet, t, CellRef::new(c.row, start.col)).borders.left;
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

/// Grows a table over an entry made right next to it, as Excel's "Include new rows and columns in
/// table" does: a cell filled in the row below adds the row (its formulas, formats and validation
/// filled down), one in the column to the right adds a column. Totals rows, other tables, protected
/// sheets and commands that enter no data (sort, Format Painter, checkboxes) stop it.
pub(crate) fn auto_expand_tables(cx: &mut crate::cmd::Ctx) {
    if !cx.entry || cx.wb.sheets.iter().all(|s| s.tables.is_empty()) || cx.changed.is_empty() {
        return;
    }
    let mut changed = cx.changed.clone();
    changed.sort_unstable();
    changed.dedup();
    // Multi-cell legacy arrays per sheet, found once and only when a table may grow: a table can't
    // hold one (as `insert.table` refuses), so it doesn't grow over one.
    let mut arrays: std::collections::HashMap<usize, Vec<RangeRef>> = std::collections::HashMap::new();

    // Rows first, in row order, so each row of a pasted block meets the table already grown by the
    // rows above it, whichever of its cells hold something; then columns, in column order, so a
    // column's header is seen before its data.
    for rows in [true, false] {
        if rows {
            changed.sort_unstable_by_key(|&(si, c)| (si, c.row, c.col));
        } else {
            changed.sort_unstable_by_key(|&(si, c)| (si, c.col, c.row));
        }
        expand_pass(cx, &changed, rows, &mut arrays);
    }
}

/// One pass of [`auto_expand_tables`] over the entered cells: growing tables down (`rows`) or right.
fn expand_pass(cx: &mut crate::cmd::Ctx, changed: &[(usize, CellRef)], rows: bool, arrays: &mut std::collections::HashMap<usize, Vec<RangeRef>>) {
    for &(si, c) in changed {
        let is_empty = match cx.wb.sheet(si) {
            Some(sh) => sh.cell(c).is_none_or(|cell| cell.value.is_empty() && cell.formula.is_none()),
            None => continue,
        };
        if is_empty {
            continue;
        }

        // Excel doesn't grow tables on a protected sheet: the new row's cells may be locked.
        let num_tables = match cx.wb.sheet(si) {
            Some(sh) if !sh.is_protected() => sh.tables.len(),
            _ => 0,
        };

        for ti in 0..num_tables {
            // Read fresh table metadata directly from the sheet without cloning the tables collection
            let Some((t_range, t_totals, t_header, num_columns)) =
                cx.wb.sheet(si).and_then(|sh| sh.tables.get(ti)).map(|t| (t.range, t.totals_row, t.header_row, t.columns.len()))
            else {
                continue;
            };

            // Downward expansion: row immediately below the table, within table columns
            let is_row_below =
                rows && c.row == t_range.end.row.saturating_add(1) && c.col >= t_range.start.col && c.col <= t_range.end.col && !t_totals;
            // Rightward expansion: column immediately to the right of the table, within table rows
            let is_col_right = !rows && c.col == t_range.end.col.saturating_add(1) && c.row >= t_range.start.row && c.row <= t_range.end.row;
            if !is_row_below && !is_col_right {
                continue;
            }
            // Only now, for a cell next to this table: the ranges it must not grow over.
            let other_ranges: Vec<RangeRef> = match cx.wb.sheet(si) {
                Some(sh) => sh.tables.iter().enumerate().filter(|(oi, _)| *oi != ti).map(|(_, ot)| ot.range).collect(),
                None => continue,
            };

            if is_row_below {
                grow_down(cx, si, c, ti, (t_range, t_header, num_columns), &other_ranges, arrays);
            } else if is_col_right {
                grow_right(cx, si, c, ti, t_range, &other_ranges, arrays);
            }
        }
    }
}

/// Grows table `ti` of sheet `si` down to the row of `c`, an entry just below it: validation,
/// formats and the calculated columns' formulas follow into the new rows.
fn grow_down(
    cx: &mut crate::cmd::Ctx,
    si: usize,
    c: CellRef,
    ti: usize,
    (t_range, t_header, num_columns): (RangeRef, bool, usize),
    other_ranges: &[RangeRef],
    arrays: &mut std::collections::HashMap<usize, Vec<RangeRef>>,
) {
    let new_range = RangeRef::new(t_range.start, CellRef::new(c.row, t_range.end.col));
    let arr = arrays.entry(si).or_insert_with(|| cx.wb.sheet(si).map(multi_cell_arrays).unwrap_or_default());
    if other_ranges.iter().chain(arr.iter()).any(|r| r.intersects(&new_range)) {
        return;
    }
    let old_end_row = t_range.end.row;
    let new_end_row = c.row;
    // Below a table with no data rows the row above is the header: the new row takes
    // neither its format nor anything else from it.
    let first_data_row = t_range.start.row.saturating_add(u32::from(t_header));
    let new_cells: Vec<CellRef> = if old_end_row < first_data_row {
        Vec::new()
    } else {
        (old_end_row.saturating_add(1)..=new_end_row)
            .flat_map(|r| (0..num_columns).map(move |i| CellRef::new(r, t_range.start.col.saturating_add(i as u32))))
            .collect()
    };
    // Cells whose own style is at most a number format (one typing a date or a percentage
    // gives): an entry there takes the column's format; anything more was pasted on purpose.
    let plain: std::collections::HashSet<CellRef> = match cx.wb.sheet(si) {
        Some(sh) => new_cells
            .iter()
            .copied()
            .filter(|&tc| Style { num_fmt: Default::default(), ..cx.wb.styles.get(sh.style_id(tc)).clone() } == Style::default())
            .collect(),
        None => return,
    };
    if let Some(sh_mut) = cx.wb.sheet_mut(si) {
        if let Some(tab_mut) = sh_mut.tables.get_mut(ti) {
            tab_mut.range = new_range;
        }

        // A column's validation covers the new row too, except cells that already have a
        // rule of their own (pasted with the entry, or set before): rules never overlap,
        // and the first one that covers a cell is the one that applies.
        let ruled: std::collections::HashSet<CellRef> =
            new_cells.iter().copied().filter(|&tc| sh_mut.validations.iter().any(|v| v.ranges.iter().any(|r| r.contains(tc)))).collect();
        for val in &mut sh_mut.validations {
            let mut pieces = Vec::new();
            for vr in &mut val.ranges {
                if old_end_row < first_data_row || vr.end.row != old_end_row || vr.start.col < t_range.start.col || vr.end.col > t_range.end.col {
                    continue;
                }
                let free = |col: u32| (old_end_row.saturating_add(1)..=new_end_row).all(|r| !ruled.contains(&CellRef::new(r, col)));
                if (vr.start.col..=vr.end.col).all(free) {
                    vr.end.row = new_end_row;
                    continue;
                }
                // Only the free runs of the new row join the rule.
                let mut run: Option<u32> = None;
                for col in vr.start.col..=vr.end.col.saturating_add(1) {
                    match (run, col <= vr.end.col && free(col)) {
                        (None, true) => run = Some(col),
                        (Some(from), false) => {
                            pieces.push(RangeRef::new(
                                CellRef::new(old_end_row.saturating_add(1), from),
                                CellRef::new(new_end_row, col.saturating_sub(1)),
                            ));
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
            val.ranges.extend(pieces);
        }

        for &target_cell in &new_cells {
            let prev_cell = CellRef::new(target_cell.row.saturating_sub(1), target_cell.col);
            let prev_style = sh_mut.style_id(prev_cell);
            let own_style = sh_mut.style_id(target_cell);
            // An entry made in the new row (by this command or before) keeps its value and
            // takes the column's format, unless it brought formatting of its own.
            if sh_mut.cell(target_cell).is_some_and(|cl| !cl.value.is_empty() || cl.formula.is_some()) {
                if prev_style != StyleId::DEFAULT
                    && plain.contains(&target_cell)
                    && let Some(cell) = sh_mut.cells.get_mut(target_cell)
                {
                    cell.style = prev_style;
                }
                continue;
            }
            // An empty cell takes the calculated column's formula, and the format of the
            // cell above unless it was formatted already.
            let style = if own_style != StyleId::DEFAULT { own_style } else { prev_style };
            // A column is calculated when its first data row holds the same formula as the
            // row above, moved: a formula typed into one cell only isn't filled down, as
            // in Excel. The copy keeps the dynamic-array mark (so a file saves it as Excel
            // wrote it) and a one-cell array stays an array.
            let first_cell = CellRef::new(first_data_row, target_cell.col);
            let first = sh_mut.cell(first_cell).and_then(|cl| cl.formula.as_ref()).and_then(|f| f.expr());
            let shifted = sh_mut.cell(prev_cell).and_then(|cl| cl.formula.as_ref()).and_then(|f| {
                let expr = f.expr()?;
                let moved = gridcraft_formula::adjust::shift_relative(first?, i64::from(prev_cell.row.saturating_sub(first_data_row)), 0);
                (moved == expr).then(|| Formula {
                    dynamic: f.dynamic,
                    array: f.array.filter(|a| a.is_single()).map(|_| RangeRef::new(target_cell, target_cell)),
                    ..Formula::from_expr(gridcraft_formula::adjust::shift_relative(expr, 1, 0))
                })
            });
            if let Some(shifted) = shifted {
                let formula = Some(std::sync::Arc::new(shifted));
                sh_mut.set_cell(target_cell, Cell { value: gridcraft_core::Value::Empty, formula, style, py: None });
            } else if style != own_style {
                sh_mut.set_cell(target_cell, Cell { value: gridcraft_core::Value::Empty, formula: None, style, py: None });
            }
        }
    }
    // Every cell of the new row is now in the table, empty or not: formulas over the
    // table's columns (ROWS, COUNTBLANK, SUM…) recalculate through them.
    cx.rebuild_deps = true;
    for r in old_end_row.saturating_add(1)..=new_end_row {
        for col in t_range.start.col..=t_range.end.col {
            cx.touch(si, CellRef::new(r, col));
        }
    }
}

/// Grows table `ti` of sheet `si` right to the column of `c`, an entry just right of it: the new
/// column is named from its header cell, or `ColumnN`.
fn grow_right(
    cx: &mut crate::cmd::Ctx,
    si: usize,
    c: CellRef,
    ti: usize,
    t_range: RangeRef,
    other_ranges: &[RangeRef],
    arrays: &mut std::collections::HashMap<usize, Vec<RangeRef>>,
) {
    let new_range = RangeRef::new(t_range.start, CellRef::new(t_range.end.row, c.col));
    let arr = arrays.entry(si).or_insert_with(|| cx.wb.sheet(si).map(multi_cell_arrays).unwrap_or_default());
    if other_ranges.iter().chain(arr.iter()).any(|r| r.intersects(&new_range)) {
        return;
    }

    // The header cell names the column when it holds something (entered in this edit or
    // earlier); otherwise the column gets the lowest free `ColumnN`, as in Excel. A table
    // without a header row keeps the name on the column only: its first row is data.
    let header_cell = CellRef::new(t_range.start.row, c.col);
    let (header_name, write_header) = match cx.wb.sheet(si) {
        Some(sh) => {
            let Some(t) = sh.tables.get(ti) else { return };
            let entered = if t.header_row { sh.value(header_cell).display() } else { String::new() };
            let wanted = if entered.trim().is_empty() { "" } else { entered.as_str() };
            let name = unique_column_name(&t.columns, wanted);
            let write = t.header_row && name != entered;
            (name, write)
        }
        None => return,
    };

    if let Some(sh_mut) = cx.wb.sheet_mut(si) {
        if write_header {
            let style = sh_mut.style_id(header_cell);
            sh_mut.set_cell(header_cell, Cell { value: gridcraft_core::Value::text(header_name.as_str()), style, ..Default::default() });
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
            cx.rebuild_deps = true;
        }
    }
    // Likewise every cell of the new column, its header included.
    for r in t_range.start.row..=t_range.end.row {
        cx.touch(si, CellRef::new(r, c.col));
    }
}

/// A name for a column added to a table: `wanted` when no column has it yet, `wanted2`, `wanted3`…
/// when one does, and the lowest free `ColumnN` (from `Column1`) when nothing was wanted. Names
/// compare without case, as structured references do.
fn unique_column_name(columns: &[TableColumn], wanted: &str) -> String {
    let taken: std::collections::HashSet<String> = columns.iter().map(|c| c.name.to_lowercase()).collect();
    let free = |n: &str| !taken.contains(&n.to_lowercase());
    if wanted.is_empty() {
        // `columns.len() + 1` names cannot all be taken by `columns.len()` columns.
        return (1..=columns.len().saturating_add(1)).map(|n| format!("Column{n}")).find(|n| free(n)).unwrap_or_default();
    }
    if free(wanted) {
        return wanted.to_string();
    }
    (2..=columns.len().saturating_add(2)).map(|n| format!("{wanted}{n}")).find(|n| free(n)).unwrap_or_default()
}

/// The ranges of the sheet's multi-cell legacy array formulas.
fn multi_cell_arrays(sh: &gridcraft_model::Sheet) -> Vec<RangeRef> {
    sh.cells.iter().filter_map(|(_, x)| x.formula.as_ref().and_then(|f| f.array)).filter(|a| !a.is_single()).collect()
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
        assert_eq!(sh.tables[0].columns[2].name, "Column1");
        assert_eq!(sh.value(CellRef::new(0, 2)), gridcraft_core::Value::from("Column1"));
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
        assert_eq!(sh.tables[0].columns[2].name, "Column1");
        assert_eq!(sh.value(CellRef::new(0, 2)), gridcraft_core::Value::from("Column1"));
        assert_eq!(sh.value(CellRef::new(1, 2)), gridcraft_core::Value::number(100.0));
        assert_eq!(sh.value(CellRef::new(2, 2)), gridcraft_core::Value::number(200.0));
    }

    fn names(s: &Session) -> Vec<String> {
        s.doc().unwrap().wb.sheet(0).unwrap().tables[0].columns.iter().map(|c| c.name.clone()).collect()
    }

    #[test]
    fn table_auto_expand_generated_names_take_the_lowest_free_column_n() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B", "Column1"], [1, 2, 3]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true})).unwrap();

        // Column1 is taken, so typing in D2 names the new column Column2 (as Excel does), then E2 gets Column3.
        s.execute("cell.set", json!({"cell": "D2", "input": 9})).unwrap();
        s.execute("cell.set", json!({"cell": "E2", "input": 9})).unwrap();
        assert_eq!(names(&s), ["A", "B", "Column1", "Column2", "Column3"]);
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.value(CellRef::new(0, 3)), Value::from("Column2"));
        assert_eq!(sh.value(CellRef::new(0, 4)), Value::from("Column3"));
    }

    #[test]
    fn table_auto_expand_typed_header_that_repeats_a_name_is_made_unique() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        s.execute("cell.set", json!({"cell": "C1", "input": "a"})).unwrap();
        assert_eq!(names(&s), ["A", "B", "a2"]);
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 2)), Value::from("a2"));
    }

    #[test]
    fn table_auto_expand_column_without_header_row_leaves_the_first_row_as_data() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["x", "y"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        // A header-less table (as opened from a file with headerRowCount="0") on A2:B3.
        {
            let sh = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap();
            sh.tables[0].header_row = false;
            sh.tables[0].range = gridcraft_core::RangeRef::parse("A2:B3").unwrap();
            sh.set_value(CellRef::new(2, 0), Value::number(3.0));
            sh.set_value(CellRef::new(2, 1), Value::number(4.0));
        }

        // Typing in a lower data row of the new column writes no name into the first data row.
        s.execute("cell.set", json!({"cell": "C3", "input": "x"})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A2:C3");
        assert_eq!(sh.value(CellRef::new(1, 2)), Value::Empty);
        assert_eq!(names(&s), ["x", "y", "Column1"]);

        // A value typed into the first data row stays a value, not the column's name.
        s.execute("cell.set", json!({"cell": "D2", "input": 42})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A2:D3");
        assert_eq!(sh.value(CellRef::new(1, 3)), Value::number(42.0));
        assert_eq!(names(&s), ["x", "y", "Column1", "Column2"]);
    }

    #[test]
    fn table_auto_expand_with_existing_content_next_to_the_table() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B", "Notes"], [1, 2, "n"], ["Total", 3, null]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // Formatting the existing cells beside and below leaves the table alone (observed in Excel).
        s.execute("home.bold", json!({"range": "A3:B3"})).unwrap();
        s.execute("home.bold", json!({"range": "C1:C2"})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().tables[0].range.a1(), "A1:B2");

        // Retyping a value already below the table takes the row in, as Excel does.
        s.execute("cell.set", json!({"cell": "A3", "input": "Total"})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().tables[0].range.a1(), "A1:B3");
        // And so does retyping the existing header beside it; its text names the new column.
        s.execute("cell.set", json!({"cell": "C1", "input": "Notes"})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:C3");
        assert_eq!(sh.value(CellRef::new(1, 2)), Value::from("n"));
        assert_eq!(names(&s), ["A", "B", "Notes"]);
    }

    fn range(s: &Session) -> String {
        s.doc().unwrap().wb.sheet(0).unwrap().tables[0].range.a1()
    }

    #[test]
    fn table_does_not_auto_expand_on_format_only_commands() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1], ["x"]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": "src"})).unwrap();
        s.execute("home.bold", json!({"range": "D1"})).unwrap();

        // Format Painter onto the filled cell below the table.
        s.execute("selection.set", json!({"range": "D1"})).unwrap();
        s.execute("edit.formatPainter", json!({})).unwrap();
        s.execute("edit.formatPainter", json!({"apply": "A3"})).unwrap();
        assert_eq!(range(&s), "A1:A2");

        // Paste Special › Formats onto it.
        s.execute("selection.set", json!({"range": "D1"})).unwrap();
        s.execute("edit.copy", json!({})).unwrap();
        s.execute("selection.set", json!({"range": "A3"})).unwrap();
        s.execute("edit.pasteSpecial", json!({"what": "formats"})).unwrap();
        assert_eq!(range(&s), "A1:A2");
    }

    #[test]
    fn table_does_not_auto_expand_on_a_protected_sheet() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, null]]})).unwrap();
        s.execute("cell.set", json!({"cell": "B2", "input": "=A2*2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        s.execute("home.lockCell", json!({"range": "A3", "on": false})).unwrap();
        s.execute("review.protectSheet", json!({})).unwrap();

        // The unlocked cell takes the entry; the table stays and no formula goes into locked B3.
        s.execute("cell.set", json!({"cell": "A3", "input": 5})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:B2");
        assert_eq!(sh.value(CellRef::new(2, 0)), Value::number(5.0));
        assert!(sh.cell(CellRef::new(2, 1)).is_none_or(|c| c.formula.is_none()));
    }

    #[test]
    fn table_auto_expand_keeps_part_of_an_array_from_being_changed() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "B1", "values": [["H"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "B1:B2", "header": true})).unwrap();
        s.execute("selection.set", json!({"range": "A3:B3"})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": "={1,2}", "array": true})).unwrap();

        // B3 is part of the legacy array A3:B3: the edit is refused even though it sits below the table.
        assert!(s.execute("cell.set", json!({"cell": "B3", "input": 9})).is_err());
        assert_eq!(range(&s), "B1:B2");
    }

    #[test]
    fn table_auto_expand_follows_what_paste_special_writes() {
        let setup = || {
            let mut s = Session::new();
            s.new_workbook();
            s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1], ["x"]]})).unwrap();
            s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
            s.execute("cell.set", json!({"cell": "D1", "input": "src"})).unwrap();
            s.execute("review.newNote", json!({"cell": "D1", "text": "note"})).unwrap();
            s.execute("data.validation", json!({"range": "D1", "type": "list", "formula1": "src,x"})).unwrap();
            s.execute("selection.set", json!({"range": "D1"})).unwrap();
            s.execute("edit.copy", json!({})).unwrap();
            s.execute("selection.set", json!({"range": "A3"})).unwrap();
            s
        };
        // Notes, validation and column widths write no entry into the filled cell below the table.
        for what in ["comments", "validation", "columnWidths"] {
            let mut s = setup();
            s.execute("edit.pasteSpecial", json!({"what": what})).unwrap();
            assert_eq!(range(&s), "A1:A2", "{what}");
        }
        // A linked paste writes a formula whatever it pastes: that is an entry.
        let mut s = setup();
        s.execute("edit.pasteSpecial", json!({"what": "formats", "link": true})).unwrap();
        assert_eq!(range(&s), "A1:A3");
    }

    #[test]
    fn table_auto_expand_fills_the_formula_into_a_formatted_empty_cell() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Qty", "Price", "Total"], [2, 10, null]]})).unwrap();
        s.execute("cell.set", json!({"cell": "C2", "input": "=A2*B2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true})).unwrap();
        s.execute("home.bold", json!({"range": "C3"})).unwrap();

        s.execute("cell.set", json!({"cell": "A3", "input": 3})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(range(&s), "A1:C3");
        assert_eq!(get(&mut s, "C3")["formula"], "=A3*B3");
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert!(s.doc().unwrap().wb.styles.get(sh.style_id(CellRef::new(2, 2))).font.bold, "C3 keeps its own format");
    }

    #[test]
    fn table_auto_expand_gives_every_entry_of_the_new_row_the_column_format() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Price"], ["a", 50]]})).unwrap();
        s.execute("home.numberFormat", json!({"range": "B2", "format": "Currency"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();

        // One write fills the whole new row: B3 takes the currency format as A3 would.
        s.execute("range.setValues", json!({"range": "A3", "values": [["b", 75]]})).unwrap();
        assert_eq!(range(&s), "A1:B3");
        assert_eq!(s.execute("cell.get", json!({"cell": "B3"})).unwrap()["text"], "$75.00");
    }

    #[test]
    fn table_auto_expand_keeps_formatting_pasted_with_the_entry() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1]]})).unwrap();
        s.execute("home.numberFormat", json!({"range": "A2", "format": "Currency"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": 5})).unwrap();
        s.execute("home.bold", json!({"range": "D1"})).unwrap();

        s.execute("selection.set", json!({"range": "D1"})).unwrap();
        s.execute("edit.copy", json!({})).unwrap();
        s.execute("selection.set", json!({"range": "A3"})).unwrap();
        s.execute("edit.paste", json!({})).unwrap();
        assert_eq!(range(&s), "A1:A3");
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert!(s.doc().unwrap().wb.styles.get(sh.style_id(CellRef::new(2, 0))).font.bold, "the pasted bold stays");
    }

    #[test]
    fn table_auto_expand_keeps_the_dynamic_array_mark_of_the_column_formula() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Qty", "Total"], [2, null]]})).unwrap();
        s.execute("cell.set", json!({"cell": "B2", "input": "=SUM(A2:A2*2)"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        // As read from a file that marks B2 as a dynamic-array formula.
        {
            let sh = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap();
            let cell = sh.cells.get_mut(CellRef::new(1, 1)).unwrap();
            let f = cell.formula.as_ref().unwrap();
            cell.formula = Some(std::sync::Arc::new(gridcraft_model::Formula { dynamic: true, ..(**f).clone() }));
        }

        s.execute("cell.set", json!({"cell": "A3", "input": 3})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        let f = sh.cell(CellRef::new(2, 1)).and_then(|c| c.formula.clone()).unwrap();
        assert_eq!(f.text, "SUM(A3:A3*2)");
        assert!(f.dynamic);
    }

    #[test]
    fn table_does_not_auto_expand_over_a_multi_cell_array() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B", "C"], [1, 2, 3]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true})).unwrap();
        s.execute("selection.set", json!({"range": "B3:C3"})).unwrap();
        s.execute("cell.set", json!({"cell": "B3", "input": "={1,2}", "array": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": 4})).unwrap();
        assert_eq!(range(&s), "A1:C2");

        // Beside the table too: an array in D2:E2 keeps column D from being added.
        s.execute("selection.set", json!({"range": "D2:E2"})).unwrap();
        s.execute("cell.set", json!({"cell": "D2", "input": "={1,2}", "array": true})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": "New"})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        assert_eq!(sh.tables[0].range.a1(), "A1:C2");
        assert_eq!(sh.tables[0].columns.len(), 3);
    }

    /// A table on A1:B2 with data A3:B5 right below it, outside the table.
    fn table_above_data() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2], [9, "note"], [3, "z"], [5, "y"]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        s
    }

    #[test]
    fn table_does_not_auto_expand_on_commands_that_enter_no_data() {
        // Sorting the data below moves its cells; it doesn't enter them.
        let mut s = table_above_data();
        s.execute("data.sort", json!({"range": "A3:B5", "header": false, "keys": [{"column": "A", "order": "asc"}]})).unwrap();
        assert_eq!(range(&s), "A1:B2");

        // Fill › Formats Only keeps the values below.
        let mut s = table_above_data();
        s.execute("edit.autoFill", json!({"source": "A2", "target": "A2:A3", "mode": "formats"})).unwrap();
        assert_eq!(range(&s), "A1:B2");

        // Checkboxes give empty cells FALSE as a control, not as typed data.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("insert.checkbox", json!({"range": "A3:A4"})).unwrap();
        assert_eq!(range(&s), "A1:A2");
    }

    #[test]
    fn table_auto_expands_on_every_way_of_entering_data() {
        let grows = |cmd: &str, p: serde_json::Value| {
            let mut s = Session::new();
            s.new_workbook();
            s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1], [null], [null]]})).unwrap();
            s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
            s.execute(cmd, p).unwrap();
            assert_eq!(range(&s), "A1:A3", "{cmd}");
        };
        grows("cell.set", json!({"cell": "A3", "input": 2}));
        grows("range.setValues", json!({"range": "A3", "values": [[2]]}));
        grows("range.fill", json!({"range": "A3", "input": "2"}));
        grows("edit.fillDown", json!({"range": "A2:A3"}));
        grows("edit.autoFill", json!({"source": "A2", "target": "A2:A3"}));
        grows("edit.fillSeries", json!({"range": "A2:A3", "type": "linear", "step": 1}));
        grows("edit.paste", json!({"text": "2", "at": "A3"}));
    }

    #[test]
    fn table_auto_expand_fills_only_calculated_columns() {
        // B3 holds a formula typed into that cell only; B2 is a constant: B4 stays empty.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 5], [2, null]]})).unwrap();
        s.execute("cell.set", json!({"cell": "B3", "input": "=A3*2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A4", "input": 3})).unwrap();
        assert_eq!(range(&s), "A1:B4");
        assert_eq!(s.execute("cell.get", json!({"cell": "B4"})).unwrap()["formula"], json!(null));

        // The same formula on every data row is a calculated column: it fills down.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, null], [2, null]]})).unwrap();
        s.execute("selection.set", json!({"range": "B2:B3"})).unwrap();
        s.execute("range.fill", json!({"range": "B2:B3", "input": "=A2*2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A4", "input": 3})).unwrap();
        assert_eq!(s.execute("cell.get", json!({"cell": "B4"})).unwrap()["formula"], "=A4*2");
    }

    #[test]
    fn table_auto_expand_below_a_header_only_table_takes_nothing_from_the_header() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"]]})).unwrap();
        s.execute("home.bold", json!({"range": "A1:B1"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B1", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "x"})).unwrap();
        assert_eq!(range(&s), "A1:B2");
        let doc = s.doc().unwrap();
        let sh = doc.wb.sheet(0).unwrap();
        for c in [CellRef::new(1, 0), CellRef::new(1, 1)] {
            assert!(!doc.wb.styles.get(sh.style_id(c)).font.bold, "{c:?} isn't a header");
        }
    }

    #[test]
    fn table_auto_expand_keeps_a_one_cell_array_formula_an_array() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, null]]})).unwrap();
        s.execute("selection.set", json!({"range": "B2"})).unwrap();
        s.execute("cell.set", json!({"cell": "B2", "input": "=SUM(A2:A2*2)", "array": true})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": 3})).unwrap();
        let sh = s.doc().unwrap().wb.sheet(0).unwrap();
        let f = sh.cell(CellRef::new(2, 1)).and_then(|c| c.formula.clone()).unwrap();
        assert_eq!(f.array, Some(gridcraft_core::RangeRef::new(CellRef::new(2, 1), CellRef::new(2, 1))));
    }

    #[test]
    fn table_auto_expands_over_every_row_of_a_ragged_block() {
        // The block's first row has its entry in B, its second row in A only: both rows join.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        s.execute("range.setValues", json!({"range": "A3", "values": [[null, 1], [2, null]]})).unwrap();
        assert_eq!(range(&s), "A1:B4");

        // A block below and to the right grows the table down first, then right.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        s.execute("range.setValues", json!({"range": "B3", "values": [[3, 4]]})).unwrap();
        assert_eq!(range(&s), "A1:C3");
    }

    #[test]
    fn table_auto_expand_never_lays_the_column_validation_over_a_cell_rule() {
        let rules = |s: &Session| -> Vec<Vec<String>> {
            s.doc().unwrap().wb.sheet(0).unwrap().validations.iter().map(|v| v.ranges.iter().map(|r| r.a1()).collect()).collect()
        };
        let table = || {
            let mut s = Session::new();
            s.new_workbook();
            s.execute("range.setValues", json!({"range": "A1", "values": [["A"], ["x"]]})).unwrap();
            s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
            s.execute("data.validation", json!({"range": "A2", "type": "list", "formula1": "x,y"})).unwrap();
            s
        };

        // A cell pasted with its own rule keeps it: the column's list doesn't cover it as well.
        let mut s = table();
        s.execute("cell.set", json!({"cell": "D1", "input": "5"})).unwrap();
        s.execute("data.validation", json!({"range": "D1", "type": "whole", "operator": "between", "formula1": "1", "formula2": "9"})).unwrap();
        s.execute("selection.set", json!({"range": "D1"})).unwrap();
        s.execute("edit.copy", json!({})).unwrap();
        s.execute("selection.set", json!({"range": "A3"})).unwrap();
        s.execute("edit.paste", json!({})).unwrap();
        assert_eq!(range(&s), "A1:A3");
        assert_eq!(rules(&s), [vec!["A2"], vec!["D1"], vec!["A3"]]);

        // So does a rule the cell had before the entry.
        let mut s = table();
        s.execute("data.validation", json!({"range": "A3", "type": "whole", "operator": "between", "formula1": "1", "formula2": "9"})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": "5"})).unwrap();
        assert_eq!(range(&s), "A1:A3");
        assert_eq!(rules(&s), [vec!["A2"], vec!["A3"]]);

        // A rule over several columns takes in only the free part of the new row.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B", "C"], ["x", "x", "x"]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true})).unwrap();
        s.execute("data.validation", json!({"range": "A2:C2", "type": "list", "formula1": "x,y"})).unwrap();
        s.execute("data.validation", json!({"range": "B3", "type": "whole", "operator": "between", "formula1": "1", "formula2": "9"})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": "y"})).unwrap();
        assert_eq!(range(&s), "A1:C3");
        assert_eq!(rules(&s), [vec!["A2:C2", "A3", "C3"], vec!["B3"]]);
    }

    #[test]
    fn table_auto_expand_keeps_manual_calculation_manual() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "E1", "input": 1})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": "=E1*2"})).unwrap();
        s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();

        // In manual mode D1 waits for F9 after E1 changes, and a table growing doesn't run it.
        s.execute("cell.set", json!({"cell": "E1", "input": 5})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": 7})).unwrap();
        assert_eq!(range(&s), "A1:A3");
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!(get(&mut s, "D1"), json!(2.0));
        s.execute("formulas.calculateNow", json!({})).unwrap();
        assert_eq!(get(&mut s, "D1"), json!(10.0));
    }

    #[test]
    fn table_auto_expand_recalculates_formulas_over_the_table() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["A", "B"], [1, 2], [3, 4]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
        s.execute("cell.set", json!({"cell": "F1", "input": "=ROWS(Table1[B])"})).unwrap();
        s.execute("cell.set", json!({"cell": "G1", "input": "=COUNTBLANK(Table1[B])"})).unwrap();
        s.execute("cell.set", json!({"cell": "H1", "input": "=COLUMNS(Table1)"})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!((get(&mut s, "F1"), get(&mut s, "G1"), get(&mut s, "H1")), (json!(2.0), json!(0.0), json!(2.0)));

        // A new row whose B cell stays empty: the row count and the blank count both move.
        s.execute("cell.set", json!({"cell": "A4", "input": 5})).unwrap();
        assert_eq!(range(&s), "A1:B4");
        assert_eq!((get(&mut s, "F1"), get(&mut s, "G1")), (json!(3.0), json!(1.0)));

        // A new column moves the column count.
        s.execute("cell.set", json!({"cell": "C2", "input": 9})).unwrap();
        assert_eq!(range(&s), "A1:C4");
        assert_eq!(get(&mut s, "H1"), json!(3.0));
    }

    #[test]
    fn table_auto_expands_on_autosum_below_it_for_one_cell_or_several() {
        for range_param in [json!(null), json!("A2:A4")] {
            let mut s = Session::new();
            s.new_workbook();
            s.execute("range.setValues", json!({"range": "A1", "values": [["A"], [1], [2]]})).unwrap();
            s.execute("insert.table", json!({"range": "A1:A3", "header": true})).unwrap();
            s.execute("selection.set", json!({"range": "A4"})).unwrap();
            let p = if range_param.is_null() { json!({}) } else { json!({"range": range_param}) };
            s.execute("formulas.autoSum", p).unwrap();
            assert_eq!(range(&s), "A1:A4", "{range_param}");
            assert_eq!(s.execute("cell.get", json!({"cell": "A4"})).unwrap()["formula"], "=SUM(A2:A3)");
        }
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
