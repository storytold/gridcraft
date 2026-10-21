//! Insert tab: tables, charts, sparklines, pictures, shapes, hyperlinks, comments and notes,
//! conditional formatting (Home › Styles) and symbols.

use gridcraft_core::{CellRef, MAX_ROWS, RangeRef};
use gridcraft_formula::adjust::{Axis, Edit};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::sheet::{rewrite, shift_cell_features};
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
            "{range?, style: \"TableStyleMedium2\", header?} (inside a table: restyles it)",
            has_doc,
            format_as_table
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
        cmd!(
            "insert.recommendedCharts",
            "Recommended Charts",
            ["Insert", "Charts"],
            None,
            "{range?, type?, subtype?} (no type: the first of insert.chartRecommendations; the ribbon button opens the dialog)",
            has_doc,
            |s, p| insert_chart(s, &if p.get("type").is_some() { p.clone() } else { with(p, "type", json!("auto")) })
        ),
        cmd!(
            query "insert.chartRecommendations",
            "Chart Recommendations",
            [],
            None,
            "{range?} → [{type, subtype, label}] chart types that suit the data's shape, best first",
            has_doc,
            |s, p| Ok(Json::Array(recommend_charts(s, p)?.into_iter().map(|(t, sub, label)| json!({"type": t, "subtype": sub, "label": label})).collect()))
        ),
        cmd!(
            "chart.set",
            "Chart Properties",
            ["Chart Design"],
            None,
            "{chart?: id, type?, title?, legend?: none|bottom|top|left|right, dataLabels?, gridlines?, quartiles?: exclusive|inclusive, style?, xTitle?, yTitle?, at?, width?, height?, dx?, dy?}",
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
            "{path? | base64?: \"...\", mime?, at?: \"B2\", width?, height?, alt?, mode?: moveOnly|moveAndSize|absolute, placement?: cell|overCells}",
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
            "object.setAnchorMode",
            "Object Properties",
            [],
            None,
            "{kind: chart|image|shape, id, mode: moveAndSize|moveOnly|absolute}",
            has_doc,
            set_anchor_mode
        ),
        cmd!(
            "insert.link",
            "Link",
            ["Insert", "Links"],
            Some("Cmd+K"),
            "{cell?, target: \"https://…\" | \"Book.xlsx\" | \"Sheet2!A1\" | \"mailto:…\", text?, tooltip?, remove?: bool}",
            has_doc,
            insert_link
        ),
        cmd!(
            "link.open",
            "Open Hyperlink",
            [],
            None,
            "{url?: \"https://…\" | \"mailto:…\" | \"Sheet2!A1\", cell?} (default: the cell's link); only http, https and mailto are opened, any other scheme is an error; the UI asks before opening",
            has_doc,
            open_link
        ),
        cmd!("review.newNote", "New Note", ["Review", "Notes"], Some("Shift+F2"), "{cell?, text, author?}", has_doc, |s, p| comment(s, p, false)),
        cmd!("review.newComment", "New Comment", ["Review", "Comments"], Some("Cmd+Shift+F2"), "{cell?, text, author?}", has_doc, |s, p| comment(
            s, p, true
        )),
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
            "{range?, stopIfTrue?, rule: {type: cellIs|expression|containsText|notContainsText|beginsWith|endsWith|blanks|noBlanks|errors|noErrors|duplicate|unique|top10|aboveAverage|timePeriod|colorScale|dataBar|iconSet, operator?, value?, value2?, formula?, text?, rank?, bottom?, percent?, below?, equal?, stdDev?, period?, colors?: [..], min?|mid?|max?: {type: min|max|num|percent|percentile|formula, value?}, color?, gradient?, showValue?, set?, reverse?, style?: {font..fill..}|preset: lightRedFill|redText|yellowFill|greenFill|redBorder|lightRedFillDarkRedText…, fill?, fontColor?, border?: hex|none, bold?, italic?, underline?, strike?}}",
            has_doc,
            add_cf
        ),
        cmd!("home.clearRules", "Clear Rules", ["Home", "Styles", "Conditional Formatting"], None, "{range?|sheet: true}", has_doc, clear_cf),
        cmd!(
            "home.manageRules",
            "Manage Rules…",
            ["Home", "Styles", "Conditional Formatting"],
            None,
            "{show?: sheet|selection, selection?: \"A1:B9,D2\"} lists the rules (index, appliesTo, stopIfTrue, params); {set?: [{rule|params, appliesTo, stopIfTrue?}] replaces them all, {update?: index, rule?: {..as home.conditionalFormat}, appliesTo?: \"=$A$1:$B$9\", stopIfTrue?, duplicate?: index, delete?: index, moveUp?: index, moveDown?: index} changes them",
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

fn chart_areas(s: &Session, p: &Json) -> Result<Vec<RangeRef>> {
    if p.get("range").is_some() {
        return target_ranges(s, p);
    }
    let d = s.doc()?;
    if d.selection.ranges.len() > 1 {
        return Ok(d.selection.ranges.clone());
    }
    Ok(vec![block(s, p)?])
}

// ---------------------------------------------------------------- tables

fn insert_table(s: &mut Session, p: &Json) -> Result<Json> {
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
            if sh.cells.iter_range(last).any(|(_, x)| x.formula.is_some() || !x.value.is_empty()) {
                return Err(EngineError::Other("To prevent possible loss of data, cells can't be pushed off the end of the worksheet.".into()));
            }
            sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.start.row, 1);
            // Like Insert Cells > Shift down: move merges, conditional formats, notes, links and pictures, and re-point every formula
            // reference (on any sheet) at the cells that moved.
            let e = Edit::InsertCells { axis: Axis::Rows, range: RangeRef::new(r.start, CellRef::new(r.start.row, r.end.col)) };
            shift_cell_features(sh, &e);
            let name = sh.name.clone();
            rewrite(&mut cx.wb, &name, &e);
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
fn format_as_table(s: &mut Session, p: &Json) -> Result<Json> {
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
            sh.set_cell(c, Cell { formula: Some(std::sync::Arc::new(Formula::new(&f))), value: gridcraft_core::Value::Empty, style: sh.style_id(c) });
        } else if let Some(l) = &col.totals_label {
            sh.set_value(c, gridcraft_core::Value::text(l.as_str()));
        } else {
            sh.remove_cell(c);
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

/// Table names follow the Microsoft name rules, as the xlsx writer does: letters, digits, `_` and `.`,
/// starting with a letter, `_` or `\`, and at most 255 characters. Anything else (a hyphen, say) would
/// read as an operator inside a structured reference, and the writer would rename the table on save.
fn valid_table_name(name: &str) -> bool {
    name.chars().count() <= 255
        && name.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '\\')
        && CellRef::parse(name).is_none()
        && !super::formulas::is_r1c1(name)
        && name.chars().enumerate().all(|(i, c)| c.is_alphanumeric() || c == '_' || c == '.' || (i == 0 && c == '\\'))
}

fn table_rename(s: &mut Session, p: &Json) -> Result<Json> {
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
        // Structured references follow the rename.
        for i in 0..cx.wb.sheets.len() {
            let keys: Vec<(CellRef, std::sync::Arc<Formula>)> =
                cx.wb.sheets.get(i).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
            // A bare name that is also a defined name resolves to the defined name, so it is not the table.
            let shadowed = cx.wb.name(&old, i).is_some();
            let Some(sh) = cx.wb.sheet_mut(i) else { continue };
            for (c, f) in keys {
                let Some(e) = f.expr() else { continue };
                let ne = e.map(&mut |x| match x {
                    gridcraft_formula::Expr::Struct(mut st) if st.table.eq_ignore_ascii_case(&old) => {
                        st.table = name.clone();
                        gridcraft_formula::Expr::Struct(st)
                    }
                    gridcraft_formula::Expr::Name(n) if !shadowed && n.eq_ignore_ascii_case(&old) => gridcraft_formula::Expr::Name(name.clone()),
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
    // A blank top-left cell above a header row marks the first column as the category labels even
    // when it holds numbers (years, X values), the usual spreadsheet convention, so it isn't
    // plotted as a series of its own (#287).
    let blank_corner = header_row && sh.value(r.start).is_empty();
    let label_col =
        r.width() > 1 && (blank_corner || (r.start.row + header_row as u32..=r.end.row).all(|row| is_text(CellRef::new(row, r.start.col))));
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
                quartiles: Default::default(),
                smooth: false,
                trendlines: Vec::new(),
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
                quartiles: Default::default(),
                smooth: false,
                trendlines: Vec::new(),
            });
        }
    }
    out
}

fn series_from_areas(sh: &Sheet, areas: &[RangeRef]) -> Result<(RangeRef, bool, Vec<Series>)> {
    let Some(first) = areas.first().copied() else { return Err(EngineError::Other("Select data for the chart first.".into())) };
    let bounds = areas.iter().skip(1).fold(first, |r, area| r.union(area));
    if areas.len() == 1 {
        let by_rows = first.width() > first.height() + 1;
        return Ok((bounds, by_rows, series_from_range(sh, first, by_rows)));
    }
    let valid_shape = first.width() == 1
        && first.height() > 1
        && areas.iter().skip(1).all(|area| area.width() == 1 && area.height() == first.height())
        && first.iter().all(|cell| sh.value(cell).is_text() || sh.value(cell).is_empty());
    if !valid_shape {
        return Err(bad("insert.chart", "multi-area charts require a text category column followed by equal-height series columns"));
    }
    let category = series_from_range(sh, first, false).into_iter().next().map(|series| series.values);
    let mut series = Vec::with_capacity(areas.len() - 1);
    for area in &areas[1..] {
        let mut area_series = series_from_range(sh, *area, false);
        let Some(mut item) = area_series.pop() else {
            return Err(EngineError::Other("Select data for the chart first.".into()));
        };
        if item.name.is_none() {
            return Err(bad("insert.chart", "multi-area series columns require headers"));
        }
        item.categories = category.clone();
        series.push(item);
    }
    Ok((bounds, false, series))
}

/// A scatter plot of a block without category labels takes its first column (or row) as the X
/// values of the others, as Excel does: two plain columns make one series of points, not two (#428).
pub fn scatter_x_from_first(kind: ChartKind, series: &mut Vec<Series>) {
    if matches!(kind, ChartKind::Scatter | ChartKind::ScatterLines) && series.len() > 1 && series[0].categories.is_none() {
        let x = series.remove(0).values;
        for se in series.iter_mut() {
            se.categories = Some(x.clone());
        }
    }
}

/// Most cells a chart's source block may span (bounds the series it builds).
const MAX_CHART_CELLS: u64 = 1_000_000;

/// Chart types that suit the shape of the data, best first, as `(type, subtype, label)` for
/// `insert.chart`. Our own heuristic from the shape of the block (series count, points per
/// series, text categories, negative values), not a copy of Excel's ranking: two numeric columns
/// without labels → scatter; one short series → column, pie, bar; long series → line; several
/// series → clustered and stacked columns and bars, line.
pub fn recommend_charts(s: &Session, p: &Json) -> Result<Vec<(&'static str, &'static str, &'static str)>> {
    let areas = chart_areas(s, p)?;
    if areas.iter().fold(0_u64, |count, area| count.saturating_add(area.count())) > MAX_CHART_CELLS {
        return Err(bad("insert.chart", "range too large for a chart"));
    }
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let (_, by_rows, series) = series_from_areas(sh, &areas)?;
    let ranges: Vec<RangeRef> = series
        .iter()
        .filter_map(|x| match gridcraft_formula::parse(&x.values) {
            Ok(gridcraft_formula::Expr::Ref(rf)) => Some(rf.range()),
            _ => None,
        })
        .collect();
    let points = ranges.first().map_or(0, RangeRef::count);
    let (mut any_number, mut non_negative) = (false, true);
    for n in ranges.iter().flat_map(|rr| rr.iter()).filter_map(|c| sh.value(c).as_f64()) {
        any_number = true;
        non_negative &= n >= 0.0;
    }
    if series.is_empty() || !any_number {
        return Err(EngineError::Other("Select data for the chart first.".into()));
    }
    let labelled = series.first().is_some_and(|x| x.categories.is_some());
    const COLUMN: (&str, &str, &str) = ("column", "", "Clustered Column");
    const BAR: (&str, &str, &str) = ("bar", "", "Clustered Bar");
    const LINE: (&str, &str, &str) = ("line", "", "Line");
    const LINE_MARKERS: (&str, &str, &str) = ("line", "markers", "Line with Markers");
    const PIE: (&str, &str, &str) = ("pie", "", "Pie");
    const DOUGHNUT: (&str, &str, &str) = ("doughnut", "", "Doughnut");
    const AREA: (&str, &str, &str) = ("area", "", "Area");
    const SCATTER: (&str, &str, &str) = ("scatter", "", "Scatter");
    const STACKED_COLUMN: (&str, &str, &str) = ("column", "stacked", "Stacked Column");
    const STACKED_BAR: (&str, &str, &str) = ("bar", "stacked", "Stacked Bar");
    let mut out = Vec::new();
    if series.len() == 2 && !labelled && !by_rows {
        out.extend([SCATTER, LINE_MARKERS, COLUMN]);
    } else if series.len() == 1 {
        if points > 12 {
            out.extend([LINE, COLUMN, AREA]);
        } else {
            out.extend([COLUMN, BAR]);
            if non_negative && points >= 2 {
                out.extend([PIE, DOUGHNUT]);
            }
            out.push(LINE_MARKERS);
        }
    } else if points > 12 {
        out.extend([LINE, COLUMN, AREA, STACKED_COLUMN]);
    } else {
        out.extend([COLUMN, BAR, STACKED_COLUMN, LINE_MARKERS, STACKED_BAR]);
    }
    Ok(out)
}

/// The chart `insert.chart` would insert for these params, without inserting it: the sheet it
/// goes on and the chart (previews in the Recommended Charts dialog).
pub fn build_chart(s: &Session, p: &Json) -> Result<(usize, Chart)> {
    let areas = chart_areas(s, p)?;
    if areas.iter().fold(0_u64, |count, area| count.saturating_add(area.count())) > MAX_CHART_CELLS {
        return Err(bad("insert.chart", "range too large for a chart"));
    }
    let mut t = str_param(p, "type").unwrap_or("column").to_string();
    let mut sub = str_param(p, "subtype").unwrap_or("").to_string();
    if t == "auto" {
        let first = recommend_charts(s, p)?.into_iter().next().unwrap_or(("column", "", ""));
        t = first.0.into();
        sub = first.1.into();
    }
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let kind = chart_kind(&t, &sub);
    let (bounds, by_rows, mut series) = series_from_areas(sh, &areas)?;
    if series.is_empty() {
        return Err(EngineError::Other("Select data for the chart first.".into()));
    }
    scatter_x_from_first(kind, &mut series);
    let id = d.wb.next_object_id();
    let at = cell_param_on(s, p, "at", sheet)?.unwrap_or_else(|| CellRef::new(bounds.start.row, bounds.end.col + 2));
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
            mode: anchor_mode_param(p).unwrap_or_default(),
        },
        title,
        series,
        legend: if matches!(kind, ChartKind::Pie | ChartKind::Doughnut) { LegendPos::Right } else { LegendPos::Bottom },
        data_labels: false,
        gridlines: !matches!(kind, ChartKind::Pie | ChartKind::Doughnut | ChartKind::Treemap | ChartKind::Sunburst | ChartKind::Funnel),
        style: 1,
        x_title: None,
        y_title: None,
        source: (areas.len() == 1).then(|| format!("{}!{}", gridcraft_formula::quote_sheet(&sh.name), bounds.a1())),
        by_rows,
        pivot: None,
    };
    Ok((sheet, chart))
}

