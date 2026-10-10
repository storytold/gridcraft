//! Insert tab: tables, charts, sparklines, pictures, shapes, hyperlinks, comments and notes,
//! conditional formatting (Home › Styles) and symbols.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::*;
use crate::selection::current_region;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.table",
            "Table",
            ["Insert", "Tables"],
            Some("Cmd+T"),
            "{range?, header?: true, style?: \"TableStyleMedium2\", name?}",
            has_doc,
            insert_table
        ),
        cmd!(
            "home.formatAsTable",
            "Format as Table",
            ["Home", "Styles"],
            None,
            "{range?, style: \"TableStyleMedium2\", header?}",
            has_doc,
            insert_table
        ),
        cmd!("table.totalRow", "Total Row", ["Table", "Table Style Options"], None, "{table?, on?: bool}", has_doc, |s, p| table_flag(
            s, p, "totals"
        )),
        cmd!("table.bandedRows", "Banded Rows", ["Table", "Table Style Options"], None, "{table?, on?}", has_doc, |s, p| table_flag(
            s,
            p,
            "bandedRows"
        )),
        cmd!("table.bandedColumns", "Banded Columns", ["Table", "Table Style Options"], None, "{table?, on?}", has_doc, |s, p| table_flag(
            s,
            p,
            "bandedCols"
        )),
        cmd!("table.headerRow", "Header Row", ["Table", "Table Style Options"], None, "{table?, on?}", has_doc, |s, p| table_flag(s, p, "header")),
        cmd!("table.firstColumn", "First Column", ["Table", "Table Style Options"], None, "{table?, on?}", has_doc, |s, p| table_flag(
            s, p, "firstCol"
        )),
        cmd!("table.lastColumn", "Last Column", ["Table", "Table Style Options"], None, "{table?, on?}", has_doc, |s, p| table_flag(s, p, "lastCol")),
        cmd!("table.style", "Table Styles", ["Table", "Table Styles"], None, "{table?, style}", has_doc, table_style),
        cmd!("table.rename", "Table Name", ["Table", "Properties"], None, "{table?, name}", has_doc, table_rename),
        cmd!("table.convertToRange", "Convert to Range", ["Table", "Tools"], None, "{table?}", has_doc, table_convert),
        cmd!(
            "table.totalFunction",
            "Totals Function",
            [],
            None,
            "{table?, column: \"Sales\", function: sum|average|count|countNums|max|min|stdDev|var|none}",
            has_doc,
            table_total_fn
        ),
        cmd!(
            "insert.chart",
            "Insert Chart",
            ["Insert", "Charts"],
            None,
            "{range?, type?: column|bar|line|pie|doughnut|area|scatter|bubble|radar|combo|histogram|waterfall|funnel|treemap|sunburst|boxWhisker|stock, subtype?: clustered|stacked|stacked100|markers, title?, at?: \"H2\", width?, height?}",
            has_doc,
            insert_chart
        ),
        cmd!("insert.recommendedCharts", "Recommended Charts", ["Insert", "Charts"], None, "{range?}", has_doc, |s, p| insert_chart(
            s,
            &with(p, "type", json!("auto"))
        )),
        cmd!(
            "chart.set",
            "Chart Properties",
            ["Chart Design"],
            None,
            "{chart?: id, type?, title?, legend?: none|bottom|top|left|right, dataLabels?, gridlines?, style?, xTitle?, yTitle?, at?, width?, height?, dx?, dy?}",
            has_doc,
            chart_set
        ),
        cmd!("chart.switchRowColumn", "Switch Row/Column", ["Chart Design", "Data"], None, "{chart?}", has_doc, chart_switch),
        cmd!("chart.delete", "Delete Chart", [], None, "{chart?}", has_doc, chart_delete),
        cmd!(
            "insert.sparkline",
            "Sparklines",
            ["Insert", "Sparklines"],
            None,
            "{range: \"B2:M2\", location: \"N2\", type?: line|column|winLoss, markers?}",
            has_doc,
            insert_sparkline
        ),
        cmd!(
            "insert.picture",
            "Picture",
            ["Insert", "Illustrations"],
            None,
            "{path? | base64?: \"...\", mime?, at?: \"B2\", width?, height?, alt?}",
            has_doc,
            insert_picture
        ),
        cmd!(
            "insert.shape",
            "Shapes",
            ["Insert", "Illustrations"],
            None,
            "{kind: rectangle|roundedRectangle|ellipse|triangle|line|arrow|textBox, at?, width?, height?, text?, fill?, line?}",
            has_doc,
            insert_shape
        ),
        cmd!(
            "insert.icons",
            "Icons",
            ["Insert", "Illustrations"],
            None,
            "{name: chart|table|sum|filter|lock|comment|picture|book|folder|search|calc|check|… , at?, size?: 48, color?: hex}",
            has_doc,
            insert_icon
        ),
        cmd!("insert.textBox", "Text Box", ["Insert", "Text"], None, "{at?, width?, height?, text?}", has_doc, |s, p| insert_shape(
            s,
            &with(p, "kind", json!("textBox"))
        )),
        cmd!("shape.setText", "Edit Text Box", [], None, "{id: textBox id, text: string}", has_doc, shape_set_text),
        cmd!("object.delete", "Delete Object", [], None, "{kind: chart|image|shape, id}", has_doc, delete_object),
        cmd!("object.move", "Move Object", [], None, "{kind: chart|image|shape, id, at?: \"C3\", dx?, dy?, width?, height?}", has_doc, move_object),
        cmd!(
            "insert.link",
            "Link",
            ["Insert", "Links"],
            Some("Cmd+K"),
            "{cell?, target: \"https://…\" | \"Sheet2!A1\" | \"mailto:…\", text?, tooltip?, remove?: bool}",
            has_doc,
            insert_link
        ),
        cmd!("review.newNote", "New Note", ["Review", "Notes"], None, "{cell?, text, author?}", has_doc, |s, p| comment(s, p, false)),
        cmd!("review.newComment", "New Comment", ["Review", "Comments"], None, "{cell?, text, author?}", has_doc, |s, p| comment(s, p, true)),
        cmd!("review.replyComment", "Reply", [], None, "{cell?, text, author?}", has_doc, reply_comment),
        cmd!("review.resolveComment", "Resolve Thread", [], None, "{cell?, resolved?: true}", has_doc, resolve_comment),
        cmd!("review.deleteComment", "Delete Comment", ["Review", "Comments"], None, "{cell?}", has_doc, delete_comment),
        cmd!("review.showNote", "Show/Hide Note", ["Review", "Notes"], None, "{cell?}", has_doc, show_note),
        cmd!(
            "insert.symbol",
            "Symbol",
            ["Insert", "Symbols"],
            None,
            "{char: \"€\"} (inserted at the end of the active cell's text)",
            has_doc,
            insert_symbol
        ),
        cmd!(
            "home.conditionalFormat",
            "Conditional Formatting",
            ["Home", "Styles"],
            None,
            "{range?, rule: {type: cellIs|expression|containsText|beginsWith|endsWith|blanks|noBlanks|errors|noErrors|duplicate|unique|top10|aboveAverage|timePeriod|colorScale|dataBar|iconSet, operator?, value?, value2?, formula?, text?, rank?, bottom?, percent?, below?, period?, colors?: [..], color?, set?, style?: {font..fill..}|preset: lightRedFill|redText|yellowFill|greenFill|redBorder|lightRedFillDarkRedText…}}",
            has_doc,
            add_cf
        ),
        cmd!("home.clearRules", "Clear Rules", ["Home", "Styles", "Conditional Formatting"], None, "{range?|sheet: true}", has_doc, clear_cf),
        cmd!(
            "home.manageRules",
            "Manage Rules…",
            ["Home", "Styles", "Conditional Formatting"],
            None,
            "{delete?: index, moveUp?: index, moveDown?: index}",
            has_doc,
            manage_cf
        ),
    ]
}

