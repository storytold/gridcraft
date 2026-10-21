use super::{CellRef, EngineError, Json, RangeRef, Result, Session, bad, edit, f64_param, json, str_param, target_range, target_sheet};

// ---------------------------------------------------------------- fill

/// Copies one edge of the selection across it (Fill Down, Up, Right, or Left).
///
/// A whole row filled down copies the row above, across the whole row. A whole column filled
/// right copies the column to the left, down the whole column. On a whole column, Ctrl+D fills
/// only the first contiguous block; cells outside that block are left as they are. Excel fills
/// the entire column.
pub(super) fn fill_dir(s: &mut Session, p: &Json, dr: i64, dc: i64) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let mut r = target_range(s, p)?;
    let full_rows = r.is_full_rows();
    let full_cols = r.is_full_cols();
    // The whole sheet is bounded on both axes. Otherwise the current region narrows only the
    // axis we fill along: rows for Fill Down/Up on a whole column, columns for Fill Right/Left
    // on a whole row. A whole-row Fill Down keeps every column, and a whole-column Fill Right
    // keeps every row — including cells outside the first contiguous block.
    let entire = full_rows && full_cols;
    let vertical = dr != 0;
    let shrink_rows = entire || (full_cols && !full_rows && vertical);
    let shrink_cols = entire || (full_rows && !full_cols && !vertical);
    if shrink_rows || shrink_cols {
        let doc = s.doc()?;
        let Some(sh) = doc.wb.sheet(sheet) else { return Ok(Json::Null) };
        let Some(used) = sh.used_range() else { return Ok(Json::Null) };
        let Some(inter) = r.intersection(&used) else { return Ok(Json::Null) };
        let Some((anchor, _)) = sh.cells.iter_range(inter).next() else { return Ok(Json::Null) };
        let region = crate::selection::current_region(sh, anchor);
        let sparse = region.intersection(&inter).unwrap_or(region);
        let (row0, row1) = if shrink_rows { (sparse.start.row, sparse.end.row) } else { (r.start.row, r.end.row) };
        let (col0, col1) = if shrink_cols { (sparse.start.col, sparse.end.col) } else { (r.start.col, r.end.col) };
        r = RangeRef::new(CellRef::new(row0, col0), CellRef::new(row1, col1));
    }
    if r.count() > crate::fill::MAX_FILL_CELLS {
        return Err(bad("edit.fill", "range too large"));
    }
    // One selected row copies the row above; one selected column copies the column to the left.
    // A sparse block is already the fill area: a single cell has nothing further to fill, and
    // pulling in the line outside the block would overwrite it.
    if dr == 1 && r.height() == 1 && !shrink_rows {
        r.start.row = r.start.row.saturating_sub(1);
    }
    if dc == 1 && r.width() == 1 && !shrink_cols {
        r.start.col = r.start.col.saturating_sub(1);
    }
    let bands = {
        let doc = s.doc()?;
        match doc.wb.sheet(sheet) {
            Some(sh) => copy_bands(sh, r, vertical),
            None => return Ok(Json::Null),
        }
    };
    edit(s, |cx| {
        for band in bands {
            crate::fill::fill(cx, sheet, source_edge(band, dr, dc), band, crate::fill::FillMode::Copy)?;
        }
        Ok(Json::Null)
    })
}

/// Above this many lanes, Copy fill visits only lanes that already store a cell.
const SPARSE_COPY_LANES: u32 = 10_000;

fn source_edge(r: RangeRef, dr: i64, dc: i64) -> RangeRef {
    if dr == 1 {
        RangeRef::new(r.start, CellRef::new(r.start.row, r.end.col))
    } else if dr == -1 {
        RangeRef::new(CellRef::new(r.end.row, r.start.col), r.end)
    } else if dc == 1 {
        RangeRef::new(r.start, CellRef::new(r.end.row, r.start.col))
    } else {
        RangeRef::new(CellRef::new(r.start.row, r.end.col), r.end)
    }
}

fn band_range(r: RangeRef, vertical: bool, a: u32, b: u32) -> RangeRef {
    if vertical {
        RangeRef::new(CellRef::new(r.start.row, a), CellRef::new(r.end.row, b))
    } else {
        RangeRef::new(CellRef::new(a, r.start.col), CellRef::new(b, r.end.col))
    }
}

