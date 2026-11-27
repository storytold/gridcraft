use super::{CellRef, EngineError, Json, RangeRef, Result, Session, TableColumn, TotalsFn, Value, bad, edit, json, ok, str_param, target_range};
use gridcraft_model::{Cell, Workbook};

pub(super) fn table_resize(s: &mut Session, p: &Json) -> Result<Json> {
    let r = str_param(p, "range").and_then(RangeRef::parse).ok_or_else(|| bad("table.resize", "missing `range`"))?;
    let d = s.doc()?;
    let si = d.wb.active_sheet();
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let ti = match str_param(p, "table") {
        Some(n) => sh.tables.iter().position(|t| t.name.eq_ignore_ascii_case(n)),
        None => sh.tables.iter().position(|t| t.range.contains(d.selection.active)),
    }
    .ok_or_else(|| bad("table.resize", "select a cell in a table"))?;
    edit(s, |cx| {
        let wb = &cx.wb;
        let sheet = wb.sheet(si).ok_or(EngineError::NoDocument)?;
        let t = sheet.tables.get(ti).ok_or_else(|| bad("table", "gone"))?;
        if r.start.row != t.range.start.row || !r.intersects(&t.range) {
            return Err(EngineError::Other(
                "The headers must remain in the same row, and the resulting table range must overlap the original table range.".into(),
            ));
        }
        // Like Excel, the new range can't take in another table, a merged area, a PivotTable or a
        // multi-cell array formula (a spill is fine: it shows #SPILL! inside the table).
        if sheet.tables.iter().enumerate().any(|(i, o)| i != ti && o.range.intersects(&r)) {
            return Err(EngineError::Other("A table can't overlap another table.".into()));
        }
        if sheet.merges.iter().any(|m| m.intersects(&r)) {
            return Err(EngineError::Other("A table can't overlap merged cells.".into()));
        }
        if sheet.pivots.iter().any(|pt| pt.last_range.unwrap_or_else(|| RangeRef::new(pt.anchor, pt.anchor)).intersects(&r)) {
            return Err(EngineError::Other("A table can't overlap a PivotTable report.".into()));
        }
        if sheet.cells.iter().any(|(_, x)| x.formula.as_ref().and_then(|f| f.array).is_some_and(|a| !a.is_single() && a.intersects(&r))) {
            return Err(EngineError::Other("Multi-cell array formulas aren't allowed in tables.".into()));
        }
        // Header text per new column (`None`: empty). Without a header row, a column that stays
        // keeps its name.
        let texts: Vec<Option<String>> = (r.start.col..=r.end.col)
            .map(|c| {
                if !t.header_row {
                    return c.checked_sub(t.range.start.col).and_then(|i| t.columns.get(i as usize)).map(|x| x.name.clone());
                }
                let at = CellRef::new(r.start.row, c);
                let v = sheet.value(at);
                let text = match v.as_text() {
                    Some(x) => x.to_string(),
                    None => crate::display::value_text(wb, sheet, at),
                };
                (!text.trim().is_empty()).then_some(text)
            })
            .collect();
        let names = resized_column_names(&texts);
        let old = t.range;
        let old_cols = t.columns.clone();
        let header_row = t.header_row;
        let mut cols = Vec::with_capacity(names.len());
        for (c, name) in (r.start.col..=r.end.col).zip(&names) {
            // A column that stays keeps its settings (totals, calculated formula): with a header
            // row it's found by name, without one by position.
            let existing = if header_row {
                // A header that was normalized into a generated name keeps the settings of the
                // column at its address, not of whichever column already has that name.
                let raw = texts.get((c - r.start.col) as usize).and_then(|x| x.as_deref());
                if c >= old.start.col && c <= old.end.col && raw != Some(name.as_str()) {
                    old_cols.get((c - old.start.col) as usize)
                } else {
                    old_cols.iter().find(|x| x.name.eq_ignore_ascii_case(name))
                }
            } else if c >= old.start.col && c <= old.end.col {
                old_cols.get((c - old.start.col) as usize)
            } else {
                None
            };
            cols.push(match existing {
                Some(x) => TableColumn { name: name.clone(), ..x.clone() },
                None => TableColumn { name: name.clone(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None },
            });
        }
        let sheet = cx.sheet_mut(si)?;
        if header_row {
            // Headers hold constant text: a generated or de-duplicated name is written into its
            // cell, so the header row and the column names never disagree.
            for (c, col) in (r.start.col..=r.end.col).zip(&cols) {
                let at = CellRef::new(r.start.row, c);
                let constant_text = sheet.value(at).as_text() == Some(col.name.as_str()) && sheet.cell(at).is_some_and(|x| x.formula.is_none());
                if !constant_text {
                    let style = sheet.style_id(at);
                    sheet.set_cell(at, Cell { value: Value::text(col.name.as_str()), style, ..Default::default() });
                }
            }
        }
        let t = sheet.tables.get_mut(ti).ok_or_else(|| bad("table", "gone"))?;
        t.columns = cols;
        t.range = r;
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Unique column names for a resized table, from each column's header text (`None` = empty), the
/// way Excel names them: a repeated header gets the lowest free suffix (`Qty`, `Qty2`, `Qty3`,
/// compared without case), and an empty header continues a `ColumnN` neighbour on its left
/// (`Column3` → `Column4`, or `Column32` when `Column4` is taken) or else takes the lowest free
/// `ColumnN`.
fn resized_column_names(texts: &[Option<String>]) -> Vec<String> {
    use std::collections::{HashMap, HashSet};
    let mut taken: HashSet<String> = HashSet::new();
    let mut next_suffix: HashMap<String, u64> = HashMap::new();
    // The lowest `ColumnN` not yet tried, so many empty headers stay linear.
    let mut lowest = 1u64;
    let mut names: Vec<String> = Vec::with_capacity(texts.len());
    let dedupe = |base: &str, taken: &HashSet<String>, next_suffix: &mut HashMap<String, u64>| {
        let k = next_suffix.entry(base.to_lowercase()).or_insert(2);
        let mut n = base.to_string();
        while taken.contains(&n.to_lowercase()) {
            n = format!("{base}{k}");
            *k = k.saturating_add(1);
        }
        n
    };
    for text in texts {
        let name = match text {
            Some(t) => dedupe(t, &taken, &mut next_suffix),
            None => {
                let left = names.last().and_then(|l| {
                    l.strip_prefix("Column")
                        .filter(|d| !d.starts_with('0') && !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|d| d.parse::<u64>().ok())
                });
                match left {
                    Some(n) => {
                        let cand = format!("Column{}", n.saturating_add(1));
                        if taken.contains(&cand.to_lowercase()) { dedupe(&format!("Column{n}"), &taken, &mut next_suffix) } else { cand }
                    }
                    None => {
                        while taken.contains(&format!("column{lowest}")) {
                            lowest = lowest.saturating_add(1);
                        }
                        format!("Column{lowest}")
                    }
                }
            }
        };
        taken.insert(name.to_lowercase());
        names.push(name);
    }
    names
}

/// Most cells (rows × columns, blanks included) Get Data imports. The rows are built densely
/// before they are written, so a few sparse records or one far-away CSV cell must not expand
/// into a rectangle of billions of slots.
const MAX_IMPORT_CELLS: u64 = 10_000_000;

fn too_many_cells() -> EngineError {
    EngineError::Other(format!("The data is too large to import ({MAX_IMPORT_CELLS} cells at most)."))
}

pub(super) fn from_csv(s: &mut Session, p: &Json) -> Result<Json> {
    let bytes = if let Some(t) = str_param(p, "text") {
        t.as_bytes().to_vec()
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path)?
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let at = paste_target(s, p)?;
    let delimiter = delimiter_param(p)?;
    let text = String::from_utf8_lossy(&bytes);
    // A JSON array of objects, unless a separator says the text is delimited: a copied range can
    // start with a field like `[Draft]`.
    if delimiter.is_none() && text.trim_start().starts_with('[') {
        let rows = json_rows(&text)?;
        return write_json_rows(s, p, at, rows);
    }
    // With no separator given (0) the reader picks comma, semicolon or tab by the text. Dates are
    // read as serials of the workbook they go into.
    let opts = gridcraft_xlsx::CsvOptions {
        delimiter: delimiter.unwrap_or(0),
        date_system: s.doc()?.wb.date_system,
        max_used_cells: Some(MAX_IMPORT_CELLS),
        ..Default::default()
    };
    // The cell limit is enforced while parsing, before an oversized file is stored.
    let (wb, report) = gridcraft_xlsx::read_csv_with_report(&bytes, &opts).map_err(|e| match e {
        gridcraft_xlsx::IoError::TooLarge(_) => too_many_cells(),
        e => EngineError::Other(e.to_string()),
    })?;
    let Some(u) = wb.sheet(0).and_then(|sh| sh.used_range()) else { return Err(EngineError::Other("The file is empty.".into())) };
    // Pasted at a cell, the text covers every field it has, blank ones at either end included, so
    // they clear what is under them; imported into a sheet of its own, it is its cells with data.
    let src = match at {
        Some(_) => {
            let end = CellRef::new(report.records.saturating_sub(1).max(u.end.row), report.fields.saturating_sub(1).max(u.end.col));
            RangeRef::new(CellRef::new(0, 0), end)
        }
        None => u,
    };
    if src.count() > MAX_IMPORT_CELLS {
        return Err(too_many_cells());
    }
    if let Some((sheet, cell)) = at {
        let range = write_parsed(s, sheet, cell, &wb, src)?;
        return pasted(s, sheet, range);
    }
    let name = new_sheet(s, p)?;
    let sheet = s.doc()?.wb.active_sheet();
    write_parsed(s, sheet, CellRef::new(0, 0), &wb, src)?;
    Ok(as_table(s, name, src.height(), src.width()))
}

/// The `delimiter` parameter as the byte the CSV reader splits at: one character other than a
/// quote or line break. The reader reads the byte back as a character, so U+0001–U+00FF all work.
/// `None` without one.
fn delimiter_param(p: &Json) -> Result<Option<u8>> {
    let Some(d) = str_param(p, "delimiter") else { return Ok(None) };
    let mut chars = d.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !matches!(c, '\0' | '"' | '\r' | '\n') => match u8::try_from(u32::from(c)) {
            Ok(byte) => Ok(Some(byte)),
            Err(_) => Err(EngineError::Other(format!("The separator can't be `{c}`: use a character such as , ; or |."))),
        },
        _ => Err(EngineError::Other("The separator must be one character, such as , ; or |, and not a quote or line break.".into())),
    }
}

/// Paste Data's `at`: a cell (or a range's top-left cell, as Paste takes a selection) on the sheet
/// it names (`'My Sheet'!C5`), else on the active sheet.
fn paste_target(s: &Session, p: &Json) -> Result<Option<(usize, CellRef)>> {
    let t = match p.get("at") {
        None | Some(Json::Null) => return Ok(None),
        Some(Json::String(t)) => t,
        Some(v) => return Err(bad("at", format!("invalid cell reference `{v}`"))),
    };
    let wb = &s.doc()?.wb;
    let (name, body) = crate::cmd::split_sheet(t);
    let sheet = match name {
        Some(n) => wb.sheet_index(&n).ok_or_else(|| bad("at", format!("no sheet named `{n}`")))?,
        None => wb.active_sheet(),
    };
    let cell =
        CellRef::parse(body).or_else(|| RangeRef::parse(body).map(|r| r.start)).ok_or_else(|| bad("at", format!("invalid cell reference `{t}`")))?;
    Ok(Some((sheet, cell)))
}

fn doesnt_fit() -> EngineError {
    EngineError::Other("The data doesn't fit on the sheet from that cell.".into())
}

/// A JSON array of objects as rows: a header row of every key, then one row per object.
fn json_rows(text: &str) -> Result<Vec<Vec<Json>>> {
    let arr: Vec<Json> = serde_json::from_str(text).map_err(|e| bad("data.getData", e.to_string()))?;
    let mut keys: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for o in &arr {
        if let Some(m) = o.as_object() {
            for k in m.keys() {
                if !seen.contains(k.as_str()) {
                    // Stop collecting at the column limit.
                    if keys.len() >= gridcraft_core::MAX_COLS as usize {
                        return Err(EngineError::Other("The data has more fields than a sheet has columns.".into()));
                    }
                    seen.insert(k.as_str());
                    keys.push(k.clone());
                }
            }
        }
    }
    if arr.is_empty() {
        return Err(EngineError::Other("Nothing to import.".into()));
    }
    if arr.len() as u64 + 1 > u64::from(gridcraft_core::MAX_ROWS) {
        return Err(EngineError::Other("The data has more rows than a sheet has.".into()));
    }
    if (arr.len() as u64 + 1).saturating_mul(keys.len().max(1) as u64) > MAX_IMPORT_CELLS {
        return Err(too_many_cells());
    }
    let mut out = vec![keys.iter().map(|k| json!(k)).collect::<Vec<_>>()];
    for o in &arr {
        out.push(keys.iter().map(|k| o.get(k).cloned().unwrap_or(Json::Null)).collect());
    }
    Ok(out)
}

fn write_json_rows(s: &mut Session, p: &Json, at: Option<(usize, CellRef)>, rows: Vec<Vec<Json>>) -> Result<Json> {
    let h = u32::try_from(rows.len()).map_err(|_| too_many_cells())?;
    let w = u32::try_from(rows.iter().map(Vec::len).max().unwrap_or(1).max(1)).map_err(|_| too_many_cells())?;
    if let Some((sheet, cell)) = at {
        let end = cell.offset(i64::from(h) - 1, i64::from(w) - 1).ok_or_else(doesnt_fit)?;
        s.execute("range.setValues", json!({"range": cell.a1(), "sheet": sheet, "values": rows}))?;
        return pasted(s, sheet, RangeRef::new(cell, end));
    }
    let name = new_sheet(s, p)?;
    s.execute("range.setValues", json!({"range": "A1", "values": rows}))?;
    Ok(as_table(s, name, h, w))
}

/// Writes the CSV reader's cells in `src` to `sheet` from `at`, as a paste replaces cells: each
/// value as the reader typed it, with the number format it gave (a date stays a date, text it
/// kept as text stays text), and a blank field clearing its cell. Returns the range written.
fn write_parsed(s: &mut Session, sheet: usize, at: CellRef, read: &Workbook, src: RangeRef) -> Result<RangeRef> {
    let end = at.offset(i64::from(src.height()) - 1, i64::from(src.width()) - 1).ok_or_else(doesnt_fit)?;
    let from = read.sheet(0).ok_or(EngineError::NoDocument)?;
    edit(s, |cx| {
        for c in src.iter() {
            let dest = CellRef::new(at.row + (c.row - src.start.row), at.col + (c.col - src.start.col));
            match from.cell(c).filter(|cell| cell.value != Value::Empty) {
                Some(cell) => {
                    let format = &read.styles.get(cell.style).num_fmt;
                    let mut style = cx.wb.sheet(sheet).map(|sh| sh.style_id(dest)).unwrap_or_default();
                    if format.as_str() != "General" {
                        style = cx.wb.styles.derive(style, |st| st.num_fmt = format.clone());
                    }
                    cx.sheet_mut(sheet)?.set_cell(dest, Cell { value: cell.value.clone(), formula: None, style, py: None });
                }
                None => {
                    cx.sheet_mut(sheet)?.remove_cell(dest);
                }
            }
            cx.changed.push((sheet, dest));
        }
        Ok(())
    })?;
    Ok(RangeRef::new(at, end))
}

/// After a paste at a cell: its sheet active and the pasted range selected, as Paste leaves them.
fn pasted(s: &mut Session, sheet: usize, range: RangeRef) -> Result<Json> {
    if s.doc()?.wb.active_sheet() != sheet {
        s.execute("sheet.activate", json!({"sheet": sheet}))?;
    }
    s.execute("selection.set", json!({"range": range.a1()}))?;
    let name = s.doc()?.wb.sheet(sheet).map(|sh| sh.name.clone());
    Ok(json!({"sheet": name, "range": range.a1(), "rows": range.height(), "cols": range.width()}))
}

/// A new sheet for an import, named by `sheet` or the file's name when that name is free.
fn new_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "sheet").map(str::to_string).or_else(|| {
        str_param(p, "path").and_then(|x| std::path::Path::new(x).file_stem().and_then(std::ffi::OsStr::to_str).map(|f| f.chars().take(31).collect()))
    });
    let r = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    Ok(r["name"].clone())
}

/// The imported block at A1 of the new sheet as a table with a header row, its columns fitted.
fn as_table(s: &mut Session, name: Json, rows: u32, cols: u32) -> Json {
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(rows.saturating_sub(1), cols.saturating_sub(1)));
    let _ = s.execute("insert.table", json!({"range": range.a1(), "header": true}));
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, cols.saturating_sub(1)).a1()}));
    json!({"sheet": name, "rows": rows, "cols": cols})
}