fn with(p: &Json, k: &str, v: Json) -> Json {
    let mut m = p.as_object().cloned().unwrap_or_default();
    m.insert(k.into(), v);
    Json::Object(m)
}

fn block(s: &Session, p: &Json) -> Result<RangeRef> {
    if p.get("range").is_some() {
        return target_range(s, p);
    }
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let sel = d.selection.current();
    if !sel.is_single() {
        return Ok(sel);
    }
    Ok(current_region(sh, d.selection.active))
}

// ---------------------------------------------------------------- tables

fn insert_table(s: &mut Session, p: &Json) -> Result<Json> {
    let r = block(s, p)?;
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    if d.wb.sheet(sheet).is_some_and(|sh| sh.tables.iter().any(|t| t.range.intersects(&r))) {
        return Err(EngineError::Other("A table can't overlap another table.".into()));
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
    if d.wb.table(&name).is_some() || d.wb.names.iter().any(|n| n.name.eq_ignore_ascii_case(&name)) {
        return Err(EngineError::Other("The name entered already exists. Enter a unique name.".into()));
    }
    let style = str_param(p, "style").unwrap_or("TableStyleMedium2").to_string();
    let id = d.wb.next_object_id();
    edit(s, |cx| {
        // Without a header row, insert one with generic names.
        let r = if header {
            r
        } else {
            let sh = cx.sheet_mut(sheet)?;
            sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.start.row, 1);
            for (i, c) in (r.start.col..=r.end.col).enumerate() {
                sh.set_value(CellRef::new(r.start.row, c), gridcraft_core::Value::text(format!("Column{}", i + 1)));
            }
            cx.structural = true;
            RangeRef::new(r.start, CellRef::new(r.end.row + 1, r.end.col))
        };
        let sh = cx.sheet_mut(sheet)?;
        let mut names: Vec<String> = Vec::new();
        for c in r.start.col..=r.end.col {
            let mut n = sh.value(CellRef::new(r.start.row, c)).display();
            if n.is_empty() {
                n = format!("Column{}", c - r.start.col + 1);
            }
            let base = n.clone();
            let mut k = 2;
            while names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                n = format!("{base}{k}");
                k += 1;
            }
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
            columns: names.into_iter().map(|n| TableColumn { name: n, totals: TotalsFn::None, totals_label: None, formula: None }).collect(),
            style,
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
        });
        cx.structural = true;
        Ok(json!({"table": name, "range": r.a1()}))
    })
}

fn find_table(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    if let Some(n) = str_param(p, "table") {
        return d.wb.table(n).ok_or_else(|| bad("table", format!("no table named `{n}`")));
    }
    let sheet = d.wb.active_sheet;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let ti = sh
        .tables
        .iter()
        .position(|t| t.range.contains(d.selection.active))
        .ok_or_else(|| EngineError::Disabled("table".into(), "select a cell in a table".into()))?;
    Ok((sheet, ti))
}