/// One band per contiguous run of lanes that store a cell, or the whole range when it is small.
fn copy_bands(sh: &gridcraft_model::Sheet, r: RangeRef, vertical: bool) -> Vec<RangeRef> {
    let lanes_n = if vertical { r.width() } else { r.height() };
    if lanes_n <= SPARSE_COPY_LANES {
        return vec![r];
    }
    let lanes: Vec<u32> = if vertical {
        let mut cols = std::collections::BTreeSet::new();
        for (c, _) in sh.cells.iter_range(r) {
            cols.insert(c.col);
        }
        cols.into_iter().collect()
    } else {
        let mut rows = Vec::new();
        let mut last = u32::MAX;
        for (c, _) in sh.cells.iter_range(r) {
            if c.row != last {
                rows.push(c.row);
                last = c.row;
            }
        }
        rows
    };
    let mut out = Vec::new();
    let Some(&first) = lanes.first() else { return out };
    let (mut start, mut prev) = (first, first);
    for &lane in lanes.iter().skip(1) {
        if lane == prev.saturating_add(1) && lane != prev {
            prev = lane;
        } else {
            out.push(band_range(r, vertical, start, prev));
            start = lane;
            prev = lane;
        }
    }
    out.push(band_range(r, vertical, start, prev));
    out
}

pub(super) fn auto_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let src = str_param(p, "source").and_then(RangeRef::parse).map_or_else(|| target_range(s, p), Ok)?;
    let Some(target) = str_param(p, "target").and_then(RangeRef::parse) else { return Err(bad("edit.autoFill", "missing `target`")) };
    let mode = match str_param(p, "mode").unwrap_or("series") {
        "copy" => crate::fill::FillMode::Copy,
        "alternate" => crate::fill::FillMode::Alternate,
        "formats" => crate::fill::FillMode::Formats,
        "values" => crate::fill::FillMode::ValuesOnly,
        "extend" => crate::fill::FillMode::Extend,
        "days" => crate::fill::FillMode::Dates(crate::fill::DateUnit::Day),
        "weekdays" => crate::fill::FillMode::Dates(crate::fill::DateUnit::Weekday),
        "months" => crate::fill::FillMode::Dates(crate::fill::DateUnit::Month),
        "years" => crate::fill::FillMode::Dates(crate::fill::DateUnit::Year),
        _ => crate::fill::FillMode::Series,
    };
    if target.count() > crate::fill::MAX_FILL_CELLS || src.count() > crate::fill::MAX_FILL_CELLS {
        return Err(bad("edit.autoFill", "range too large"));
    }
    // Excel refuses a target that doesn't hold the source in a corner. Such targets used to fill
    // every source lane across the whole target extent: billions of cells for whole rows (`2:3`)
    // into whole columns (`A:A`).
    if !crate::fill::holds_source_in_corner(src, target) {
        return Err(bad("edit.autoFill", "the target must contain the source in one of its corners"));
    }
    edit(s, |cx| {
        crate::fill::fill(cx, sheet, src, target, mode)?;
        *cx.sel = crate::Selection { active: src.start, anchor: src.start, ranges: vec![target.union(&src)] };
        Ok(Json::Null)
    })
}

pub(super) fn fill_series(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let r = target_range(s, p)?;
    let rows = str_param(p, "direction").map(|d| d == "rows").unwrap_or(r.width() > r.height());
    let kind = str_param(p, "type").unwrap_or("linear").to_string();
    let step = f64_param(p, "step").unwrap_or(1.0);
    let stop = f64_param(p, "stop");
    let unit = str_param(p, "dateUnit").unwrap_or("day").to_string();
    edit(s, |cx| {
        crate::fill::series(cx, sheet, crate::fill::SeriesOptions { range: r, rows, kind: &kind, step, stop, unit: &unit })?;
        Ok(Json::Null)
    })
}

pub(super) fn flash_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let r = target_range(s, p)?;
    let n = edit(s, |cx| crate::fill::flash_fill(cx, sheet, r.start))?;
    if n == 0 {
        return Err(EngineError::Other(
            "We looked at all the data next to your selection and didn't see a pattern for filling in values for you.".into(),
        ));
    }
    Ok(json!({"filled": n}))
}
