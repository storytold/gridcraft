//! Insert tab: tables, charts, sparklines, pictures, shapes, hyperlinks, comments and notes,
//! conditional formatting (Home › Styles) and symbols.

use gridcraft_core::{CellRef, MAX_ROWS, RangeRef};
use gridcraft_formula::adjust::{Axis, Edit};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::sheet::{rewrite, shift_cell_features};
use super::*;
use crate::selection::current_region;

mod chart;
mod cond_format;
mod links;
mod objects;
mod table;

pub use chart::{build_chart, chart_kind, recommend_charts, scatter_x_from_first, series_from_range};
use chart::{chart_delete, chart_set, chart_switch, insert_chart};
use cond_format::{add_cf, clear_cf, insert_icon, manage_cf};
use links::{comment, delete_comment, insert_link, open_link, reply_comment, resolve_comment, show_note};
use objects::{delete_object, insert_picture, insert_shape, insert_sparkline, move_object, set_anchor_mode, shape_set_text};
use table::{format_as_table, insert_table, table_convert, table_flag, table_rename, table_style, table_total_fn};

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
            "{chart?: id, type?, subtype?, title?, legend?: none|bottom|top|left|right, dataLabels?, gridlines?, quartiles?: exclusive|inclusive, style?, xTitle?, yTitle?, at?, width?, height?, dx?, dy?}",
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
            "{range?, rule: {type: cellIs|expression|containsText|beginsWith|endsWith|blanks|noBlanks|errors|noErrors|duplicate|unique|top10|aboveAverage|timePeriod|colorScale|dataBar|iconSet, operator?, value?, value2?, formula?, text?, rank?, bottom?, percent?, below?, period?, colors?: [..], color?, set?, style?: {font?: {bold?, italic?, color?}, fill?: \"#FFC7CE\" | {pattern?, fg?, bg?}, borders?, num_fmt?} (colours \"#RRGGBB\") | preset?: lightRedFill|redText|yellowFill|greenFill|redBorder|lightRedFillDarkRedText…}; a text `value` is matched as text, or kept when already a quoted literal}",
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

fn insert_symbol(s: &mut Session, p: &Json) -> Result<Json> {
    let ch = str_param(p, "char").unwrap_or("").to_string();
    let d = s.doc()?;
    let at = d.selection.active;
    let cur = d.wb.active().and_then(|sh| sh.cell(at)).map(|c| c.input_text()).unwrap_or_default();
    s.execute("cell.set", json!({"cell": at.a1(), "input": format!("{cur}{ch}")}))
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
}