fn table_flag(s: &mut Session, p: &Json, which: &str) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let on = bool_param(p, "on");
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let Some(t) = sh.tables.get_mut(ti) else { return Ok(Json::Null) };
        match which {
            "totals" => {
                let v = on.unwrap_or(!t.totals_row);
                if v != t.totals_row {
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
                            sh.cells.remove(CellRef::new(row, c));
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
            "header" => t.filter_button = on.unwrap_or(!t.filter_button),
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
            sh.cells
                .set(c, Cell { formula: Some(std::sync::Arc::new(Formula::new(&f))), value: gridcraft_core::Value::Empty, style: sh.style_id(c) });
        } else if let Some(l) = &col.totals_label {
            sh.set_value(c, gridcraft_core::Value::text(l.as_str()));
        } else {
            sh.cells.remove(c);
        }
    }
}

fn table_total_fn(s: &mut Session, p: &Json) -> Result<Json> {
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

fn table_style(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let style = str_param(p, "style").unwrap_or("TableStyleMedium2").to_string();
    edit(s, |cx| {
        if let Some(t) = cx.sheet_mut(si)?.tables.get_mut(ti) {
            t.style = style.clone();
        }
        Ok(Json::Null)
    })
}

fn table_rename(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    let name = str_param(p, "name").ok_or_else(|| bad("table.rename", "missing `name`"))?.to_string();
    if name.is_empty()
        || name.contains(' ')
        || name.chars().next().is_some_and(|c| c.is_ascii_digit())
        || gridcraft_core::CellRef::parse(&name).is_some()
    {
        return Err(EngineError::Other("The name that you entered is not valid.".into()));
    }
    if s.doc()?.wb.table(&name).is_some_and(|x| x != (si, ti)) {
        return Err(EngineError::Other("The name entered already exists. Enter a unique name.".into()));
    }
    edit(s, |cx| {
        let old = cx.wb.sheet(si).and_then(|sh| sh.tables.get(ti)).map(|t| t.name.clone()).unwrap_or_default();
        if let Some(t) = cx.sheet_mut(si)?.tables.get_mut(ti) {
            t.name = name.clone();
        }
        // Structured references follow the rename.
        for i in 0..cx.wb.sheets.len() {
            let keys: Vec<(CellRef, std::sync::Arc<Formula>)> =
                cx.wb.sheets.get(i).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
            let Some(sh) = cx.wb.sheet_mut(i) else { continue };
            for (c, f) in keys {
                let Some(e) = f.expr() else { continue };
                let ne = e.map(&mut |x| match x {
                    gridcraft_formula::Expr::Struct(mut st) if st.table.eq_ignore_ascii_case(&old) => {
                        st.table = name.clone();
                        gridcraft_formula::Expr::Struct(st)
                    }
                    gridcraft_formula::Expr::Name(n) if n.eq_ignore_ascii_case(&old) => gridcraft_formula::Expr::Name(name.clone()),
                    o => o,
                });
                if let Some(cell) = sh.cells.get_mut(c) {
                    cell.formula = Some(std::sync::Arc::new(Formula::from_expr(ne)));
                }
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn table_convert(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_table(s, p)?;
    edit(s, |cx| {
        // Apply the table look as plain formatting, then drop the table.
        let wb = &mut cx.wb;
        let Some(t) = wb.sheet(si).and_then(|sh| sh.tables.get(ti)).cloned() else { return Ok(Json::Null) };
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

// ---------------------------------------------------------------- charts

pub fn chart_kind(t: &str, sub: &str) -> ChartKind {
    match (t, sub) {
        ("column", "stacked") => ChartKind::ColumnStacked,
        ("column", "stacked100") => ChartKind::ColumnStacked100,
        ("bar", "stacked") => ChartKind::BarStacked,
        ("bar", "stacked100") => ChartKind::BarStacked100,
        ("bar", _) => ChartKind::BarClustered,
        ("line", "markers") => ChartKind::LineMarkers,
        ("line", "stacked") => ChartKind::LineStacked,
        ("line", _) => ChartKind::Line,
        ("pie", _) => ChartKind::Pie,
        ("doughnut", _) => ChartKind::Doughnut,
        ("area", "stacked") => ChartKind::AreaStacked,
        ("area", _) => ChartKind::Area,
        ("scatter", "lines") => ChartKind::ScatterLines,
        ("scatter", _) => ChartKind::Scatter,
        ("bubble", _) => ChartKind::Bubble,
        ("radar", _) => ChartKind::Radar,
        ("histogram", _) => ChartKind::Histogram,
        ("waterfall", _) => ChartKind::Waterfall,
        ("funnel", _) => ChartKind::Funnel,
        ("treemap", _) => ChartKind::Treemap,
        ("sunburst", _) => ChartKind::Sunburst,
        ("boxWhisker", _) => ChartKind::BoxWhisker,
        ("stock", _) => ChartKind::Stock,
        ("combo", _) => ChartKind::Combo,
        _ => ChartKind::ColumnClustered,
    }
}

/// Builds series from a block: first row/column as names/categories when they're text.
pub fn series_from_range(sh: &Sheet, r: RangeRef, by_rows: bool) -> Vec<Series> {
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
    let abs1 = |c: CellRef| format!("{q}!${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1);
    let is_text = |c: CellRef| sh.value(c).is_text() || sh.value(c).is_empty();
    let header_row = r.height() > 1
        && (r.start.col..=r.end.col).any(|c| sh.value(CellRef::new(r.start.row, c)).is_text())
        && (r.start.col..=r.end.col).filter(|c| *c != r.start.col).all(|c| is_text(CellRef::new(r.start.row, c)));
    let label_col = r.width() > 1 && (r.start.row + header_row as u32..=r.end.row).all(|row| is_text(CellRef::new(row, r.start.col)));
    let data = RangeRef::new(CellRef::new(r.start.row + header_row as u32, r.start.col + label_col as u32), r.end);
    let mut out = Vec::new();
    if !by_rows {
        for c in data.start.col..=data.end.col {
            out.push(Series {
                name: header_row.then(|| abs1(CellRef::new(r.start.row, c))),
                categories: label_col.then(|| abs(RangeRef::new(CellRef::new(data.start.row, r.start.col), CellRef::new(data.end.row, r.start.col)))),
                values: abs(RangeRef::new(CellRef::new(data.start.row, c), CellRef::new(data.end.row, c))),
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
            });
        }
    } else {
        for row in data.start.row..=data.end.row {
            out.push(Series {
                name: label_col.then(|| abs1(CellRef::new(row, r.start.col))),
                categories: header_row
                    .then(|| abs(RangeRef::new(CellRef::new(r.start.row, data.start.col), CellRef::new(r.start.row, data.end.col)))),
                values: abs(RangeRef::new(CellRef::new(row, data.start.col), CellRef::new(row, data.end.col))),
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
            });
        }
    }
    out
}

fn insert_chart(s: &mut Session, p: &Json) -> Result<Json> {
    let r = block(s, p)?;
    if r.count() > 1_000_000 {
        return Err(bad("insert.chart", "range too large for a chart"));
    }
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let mut t = str_param(p, "type").unwrap_or("column").to_string();
    // Excel plots series by the longer dimension (more rows → series in columns).
    let data_h = r.height();
    let data_w = r.width();
    let by_rows = data_w > data_h + 1;
    if t == "auto" {
        t = if data_h > 12 && data_w <= 3 { "line".into() } else { "column".into() };
    }
    let kind = chart_kind(&t, str_param(p, "subtype").unwrap_or(""));
    let series = series_from_range(sh, r, by_rows);
    if series.is_empty() {
        return Err(EngineError::Other("Select data for the chart first.".into()));
    }
    let id = d.wb.next_object_id();
    let at = cell_param(p, "at").unwrap_or_else(|| CellRef::new(r.start.row, r.end.col + 2));
    let title = str_param(p, "title").map(str::to_string).or_else(|| {
        if series.len() == 1 {
            series[0].name.as_ref().map(|n| gridcraft_calc::evaluate(&d.wb, sheet, at, n).display())
        } else {
            Some("Chart Title".into())
        }
    });
    let chart = Chart {
        id,
        kind,
        anchor: Anchor {
            cell: at,
            dx: 0.0,
            dy: 0.0,
            width: f64_param(p, "width").unwrap_or(480.0) as f32,
            height: f64_param(p, "height").unwrap_or(288.0) as f32,
        },
        title,
        series,
        legend: if matches!(kind, ChartKind::Pie | ChartKind::Doughnut) { LegendPos::Right } else { LegendPos::Bottom },
        data_labels: false,
        gridlines: !matches!(kind, ChartKind::Pie | ChartKind::Doughnut | ChartKind::Treemap | ChartKind::Sunburst | ChartKind::Funnel),
        style: 1,
        x_title: None,
        y_title: None,
        source: Some(format!("{}!{}", gridcraft_formula::quote_sheet(&sh.name), r.a1())),
        by_rows,
    };
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.charts.push(chart);
        Ok(json!({"chart": id}))
    })
}

fn find_chart(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    let sheet = d.wb.active_sheet;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let idx = match u32_param(p, "chart") {
        Some(id) => sh.charts.iter().position(|c| c.id == id),
        None => sh.charts.len().checked_sub(1),
    };
    idx.map(|i| (sheet, i)).ok_or_else(|| bad("chart", "no such chart"))
}

fn chart_set(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let Some(c) = sh.charts.get_mut(ci) else { return Ok(Json::Null) };
        if let Some(t) = str_param(p, "type") {
            c.kind = chart_kind(t, str_param(p, "subtype").unwrap_or(""));
        }
        if let Some(t) = p.get("title") {
            c.title = t.as_str().map(str::to_string);
        }
        if let Some(l) = str_param(p, "legend") {
            c.legend = match l {
                "none" => LegendPos::None,
                "top" => LegendPos::Top,
                "left" => LegendPos::Left,
                "right" => LegendPos::Right,
                _ => LegendPos::Bottom,
            };
        }
        if let Some(v) = bool_param(p, "dataLabels") {
            c.data_labels = v;
        }
        if let Some(v) = bool_param(p, "gridlines") {
            c.gridlines = v;
        }
        if let Some(v) = u32_param(p, "style") {
            c.style = v.clamp(1, 16);
        }
        if let Some(t) = p.get("xTitle") {
            c.x_title = t.as_str().map(str::to_string);
        }
        if let Some(t) = p.get("yTitle") {
            c.y_title = t.as_str().map(str::to_string);
        }
        if let Some(at) = cell_param(p, "at") {
            c.anchor.cell = at;
        }
        if let Some(w) = f64_param(p, "width") {
            c.anchor.width = w.clamp(20.0, 10000.0) as f32;
        }
        if let Some(h) = f64_param(p, "height") {
            c.anchor.height = h.clamp(20.0, 10000.0) as f32;
        }
        if let Some(v) = f64_param(p, "dx") {
            c.anchor.dx = v as f32;
        }
        if let Some(v) = f64_param(p, "dy") {
            c.anchor.dy = v as f32;
        }
        Ok(Json::Null)
    })
}

fn chart_switch(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    edit(s, |cx| {
        let Some(sh) = cx.wb.sheet(si) else { return Ok(Json::Null) };
        let Some(c) = sh.charts.get(ci) else { return Ok(Json::Null) };
        let Some(src) = c.source.clone() else { return Ok(Json::Null) };
        let (sheet_name, body) = split_sheet(&src);
        let src_sheet = sheet_name.and_then(|n| cx.wb.sheet_index(&n)).unwrap_or(si);
        let Some(r) = RangeRef::parse(body) else { return Ok(Json::Null) };
        let Some(ssh) = cx.wb.sheet(src_sheet) else { return Ok(Json::Null) };
        let by_rows = !c.by_rows;
        let series = series_from_range(ssh, r, by_rows);
        if let Some(c) = cx.sheet_mut(si)?.charts.get_mut(ci) {
            c.by_rows = by_rows;
            c.series = series;
        }
        Ok(Json::Null)
    })
}

fn chart_delete(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    edit(s, |cx| {
        cx.sheet_mut(si)?.charts.remove(ci);
        Ok(Json::Null)
    })
}

fn insert_sparkline(s: &mut Session, p: &Json) -> Result<Json> {
    let src = str_param(p, "range").ok_or_else(|| bad("insert.sparkline", "missing `range`"))?.to_string();
    let loc = cell_param(p, "location").ok_or_else(|| bad("insert.sparkline", "missing `location`"))?;
    let kind = match str_param(p, "type") {
        Some("column") => SparklineKind::Column,
        Some("winLoss") => SparklineKind::WinLoss,
        _ => SparklineKind::Line,
    };
    let markers = bool_param(p, "markers").unwrap_or(false);
    let sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        sh.sparklines.retain(|x| x.cell != loc);
        sh.sparklines.push(Sparkline { cell: loc, source: src.clone(), kind, color: Color::Theme(4, 0), markers });
        Ok(Json::Null)
    })
}

fn insert_picture(s: &mut Session, p: &Json) -> Result<Json> {
    let data = if let Some(b) = str_param(p, "base64") {
        crate::io::base64_decode(b).ok_or_else(|| bad("insert.picture", "invalid base64"))?
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path)?
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("insertPicture".into(), json!({})));
        return ok();
    };
    if data.len() > 64 * 1024 * 1024 {
        return Err(bad("insert.picture", "image larger than 64 MB"));
    }
    let mime = str_param(p, "mime")
        .map(str::to_string)
        .unwrap_or_else(|| if data.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png".into() } else { "image/jpeg".into() });
    let (w, h) = crate::io::image_size(&data).unwrap_or((320, 240));
    let d = s.doc()?;
    let id = d.wb.next_object_id();
    let at = cell_param(p, "at").unwrap_or(d.selection.active);
    let scale = (640.0 / w.max(1) as f32).min(1.0);
    let img = Image {
        id,
        anchor: Anchor {
            cell: at,
            dx: 0.0,
            dy: 0.0,
            width: f64_param(p, "width").map(|v| v as f32).unwrap_or(w as f32 * scale),
            height: f64_param(p, "height").map(|v| v as f32).unwrap_or(h as f32 * scale),
        },
        data,
        mime,
        alt: str_param(p, "alt").unwrap_or("").into(),
    };
    let sheet = d.wb.active_sheet;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.images.push(img);
        Ok(json!({"image": id}))
    })
}

fn insert_shape(s: &mut Session, p: &Json) -> Result<Json> {
    let kind = match str_param(p, "kind").unwrap_or("rectangle") {
        "roundedRectangle" => ShapeKind::RoundedRectangle,
        "ellipse" | "oval" => ShapeKind::Ellipse,
        "triangle" => ShapeKind::Triangle,
        "line" => ShapeKind::Line,
        "arrow" => ShapeKind::Arrow,
        "textBox" => ShapeKind::TextBox,
        _ => ShapeKind::Rectangle,
    };
    let d = s.doc()?;
    let id = d.wb.next_object_id();
    let at = cell_param(p, "at").unwrap_or(d.selection.active);
    let fill =
        super::format::color_param(p.get("fill")).unwrap_or(if kind == ShapeKind::TextBox { Color::rgb(255, 255, 255) } else { Color::Theme(4, 0) });
    let line = super::format::color_param(p.get("line")).unwrap_or(if kind == ShapeKind::TextBox {
        Color::rgb(0x80, 0x80, 0x80)
    } else {
        Color::Theme(4, -250)
    });
    let shape = Shape {
        id,
        kind,
        anchor: Anchor {
            cell: at,
            dx: 0.0,
            dy: 0.0,
            width: f64_param(p, "width").unwrap_or(144.0) as f32,
            height: f64_param(p, "height").unwrap_or(if kind == ShapeKind::Line { 0.0 } else { 96.0 }) as f32,
        },
        fill,
        line,
        text: str_param(p, "text").unwrap_or("").into(),
    };
    let sheet = d.wb.active_sheet;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}

/// A text box holds at most as many characters as a cell.
const SHAPE_TEXT_MAX_CHARS: usize = 32_767;

fn shape_set_text(s: &mut Session, p: &Json) -> Result<Json> {
    let id =
        p.get("id").and_then(Json::as_u64).and_then(|id| u32::try_from(id).ok()).ok_or_else(|| bad("shape.setText", "`id` must be a text box id"))?;
    let text = str_param(p, "text").ok_or_else(|| bad("shape.setText", "`text` must be a string"))?;
    if text.chars().nth(SHAPE_TEXT_MAX_CHARS).is_some() {
        return Err(bad("shape.setText", "`text` is longer than 32,767 characters"));
    }
    let d = s.doc()?;
    let sheet = d.wb.active_sheet;
    let sh = d.wb.active().ok_or_else(|| bad("shape.setText", "no such text box"))?;
    let shape = sh.shapes.iter().find(|shape| shape.id == id).ok_or_else(|| bad("shape.setText", "no such text box"))?;
    if shape.kind != ShapeKind::TextBox {
        return Err(bad("shape.setText", "the shape is not a text box"));
    }
    // Sheet protection locks drawing objects (there is no "edit objects" allowance yet).
    if sh.is_protected() {
        return Err(EngineError::Other(PROTECTED.into()));
    }
    if shape.text == text {
        return ok();
    }
    edit(s, |cx| {
        let shape = cx.sheet_mut(sheet)?.shapes.iter_mut().find(|shape| shape.id == id).ok_or_else(|| bad("shape.setText", "no such text box"))?;
        shape.text = text.into();
        Ok(Json::Null)
    })
}

fn delete_object(s: &mut Session, p: &Json) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("object.delete", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("").to_string();
    let sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        match kind.as_str() {
            "chart" => sh.charts.retain(|c| c.id != id),
            "image" => sh.images.retain(|c| c.id != id),
            _ => sh.shapes.retain(|c| c.id != id),
        }
        Ok(Json::Null)
    })
}

