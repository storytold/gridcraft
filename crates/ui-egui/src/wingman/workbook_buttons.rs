//! Native Home/Data controls whose interactive defaults (selection, active filter, dialog)
//! are unsuitable for agent calls. All targets are verified again at commit time.
use super::{Tool, object, string};
use gridcraft_engine::{
    DocState,
    core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef},
    model::Sheet,
};
use serde_json::{Value as Json, json};
#[path = "workbook_button_details.rs"]
mod details;
pub(super) use details::read;

const fn control(name: &'static str, help: &'static str, cmd: &'static str, required: &'static [&'static str]) -> Tool {
    Tool { name, help, command: Some(cmd), required, optional: &[] }
}
const RANGE: &[&str] = &["sheet", "range"];
const LINES: &[&str] = &["sheet", "lines"];
const TABLE_FLAG: &[&str] = &["sheet", "table", "on"];

pub(super) const TOOLS: &[Tool] = &[
    control(
        "conditional_format",
        "Add a Home conditional format on an A1 range: cell comparison, text, formula, top/bottom, averages, color scales or presence/duplicates. Select rule settings for its type.",
        "home.conditionalFormat",
        &["sheet", "range", "cfRule"],
    ),
    control(
        "clear_format_rules",
        "Clear rules from this range; refuses to remove a rule area that extends beyond the requested rectangle.",
        "home.clearRules",
        RANGE,
    ),
    control(
        "insert_rows",
        "Insert blank worksheet rows before these row numbers. Existing cells and references shift down. Example lines: 5:7.",
        "home.insertRows",
        LINES,
    ),
    control(
        "delete_rows",
        "Delete the complete specified worksheet rows, including contents, and shift lower rows up. Example lines: 5:7.",
        "home.deleteRows",
        LINES,
    ),
    control(
        "insert_columns",
        "Insert blank worksheet columns before these column letters. Existing cells and references shift right. Example lines: B:D.",
        "home.insertColumns",
        LINES,
    ),
    control(
        "delete_columns",
        "Delete the complete specified worksheet columns, including contents, and shift later columns left. Example lines: B:D.",
        "home.deleteColumns",
        LINES,
    ),
    control(
        "clear_filter",
        "Clear all criteria (keep the filter header) on the active worksheet's existing AutoFilter. Use its exact range.",
        "data.clearFilter",
        RANGE,
    ),
    control(
        "reapply_filter",
        "Refresh the active worksheet's existing AutoFilter after its data changes. Use its exact range.",
        "data.reapply",
        RANGE,
    ),
    control("set_table_style", "Select a Table Design gallery style by name on a named table.", "table.style", &["sheet", "table", "tableStyle"]),
    control("table_banded_rows", "Show or hide alternate row shading on the named table.", "table.bandedRows", TABLE_FLAG),
    control("table_banded_columns", "Show or hide alternate column shading on the named table.", "table.bandedColumns", TABLE_FLAG),
    control("table_first_column", "Emphasize or reset the first table column.", "table.firstColumn", TABLE_FLAG),
    control("table_last_column", "Emphasize or reset the last table column.", "table.lastColumn", TABLE_FLAG),
    control("table_filter_buttons", "Show or hide table header filter buttons; refuses to drop active criteria.", "table.headerRow", TABLE_FLAG),
    control(
        "table_totals",
        "Show or hide the native totals row. Needs an empty row immediately below to enable; refuses to discard manually entered totals.",
        "table.totalRow",
        TABLE_FLAG,
    ),
    control(
        "table_total_function",
        "Set a totals-row calculation for a table column by its header name, not its worksheet letter.",
        "table.totalFunction",
        &["sheet", "table", "headerName", "aggregation"],
    ),
    control(
        "set_validation",
        "Set data validation. List source is either literal entries or a local A1 range in the same sheet; numeric rules accept numbers (date/time use Excel serials).",
        "data.validation",
        &["sheet", "range", "validationRule"],
    ),
    control("clear_validation", "Remove validation in this rectangle without changing existing cell values.", "data.validation", RANGE),
    Tool {
        name: "inspect_table",
        command: None,
        help: "Read named table style, header filters, total and band settings, and columns with totals functions.",
        required: &["sheet", "table"],
        optional: &["offset"],
    },
    Tool {
        name: "inspect_rules",
        command: None,
        help: "List conditional format and validation rule areas intersecting the requested rectangle and their bounded formula/value previews.",
        required: &["sheet", "range"],
        optional: &["offset"],
    },
];

pub(super) fn field(key: &str) -> Option<Json> {
    let choices = |v: &[&str]| json!({"type":"string","enum":v});
    let scalar = json!({"type":["number","string"],"maxLength":1024});
    match key {
        "lines" => Some(
            json!({"type":"string","maxLength":32,"description":"Consecutive whole rows (5 or 5:7) or whole columns (B or B:D); not a cell rectangle."}),
        ),
        "cfRule" => Some(object(
            json!({
                "type":choices(&["cellIs","expression","containsText","notContainsText","beginsWith","endsWith","blanks","noBlanks","errors","noErrors","duplicate","unique","top10","aboveAverage","timePeriod","colorScale","iconSet"]),
                "set":choices(&["3Arrows","3TrafficLights1","3Symbols","4Rating","5Arrows","5Quarters"]),"reverse":{"type":"boolean"},
                "operator":choices(&["between","notBetween","equal","notEqual","less","lessOrEqual","greater","greaterOrEqual"]),
                "value":scalar,"value2":scalar,"text":string(1024),"formula":string(8192),
                "preset":choices(&["lightRedFillDarkRedText","lightRedFill","redText","yellowFill","greenFill","redBorder"]),
                "rank":{"type":"integer","minimum":1,"maximum":1000},"percent":{"type":"boolean"},"bottom":{"type":"boolean"},"below":{"type":"boolean"},
                "period":choices(&["yesterday","today","tomorrow","last7Days","lastWeek","thisWeek","nextWeek","lastMonth","thisMonth","nextMonth"]),
                "colors":{"type":"array","minItems":2,"maxItems":3,"items":string(7)}
            }),
            &["type"],
        )),
        _ => details::field(key),
    }
}

fn conditional(rule: &Json) -> Result<(), String> {
    let kind = rule["type"].as_str().ok_or("Missing conditional rule type.")?;
    let (required, allowed): (&[&str], &[&str]) = match kind {
        "cellIs" => (&["operator", "value"], &["operator", "value", "value2", "preset"]),
        "expression" => (&["formula"], &["formula", "preset"]),
        "containsText" | "notContainsText" | "beginsWith" | "endsWith" => (&["text"], &["text", "preset"]),
        "blanks" | "noBlanks" | "errors" | "noErrors" | "duplicate" | "unique" => (&[], &["preset"]),
        "top10" => (&[], &["rank", "bottom", "percent", "preset"]),
        "aboveAverage" => (&[], &["below", "preset"]),
        "timePeriod" => (&["period"], &["period", "preset"]),
        "colorScale" => (&["colors"], &["colors"]),
        "iconSet" => (&["set"], &["set", "reverse"]),
        _ => return Err("Unknown conditional rule type.".into()),
    };
    let obj = rule.as_object().ok_or("Supply a conditional rule object.")?;
    if let Some(key) = obj.keys().find(|k| *k != "type" && !allowed.contains(&k.as_str())) {
        return Err(format!("The {kind} rule does not accept {key}."));
    }
    if let Some(key) = required.iter().find(|k| !obj.contains_key(**k)) {
        return Err(format!("The {kind} rule requires {key}."));
    }
    if kind == "cellIs" && (matches!(rule["operator"].as_str(), Some("between" | "notBetween")) != obj.contains_key("value2")) {
        return Err("Supply value2 only for between or notBetween, where both endpoints are required.".into());
    }
    if ["formula", "text", "value", "value2"]
        .iter()
        .filter_map(|k| rule[*k].as_str())
        .any(|s| s.trim().is_empty() || s.chars().count() > if kind == "expression" { 8192 } else { 1024 })
    {
        return Err("The rule text or expression is empty or too long.".into());
    }
    if rule["percent"] == true && rule["rank"].as_u64().is_some_and(|rank| rank > 100) {
        return Err("Use a percentile from 1 to 100.".into());
    }
    if kind == "colorScale"
        && !rule["colors"].as_array().is_some_and(|v| {
            (2..=3).contains(&v.len())
                && v.iter().all(|c| c.as_str().is_some_and(|s| s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())))
        })
    {
        return Err("Select two or three #RRGGBB colors in order from minimum to maximum.".into());
    }
    Ok(())
}