fn insert_chart(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, chart) = build_chart(s, p)?;
    let id = chart.id;
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
    let at = cell_param_on(s, p, "at", si)?;
    let quartiles = p
        .get("quartiles")
        .map(|v| match v.as_str() {
            Some("exclusive") => Ok(gridcraft_model::QuartileMethod::Exclusive),
            Some("inclusive") => Ok(gridcraft_model::QuartileMethod::Inclusive),
            _ => Err(bad("quartiles", "expected exclusive or inclusive")),
        })
        .transpose()?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let Some(c) = sh.charts.get_mut(ci) else { return Ok(Json::Null) };
        if let Some(t) = str_param(p, "type") {
            c.kind = chart_kind(t, str_param(p, "subtype").unwrap_or(""));
        }
        if let Some(method) = quartiles {
            for series in &mut c.series {
                series.quartiles = method;
            }
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
        if let Some(at) = at {
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
        // Charts read from a file have no source range: take the block their series span.
        let Some((src, by_rows)) = c.source.clone().map(|s| (s, c.by_rows)).or_else(|| series_block(&c.series)) else { return Ok(Json::Null) };
        let (sheet_name, body) = split_sheet(&src);
        let src_sheet = sheet_name.and_then(|n| cx.wb.sheet_index(&n)).unwrap_or(si);
        let Some(mut r) = RangeRef::parse(body) else { return Ok(Json::Null) };
        let Some(ssh) = cx.wb.sheet(src_sheet) else { return Ok(Json::Null) };
        // Whole-column/row references (common in files) span a million cells: clip the block's
        // far edges to the cells in use, then refuse anything still too large to chart.
        if let Some(used) = ssh.used_range() {
            r.end.row = r.end.row.min(used.end.row).max(r.start.row);
            r.end.col = r.end.col.min(used.end.col).max(r.start.col);
        }
        if r.count() > MAX_CHART_CELLS {
            return Err(bad("chart.switchRowColumn", "range too large for a chart"));
        }
        let by_rows = !by_rows;
        let series = series_from_range(ssh, r, by_rows);
        if let Some(c) = cx.sheet_mut(si)?.charts.get_mut(ci) {
            c.source.get_or_insert(src);
            c.by_rows = by_rows;
            c.series = series;
            scatter_x_from_first(c.kind, &mut c.series);
        }
        Ok(Json::Null)
    })
}

