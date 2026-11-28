//! Data tools bind native operations to explicit targets. The ribbon may otherwise infer a
//! table, column or selection from the cursor, even when a range was supplied.

use super::limited_range;
use gridcraft_engine::DocState;
use gridcraft_engine::core::{CellRef, RangeRef, col_to_letters, letters_to_col};
use gridcraft_engine::model::Sheet;
use serde_json::{Value as Json, json};

// These tools translate user-facing arguments rather than exposing the native command shape.
// A saved native script cannot be projected back into these arguments without reinterpreting it.
pub(super) fn translates(name: &str) -> bool {
    matches!(name, "size_rows" | "size_columns" | "data_bars" | "filter_column" | "define_range")
}

fn column(r: RangeRef, value: &Json) -> Result<u32, String> {
    value
        .as_str()
        .and_then(letters_to_col)
        .filter(|n| (r.start.col..=r.end.col).contains(n))
        .ok_or_else(|| "Column must be worksheet letters inside the specified rectangle.".into())
}

pub(super) fn prepare(d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    let range = |p: &Json| limited_range(p["range"].as_str().ok_or("Missing A1 range")?);
    let active = d.wb.active().is_some_and(|a| a.uid.get() == sh.uid.get());
    match name {
        "fill_down" if range(p)?.height() < 2 => return Err("Include the source row and at least one destination row.".into()),
        "fill_right" if range(p)?.width() < 2 => return Err("Include the source column and at least one destination column.".into()),
        "size_rows" => {
            let r = range(p)?;
            *p = json!({"sheet":sh.name,"rows":format!("{}:{}",r.start.row+1,r.end.row+1),"points":p["points"]});
        }
        "size_columns" => {
            let r = range(p)?;
            *p = json!({"sheet":sh.name,"cols":format!("{}:{}",col_to_letters(r.start.col),col_to_letters(r.end.col)),"chars":p["chars"]});
        }
        "sort_rows" => {
            let r = range(p)?;
            if r.height() < 2 + u32::from(p["header"] == true) {
                return Err("Provide at least two data rows, in addition to any header.".into());
            }
            let mut seen = Vec::new();
            for key in p["keys"].as_array().ok_or("Missing sort keys")? {
                let col = column(r, &key["column"])?;
                if seen.contains(&col) {
                    return Err("Each sort column must be unique.".into());
                }
                seen.push(col);
            }
        }
        "enable_filter" => {
            let r = range(p)?;
            if sh.autofilter.is_some() {
                return Err("This worksheet already has an AutoFilter. Use filter_column with its range from workbook_info.".into());
            }
            if r.height() < 2 {
                return Err("A filter needs a header and at least one data row.".into());
            }
            if sh.tables.iter().any(|t| t.range.intersects(&r)) || active && sh.table_at(d.selection.active).is_some() {
                return Err(
                    "This tool installs a worksheet filter outside tables; both the target and the active cell must be outside a table.".into()
                );
            }
        }
        "filter_column" => {
            let r = p["range"].as_str().and_then(RangeRef::parse).ok_or("Specify the existing filter's exact A1 range.")?;
            if !active || !sh.autofilter.as_ref().is_some_and(|af| af.range == r) {
                return Err("Specify the active worksheet and its existing AutoFilter range from workbook_info.".into());
            }
            column(r, &p["column"])?;
            let clear = p["clear"] == true;
            if clear == p.get("matches").is_some() {
                return Err("Supply either a list of matches or clear=true, not both.".into());
            }
            *p = if clear { json!({"column":p["column"],"clear":true}) } else { json!({"column":p["column"],"values":p["matches"]}) };
        }
        "data_bars" => {
            let c = p["color"].as_str().unwrap_or("");
            if c.len() != 7 || !c.starts_with('#') || !c[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Specify a #RRGGBB color for the bars.".into());
            }
            *p = json!({"sheet":sh.name,"range":p["range"],"rule":{"type":"dataBar","color":c,"gradient":p["gradient"].as_bool().unwrap_or(true)}});
        }
        "rename_table" | "define_range" => {
            if d.wb.protected_structure {
                return Err("Workbook structure is protected.".into());
            }
            let n = p["name"].as_str().ok_or("Missing name")?;
            let lower = n.to_ascii_lowercase();
            if ["_gc_", "_sc_", "_xlnm."].iter().any(|p| lower.starts_with(p)) {
                return Err("Reserved workbook name.".into());
            }
            if name == "rename_table" {
                let table = p["table"].as_str().and_then(|s| d.wb.table(s));
                if !table.and_then(|(si, _)| d.wb.sheet(si)).is_some_and(|s| s.uid.get() == sh.uid.get()) {
                    return Err("No table with that name on the requested sheet.".into());
                }
            } else {
                if d.wb.table(n).is_some() || d.wb.names.iter().any(|x| x.name.eq_ignore_ascii_case(n)) {
                    return Err("That name is already in use.".into());
                }
                let r = range(p)?;
                let abs = |c: CellRef| format!("${}${}", col_to_letters(c.col), c.row + 1);
                let body = if r.is_single() { abs(r.start) } else { format!("{}:{}", abs(r.start), abs(r.end)) };
                *p = json!({"name":n,"scope":"Workbook","refersTo":format!("={}!{}",gridcraft_engine::formula::quote_sheet(&sh.name),body)});
            }
        }
        _ => {}
    }
    Ok(())
}