fn move_object(s: &mut Session, p: &Json) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("object.move", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("").to_string();
    let sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let anchor = match kind.as_str() {
            "chart" => sh.charts.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            "image" => sh.images.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            _ => sh.shapes.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
        };
        let Some(a) = anchor else { return Err(bad("object.move", "no such object")) };
        if let Some(at) = cell_param(p, "at") {
            a.cell = at;
        }
        if let Some(v) = f64_param(p, "dx") {
            a.dx = v as f32;
        }
        if let Some(v) = f64_param(p, "dy") {
            a.dy = v as f32;
        }
        if let Some(v) = f64_param(p, "width") {
            a.width = v.clamp(1.0, 10000.0) as f32;
        }
        if let Some(v) = f64_param(p, "height") {
            a.height = v.clamp(0.0, 10000.0) as f32;
        }
        Ok(Json::Null)
    })
}

// ---------------------------------------------------------------- links & comments

fn insert_link(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param(p, "cell").unwrap_or(s.doc()?.selection.active);
    if bool_param(p, "remove").unwrap_or(false) {
        return edit(s, |cx| {
            cx.sheet_mut(sheet)?.hyperlinks.remove(&at);
            Ok(Json::Null)
        });
    }
    let Some(target) = str_param(p, "target").map(str::to_string) else {
        s.ui_requests.push(crate::UiRequest::Dialog("insertLink".into(), json!({})));
        return ok();
    };
    let text = str_param(p, "text").map(str::to_string);
    let tooltip = str_param(p, "tooltip").map(str::to_string);
    edit(s, |cx| {
        let link_style = cx.wb.styles.derive(cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default(), |st| {
            st.font.color = Color::Theme(10, 0);
            st.font.underline = Underline::Single;
        });
        let sh = cx.sheet_mut(sheet)?;
        if let Some(t) = text.clone().or_else(|| sh.value(at).is_empty().then(|| target.clone())) {
            sh.cells.set(at, Cell { value: gridcraft_core::Value::text(t.as_str()), formula: None, style: link_style });
        } else {
            sh.set_style(at, link_style);
        }
        sh.hyperlinks.insert(at, Hyperlink { target: target.clone(), tooltip: tooltip.clone() });
        cx.touch(sheet, at);
        Ok(Json::Null)
    })
}