/// The block a chart's series come from, and whether each series is a row of it: the bounding
/// range of the series names, categories and values, when they're all references to one sheet.
fn series_block(series: &[Series]) -> Option<(String, bool)> {
    let mut sheet: Option<String> = None;
    let mut bounds: Option<RangeRef> = None;
    let mut rows = 0;
    for s in series {
        for (i, f) in [Some(&s.values), s.categories.as_ref(), s.name.as_ref()].into_iter().flatten().enumerate() {
            let Ok(gridcraft_formula::Expr::Ref(r)) = gridcraft_formula::parse(f) else { return None };
            let gridcraft_formula::SheetSel::Named(n) = &r.sheet else { return None };
            if !sheet.get_or_insert_with(|| n.clone()).eq_ignore_ascii_case(n) {
                return None;
            }
            let range = r.range();
            if i == 0 && range.height() == 1 && range.width() > 1 {
                rows += 1;
            }
            bounds = Some(bounds.map_or(range, |b| b.union(&range)));
        }
    }
    Some((format!("{}!{}", gridcraft_formula::quote_sheet(&sheet?), bounds?.a1()), rows > 0 && rows == series.len()))
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
    let loc = cell_param(s, p, "location")?.ok_or_else(|| bad("insert.sparkline", "missing `location`"))?;
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
    let placement = str_param(p, "placement").unwrap_or("overCells");
    if !matches!(placement, "cell" | "overCells") {
        return Err(bad("insert.picture", "placement must be cell or overCells"));
    }
    let data = if let Some(b) = str_param(p, "base64") {
        crate::io::base64_decode(b).ok_or_else(|| bad("insert.picture", "invalid base64"))?
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path)?
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("insertPicture".into(), json!({"placement": placement})));
        return ok();
    };
    if placement == "cell" {
        let mime = validate_cell_picture(&data)?;
        let d = s.doc()?;
        let sheet = d.wb.active_sheet;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        let at = match str_param(p, "at") {
            Some(a) => CellRef::parse(a).ok_or_else(|| bad("insert.picture", "invalid cell address"))?,
            None => d.selection.active,
        };
        let at = sh.merge_at(at).map(|r| r.start).unwrap_or(at);
        let style = sh.style_id(at);
        if sh.is_protected() && d.wb.styles.get(style).protection.locked && locked_cell_error(sh, at, &d.unlocked_ranges).is_some() {
            return Err(EngineError::Other("The cell you're trying to change is on a protected sheet.".into()));
        }
        let picture = CellPicture { data, mime: mime.into(), alt: str_param(p, "alt").unwrap_or("").into() };
        return edit(s, |cx| {
            cx.sheet_mut(sheet)?.set_picture(at, std::sync::Arc::new(picture));
            cx.touch(sheet, at);
            Ok(json!({"cell": at.a1(), "placement": "cell"}))
        });
    }
    if data.len() > 64 * 1024 * 1024 {
        return Err(bad("insert.picture", "image larger than 64 MB"));
    }
    let mime = str_param(p, "mime")
        .map(str::to_string)
        .unwrap_or_else(|| if data.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png".into() } else { "image/jpeg".into() });
    let (w, h) = crate::io::image_size(&data).unwrap_or((320, 240));
    let d = s.doc()?;
    let id = d.wb.next_object_id();
    let at = cell_param(s, p, "at")?.unwrap_or(d.selection.active);
    let scale = (640.0 / w.max(1) as f32).min(1.0);
    let img = Image {
        id,
        anchor: Anchor {
            cell: at,
            dx: 0.0,
            dy: 0.0,
            width: f64_param(p, "width").map(|v| v as f32).unwrap_or(w as f32 * scale),
            height: f64_param(p, "height").map(|v| v as f32).unwrap_or(h as f32 * scale),
            // Excel inserts pictures as "Move but don't size with cells" (`editAs="oneCell"`).
            mode: anchor_mode_param(p).unwrap_or(gridcraft_model::AnchorMode::MoveOnly),
        },
        data: data.into(),
        mime,
        alt: str_param(p, "alt").unwrap_or("").into(),
    };
    let sheet = d.wb.active_sheet;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.images.push(img);
        Ok(json!({"image": id}))
    })
}

/// Validate before mutating the workbook; header dimensions alone do not prove the pixels decode.
fn validate_cell_picture(data: &[u8]) -> Result<&'static str> {
    if data.len() > 16 * 1024 * 1024 {
        return Err(bad("insert.picture", "cell pictures must be at most 16 MiB"));
    }
    let format = image::guess_format(data).map_err(|_| bad("insert.picture", "choose a valid PNG or JPEG picture"))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(bad("insert.picture", "cell pictures support PNG and JPEG")),
    };
    let (width, height) = image::ImageReader::with_format(std::io::Cursor::new(data), format)
        .into_dimensions()
        .map_err(|_| bad("insert.picture", "invalid image dimensions"))?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(bad("insert.picture", "cell pictures are limited to 8192 pixels per side and 16 million pixels"));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| bad("insert.picture", "picture cannot be decoded within the 64 MiB limit"))?;
    Ok(mime)
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
    let at = cell_param(s, p, "at")?.unwrap_or(d.selection.active);
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
            mode: anchor_mode_param(p).unwrap_or_default(),
        },
        fill,
        line,
        text: str_param(p, "text").unwrap_or("").into(),
        format: Default::default(),
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
            "slicer" => sh.slicers.retain(|c| c.id != id),
            _ => sh.shapes.retain(|c| c.id != id),
        }
        Ok(Json::Null)
    })
}

fn move_object(s: &mut Session, p: &Json) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("object.move", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("").to_string();
    let sheet = s.doc()?.wb.active_sheet;
    let at = cell_param(s, p, "at")?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let anchor = match kind.as_str() {
            "chart" => sh.charts.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            "image" => sh.images.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            "slicer" => sh.slicers.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            "shape" => sh.shapes.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            _ => None,
        };
        let Some(a) = anchor else { return Err(bad("object.move", "no such object")) };
        if let Some(at) = at {
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

/// Sets an object's anchor mode (Excel's "Move and size with cells" / "Move but don't size with
/// cells" / "Don't move or size with cells"), so it follows — or ignores — its rows and columns
/// when the sheet is sorted or edited structurally.
fn set_anchor_mode(s: &mut Session, p: &Json) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("object.setAnchorMode", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("").to_string();
    if !matches!(kind.as_str(), "chart" | "image" | "shape") {
        return Err(bad("object.setAnchorMode", "kind must be chart, image or shape"));
    }
    let mode = anchor_mode_param(p).ok_or_else(|| bad("object.setAnchorMode", "mode must be moveAndSize, moveOnly or absolute"))?;
    let sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let anchor = match kind.as_str() {
            "chart" => sh.charts.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            "image" => sh.images.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
            _ => sh.shapes.iter_mut().find(|c| c.id == id).map(|c| &mut c.anchor),
        };
        let Some(a) = anchor else { return Err(bad("object.setAnchorMode", "no such object")) };
        a.mode = mode;
        Ok(json!({"mode": anchor_mode_name(mode)}))
    })
}

// ---------------------------------------------------------------- links & comments

fn insert_link(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param_on(s, p, "cell", sheet)?.unwrap_or(s.doc()?.selection.active);
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
    let external = links_outside(&s.doc()?.wb, sheet, &target);
    edit(s, |cx| {
        let link_style = cx.wb.styles.derive(cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default(), |st| {
            st.font.color = Color::Theme(10, 0);
            st.font.underline = Underline::Single;
        });
        let sh = cx.sheet_mut(sheet)?;
        if let Some(t) = text.clone().or_else(|| sh.value(at).is_empty().then(|| target.clone())) {
            sh.set_cell(at, Cell { value: gridcraft_core::Value::text(t.as_str()), formula: None, style: link_style });
        } else {
            sh.set_style(at, link_style);
        }
        sh.hyperlinks.insert(at, Hyperlink { target: target.clone(), tooltip: tooltip.clone(), external });
        cx.touch(sheet, at);
        Ok(Json::Null)
    })
}

/// Follows a link: a reference selects it, an `http`/`https`/`mailto` address is handed to the
/// host (`UiRequest::OpenUrl`); anything else is refused ([`crate::links::classify`]). Opens no
/// dialog; the interactive click asks for confirmation first, in the UI.
fn open_link(s: &mut Session, p: &Json) -> Result<Json> {
    let target = match str_param(p, "url").or_else(|| str_param(p, "target")) {
        Some(t) => t.to_string(),
        None => {
            let sheet = target_sheet(s, p)?;
            let at = cell_param_on(s, p, "cell", sheet)?.unwrap_or(s.doc()?.selection.active);
            let Some(h) = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.hyperlinks.get(&at)) else {
                return Err(EngineError::Other("There's no hyperlink in this cell.".into()));
            };
            h.target.clone()
        }
    };
    match crate::links::classify(&target) {
        crate::links::LinkKind::Internal(r) => super::edit::go_to(s, &json!({"reference": r.trim_start_matches('#')})),
        crate::links::LinkKind::External(u) => {
            s.ui_requests.push(crate::UiRequest::OpenUrl(u.clone()));
            Ok(json!({"opened": u}))
        }
        crate::links::LinkKind::Refused(why) => Err(EngineError::Other(why)),
    }
}