pub(super) fn prepare(d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    if p.get("table").is_some() {
        return details::table(d, sh, name, p);
    }
    if let Some(raw) = p["lines"].as_str() {
        let rows = matches!(name, "insert_rows" | "delete_rows");
        let normalized = if raw.contains(':') { raw.to_owned() } else { format!("{raw}:{raw}") };
        let r = RangeRef::parse(&normalized)
            .filter(|r| if rows { r.is_full_rows() && r.height() <= 1024 } else { r.is_full_cols() && r.width() <= 128 })
            .ok_or("Specify at most 1,024 consecutive whole rows or 128 consecutive whole columns, using the appropriate axis.")?;
        if sh.pivots.iter().any(|pivot| r.contains(pivot.anchor) || pivot.last_range.is_some_and(|area| area.intersects(&r))) {
            return Err("Those lines intersect a PivotTable; use the native PivotTable controls.".into());
        }
        if matches!(name, "insert_rows" | "insert_columns") {
            // Native insert already refuses to push nonblank cells or spills off the grid.
            // Notes, line formatting and object anchors need not have an entry in the cell store.
            let (edge, line_info) = if rows { (MAX_ROWS - r.height(), &sh.rows) } else { (MAX_COLS - r.width(), &sh.cols) };
            let lost = |c: CellRef| if rows { c.row >= edge } else { c.col >= edge };
            let lost_cell = sh.comments.keys().chain(sh.hyperlinks.keys()).chain(sh.cell_pictures.keys()).copied().any(lost)
                || sh.sparklines.iter().any(|sp| lost(sp.cell))
                || sh.object_anchors().any(|a| lost(a.cell));
            let lost_area = sh
                .merges
                .iter()
                .copied()
                .chain(sh.tables.iter().map(|t| t.range))
                .chain(sh.autofilter.iter().map(|f| f.range))
                .chain(sh.pivots.iter().filter_map(|p| p.last_range))
                .any(|r| lost(r.end));
            if lost_cell || lost_area || line_info.range(edge..).next().is_some() {
                return Err("Inserting these lines would push worksheet formatting, metadata or objects off the sheet.".into());
            }
        }
        *p = if rows { json!({"sheet":sh.name,"rows":r.a1()}) } else { json!({"sheet":sh.name,"cols":r.a1()}) };
        return Ok(());
    }
    let r =
        p["range"].as_str().and_then(RangeRef::parse).filter(|r| r.count() <= 100_000).ok_or("Supply one A1 rectangle of at most 100,000 cells.")?;
    match name {
        "set_validation" | "clear_validation" => {
            details::validation(name, sh, r, p)?;
        }
        "conditional_format" => {
            conditional(&p["cfRule"])?;
            // The native priority counter uses a u32 and increments every existing priority.
            if sh.cond_formats.iter().any(|cf| cf.priority == u32::MAX) {
                return Err("An existing rule priority cannot be incremented.".into());
            }
            *p = json!({"sheet":sh.name,"range":r.a1(),"rule":p["cfRule"]});
        }
        "clear_format_rules" => {
            if sh
                .cond_formats
                .iter()
                .flat_map(|rule| &rule.ranges)
                .any(|area| area.intersects(&r) && !(r.contains(area.start) && r.contains(area.end)))
            {
                return Err("A conditional format extends outside this rectangle. Select its complete area before clearing rules.".into());
            }
        }
        "clear_filter" | "reapply_filter" => {
            if d.wb.active().is_none_or(|a| a.uid != sh.uid) || !sh.autofilter.as_ref().is_some_and(|f| f.range == r) {
                return Err("Specify the active worksheet and its exact existing AutoFilter range.".into());
            }
            *p = json!({});
        }
        _ => return Err("Unknown worksheet control.".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use crate::{Services, SheetApp};
    use gridcraft_engine::Session;
    use serde_json::json;
    #[test]
    fn conditional_areas_and_validation_lists_keep_their_declared_bounds() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let range = json!({"sheet":sheet,"range":"D4:D8"});
        let mut rule = range.clone();
        rule["cfRule"] = json!({"type":"iconSet","set":"4Rating","reverse":true});
        execute(&mut app, "conditional_format", &rule, true).unwrap();
        assert!(execute(&mut app, "clear_format_rules", &json!({"sheet":sheet,"range":"D5"}), true).is_err());
        let mut list = range.clone();
        list["validationRule"] = json!({"type":"list","source":"F1:F3"});
        execute(&mut app, "set_validation", &list, true).unwrap();
        let entries = execute(&mut app, "inspect_rules", &range, false).unwrap();
        assert_eq!(entries["page"]["items"][0]["rule"]["set"], "4Rating");
        assert_eq!(entries["page"]["items"][1]["formula1"], "$F$1:$F$3");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(execute(&mut app, "inspect_rules", &range, false).unwrap()["page"]["items"].as_array().unwrap().len(), 1);
        list["validationRule"] = json!({"type":"list","source":"https://example.invalid"});
        assert!(execute(&mut app, "set_validation", &list, true).is_err());
        list["validationRule"] = json!({"type":"whole","operator":"between","value":10,"value2":9_007_199_254_740_993_u64});
        assert!(execute(&mut app, "set_validation", &list, true).unwrap_err().contains("exactly"));
        for token in ["9007199254740993.0", "9.007199254740993e15", "18446744073709551616"] {
            list["validationRule"]["value2"] = serde_json::from_str(token).unwrap();
            assert!(execute(&mut app, "set_validation", &list, true).unwrap_err().contains("exactly"));
        }
        list["validationRule"]["value2"] = json!(9_007_199_254_740_994_u64);
        execute(&mut app, "set_validation", &list, true).unwrap();
    }
    #[test]
    fn totals_do_not_overwrite_other_rows_or_custom_formulas() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let vals = json!({"sheet":sheet,"range":"A1:B2","values":[["city","pop"],["Oslo",100]]});
        execute(&mut app, "set_values", &vals, true).unwrap();
        execute(&mut app, "create_table", &json!({"sheet":sheet,"range":"A1:B2","name":"Cities","header":true}), true).unwrap();
        let mut arg = json!({"sheet":sheet,"table":"Cities","on":true});
        execute(&mut app, "set_cell", &json!({"sheet":sheet,"cell":"A3","input":"mine"}), true).unwrap();
        assert!(execute(&mut app, "table_totals", &arg, true).is_err());
        app.run("edit.undo", json!({})).unwrap();
        execute(&mut app, "table_totals", &arg, true).unwrap();
        execute(&mut app, "set_cell", &json!({"sheet":sheet,"cell":"B3","input":"=1+1"}), true).unwrap();
        arg["on"] = json!(false);
        assert!(execute(&mut app, "table_totals", &arg, true).is_err());
        assert!(
            execute(&mut app, "table_total_function", &json!({"sheet":sheet,"table":"Cities","headerName":"pop","aggregation":"max"}), true).is_err()
        );
    }
}