fn comment(s: &mut Session, p: &Json, threaded: bool) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param(p, "cell").unwrap_or(s.doc()?.selection.active);
    let text = str_param(p, "text").unwrap_or("").to_string();
    let author = str_param(p, "author").map(str::to_string).unwrap_or_else(|| s.prefs.user_name.clone());
    edit(s, |cx| {
        cx.sheet_mut(sheet)?
            .comments
            .insert(at, Comment { author: author.clone(), text: text.clone(), replies: vec![], threaded, resolved: false, visible: false });
        Ok(json!({"cell": at.a1()}))
    })
}

fn reply_comment(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param(p, "cell").unwrap_or(s.doc()?.selection.active);
    let text = str_param(p, "text").unwrap_or("").to_string();
    let author = str_param(p, "author").map(str::to_string).unwrap_or_else(|| s.prefs.user_name.clone());
    edit(s, |cx| {
        let c = cx.sheet_mut(sheet)?.comments.get_mut(&at).ok_or_else(|| EngineError::Other("There's no comment in this cell.".into()))?;
        c.replies.push((author.clone(), text.clone()));
        Ok(Json::Null)
    })
}

fn resolve_comment(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param(p, "cell").unwrap_or(s.doc()?.selection.active);
    let v = bool_param(p, "resolved").unwrap_or(true);
    edit(s, |cx| {
        if let Some(c) = cx.sheet_mut(sheet)?.comments.get_mut(&at) {
            c.resolved = v;
        }
        Ok(Json::Null)
    })
}