/// Sorts a link target as the two pages of Excel's Insert Hyperlink dialog do: "Place in This
/// Document" is a sheet reference, a defined name or `#…`; anything else ("Existing File or Web
/// Page") is outside the workbook and saved as a relationship.
fn links_outside(wb: &Workbook, sheet: usize, target: &str) -> bool {
    let t = target.trim();
    if t.starts_with('#') || wb.name(t, sheet).is_some() || wb.table(t).is_some() {
        return false;
    }
    let (on, body) = split_sheet(t);
    // Sheet names can't hold `: \ / ? * [ ]`, so `https://…!A1` or `C:\…!A1` is no sheet reference,
    // and `Book.xlsx#Sheet1!A1` is a place in another file unless a sheet has that name.
    let on_sheet = on.is_none_or(|name| {
        wb.sheets.iter().any(|s| s.name.eq_ignore_ascii_case(&name))
            || (!name.is_empty() && !name.contains([':', '\\', '/', '?', '*', '[', ']', '#']))
    });
    !(on_sheet && RangeRef::parse(body).is_some())
}

fn comment(s: &mut Session, p: &Json, threaded: bool) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param_on(s, p, "cell", sheet)?.unwrap_or(s.doc()?.selection.active);
    let text = str_param(p, "text").unwrap_or("").to_string();
    let author = str_param(p, "author").map(str::to_string).unwrap_or_else(|| s.prefs.user_name.clone());
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.comments.insert(
            at,
            Comment {
                author: author.clone(),
                text: text.clone(),
                replies: vec![],
                threaded,
                resolved: false,
                visible: false,
                dates: vec![],
                note_box: None,
            },
        );
        Ok(json!({"cell": at.a1()}))
    })
}

