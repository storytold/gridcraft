use super::super::{bad, str_param};
use crate::{EngineError, Result, Session, Trust, pdf_import};
use serde_json::{Value as Json, json};
use std::sync::{Arc, atomic::AtomicBool};
const CMD: &str = "data.fromPdf";
fn fail(e: pdf_import::ImportError) -> EngineError {
    EngineError::Other(e.to_string())
}
fn page(p: &Json) -> Result<u32> {
    let n = match p.get("page") {
        None => 1,
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| bad(CMD, "page must be a positive integer"))?,
    };
    Ok(n)
}
pub(super) fn preview(_: &mut Session, p: &Json) -> Result<Json> {
    let path = str_param(p, "path").ok_or_else(|| bad("data.pdfTables", "missing path"))?;
    let bytes = pdf_import::read(std::path::Path::new(path)).map_err(fail)?;
    let result = pdf_import::inspect(bytes, page(p)?, Arc::new(AtomicBool::new(false))).map_err(fail)?;
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}
fn flag(p: &Json, key: &str, default: bool) -> Result<bool> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| bad(CMD, format!("{key} must be true or false"))),
    }
}
fn rows(p: &Json) -> Result<Vec<Vec<String>>> {
    if let Some(rows) = p.get("rows") {
        let rows = rows.as_array().ok_or_else(|| bad(CMD, "rows must be an array of text rows"))?;
        let width = rows.first().and_then(Json::as_array).map_or(0, Vec::len);
        if width == 0 || rows.is_empty() || rows.len().checked_mul(width).is_none_or(|n| n > pdf_import::MAX_CELLS) {
            return Err(bad(CMD, "empty or oversized table"));
        }
        let mut bytes = 0usize;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let row = row.as_array().filter(|row| row.len() == width).ok_or_else(|| bad(CMD, "all rows must have the same number of columns"))?;
            let mut cells = Vec::with_capacity(width);
            for value in row {
                let text = value.as_str().ok_or_else(|| bad(CMD, "preview cells must be text"))?;
                if text.encode_utf16().take(pdf_import::MAX_CELL_TEXT + 1).count() > pdf_import::MAX_CELL_TEXT {
                    return Err(bad(CMD, "cell text exceeds the XLSX limit"));
                }
                bytes = bytes.saturating_add(text.len());
                if bytes > 8 * 1024 * 1024 {
                    return Err(bad(CMD, "table text is too large"));
                }
                cells.push(text.to_owned());
            }
            out.push(cells);
        }
        return Ok(out);
    }
    let path = str_param(p, "path").ok_or_else(|| bad(CMD, "missing path or confirmed preview rows"))?;
    let bytes = pdf_import::read(std::path::Path::new(path)).map_err(fail)?;
    let preview = pdf_import::inspect(bytes, page(p)?, Arc::new(AtomicBool::new(false))).map_err(fail)?;
    let index = match p.get("table") {
        None => 0,
        Some(v) => v
            .as_u64()
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| bad(CMD, "table must be a positive integer"))?,
    };
    preview.tables.into_iter().nth(index).map(|t| t.rows).ok_or_else(|| bad(CMD, "no supported table at the selected page and table number"))
}
fn typed_rows(p: &Json, rows: Vec<Vec<String>>, headers: bool) -> Result<Vec<Vec<gridcraft_core::Value>>> {
    let width = rows.first().map_or(0, Vec::len);
    let mut numeric = vec![false; width];
    if let Some(columns) = p.get("numericColumns") {
        for col in columns.as_array().ok_or_else(|| bad(CMD, "numericColumns must be an array"))? {
            let i = col
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < width)
                .ok_or_else(|| bad(CMD, "numeric column is outside the table"))?;
            numeric[i] = true;
        }
    }
    rows.into_iter()
        .enumerate()
        .map(|(r, row)| {
            row.into_iter()
                .enumerate()
                .map(|(c, text)| {
                    if numeric[c] && !(headers && r == 0) && !text.trim().is_empty() {
                        let n =
                            text.trim().parse::<f64>().ok().filter(|n| n.is_finite()).ok_or_else(|| {
                                bad(CMD, format!("row {}, column {} is not a plain number; choose Text to preserve it", r + 1, c + 1))
                            })?;
                        Ok(gridcraft_core::Value::Number(n))
                    } else if text.is_empty() {
                        Ok(gridcraft_core::Value::Empty)
                    } else {
                        Ok(gridcraft_core::Value::text(text))
                    }
                })
                .collect()
        })
        .collect()
}
pub(super) fn load(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook structure is protected.".into()));
    }
    let as_table = flag(p, "asTable", true)?;
    let headers = flag(p, "headers", true)?;
    let rows = rows(p)?;
    // The standard table command renames blank/duplicate headings. Require an explicit
    // alternate layout instead of silently changing source data during PDF import.
    if as_table && headers {
        let mut names = std::collections::HashSet::new();
        if rows.first().is_some_and(|row| row.iter().any(|t| t.is_empty() || !names.insert(t.to_ascii_lowercase()))) {
            return Err(bad(
                CMD,
                "table headers must be nonempty and unique; disable Format as table or First row contains headers to preserve these cells",
            ));
        }
    }
    let width = rows.first().map_or(0, Vec::len);
    let height = rows.len();
    let output_rows = height.saturating_add(usize::from(as_table && !headers));
    if width > gridcraft_core::MAX_COLS as usize || output_rows > gridcraft_core::MAX_ROWS as usize {
        return Err(bad(CMD, "table does not fit in a worksheet"));
    }
    let rows = typed_rows(p, rows, headers)?;
    let wb = &s.doc()?.wb;
    let name = if let Some(name) = str_param(p, "sheet") {
        wb.check_sheet_name(name, None).map_err(EngineError::Other)?;
        name.to_owned()
    } else {
        (1..=wb.sheets.len().saturating_add(2))
            .map(|n| if n == 1 { "PDF Table".into() } else { format!("PDF Table ({n})") })
            .find(|n| wb.sheet_index(n).is_none())
            .ok_or_else(|| bad(CMD, "no available sheet name"))?
    };
    let max = wb.names.len().saturating_add(wb.sheets.iter().map(|sh| sh.tables.len()).sum::<usize>()).saturating_add(2);
    let table_name = (1..=max)
        .map(|n| format!("PDFTable{n}"))
        .find(|n| wb.table(n).is_none() && !wb.names.iter().any(|x| x.name.eq_ignore_ascii_case(n)))
        .ok_or_else(|| bad(CMD, "no available table name"))?;
    let output = s.grouped("Import PDF table", |s| {
        s.execute("home.insertSheet", json!({"name":name}))?;
        // PDF text is data, never executable formula input. The public bulk writer still
        // interprets leading '=' with parse:false, so write typed scalar cells through edit.
        super::super::edit(s, |cx| {
            let sheet = cx.wb.active_sheet();
            let plain = cx.wb.sheet(sheet).map(|sh| sh.style_id(gridcraft_core::CellRef::new(0, 0))).unwrap_or_default();
            let wrapped = cx.wb.styles.derive(plain, |style| style.align.wrap = true);
            for (r, row) in rows.iter().enumerate() {
                for (c, value) in row.iter().enumerate() {
                    if matches!(value, gridcraft_core::Value::Empty) {
                        continue;
                    }
                    let at = gridcraft_core::CellRef::new(r as u32, c as u32);
                    let style = if matches!(value,gridcraft_core::Value::Text(t) if t.contains('\n')) { wrapped } else { plain };
                    cx.sheet_mut(sheet)?.set_cell(at, gridcraft_model::Cell { value: value.clone(), formula: None, style, py: None });
                    cx.touch(sheet, at);
                }
            }
            Ok(())
        })?;
        let range =
            gridcraft_core::RangeRef::new(gridcraft_core::CellRef::new(0, 0), gridcraft_core::CellRef::new(height as u32 - 1, width as u32 - 1));
        if as_table {
            s.execute("insert.table", json!({"range":range.a1(),"header":headers,"name":table_name}))?;
        }
        s.execute("home.autofitColumnWidth", json!({"cols":gridcraft_core::RangeRef::cols(0,width as u32-1).a1()}))?;
        Ok(json!({"sheet":name,"rows":output_rows,"cols":width,"table":if as_table{Some(table_name.clone())}else{None}}))
    })?;
    let d = s.doc_mut()?;
    d.trust = d.trust.receive(Trust::FromFile);
    Ok(output)
}