pub(super) fn from_table_range(s: &mut Session, p: &Json) -> Result<Json> {
    let r = target_range(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let r = if r.is_single() { crate::selection::current_region(sh, r.start) } else { r };
    let text = crate::display::range_text(&d.wb, d.wb.active_sheet(), r);
    from_csv(s, &json!({"text": text, "delimiter": "\t", "sheet": format!("{} (2)", sh.name).chars().take(31).collect::<String>()}))
}

pub(super) fn queries(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.sheets
            .iter()
            .flat_map(|sh| {
                sh.tables
                    .iter()
                    .map(move |t| json!({"name": t.name, "sheet": sh.name, "range": t.range.a1(), "rows": t.range.height().saturating_sub(1)}))
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn s() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }

    #[test]
    fn pasted_text_keeps_its_place_formats_and_sheet() {
        use gridcraft_core::{CellRef, Value};
        let mut s = s();
        s.execute("cell.set", json!({"cell": "B2", "input": "old"})).unwrap();
        // Another sheet is active when the paste aimed at Sheet1 runs.
        s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
        // A separator rules out JSON (`[Draft]` is a field), and the blank first field keeps the
        // rest one column right of `at`.
        let text = "\t[Draft]\t2026-01-15\n\t\t15%";
        s.execute("data.fromTextCsv", json!({"text": text, "delimiter": "\t", "at": "Sheet1!B2"})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(sh.name, "Sheet1", "the paste shows its sheet");
        let at = |a1: &str| CellRef::parse(a1).unwrap();
        assert_eq!(sh.value(at("B2")), Value::Empty, "a blank field clears what was there");
        assert_eq!(sh.value(at("C2")), Value::text("[Draft]"));
        for a1 in ["D2", "D3"] {
            assert!(matches!(sh.value(at(a1)), Value::Number(_)), "{a1}");
            assert_ne!(d.wb.styles.get(sh.style_id(at(a1))).num_fmt.as_str(), "General", "{a1} keeps the format the reader gave it");
        }
        let other = d.wb.sheet(d.wb.sheet_index("Other").unwrap()).unwrap();
        assert!(other.used_range().is_none(), "the active sheet at OK is left alone");
    }

    #[test]
    fn a_paste_clears_under_trailing_blanks_and_dates_follow_the_workbooks_date_system() {
        use gridcraft_core::{CellRef, DateSystem, Value};
        let at = |a1: &str| CellRef::parse(a1).unwrap();
        let pasted = |system: DateSystem| {
            let mut s = s();
            std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).date_system = system;
            s.execute("range.setValues", json!({"range": "B1", "values": [["x", "y"], ["z", "w"]]})).unwrap();
            // The second field and the second record are blank: they clear C1 and B2:C2.
            s.execute("data.fromTextCsv", json!({"text": "2026-01-15,\n,\n", "delimiter": ",", "at": "B1"})).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap().clone();
            for a1 in ["C1", "B2", "C2"] {
                assert_eq!(sh.value(at(a1)), Value::Empty, "{a1}");
            }
            match sh.value(at("B1")) {
                Value::Number(n) => n,
                v => panic!("{v:?}"),
            }
        };
        // The same day is 1,462 serials lower in the 1904 date system.
        assert_eq!(pasted(DateSystem::D1900) - pasted(DateSystem::D1904), 1462.0);
    }

    #[test]
    fn a_separator_is_one_character_the_reader_can_split_at() {
        let mut s = s();
        for bad in ["||", "", "\"", "€"] {
            assert!(s.execute("data.fromTextCsv", json!({"text": "a||b", "delimiter": bad})).is_err(), "{bad:?}");
        }
        // Up to U+00FF the reader splits at the character itself.
        let out = s.execute("data.fromTextCsv", json!({"text": "a§b\n1§2", "delimiter": "§"})).unwrap();
        assert_eq!(out["cols"].as_u64(), Some(2), "{out}");
    }

    #[test]
    fn sparse_imports_are_refused_before_their_rows_are_built() {
        let mut s = s();
        let sheets = s.doc().unwrap().wb.sheets.len();
        // 5,000 records with one field each: 5,001 × 5,000 slots once every record gets every field.
        let records: Vec<_> = (0..5_000).map(|i| json!({ format!("k{i}"): i })).collect();
        let text = serde_json::to_string(&records).unwrap();
        let err = s.execute("data.getData", json!({"text": text})).expect_err("too many cells");
        assert!(err.to_string().contains("too large"), "{err}");
        // A CSV whose last line reaches 16,001 columns, 1,000 lines down.
        let text = format!("1{}{}x", "\n".repeat(999), ",".repeat(16_000));
        let err = s.execute("data.fromTextCsv", json!({"text": text})).expect_err("too many cells");
        assert!(err.to_string().contains("too large"), "{err}");
        assert_eq!(s.doc().unwrap().wb.sheets.len(), sheets);
        // Small imports still work.
        let out = s.execute("data.getData", json!({"text": r#"[{"a": 1}, {"b": 2}]"#})).unwrap();
        assert_eq!((out["rows"].as_u64(), out["cols"].as_u64()), (Some(3), Some(2)), "{out}");
    }
}
