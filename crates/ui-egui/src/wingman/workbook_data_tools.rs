//! Guard each native Data command's actual scope. Native outline falls back to current
//! selection for malformed spans; Remove Duplicates would compact everything in range.
use super::super::string;
use gridcraft_engine::{
    DocState, cmd,
    core::{CellRef, RangeRef, letters_to_col},
    model::Sheet,
};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

pub(super) fn field(key: &str) -> Option<Json> {
    Some(match key {
        "keyColumns" => {
            json!({"type":"array","minItems":1,"maxItems":32,"items":string(3),"description":"Absolute column letters in the source rectangle, e.g. [\"B\",\"D\"]"})
        }
        "outlineAxis" => json!({"type":"string","enum":["rows","columns"]}),
        "outlineSpan" => {
            json!({"type":"string","maxLength":24,"description":"Whole line span, e.g. 2:8 or C:E (use 2:2 or C:C for a single row/column)."})
        }
        "formulaCell" | "changingCell" => string(20),
        "targetValue" => json!({"type":"number"}),
        _ => return None,
    })
}

pub(super) fn prepare(sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    match name {
        "group_outline" | "ungroup_outline" => {
            let rows = match p["outlineAxis"].as_str() {
                Some("rows") => true,
                Some("columns") => false,
                _ => return Err("Specify rows or columns.".into()),
            };
            let span = p["outlineSpan"].as_str().ok_or("Specify a whole-line span.")?;
            // Re-emit a native span of the selected axis; don't let a rectangle like A1:A3,
            // or the opposite axis, become a command on the ambient selection.
            let (a, b) = span.split_once(':').ok_or("Use 2:8 for rows or C:E for columns.")?;
            if ![a, b].iter().all(|s| !s.is_empty() && s.bytes().all(|c| if rows { c.is_ascii_digit() } else { c.is_ascii_alphabetic() })) {
                return Err("The span must use only the requested axis.".into());
            }
            let r = RangeRef::parse(span).ok_or("Invalid outline span.")?;
            if (if rows { r.height() } else { r.width() }) > 4096 {
                return Err("Group or ungroup up to 4,096 rows or columns per action.".into());
            }
            let span = if rows {
                format!("{}:{}", r.start.row + 1, r.end.row + 1)
            } else {
                format!("{}:{}", gridcraft_engine::core::col_to_letters(r.start.col), gridcraft_engine::core::col_to_letters(r.end.col))
            };
            *p = if rows { json!({"sheet":sh.name,"rows":span}) } else { json!({"sheet":sh.name,"cols":span}) };
        }
        "remove_duplicates" => {
            let r = p["range"].as_str().and_then(RangeRef::parse).ok_or("Supply an explicit A1 data rectangle.")?;
            if r.count() > 32_768 || r.height() < 2 || r.is_full_cols() || r.is_full_rows() {
                return Err("Supply at least two rows and at most 32,768 cells in a finite data rectangle.".into());
            }
            if sh.cells.legacy_array_intersects(r)
                || sh.merges.iter().chain(sh.spill_ranges.values()).any(|v| r.intersects(v))
                || sh.pivots.iter().filter_map(|t| t.last_range).any(|v| r.intersects(&v))
            {
                return Err("The rectangle intersects merged cells, an array, a spill, or a PivotTable report.".into());
            }
            if sh.rows.range(r.start.row..=r.end.row).any(|(_, l)| l.hidden) || sh.cols.range(r.start.col..=r.end.col).any(|(_, l)| l.hidden) {
                return Err("The rectangle contains hidden rows or columns. Unhide the records before compacting them.".into());
            }
            if sh.tables.iter().any(|t| t.range.intersects(&r)) || sh.autofilter.as_ref().is_some_and(|f| r.intersects(&f.range)) {
                return Err("Use a standalone data rectangle outside tables and filters; compaction does not resize those objects.".into());
            }
            if sh.cells.iter_range(r).any(|(_, c)| c.formula.is_some() || c.py.is_some()) {
                return Err("Remove Duplicates accepts values only. It cannot safely relocate formula or Python records.".into());
            }
            if let Some(keys) = p.as_object_mut().and_then(|o| o.remove("keyColumns")) {
                let mut cols = BTreeSet::new();
                for v in keys.as_array().filter(|a| !a.is_empty() && a.len() <= 32).ok_or("Select 1–32 key columns.")? {
                    let c = v
                        .as_str()
                        .and_then(letters_to_col)
                        .filter(|c| (r.start.col..=r.end.col).contains(c))
                        .ok_or("A key column is invalid or outside the selected range.")?;
                    if !cols.insert(c) {
                        return Err("Duplicate key column.".into());
                    }
                }
                p["columns"] = keys;
            }
        }
        _ => return Err("Unknown Data action.".into()),
    }
    Ok(())
}