fn reply_comment(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param_on(s, p, "cell", sheet)?.unwrap_or(s.doc()?.selection.active);
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
    let at = cell_param_on(s, p, "cell", sheet)?.unwrap_or(s.doc()?.selection.active);
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
    let at = cell_param_on(s, p, "cell", sheet)?.unwrap_or(s.doc()?.selection.active);
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

/// The format of a Highlight Cells preset (`lightRedFillDarkRedText`, `redBorder`…).
pub fn preset_style(name: &str) -> Style {
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

/// Whether `v` is one formula string literal: `"…"` with every inner quote doubled.
fn is_string_literal(v: &str) -> bool {
    v.len() >= 2 && v.starts_with('"') && v.ends_with('"') && v.get(1..v.len() - 1).is_some_and(|inner| !inner.replace("\"\"", "").contains('"'))
}

/// A conditional format's `style` as the model's `Style` JSON (`font`, `fill`, `borders`, `align`,
/// `num_fmt`, `protection`, merged over the default by the caller), where a colour may also be
/// written as `format_range` takes it (`"#RRGGBB"`, `{theme, tint}`, `"auto"`) and `fill` may be a
/// colour alone (a solid fill) or `"none"` (no fill). An unknown key is an error rather than
/// dropped: dropped, it left a rule with an empty format that matches and shows nothing.
fn cf_style_json(st: &Json) -> Result<Json> {
    const KEYS: [&str; 6] = ["font", "fill", "borders", "align", "num_fmt", "protection"];
    let err = |m: String| bad("home.conditionalFormat", m);
    let Some(obj) = st.as_object() else { return Err(err("`style` must be an object, e.g. {\"fill\": \"#FFC7CE\"}".into())) };
    let mut st = obj.clone();
    if let Some(k) = st.keys().find(|k| !KEYS.contains(&k.as_str())) {
        let hint = if k == "preset" { "; a preset goes in `rule.preset`" } else { "" };
        return Err(err(format!("unknown key `{k}` in `style` (known: {}){hint}", KEYS.join(", "))));
    }
    // A colour written the way the other formatting commands take it, as the model stores it.
    let color = |v: &mut Json, what: &str| -> Result<()> {
        if v.is_string() || v.get("theme").is_some() {
            let c = super::format::color_param(Some(v)).ok_or_else(|| err(format!("invalid colour {v} for `{what}`")))?;
            *v = serde_json::to_value(c).map_err(|e| EngineError::Other(e.to_string()))?;
        }
        Ok(())
    };
    if let Some(f) = st.get_mut("font").and_then(Json::as_object_mut)
        && let Some(c) = f.get_mut("color")
    {
        color(c, "font.color")?;
    }
    if let Some(fill) = st.get_mut("fill") {
        // `"none"` (or `"auto"`) is no fill, as `home.fillColor` reads it, not a solid automatic one.
        if fill.is_string() && super::format::color_param(Some(fill)) == Some(Color::Auto) {
            *fill = serde_json::to_value(Fill::default()).map_err(|e| EngineError::Other(e.to_string()))?;
        } else if fill.is_string() || fill.get("theme").is_some() {
            *fill = json!({"pattern": "Solid", "fg": fill.clone()});
        }
        if let Some(f) = fill.as_object_mut() {
            for k in ["fg", "bg"] {
                if let Some(c) = f.get_mut(k) {
                    color(c, &format!("fill.{k}"))?;
                }
            }
            // A colour with no pattern is a solid fill, not an invisible one.
            if f.contains_key("fg") && !f.contains_key("pattern") {
                f.insert("pattern".into(), json!("Solid"));
            }
        }
    }
    if let Some(b) = st.get_mut("borders").and_then(Json::as_object_mut) {
        for (side, line) in b.iter_mut() {
            if let Some(c) = line.get_mut("color") {
                color(c, &format!("borders.{side}.color"))?;
            }
        }
    }
    Ok(Json::Object(st))
}

/// #1999: a formula that doesn't parse must never reach the workbook — Excel refuses to open
/// files carrying it, even in repair mode. Validated here so the API and the New Rule dialog
/// (which dispatches this command) share one gate, the way Excel's object model refuses.
fn check_cf_formula(cmd: &str, what: &str, formula: &str) -> Result<()> {
    // Strip exactly one leading '=', the way the XLSX writer does: a value like '===A1'
    // only parses after several strips and would still be saved unparseable. (#1999 review)
    if gridcraft_formula::parse(formula.strip_prefix('=').unwrap_or(formula)).is_err() {
        return Err(bad(cmd, format!("invalid {what} formula `{formula}` — it does not parse; Excel would reject the saved file")));
    }
    Ok(())
}

fn check_cf_value(what: &str, v: &gridcraft_model::features::CfValueKind) -> Result<()> {
    if let gridcraft_model::features::CfValueKind::Formula(f) = v {
        check_cf_formula("home.conditionalFormat", what, f)?;
    }
    Ok(())
}

fn check_cf_rule(rule: &CfRule) -> Result<()> {
    const CMD: &str = "home.conditionalFormat";
    match rule {
        CfRule::CellIs { a, b, .. } => {
            check_cf_formula(CMD, "comparison value", a)?;
            if let Some(b) = b {
                check_cf_formula(CMD, "second comparison value", b)?;
            }
        }
        CfRule::Expression { formula, .. } => check_cf_formula(CMD, "rule", formula)?,
        CfRule::DataBar { min, max, .. } => {
            check_cf_value("data bar minimum", min)?;
            check_cf_value("data bar maximum", max)?;
        }
        CfRule::ColorScale { stops } => {
            for (kind, _) in stops {
                check_cf_value("color scale threshold", kind)?;
            }
        }
        CfRule::IconSet { thresholds, .. } => {
            for kind in thresholds {
                check_cf_value("icon set threshold", kind)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn add_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let sheet = target_sheet(s, p)?;
    let Some(rule) = p.get("rule") else {
        s.ui_requests.push(crate::UiRequest::Dialog("newFormattingRule".into(), json!({})));
        return ok();
    };
    let rule = parse_rule(rule, &s.doc()?.wb.theme)?;
    let stop_if_true = bool_param(p, "stopIfTrue").unwrap_or(false);
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        // New rules take precedence over older ones.
        sh.cond_formats.insert(0, CondFormat { ranges: ranges.clone(), rule, priority: 1, stop_if_true });
        renumber_cf(&mut sh.cond_formats);
        Ok(json!({"rules": sh.cond_formats.len()}))
    })
}

/// The plain format overrides that describe `st`, as [`rule_params`] lists them. They are
/// coarser than the style (one border colour on four thin sides, a solid fill, a single
/// underline), so [`rule_style`] applies only those that differ from these.
fn style_overrides(st: &Style, theme: &Theme) -> serde_json::Map<String, Json> {
    let hex = |c: &Color| c.hex(theme);
    let line = [st.borders.left, st.borders.right, st.borders.top, st.borders.bottom].into_iter().find(|l| l.style != BorderStyle::None);
    let mut m = serde_json::Map::new();
    m.insert("fill".into(), if st.fill.pattern == PatternType::None { json!("none") } else { hex(&st.fill.fg).map_or(Json::Null, Json::from) });
    m.insert("fontColor".into(), hex(&st.font.color).map_or(json!("auto"), Json::from));
    m.insert("border".into(), line.map_or(json!("none"), |l| json!(hex(&l.color).unwrap_or_else(|| "#000000".into()))));
    m.insert("bold".into(), json!(st.font.bold));
    m.insert("italic".into(), json!(st.font.italic));
    m.insert("underline".into(), json!(st.font.underline != Underline::None));
    m.insert("strike".into(), json!(st.font.strike));
    m
}

/// The style a rule's params give: `style` (a partial style) or a `preset`, then the plain
/// overrides `fill`, `fontColor`, `border` (colours, `none` clears), `bold`, `italic`,
/// `underline` and `strike`. An override equal to what the base style already shows is left
/// out, so a rule sent back unchanged keeps its partial borders, patterns and underline kind.
fn rule_style(rule: &Json, theme: &Theme) -> Result<Box<Style>> {
    let preset = rule.get("preset").and_then(Json::as_str);
    let mut st = match rule.get("style") {
        Some(st) if !st.is_null() => {
            // A listed rule's `style` carries `dxf_stated` (the parts a file states at their
            // default value): it is kept as is, not checked as a format key.
            let mut st = st.clone();
            let stated = st.as_object_mut().and_then(|o| o.remove("dxf_stated"));
            let st = cf_style_json(&st)?;
            let base = serde_json::to_value(Style::default()).map_err(|e| EngineError::Other(e.to_string()))?;
            let mut style: Style =
                serde_json::from_value(super::format::merge_json(base, &st)).map_err(|e| bad("home.conditionalFormat", e.to_string()))?;
            if let Some(d) = stated {
                style.dxf_stated = serde_json::from_value(d).map_err(|e| bad("home.conditionalFormat", e.to_string()))?;
            }
            style
        }
        // Plain overrides alone start from an empty format, not from the default preset.
        _ if preset.is_none() && ["fill", "fontColor", "border", "bold", "italic", "underline", "strike"].iter().any(|k| rule.get(*k).is_some()) => {
            Style::default()
        }
        _ => preset_style(preset.unwrap_or("lightRedFillDarkRedText")),
    };
    let shown = style_overrides(&st, theme);
    let changed = |k: &str| rule.get(k).filter(|v| shown.get(k) != Some(*v));
    // Colours compare by the colour they give, so `#ffc7ce`, `#FFC7CE` and a theme slot of the
    // same shade all count as unchanged.
    let changed_color = |k: &str| {
        let c = super::format::color_param(rule.get(k))?;
        let base = super::format::color_param(shown.get(k));
        (base.map(|b| b.hex(theme)) != Some(c.hex(theme))).then_some(c)
    };
    if let Some(c) = changed_color("fill") {
        st.fill = if c == Color::Auto { Fill::default() } else { Fill::solid(c) };
    }
    if let Some(c) = changed_color("fontColor") {
        st.font.color = c;
    }
    if let Some(c) = changed_color("border") {
        st.borders = if c == Color::Auto {
            Borders::default()
        } else {
            let l = BorderLine { style: BorderStyle::Thin, color: c };
            Borders { left: l, right: l, top: l, bottom: l, ..Default::default() }
        };
    }
    let flag = |k: &str| changed(k).and_then(Json::as_bool);
    if let Some(b) = flag("bold") {
        st.font.bold = b;
    }
    if let Some(b) = flag("italic") {
        st.font.italic = b;
    }
    if let Some(b) = flag("underline") {
        st.font.underline = if b { Underline::Single } else { Underline::None };
    }
    if let Some(b) = flag("strike") {
        st.font.strike = b;
    }
    Ok(Box::new(st))
}

/// A threshold from `{type: min|max|num|percent|percentile|formula, value}`.
fn cf_value_kind(v: Option<&Json>, default: CfValueKind) -> CfValueKind {
    match v {
        Some(Json::Object(o)) => {
            let val = o.get("value").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(0.0);
            match o.get("type").and_then(Json::as_str).unwrap_or("num") {
                "min" => CfValueKind::Min,
                "max" => CfValueKind::Max,
                "percent" => CfValueKind::Percent(val),
                "percentile" => CfValueKind::Percentile(val),
                // Kept as typed: the check and the file writer each take off one leading `=` (#1999).
                "formula" => CfValueKind::Formula(o.get("value").map(super::edit::json_to_input).unwrap_or_default()),
                _ => CfValueKind::Number(val),
            }
        }
        _ => default,
    }
}

fn cf_value_json(k: &CfValueKind) -> Json {
    match k {
        CfValueKind::Min => json!({"type": "min"}),
        CfValueKind::Max => json!({"type": "max"}),
        CfValueKind::Number(v) => json!({"type": "num", "value": v}),
        CfValueKind::Percent(v) => json!({"type": "percent", "value": v}),
        CfValueKind::Percentile(v) => json!({"type": "percentile", "value": v}),
        CfValueKind::Formula(f) => json!({"type": "formula", "value": if f.starts_with('=') { f.clone() } else { format!("={f}") }}),
    }
}

/// A rule from its `home.conditionalFormat` params.
fn parse_rule(rule: &Json, theme: &Theme) -> Result<CfRule> {
    let g = |k: &str| rule.get(k).and_then(Json::as_str).map(str::to_string);
    let flag = |k: &str| rule.get(k).and_then(Json::as_bool);
    let kind = g("type").unwrap_or_else(|| "cellIs".into());
    let style = || rule_style(rule, theme);
    let rule = match kind.as_str() {
        "cellIs" => {
            // Excel's file and VBA names (`lessThan`, …) are accepted too.
            let op = match g("operator").as_deref().unwrap_or("greater") {
                "between" => CfOperator::Between,
                "notBetween" => CfOperator::NotBetween,
                "equal" => CfOperator::Equal,
                "notEqual" => CfOperator::NotEqual,
                "less" | "lessThan" => CfOperator::Less,
                "greaterOrEqual" | "greaterThanOrEqual" => CfOperator::GreaterOrEqual,
                "lessOrEqual" | "lessThanOrEqual" => CfOperator::LessOrEqual,
                "greater" | "greaterThan" => CfOperator::Greater,
                other => return Err(bad("home.conditionalFormat", format!("unknown operator `{other}`"))),
            };
            let a = rule.get("value").map(super::edit::json_to_input).unwrap_or_default();
            let b = rule.get("value2").map(super::edit::json_to_input);
            let quote = |v: String| {
                // A number, a formula, or text already written as a string literal (`"Closed"`, as
                // in the file and in openpyxl's `CellIsRule`) is kept; other text is quoted.
                if gridcraft_core::parse::parse_number_text(&v).is_some() || v.starts_with('=') {
                    v.trim_start_matches('=').to_string()
                } else if is_string_literal(&v) {
                    v
                } else {
                    format!("\"{}\"", v.replace('"', "\"\""))
                }
            };
            CfRule::CellIs { op, a: quote(a), b: b.map(quote), style: style()? }
        }
        "expression" => CfRule::Expression { formula: g("formula").unwrap_or_default().trim_start_matches('=').to_string(), style: style()? },
        "containsText" => CfRule::ContainsText { text: g("text").unwrap_or_default(), style: style()? },
        "notContainsText" => CfRule::NotContainsText { text: g("text").unwrap_or_default(), style: style()? },
        "beginsWith" => CfRule::BeginsWith { text: g("text").unwrap_or_default(), style: style()? },
        "endsWith" => CfRule::EndsWith { text: g("text").unwrap_or_default(), style: style()? },
        "blanks" => CfRule::Blanks { style: style()? },
        "noBlanks" => CfRule::NoBlanks { style: style()? },
        "errors" => CfRule::Errors { style: style()? },
        "noErrors" => CfRule::NoErrors { style: style()? },
        "duplicate" => CfRule::Duplicate { style: style()? },
        "unique" => CfRule::Unique { style: style()? },
        "top10" => CfRule::Top10 {
            bottom: flag("bottom").unwrap_or(false),
            percent: flag("percent").unwrap_or(false),
            rank: rule.get("rank").and_then(Json::as_u64).unwrap_or(10).clamp(1, 1000) as u32,
            style: style()?,
        },
        "aboveAverage" => CfRule::AboveAverage {
            below: flag("below").unwrap_or(false),
            equal: flag("equal").unwrap_or(false),
            std_dev: rule.get("stdDev").and_then(Json::as_u64).unwrap_or(0).min(3) as u8,
            style: style()?,
        },
        "timePeriod" => CfRule::TimePeriod { period: g("period").unwrap_or_else(|| "today".into()), style: style()? },
        "colorScale" => {
            let cols: Vec<Color> = rule
                .get("colors")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(|c| super::format::color_param(Some(c))).collect())
                .unwrap_or_else(|| vec![Color::rgb(0xF8, 0x69, 0x6B), Color::rgb(0xFF, 0xEB, 0x84), Color::rgb(0x63, 0xBE, 0x7B)]);
            let min = cf_value_kind(rule.get("min"), CfValueKind::Min);
            let max = cf_value_kind(rule.get("max"), CfValueKind::Max);
            let stops = match cols.len() {
                2 => vec![(min, cols[0]), (max, cols[1])],
                _ => vec![
                    (min, cols.first().copied().unwrap_or_default()),
                    (cf_value_kind(rule.get("mid"), CfValueKind::Percentile(50.0)), cols.get(1).copied().unwrap_or_default()),
                    (max, cols.get(2).copied().unwrap_or_default()),
                ],
            };
            CfRule::ColorScale { stops }
        }
        "dataBar" => CfRule::DataBar {
            min: cf_value_kind(rule.get("min"), CfValueKind::Min),
            max: cf_value_kind(rule.get("max"), CfValueKind::Max),
            color: super::format::color_param(rule.get("color")).unwrap_or(Color::rgb(0x63, 0x8E, 0xC6)),
            gradient: flag("gradient").unwrap_or(true),
            show_value: flag("showValue").unwrap_or(true),
        },
        "iconSet" => {
            let set = g("set").unwrap_or_else(|| "3TrafficLights1".into());
            let thresholds: Vec<CfValueKind> = match rule.get("thresholds") {
                Some(t) if !t.is_null() => {
                    serde_json::from_value(t.clone()).map_err(|e| bad("home.conditionalFormat", format!("`thresholds`: {e}")))?
                }
                _ => vec![],
            };
            // The evaluator reads one threshold per icon, or one fewer (no lower bound); any other
            // count would be stored and then ignored for the defaults.
            let count = icon_count(&set);
            if !thresholds.is_empty() && thresholds.len() != count && thresholds.len() + 1 != count {
                return Err(bad("home.conditionalFormat", format!("icon set `{set}` takes {count} thresholds, or {}", count - 1)));
            }
            CfRule::IconSet { set, thresholds, reverse: flag("reverse").unwrap_or(false), show_value: flag("showValue").unwrap_or(true) }
        }
        other => return Err(bad("home.conditionalFormat", format!("unknown rule type `{other}`"))),
    };
    check_cf_rule(&rule)?;
    Ok(rule)
}

/// A rule as `home.conditionalFormat` params, the inverse of [`parse_rule`]: Manage Rules edits
/// a rule through these. The format is given as plain overrides, plus the whole `style` so that
/// parts the overrides don't cover (number format, partial borders) survive an edit.
fn rule_params(rule: &CfRule, theme: &Theme) -> Json {
    let hex = |c: &Color| c.hex(theme).map(Json::from).unwrap_or(Json::Null);
    let unformula = |f: &str| {
        if is_string_literal(f) {
            let t = f.get(1..f.len() - 1).unwrap_or_default().replace("\"\"", "\"");
            // Text that would read back as a number, a formula or a literal stays quoted, as
            // `parse_rule` keeps a string literal as it is.
            if gridcraft_core::parse::parse_number_text(&t).is_some() || t.starts_with('=') || is_string_literal(&t) { f.to_string() } else { t }
        } else if gridcraft_core::parse::parse_number_text(f).is_some() {
            f.to_string()
        } else {
            format!("={f}")
        }
    };
    let (mut v, style) = match rule {
        CfRule::CellIs { op, a, b, style } => {
            let op = match op {
                CfOperator::Between => "between",
                CfOperator::NotBetween => "notBetween",
                CfOperator::Equal => "equal",
                CfOperator::NotEqual => "notEqual",
                CfOperator::Greater => "greater",
                CfOperator::Less => "less",
                CfOperator::GreaterOrEqual => "greaterOrEqual",
                CfOperator::LessOrEqual => "lessOrEqual",
            };
            let mut v = json!({"type": "cellIs", "operator": op, "value": unformula(a)});
            if let Some(b) = b {
                v["value2"] = json!(unformula(b));
            }
            (v, Some(style))
        }
        CfRule::Expression { formula, style } => (json!({"type": "expression", "formula": format!("={formula}")}), Some(style)),
        CfRule::ContainsText { text, style } => (json!({"type": "containsText", "text": text}), Some(style)),
        CfRule::NotContainsText { text, style } => (json!({"type": "notContainsText", "text": text}), Some(style)),
        CfRule::BeginsWith { text, style } => (json!({"type": "beginsWith", "text": text}), Some(style)),
        CfRule::EndsWith { text, style } => (json!({"type": "endsWith", "text": text}), Some(style)),
        CfRule::Blanks { style } => (json!({"type": "blanks"}), Some(style)),
        CfRule::NoBlanks { style } => (json!({"type": "noBlanks"}), Some(style)),
        CfRule::Errors { style } => (json!({"type": "errors"}), Some(style)),
        CfRule::NoErrors { style } => (json!({"type": "noErrors"}), Some(style)),
        CfRule::Duplicate { style } => (json!({"type": "duplicate"}), Some(style)),
        CfRule::Unique { style } => (json!({"type": "unique"}), Some(style)),
        CfRule::Top10 { bottom, percent, rank, style } => (json!({"type": "top10", "bottom": bottom, "percent": percent, "rank": rank}), Some(style)),
        CfRule::AboveAverage { below, equal, std_dev, style } => {
            (json!({"type": "aboveAverage", "below": below, "equal": equal, "stdDev": std_dev}), Some(style))
        }
        CfRule::TimePeriod { period, style } => (json!({"type": "timePeriod", "period": period}), Some(style)),
        CfRule::ColorScale { stops } => {
            let mut v = json!({"type": "colorScale", "colors": stops.iter().map(|(_, c)| hex(c)).collect::<Vec<_>>()});
            let keys: &[&str] = if stops.len() == 2 { &["min", "max"] } else { &["min", "mid", "max"] };
            for (k, (kind, _)) in keys.iter().zip(stops) {
                v[*k] = cf_value_json(kind);
            }
            (v, None)
        }
        CfRule::DataBar { min, max, color, gradient, show_value } => (
            json!({"type": "dataBar", "color": hex(color), "gradient": gradient, "showValue": show_value, "min": cf_value_json(min), "max": cf_value_json(max)}),
            None,
        ),
        CfRule::IconSet { set, thresholds, reverse, show_value } => {
            (json!({"type": "iconSet", "set": set, "thresholds": thresholds, "reverse": reverse, "showValue": show_value}), None)
        }
    };
    if let Some(st) = style {
        for (k, x) in style_overrides(st, theme) {
            v[k.as_str()] = x;
        }
        v["style"] = serde_json::to_value(st).unwrap_or(Json::Null);
    }
    v
}

/// `=$A$1:$B$9,$D$2` for a rule's ranges, as Manage Rules shows them.
fn applies_to(ranges: &[RangeRef]) -> String {
    let a = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row as u64 + 1);
    let parts: Vec<String> = ranges
        .iter()
        .map(|r| {
            if r.is_single() {
                a(r.start)
            } else if r.is_full_cols() || r.is_full_rows() {
                r.a1()
            } else {
                format!("{}:{}", a(r.start), a(r.end))
            }
        })
        .collect();
    format!("={}", parts.join(","))
}

/// The ranges of an `appliesTo` like `=$A$1:$B$9,$D$2` on `sheet`. A `Sheet!` prefix must name
/// that sheet: a rule's ranges live on its own sheet, so another sheet's range is refused rather
/// than read as the same address here.
fn parse_applies_to(text: &str, wb: &Workbook, sheet: usize) -> Result<Vec<RangeRef>> {
    let body = text.trim().trim_start_matches('=');
    let mut out = Vec::new();
    for part in body.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let (name, part) = split_sheet(part);
        if let Some(name) = name
            && wb.sheet_index(&name) != Some(sheet)
        {
            return Err(bad("home.manageRules", format!("`appliesTo` names another sheet: `{name}`")));
        }
        out.push(RangeRef::parse(part).ok_or_else(|| bad("home.manageRules", format!("not a range: `{part}`")))?);
    }
    if out.is_empty() {
        return Err(bad("home.manageRules", "`appliesTo` names no range"));
    }
    Ok(out)
}