fn delete_comment(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.comments.retain(|c, _| !ranges.iter().any(|r| r.contains(*c)));
        Ok(Json::Null)
    })
}

fn show_note(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param(p, "cell").unwrap_or(s.doc()?.selection.active);
    edit(s, |cx| {
        if let Some(c) = cx.sheet_mut(sheet)?.comments.get_mut(&at) {
            c.visible = !c.visible;
        }
        Ok(Json::Null)
    })
}

fn insert_symbol(s: &mut Session, p: &Json) -> Result<Json> {
    let ch = str_param(p, "char").unwrap_or("").to_string();
    let d = s.doc()?;
    let at = d.selection.active;
    let cur = d.wb.active().and_then(|sh| sh.cell(at)).map(|c| c.input_text()).unwrap_or_default();
    s.execute("cell.set", json!({"cell": at.a1(), "input": format!("{cur}{ch}")}))
}

// ---------------------------------------------------------------- conditional formatting

fn preset_style(name: &str) -> Style {
    let mut s = Style::default();
    match name {
        "redText" => s.font.color = Color::rgb(0x9C, 0x00, 0x06),
        "yellowFill" | "yellowFillDarkYellowText" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xEB, 0x9C));
            s.font.color = Color::rgb(0x9C, 0x57, 0x00);
        }
        "greenFill" | "greenFillDarkGreenText" => {
            s.fill = Fill::solid(Color::rgb(0xC6, 0xEF, 0xCE));
            s.font.color = Color::rgb(0x00, 0x61, 0x00);
        }
        "lightRedFill" => s.fill = Fill::solid(Color::rgb(0xFF, 0xC7, 0xCE)),
        "redBorder" => {
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0xC0, 0x00, 0x00) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        _ => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xC7, 0xCE));
            s.font.color = Color::rgb(0x9C, 0x00, 0x06);
        }
    }
    s
}

