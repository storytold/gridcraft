use super::{CellRef, EngineError, Json, RangeRef, Result, Session, TableColumn, TotalsFn, Value, bad, edit, json, ok, str_param, target_range};

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

pub(super) fn from_csv(s: &mut Session, p: &Json) -> Result<Json> {
    let bytes = if let Some(t) = str_param(p, "text") {
        t.as_bytes().to_vec()
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path)?
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let text = String::from_utf8_lossy(&bytes);
    // JSON array of objects → rows.
    let rows: Vec<Vec<Json>> =
        if text.trim_start().starts_with('[') {
            let arr: Vec<Json> = serde_json::from_str(&text).map_err(|e| bad("data.getData", e.to_string()))?;
            let mut keys: Vec<String> = Vec::new();
            for o in &arr {
                if let Some(m) = o.as_object() {
                    for k in m.keys() {
                        if !keys.contains(k) {
                            keys.push(k.clone());
                        }
                    }
                }
            }
            let mut out = vec![keys.iter().map(|k| json!(k)).collect::<Vec<_>>()];
            for o in &arr {
                out.push(keys.iter().map(|k| o.get(k).cloned().unwrap_or(Json::Null)).collect());
            }
            out
        } else {
            let delim = str_param(p, "delimiter")
                .and_then(|d| d.chars().next())
                .unwrap_or(if text.lines().next().is_some_and(|l| l.contains('\t')) { '\t' } else { ',' });
            let opts = gridcraft_xlsx::CsvOptions { delimiter: delim as u8, ..Default::default() };
            let wb = gridcraft_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
            let Some(u) = sh.used_range() else { return Err(EngineError::Other("The file is empty.".into())) };
            (u.start.row..=u.end.row)
                .map(|r| {
                    (u.start.col..=u.end.col)
                        .map(|c| match sh.value(CellRef::new(r, c)) {
                            Value::Number(n) => json!(n),
                            Value::Bool(b) => json!(b),
                            Value::Empty => Json::Null,
                            v => json!(v.display()),
                        })
                        .collect()
                })
                .collect()
        };
    if rows.is_empty() || rows.len() > 1_048_576 {
        return Err(EngineError::Other("Nothing to import.".into()));
    }
    let name = str_param(p, "sheet").map(str::to_string).or_else(|| {
        str_param(p, "path").and_then(|x| std::path::Path::new(x).file_stem().and_then(std::ffi::OsStr::to_str).map(|f| f.chars().take(31).collect()))
    });
    let r = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    let w = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    s.execute("range.setValues", json!({"range": "A1", "values": rows}))?;
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(rows.len() as u32 - 1, w as u32 - 1));
    let _ = s.execute("insert.table", json!({"range": range.a1(), "header": true}));
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, w as u32 - 1).a1()}));
    Ok(json!({"sheet": r["name"], "rows": rows.len(), "cols": w}))
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