fn renumber_cf(rules: &mut [CondFormat]) {
    for (k, cf) in rules.iter_mut().enumerate() {
        cf.priority = k as u32 + 1;
    }
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
    if let Some(list) = p.get("set") {
        // The whole list at once, highest priority first (Manage Rules › Cancel puts it back).
        // A listed `rule` restores exactly; `params` go through the same parsing as a new rule.
        let theme = s.doc()?.wb.theme.clone();
        let mut rules = Vec::new();
        for (i, x) in list.as_array().ok_or_else(|| bad("home.manageRules", "`set` is a list of rules"))?.iter().enumerate() {
            let ranges = parse_applies_to(x.get("appliesTo").and_then(Json::as_str).unwrap_or(""), &s.doc()?.wb, sheet)?;
            let rule = match (x.get("rule"), x.get("params")) {
                (Some(r), _) => match serde_json::from_value::<CfRule>(r.clone()) {
                    Ok(rule) => {
                        check_cf_rule(&rule)?;
                        rule
                    }
                    // Params written as `rule` (they name a `type`) still parse; a serialized rule
                    // that doesn't deserialize is refused rather than read as a default rule.
                    Err(_) if r.get("type").is_some_and(Json::is_string) => parse_rule(r, &theme)?,
                    Err(e) => return Err(bad("home.manageRules", format!("`set` entry {i}: {e}"))),
                },
                (None, Some(p)) => parse_rule(p, &theme)?,
                (None, None) => return Err(bad("home.manageRules", format!("`set` entry {i} has no `rule` or `params`"))),
            };
            let stop_if_true = bool_param(x, "stopIfTrue").unwrap_or(false);
            rules.push(CondFormat { ranges, rule, priority: i as u32 + 1, stop_if_true });
        }
        return edit(s, |cx| {
            cx.sheet_mut(sheet)?.cond_formats = rules;
            Ok(Json::Null)
        });
    }
    if ["update", "duplicate", "delete", "moveUp", "moveDown"].iter().all(|k| p.get(*k).is_none()) {
        let d = s.doc()?;
        // Current Selection lists the rules that touch the selection (`selection`, else the
        // active sheet's); the default is the whole sheet.
        let only = match (str_param(p, "show"), str_param(p, "selection")) {
            (Some("selection"), Some(t)) => Some(parse_applies_to(t, &d.wb, sheet)?),
            (Some("selection"), None) if sheet == d.wb.active_sheet => Some(d.selection.ranges.clone()),
            _ => None,
        };
        let list: Vec<Json> =
            d.wb.sheet(sheet)
                .map(|sh| {
                    sh.cond_formats
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| only.as_ref().is_none_or(|sel| c.ranges.iter().any(|r| sel.iter().any(|x| x.intersects(r)))))
                        .map(|(i, c)| {
                            json!({
                                "index": i,
                                "ranges": c.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>(),
                                "appliesTo": applies_to(&c.ranges),
                                "stopIfTrue": c.stop_if_true,
                                "params": rule_params(&c.rule, &d.wb.theme),
                                "rule": c.rule,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
        let total = d.wb.sheet(sheet).map_or(0, |sh| sh.cond_formats.len());
        return Ok(json!({"rules": list, "total": total}));
    }
    let update = u32_param(p, "update").map(|v| v as usize);
    let rule = match (update, p.get("rule")) {
        (Some(_), Some(r)) => Some(parse_rule(r, &s.doc()?.wb.theme)?),
        _ => None,
    };
    let ranges = match (update, str_param(p, "appliesTo")) {
        (Some(_), Some(t)) => Some(parse_applies_to(t, &s.doc()?.wb, sheet)?),
        _ => None,
    };
    let stop = bool_param(p, "stopIfTrue");
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(cf) = update.and_then(|i| sh.cond_formats.get_mut(i)) {
            if let Some(r) = rule {
                cf.rule = r;
            }
            if let Some(r) = ranges {
                cf.ranges = r;
            }
            if let Some(b) = stop {
                cf.stop_if_true = b;
            }
        }
        // The copy goes right below the rule it copies.
        if let Some(i) = u32_param(p, "duplicate").map(|v| v as usize)
            && let Some(cf) = sh.cond_formats.get(i).cloned()
        {
            sh.cond_formats.insert(i + 1, cf);
        }
        // The count is taken again after each step: a delete shortens the list.
        if let Some(i) = u32_param(p, "delete").map(|v| v as usize)
            && i < sh.cond_formats.len()
        {
            sh.cond_formats.remove(i);
        }
        if let Some(i) = u32_param(p, "moveUp").map(|v| v as usize)
            && i > 0
            && i < sh.cond_formats.len()
        {
            sh.cond_formats.swap(i, i - 1);
        }
        if let Some(i) = u32_param(p, "moveDown").map(|v| v as usize)
            && i + 1 < sh.cond_formats.len()
        {
            sh.cond_formats.swap(i, i + 1);
        }
        renumber_cf(&mut sh.cond_formats);
        Ok(Json::Null)
    })
}

fn insert_icon(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").unwrap_or("check").to_string();
    let size = f64_param(p, "size").unwrap_or(48.0).clamp(8.0, 1000.0) as f32;
    let d = s.doc()?;
    let id = d.wb.next_object_id();
    let at = cell_param(s, p, "at")?.unwrap_or(d.selection.active);
    let color = super::format::color_param(p.get("color")).unwrap_or(Color::Theme(1, 0));
    let shape = Shape {
        id,
        kind: ShapeKind::Icon,
        anchor: Anchor { cell: at, dx: 4.0, dy: 4.0, width: size, height: size, mode: anchor_mode_param(p).unwrap_or_default() },
        fill: color,
        line: Color::Auto,
        text: name,
        format: Default::default(),
    };
    let sheet = d.wb.active_sheet;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manage_rules_delete_then_move() {
        let mut s = Session::new();
        s.new_workbook();
        for v in ["1", "2"] {
            let rule = json!({"type": "cellIs", "operator": "greaterThan", "value": v, "preset": "lightRedFill"});
            s.execute("home.conditionalFormat", json!({"range": "A1:A3", "rule": rule})).unwrap();
        }
        // Deleting first left two rules' worth of indices for the move: it panicked.
        s.execute("home.manageRules", json!({"delete": 0, "moveUp": 1})).unwrap();
        s.execute("home.manageRules", json!({"delete": 0, "moveDown": 0})).unwrap();
        let rules = s.execute("home.manageRules", json!({})).unwrap();
        assert_eq!(rules["rules"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn table_without_header_keeps_outside_references() {
        // #1030: the data moved down for the new header row, but formulas outside kept the old addresses.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
        s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": "10"})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "20"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A1:A2)"})).unwrap();
        s.execute("cell.set", json!({"cell": "C1", "input": "=A2"})).unwrap();
        s.execute("cell.set", json!({"sheet": "Two", "cell": "A1", "input": "=Sheet1!A2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": false})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], "Column1");
        assert_eq!(get(&mut s, "A2")["value"], json!(10.0));
        assert_eq!(get(&mut s, "B1")["formula"], "=SUM(A2:A3)");
        assert_eq!(get(&mut s, "B1")["value"], json!(30.0));
        assert_eq!(get(&mut s, "C1")["formula"], "=A3");
        assert_eq!(get(&mut s, "C1")["value"], json!(20.0));
        let other = s.execute("cell.get", json!({"sheet": "Two", "cell": "A1"})).unwrap();
        assert_eq!(other["formula"], "=Sheet1!A3");
        assert_eq!(other["value"], json!(20.0));
    }

    #[test]
    fn table_names_refuse_defined_names_and_r1c1() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1:A2", "values": [["h"], ["1"]]})).unwrap();
        s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=0.2"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2"})).unwrap();
        let e = s.execute("table.rename", json!({"table": "Table1", "name": "Rate"})).unwrap_err();
        assert!(e.to_string().contains("already exists"), "{e}");
        let e = s.execute("table.rename", json!({"table": "Table1", "name": "rate"})).unwrap_err();
        assert!(e.to_string().contains("already exists"), "{e}");
        for n in ["R2C3", "c", "r", "RC", "C5"] {
            let e = s.execute("table.rename", json!({"table": "Table1", "name": n})).unwrap_err();
            assert!(e.to_string().contains("not valid"), "{n}: {e}");
            let e = s.execute("insert.table", json!({"range": "C1:C2", "name": n})).unwrap_err();
            assert!(e.to_string().contains("not valid"), "{n}: {e}");
        }
        s.execute("table.rename", json!({"table": "Table1", "name": "Table1"})).unwrap();
        s.execute("table.rename", json!({"table": "Table1", "name": "Rates"})).unwrap();
    }

    #[test]
    fn manage_rules_lists_params_that_recreate_the_rule() {
        let mut s = Session::new();
        s.new_workbook();
        let rules = [
            json!({"type": "cellIs", "operator": "between", "value": "1", "value2": "say \"hi\"", "fill": "#FFEB9C", "bold": true}),
            json!({"type": "expression", "formula": "=MOD(ROW(),2)=0", "preset": "redBorder"}),
            json!({"type": "aboveAverage", "below": true, "equal": true, "stdDev": 1}),
            json!({"type": "colorScale", "colors": ["#F8696B", "#63BE7B"], "min": {"type": "num", "value": 5}}),
            json!({"type": "dataBar", "showValue": false, "max": {"type": "formula", "value": "=$Z$1"}}),
        ];
        for r in &rules {
            s.execute("home.conditionalFormat", json!({"range": "A1:A9", "rule": r})).unwrap();
        }
        let listed = s.execute("home.manageRules", json!({})).unwrap()["rules"].as_array().cloned().unwrap();
        let before: Vec<Json> = listed.iter().map(|r| r["rule"].clone()).collect();
        // Each rule's params replace it with itself.
        for (i, r) in listed.iter().enumerate() {
            s.execute("home.manageRules", json!({"update": i, "rule": r["params"]})).unwrap();
        }
        let after = s.execute("home.manageRules", json!({})).unwrap();
        let after: Vec<Json> = after["rules"].as_array().unwrap().iter().map(|r| r["rule"].clone()).collect();
        assert_eq!(before, after);
        assert_eq!(listed[0]["appliesTo"], "=$A$1:$A$9");
        assert_eq!(listed[4]["params"]["value2"], "say \"hi\"");
        assert_eq!(listed[4]["params"]["fill"], "#FFEB9C");
    }

    #[test]
    fn listed_cell_values_read_back_as_the_same_rule() {
        let mut s = Session::new();
        s.new_workbook();
        // Text that looks like a number or a formula, and a formula that starts and ends with a quote.
        for value in [json!("\"5\""), json!("\"=x\""), json!("=\"a\"&A1&\"b\""), json!("plain text"), json!(7)] {
            s.execute("home.conditionalFormat", json!({"range": "A1:A3", "rule": {"type": "cellIs", "operator": "equal", "value": value}})).unwrap();
            let listed = s.execute("home.manageRules", json!({})).unwrap()["rules"][0].clone();
            s.execute("home.manageRules", json!({"update": 0, "rule": listed["params"]})).unwrap();
            let again = s.execute("home.manageRules", json!({})).unwrap()["rules"][0]["rule"].clone();
            assert_eq!(again, listed["rule"], "{value}");
        }
        assert_eq!(s.execute("home.manageRules", json!({})).unwrap()["total"], 5);
    }

    #[test]
    fn edit_and_restore_keep_format_details_the_plain_overrides_cant_show() {
        let mut s = Session::new();
        s.new_workbook();
        // A bottom-only medium border, a pattern fill and a double underline, as a file can hold.
        let style = json!({
            "borders": {"bottom": {"style": "Medium", "color": {"Rgb": 0x0000FF}}},
            "fill": {"pattern": "Gray125", "fg": {"Rgb": 0xFF0000}, "bg": "Auto"},
            "font": {"underline": "Double"},
        });
        s.execute("home.conditionalFormat", json!({"range": "A1:A9", "rule": {"type": "blanks", "style": style}})).unwrap();
        let listed = s.execute("home.manageRules", json!({})).unwrap()["rules"][0].clone();
        let original = listed["rule"].clone();
        // Edit Rule… with the format untouched.
        s.execute("home.manageRules", json!({"update": 0, "rule": listed["params"]})).unwrap();
        let after = s.execute("home.manageRules", json!({})).unwrap()["rules"][0]["rule"].clone();
        assert_eq!(after, original);
        // The same colours written another way (lower case) are no change either.
        let mut lower = listed["params"].clone();
        for k in ["fill", "fontColor", "border"] {
            if let Some(c) = lower[k].as_str().map(str::to_ascii_lowercase) {
                lower[k] = json!(c);
            }
        }
        s.execute("home.manageRules", json!({"update": 0, "rule": lower})).unwrap();
        let after = s.execute("home.manageRules", json!({})).unwrap()["rules"][0]["rule"].clone();
        assert_eq!(after, original);
        // Edit Rule… changing only bold keeps the rest.
        let mut bold = listed["params"].clone();
        bold["bold"] = json!(true);
        s.execute("home.manageRules", json!({"update": 0, "rule": bold})).unwrap();
        let after = s.execute("home.manageRules", json!({})).unwrap()["rules"][0]["rule"].clone();
        assert_eq!(after["Blanks"]["style"]["font"]["bold"], true);
        assert_eq!(after["Blanks"]["style"]["font"]["underline"], "Double");
        assert_eq!(after["Blanks"]["style"]["fill"], original["Blanks"]["style"]["fill"]);
        assert_eq!(after["Blanks"]["style"]["borders"], original["Blanks"]["style"]["borders"]);
        // Cancel puts back the listed rule exactly.
        let saved = json!([{"rule": original, "appliesTo": listed["appliesTo"], "stopIfTrue": false}]);
        s.execute("home.manageRules", json!({"set": saved})).unwrap();
        let back = s.execute("home.manageRules", json!({})).unwrap()["rules"][0]["rule"].clone();
        assert_eq!(back, original);
        // A serialized rule whose formula doesn't parse is refused like a new one (#1999).
        let bad_rule = json!({"Expression": {"formula": "A1>", "style": Style::default()}});
        assert!(s.execute("home.manageRules", json!({"set": [{"rule": bad_rule, "appliesTo": "=$A$1"}]})).is_err());
        let icons = json!({"type": "iconSet", "set": "3Arrows", "thresholds": [{"Formula": "=1+"}]});
        assert!(s.execute("home.conditionalFormat", json!({"range": "B1:B3", "rule": icons})).is_err());
        // A malformed serialized rule is refused, not read as a default rule.
        assert!(s.execute("home.manageRules", json!({"set": [{"rule": {"Blanks": {}}, "appliesTo": "=$A$1"}]})).is_err());
        // Icon thresholds: one per icon or one fewer, nothing else.
        let three = |n: usize| json!({"type": "iconSet", "set": "3Arrows", "thresholds": vec![json!({"Percent": 50.0}); n]});
        assert!(s.execute("home.conditionalFormat", json!({"range": "C1:C3", "rule": three(2)})).is_ok());
        assert!(s.execute("home.conditionalFormat", json!({"range": "C1:C3", "rule": three(3)})).is_ok());
        assert!(s.execute("home.conditionalFormat", json!({"range": "C1:C3", "rule": three(5)})).is_err());
        // An entry with neither `rule` nor `params` is refused.
        assert!(s.execute("home.manageRules", json!({"set": [{"appliesTo": "=$A$1"}]})).is_err());
    }

    #[test]
    fn manage_rules_work_on_a_sheet_that_is_not_active() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
        s.execute("sheet.activate", json!({"sheet": 1})).unwrap();
        // The manager opened on Sheet1 with A1:A3 selected; Two is active now.
        let rule = json!({"type": "blanks", "fill": "#FFC7CE"});
        s.execute("home.conditionalFormat", json!({"sheet": 0, "range": "A1:A3", "rule": rule})).unwrap();
        s.execute("home.conditionalFormat", json!({"sheet": 0, "range": "D1:D3", "rule": rule})).unwrap();
        let listed = s.execute("home.manageRules", json!({"sheet": 0, "show": "selection", "selection": "A1:A3"})).unwrap();
        let ranges: Vec<Json> = listed["rules"].as_array().unwrap().iter().map(|r| r["appliesTo"].clone()).collect();
        assert_eq!(ranges, vec![json!("=$A$1:$A$3")]);
        let two = s.execute("home.manageRules", json!({"sheet": 1})).unwrap();
        assert_eq!(two["rules"].as_array().map(Vec::len), Some(0));
        assert!(s.execute("home.manageRules", json!({"sheet": 0, "show": "selection", "selection": "nope"})).is_err());
        // Applies to may name the rule's own sheet, never another one.
        s.execute("home.manageRules", json!({"sheet": 0, "update": 0, "appliesTo": "=Sheet1!$A$1:$A$5"})).unwrap();
        let first = s.execute("home.manageRules", json!({"sheet": 0})).unwrap();
        assert_eq!(first["rules"][0]["appliesTo"], "=$A$1:$A$5");
        assert!(s.execute("home.manageRules", json!({"sheet": 0, "update": 0, "appliesTo": "=Two!$B$2"})).is_err());
    }

    #[test]
    fn manage_rules_edits_duplicates_and_filters() {
        let mut s = Session::new();
        s.new_workbook();
        let rule = json!({"type": "blanks", "fontColor": "#FF0000"});
        s.execute("home.conditionalFormat", json!({"range": "A1:A3", "rule": rule})).unwrap();
        s.execute("home.conditionalFormat", json!({"range": "C1:C3", "rule": {"type": "errors"}, "stopIfTrue": true})).unwrap();
        s.execute("home.manageRules", json!({"update": 1, "appliesTo": "=$A$1:$A$5,$E$1", "stopIfTrue": true})).unwrap();
        s.execute("home.manageRules", json!({"duplicate": 0})).unwrap();
        let all = s.execute("home.manageRules", json!({})).unwrap();
        let all = all["rules"].as_array().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0]["params"]["type"], "errors");
        assert_eq!(all[0]["stopIfTrue"], true);
        assert_eq!(all[1]["params"]["type"], "errors");
        assert_eq!(all[2]["appliesTo"], "=$A$1:$A$5,$E$1");
        assert_eq!(all[2]["params"]["fontColor"], "#FF0000");
        // Plain overrides alone don't bring the default preset's fill.
        assert_eq!(all[2]["params"]["fill"], "none");
        s.execute("selection.set", json!({"range": "E1"})).unwrap();
        let sel = s.execute("home.manageRules", json!({"show": "selection"})).unwrap();
        let idx: Vec<u64> = sel["rules"].as_array().unwrap().iter().filter_map(|r| r["index"].as_u64()).collect();
        assert_eq!(idx, vec![2]);
        assert!(s.execute("home.manageRules", json!({"update": 0, "appliesTo": "=nope"})).is_err());
        assert!(s.execute("home.manageRules", json!({"update": 9, "stopIfTrue": true})).is_ok());
        // `set` puts back a listing taken earlier.
        let saved: Vec<Json> =
            all.iter().map(|r| json!({"params": r["params"], "appliesTo": r["appliesTo"], "stopIfTrue": r["stopIfTrue"]})).collect();
        s.execute("home.manageRules", json!({"delete": 0})).unwrap();
        s.execute("home.manageRules", json!({"set": saved})).unwrap();
        let back = s.execute("home.manageRules", json!({})).unwrap();
        assert_eq!(back["rules"].as_array().map(Vec::len), Some(3));
        assert_eq!(back["rules"][0]["stopIfTrue"], true);
        assert!(s.execute("home.manageRules", json!({"set": [{"rule": {"type": "blanks"}}]})).is_err());
    }
}