fn add_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let sheet = target_sheet(s, p)?;
    let Some(rule) = p.get("rule") else {
        s.ui_requests.push(crate::UiRequest::Dialog("newFormattingRule".into(), json!({})));
        return ok();
    };
    let g = |k: &str| rule.get(k).and_then(Json::as_str).map(str::to_string);
    let style = Box::new(match rule.get("style") {
        Some(st) if st.is_object() => {
            let base = serde_json::to_value(Style::default()).map_err(|e| EngineError::Other(e.to_string()))?;
            serde_json::from_value(super::format::merge_json(base, st)).map_err(|e| bad("home.conditionalFormat", e.to_string()))?
        }
        _ => preset_style(g("preset").as_deref().unwrap_or("lightRedFillDarkRedText")),
    });
    let kind = g("type").unwrap_or_else(|| "cellIs".into());
    let value_kind = |v: Option<&Json>, default: CfValueKind| -> CfValueKind {
        match v {
            Some(Json::Object(o)) => {
                let t = o.get("type").and_then(Json::as_str).unwrap_or("num");
                let val = o.get("value").and_then(Json::as_f64).unwrap_or(0.0);
                match t {
                    "min" => CfValueKind::Min,
                    "max" => CfValueKind::Max,
                    "percent" => CfValueKind::Percent(val),
                    "percentile" => CfValueKind::Percentile(val),
                    "formula" => CfValueKind::Formula(o.get("value").and_then(Json::as_str).unwrap_or("").into()),
                    _ => CfValueKind::Number(val),
                }
            }
            _ => default,
        }
    };
    let rule = match kind.as_str() {
        "cellIs" => {
            let op = match g("operator").as_deref().unwrap_or("greater") {
                "between" => CfOperator::Between,
                "notBetween" => CfOperator::NotBetween,
                "equal" => CfOperator::Equal,
                "notEqual" => CfOperator::NotEqual,
                "less" => CfOperator::Less,
                "greaterOrEqual" => CfOperator::GreaterOrEqual,
                "lessOrEqual" => CfOperator::LessOrEqual,
                _ => CfOperator::Greater,
            };
            let a = rule.get("value").map(super::edit::json_to_input).unwrap_or_default();
            let b = rule.get("value2").map(super::edit::json_to_input);
            let quote = |v: String| {
                if gridcraft_core::parse::parse_number_text(&v).is_some() || v.starts_with('=') {
                    v.trim_start_matches('=').to_string()
                } else {
                    format!("\"{}\"", v.replace('"', "\"\""))
                }
            };
            CfRule::CellIs { op, a: quote(a), b: b.map(quote), style }
        }
        "expression" => CfRule::Expression { formula: g("formula").unwrap_or_default().trim_start_matches('=').to_string(), style },
        "containsText" => CfRule::ContainsText { text: g("text").unwrap_or_default(), style },
        "notContainsText" => CfRule::NotContainsText { text: g("text").unwrap_or_default(), style },
        "beginsWith" => CfRule::BeginsWith { text: g("text").unwrap_or_default(), style },
        "endsWith" => CfRule::EndsWith { text: g("text").unwrap_or_default(), style },
        "blanks" => CfRule::Blanks { style },
        "noBlanks" => CfRule::NoBlanks { style },
        "errors" => CfRule::Errors { style },
        "noErrors" => CfRule::NoErrors { style },
        "duplicate" => CfRule::Duplicate { style },
        "unique" => CfRule::Unique { style },
        "top10" => CfRule::Top10 {
            bottom: rule.get("bottom").and_then(Json::as_bool).unwrap_or(false),
            percent: rule.get("percent").and_then(Json::as_bool).unwrap_or(false),
            rank: rule.get("rank").and_then(Json::as_u64).unwrap_or(10) as u32,
            style,
        },
        "aboveAverage" => CfRule::AboveAverage { below: rule.get("below").and_then(Json::as_bool).unwrap_or(false), equal: false, std_dev: 0, style },
        "timePeriod" => CfRule::TimePeriod { period: g("period").unwrap_or_else(|| "today".into()), style },
        "colorScale" => {
            let cols: Vec<Color> = rule
                .get("colors")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(|c| super::format::color_param(Some(c))).collect())
                .unwrap_or_else(|| vec![Color::rgb(0xF8, 0x69, 0x6B), Color::rgb(0xFF, 0xEB, 0x84), Color::rgb(0x63, 0xBE, 0x7B)]);
            let stops = match cols.len() {
                2 => vec![(CfValueKind::Min, cols[0]), (CfValueKind::Max, cols[1])],
                _ => vec![
                    (CfValueKind::Min, cols.first().copied().unwrap_or_default()),
                    (CfValueKind::Percentile(50.0), cols.get(1).copied().unwrap_or_default()),
                    (CfValueKind::Max, cols.get(2).copied().unwrap_or_default()),
                ],
            };
            CfRule::ColorScale { stops }
        }
        "dataBar" => CfRule::DataBar {
            min: value_kind(rule.get("min"), CfValueKind::Min),
            max: value_kind(rule.get("max"), CfValueKind::Max),
            color: super::format::color_param(rule.get("color")).unwrap_or(Color::rgb(0x63, 0x8E, 0xC6)),
            gradient: rule.get("gradient").and_then(Json::as_bool).unwrap_or(true),
            show_value: true,
        },
        "iconSet" => CfRule::IconSet {
            set: g("set").unwrap_or_else(|| "3TrafficLights1".into()),
            thresholds: vec![],
            reverse: rule.get("reverse").and_then(Json::as_bool).unwrap_or(false),
            show_value: true,
        },
        other => return Err(bad("home.conditionalFormat", format!("unknown rule type `{other}`"))),
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let priority = sh.cond_formats.iter().map(|c| c.priority).max().unwrap_or(0) + 1;
        // New rules take precedence over older ones.
        for cf in sh.cond_formats.iter_mut() {
            cf.priority += 1;
        }
        sh.cond_formats.insert(0, CondFormat { ranges: ranges.clone(), rule, priority: 1, stop_if_true: false });
        let _ = priority;
        Ok(json!({"rules": sh.cond_formats.len()}))
    })
}