pub(super) fn preview(d: &DocState, sh: &Sheet, p: &Json) -> Result<Json, String> {
    let at = |key: &str| p[key].as_str().and_then(CellRef::parse).ok_or_else(|| format!("Supply one local A1 cell for {key}."));
    let set = at("formulaCell")?;
    let changing = at("changingCell")?;
    let target = p["targetValue"].as_f64().filter(|v| v.is_finite()).ok_or("Supply a finite numeric target.")?;
    if d.wb.sheets.len() > 64 || d.wb.sheets.iter().try_fold(0_usize, |n, s| n.checked_add(s.cells.len())).is_none_or(|n| n > 8192) {
        return Err("Use the Data ribbon for Goal Seek on workbooks over 8,192 stored cells or 64 sheets.".into());
    }
    if sh.is_protected() && d.wb.styles.get(sh.style_id(set)).protection.hidden {
        return Err("The formula cell is protected and hidden.".into());
    }
    if sh.merges.iter().chain(sh.spill_ranges.values()).any(|r| r.contains(changing))
        || sh.pivots.iter().filter_map(|t| t.last_range).any(|r| r.contains(changing))
    {
        return Err("Choose an independent numeric input outside merges, spills and PivotTables.".into());
    }
    let v = sh.value(changing);
    if !(v.is_empty() || v.is_number()) {
        return Err("The changing cell must contain a numeric constant or be empty.".into());
    }
    let si = d.wb.sheet_index(&sh.name).ok_or("No such sheet.")?;
    let mut out = cmd::whatif::preview_goal_seek(d, si, set, changing, target, 32).map_err(|e| e.to_string())?;
    out["applied"] = json!(false);
    out["changingCell"] = json!(changing.a1());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;

    #[test]
    fn data_scopes_never_fall_back_to_the_ambient_selection() {
        let mut s = Session::new();
        s.new_workbook();
        let sh = s.active().unwrap().wb.active().unwrap();
        let group = |axis: &str, span: &str| {
            let mut p = json!({"outlineAxis":axis,"outlineSpan":span});
            prepare(sh, "group_outline", &mut p).map(|()| p)
        };
        assert_eq!(group("columns", "E:C").unwrap()["cols"], "C:E");
        assert!(group("rows", "C:E").is_err());
        assert!(group("rows", "A1:A3").is_err());
        assert!(group("rows", "1:999999").is_err());
        assert!(prepare(sh, "remove_duplicates", &mut json!({"range":"B2:D10","keyColumns":["A"],"header":true})).is_err());
    }

    #[test]
    fn compaction_refuses_to_relocate_formula_records() {
        let mut s = Session::new();
        s.new_workbook();
        for (a, v) in [("A2", "X"), ("A3", "X"), ("A4", "Y"), ("B4", "=A4")] {
            s.execute("cell.set", json!({"cell":a,"input":v})).unwrap();
        }
        let sh = s.active().unwrap().wb.active().unwrap();
        assert!(
            prepare(sh, "remove_duplicates", &mut json!({"range":"A2:B4","header":false,"keyColumns":["A"]})).unwrap_err().contains("values only")
        );
    }
    #[test]
    fn goal_seek_preview_never_commits_the_candidate() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
        s.execute("cell.set", json!({"cell":"A2","input":"=A1*2"})).unwrap();
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let out = preview(d, sh, &json!({"formulaCell":"A2","changingCell":"A1","targetValue":12})).unwrap();
        assert_eq!(out["converged"], true);
        assert!((out["value"].as_f64().unwrap() - 6.0).abs() < 0.001);
        assert_eq!(sh.value(CellRef::new(0, 0)).as_f64(), Some(1.0));
    }
}