fn clear_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let whole = bool_param(p, "sheet").unwrap_or(false);
    let ranges = target_ranges(s, p)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if whole {
            sh.cond_formats.clear();
        } else {
            for cf in sh.cond_formats.iter_mut() {
                cf.ranges.retain(|r| !ranges.iter().any(|x| x.intersects(r)));
            }
            sh.cond_formats.retain(|c| !c.ranges.is_empty());
        }
        Ok(Json::Null)
    })
}

fn manage_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    if p.get("delete").is_none() && p.get("moveUp").is_none() && p.get("moveDown").is_none() {
        let list: Vec<Json> = s
            .doc()?
            .wb
            .sheet(sheet)
            .map(|sh| {
                sh.cond_formats.iter().map(|c| json!({"ranges": c.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>(), "rule": c.rule})).collect()
            })
            .unwrap_or_default();
        return Ok(json!({"rules": list}));
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let n = sh.cond_formats.len();
        if let Some(i) = u32_param(p, "delete").map(|v| v as usize)
            && i < n
        {
            sh.cond_formats.remove(i);
        }
        if let Some(i) = u32_param(p, "moveUp").map(|v| v as usize)
            && i > 0
            && i < n
        {
            sh.cond_formats.swap(i, i - 1);
        }
        if let Some(i) = u32_param(p, "moveDown").map(|v| v as usize)
            && i + 1 < n
        {
            sh.cond_formats.swap(i, i + 1);
        }
        for (k, cf) in sh.cond_formats.iter_mut().enumerate() {
            cf.priority = k as u32 + 1;
        }
        Ok(Json::Null)
    })
}

fn insert_icon(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").unwrap_or("check").to_string();
    let size = f64_param(p, "size").unwrap_or(48.0).clamp(8.0, 1000.0) as f32;
    let d = s.doc()?;
    let id = d.wb.next_object_id();
    let at = cell_param(p, "at").unwrap_or(d.selection.active);
    let color = super::format::color_param(p.get("color")).unwrap_or(Color::Theme(1, 0));
    let shape = Shape {
        id,
        kind: ShapeKind::Icon,
        anchor: Anchor { cell: at, dx: 4.0, dy: 4.0, width: size, height: size },
        fill: color,
        line: Color::Auto,
        text: name,
    };
    let sheet = d.wb.active_sheet;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}
