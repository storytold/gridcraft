//! Data ▸ Get Data ▸ From Web: fetch a page through the host's [`crate::webquery::WebFetch`],
//! preview its tables and load one into a new sheet, with a saved connection `data.webRefresh`
//! re-fetches (Excel's Navigator, Load and Refresh).

use serde_json::{Value as Json, json};

use super::analysis::OVERWRITE_ERROR;
use super::insert::write_totals;
use super::*;
use crate::webquery::{DEFAULT_MAX_BYTES, DEFAULT_MAX_ROWS, MAX_COLS, MAX_PAGE_CELLS, MAX_TABLES, MAX_TEXT, WebRequest, WebTable};
use gridcraft_core::MAX_ROWS;
use gridcraft_model::{TableColumn, TotalsFn, Workbook};

/// Preview rows and columns per table (the Navigator's preview pane).
const PREVIEW_ROWS: usize = 10;
const PREVIEW_COLS: usize = 8;

/// The From Web commands: import (`data.fromWeb`), the Navigator preview (`data.webPreview`)
/// and the saved-connection refresh (`data.webRefresh`).
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.fromWeb",
            "From Web",
            ["Data", "Get & Transform Data"],
            None,
            "{url: https page, table?: page table name (default: the first), sheet?: new sheet name, maxRows?: 1..1048575} → imports the table into a new sheet as a table and saves the connection",
            has_doc,
            from_web
        ),
        cmd!(
            query "data.webPreview",
            "Web Tables Preview",
            [],
            None,
            "{url} → {tables: [{name, rows, cols, preview: first rows}]} (the Navigator's table detection)",
            has_doc,
            web_preview
        ),
        cmd!(
            "data.webRefresh",
            "Refresh Web Table",
            ["Data", "Queries & Connections"],
            None,
            "{url? | name?: the saved connection (default: all)} → re-fetches and rewrites the imported table(s) in place",
            has_doc,
            web_refresh
        ),
    ]
}

/// Wall-clock ms since the epoch; `0` on wasm, where reading the clock traps.
#[allow(clippy::disallowed_methods)]
fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// A page request from the command params, with the caps clamped (shared by the three commands).
fn request(p: &Json) -> Result<WebRequest> {
    const ID: &str = "data.fromWeb";
    // HTTPS only, like the fetch provider itself: a plain-http address must be refused before
    // the prefetched paths too, or a caller-supplied page bypasses the https-only contract.
    let url = str_param(p, "url").map(str::trim).filter(|u| !u.is_empty()).ok_or_else(|| bad(ID, "missing `url`"))?;
    // HTTPS only and no userinfo, like the fetch provider itself: both are refused before the
    // prefetched paths too, or a caller-supplied page bypasses the provider's contract.
    crate::webquery::validate_url(url).map_err(EngineError::Other)?;
    let max_rows = p
        .get("maxRows")
        .and_then(Json::as_u64)
        .map_or(DEFAULT_MAX_ROWS, |n| usize::try_from(n).unwrap_or(DEFAULT_MAX_ROWS).clamp(1, DEFAULT_MAX_ROWS));
    Ok(WebRequest { url: url.to_string(), max_rows, max_cols: MAX_COLS, max_bytes: DEFAULT_MAX_BYTES })
}

/// `data.fromWeb`: import the page table (Navigator choice or the first) into a new sheet
/// and save its connection; with no `url` (interactive call) it opens the From Web dialog.
fn from_web(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "url").map(|u| u.trim().is_empty()).unwrap_or(true) {
        s.ui_requests.push(crate::UiRequest::Dialog("fromWeb".into(), json!({})));
        return Ok(Json::Null);
    }
    let req = request(p)?;
    let want = str_param(p, "table").map(str::trim).filter(|t| !t.is_empty());
    let occurrence = p.get("occurrence").and_then(Json::as_u64).unwrap_or(0) as usize;
    // A background worker's download comes back through the command as `prefetched`; without
    // one, the fetch runs here.
    let tables = fetch_tables(s, &req, p)?;
    // The prefetched path gets the same `max_rows` cut the HTML path applies while parsing.
    let mut tables = tables;
    for t in &mut tables {
        t.rows.truncate(req.max_rows);
    }
    let table = pick_table(&tables, want, occurrence).ok_or_else(|| EngineError::Other(no_table_message(&tables)))?;
    let occurrence = tables.iter().take_while(|t| !std::ptr::eq(*t, table)).filter(|t| t.name.eq_ignore_ascii_case(&table.name)).count();
    let name = str_param(p, "sheet").map(|n| n.chars().take(31).collect::<String>()).unwrap_or_else(|| default_sheet_name(&req.url));
    load_table(s, &req, table, &name, true, occurrence)
}

/// The page's tables: a background worker's download (`prefetched` for this URL, or the
/// `prefetchedByUrl` entry for it), else the fetch source's own download.
fn fetch_tables(s: &Session, req: &WebRequest, p: &Json) -> Result<Vec<WebTable>> {
    if p.get("prefetched").is_some() {
        return prefetched_tables(p);
    }
    if p.get("prefetchedByUrl").is_some() {
        return prefetched_for_url(p, &req.url);
    }
    let Some(source) = s.web_source.clone() else {
        return Err(EngineError::Other("Web page import is not available in this build.".into()));
    };
    source(req).map_err(EngineError::Other)
}

/// The site's own name (`https://www.example.com/x` → `example`), capped at 31 characters.
/// Country suffixes that register under a second level (`.co.uk`, `.com.au`) are skipped, so
/// `https://sub.example.co.uk` is `example` too.
fn default_sheet_name(url: &str) -> String {
    const SECOND_LEVEL_CCTLDS: [&str; 12] = ["uk", "au", "jp", "br", "cn", "in", "kr", "nz", "za", "sg", "hk", "tw"];
    const SECOND_LEVEL_PREFIXES: [&str; 9] = ["co", "com", "org", "net", "gov", "ac", "edu", "or", "ne"];
    let authority = url.trim_start_matches("https://").split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default().split(':').next().unwrap_or_default();
    let labels: Vec<&str> = host.split('.').filter(|p| !p.is_empty() && *p != "www").collect();
    let base = match labels.len() {
        0 => "Web",
        1 => labels[0],
        n => {
            let last = labels[n - 1];
            let second = labels[n - 2];
            if SECOND_LEVEL_CCTLDS.contains(&last) && SECOND_LEVEL_PREFIXES.contains(&second) && n > 2 { labels[n - 3] } else { second }
        }
    };
    base.chars().take(31).collect()
}

/// The table `want` names (page name or 1-based index), else the first one. A page can hold
/// several tables with the same name (duplicate ids): `occurrence` picks which one, zero-based.
fn pick_table<'a>(tables: &'a [WebTable], want: Option<&str>, occurrence: usize) -> Option<&'a WebTable> {
    match want {
        // A missing occurrence is no match: the page's tables changed, and importing a
        // different same-named one would silently replace the user's data.
        Some(w) => tables
            .iter()
            .filter(|t| t.name.eq_ignore_ascii_case(w))
            .nth(occurrence)
            .or_else(|| w.parse::<usize>().ok().and_then(|n| tables.get(n.saturating_sub(1)))),
        None => tables.first(),
    }
}

/// The error message for a table the page does not hold, naming the ones it does.
fn no_table_message(tables: &[WebTable]) -> String {
    if tables.is_empty() {
        "No tables were found on the page.".into()
    } else {
        format!("The page has no table by that name. It holds: {}.", tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", "))
    }
}

/// Writes `table` to a new sheet as a table and (when `save`) records the connection.
pub(crate) fn load_table(s: &mut Session, req: &WebRequest, table: &WebTable, sheet_name: &str, save: bool, occurrence: usize) -> Result<Json> {
    s.grouped("From Web", |s| load_table_steps(s, req, table, sheet_name, save, occurrence))
}

/// The import as one undo step: a failure after the sheet was inserted rolls the whole
/// workbook back, so no empty sheet or half-written table is left behind.
fn load_table_steps(s: &mut Session, req: &WebRequest, table: &WebTable, sheet_name: &str, save: bool, occurrence: usize) -> Result<Json> {
    let rows = json_rows(table);
    if rows.is_empty() {
        return Err(EngineError::Other("The table is empty.".into()));
    }
    s.execute("home.insertSheet", json!({"name": sheet_name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    let r = s.doc()?;
    let sheet = r.wb.active_sheet.get();
    let sheet_name = r.wb.sheet(sheet).map(|sh| sh.name.clone()).unwrap_or_default();
    s.execute("range.setValues", json!({"range": "A1", "sheet": sheet, "values": rows}))?;
    let w = table.rows.iter().map(Vec::len).max().unwrap_or(1);
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(table.rows.len() as u32 - 1, w as u32 - 1));
    // The table takes the page table's name (spaces and other characters a table name cannot
    // hold become `_`); a taken name gets a generated one, and the connection saves the name
    // that was actually created.
    let sanitize = |n: &str| n.chars().take(255).map(|c| if c.is_alphanumeric() || c == '_' || c == '.' { c } else { '_' }).collect::<String>();
    let mut tname = sanitize(&table.name);
    if !tname.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\') {
        tname = format!("Web_{tname}");
    }
    s.execute("insert.table", json!({"range": range.a1(), "header": true, "name": tname})).or_else(|e| {
        // The name is taken or still not valid: a generated one keeps the import working.
        let taken = e.to_string().contains("already exists");
        let generated = s
            .doc()
            .map(|d| (1..).map(|n| format!("Web_Table{n}")).find(|n| d.wb.table(n).is_none()).unwrap_or_else(|| "Web_Table".into()))
            .unwrap_or_else(|_| "Web_Table".into());
        if !taken && !e.to_string().contains("not valid") {
            return Err(e);
        }
        s.execute("insert.table", json!({"range": range.a1(), "header": true, "name": generated}))
    })?;
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, w as u32 - 1).a1()}));
    // The table that was actually created: its name may be a generated one when the page
    // table's name was taken or not valid. The connection and the result both report it, and
    // the connection binds to the table's stable id, not its name.
    let (created_name, created_id) = {
        let d = s.doc()?;
        d.wb.sheet(sheet)
            .and_then(|sh| sh.tables.iter().find(|t| t.range == range).map(|t| (t.name.clone(), t.id)))
            .unwrap_or_else(|| (tname.clone(), 0))
    };
    if save {
        let synced = now_ms();
        let name = created_name.clone();
        let q = gridcraft_model::WebQuery {
            name,
            table_id: (created_id > 0).then_some(created_id),
            url: req.url.clone(),
            table: table.name.clone(),
            occurrence,
            sheet: sheet_name.clone(),
            last_synced: Some(synced),
        };
        edit(s, |cx| {
            cx.wb.web_queries.retain(|x| !(x.sheet.eq_ignore_ascii_case(&q.sheet) && x.name.eq_ignore_ascii_case(&q.name)));
            cx.wb.web_queries.push(q.clone());
            cx.structural = true;
            Ok(())
        })?;
    }
    Ok(json!({"sheet": sheet_name, "table": created_name, "rows": table.rows.len(), "cols": w, "tables_found": table_count(s)}))
}

/// Keeps the saved connections pointing at their table after `table.rename`: the connection's
/// `name` follows, so the Queries & Connections pane and `data.webRefresh` name lookups stay
/// current. The stable `table_id` already finds the table; this keeps the human-facing name.
/// Only connections on the renamed table's sheet match: another sheet's connection may name a
/// different table by the same name.
pub(crate) fn rename_connection(wb: &mut Workbook, sheet: &str, old: &str, name: &str) {
    for q in wb.web_queries.iter_mut() {
        if q.sheet.eq_ignore_ascii_case(sheet) && q.name.eq_ignore_ascii_case(old) {
            q.name = name.to_string();
        }
    }
}

/// Keeps the connection's sheet name current after `sheet.rename`.
pub(crate) fn rename_connection_sheet(wb: &mut Workbook, old: &str, name: &str) {
    for q in wb.web_queries.iter_mut() {
        if q.sheet.eq_ignore_ascii_case(old) {
            q.sheet = name.to_string();
        }
    }
}

/// How many web connections the workbook holds (the import result's `tables_found`).
fn table_count(s: &Session) -> usize {
    s.doc().map(|d| d.wb.web_queries.len()).unwrap_or(0)
}

/// The table as `range.setValues` rows: cell text parsed like typed input, so numbers, dates and
/// booleans keep their types (Excel's column detection). A cell that would become a formula
/// (`=`, or `+`/`-`/`@` starting one) is escaped with the leading apostrophe, which parses as
/// the text itself: page content never becomes live formulas (CSV injection).
fn json_rows(table: &WebTable) -> Vec<Vec<Json>> {
    table
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| {
                    if c.is_empty() {
                        Json::Null
                    } else if would_be_formula(c) {
                        json!(format!("'{c}"))
                    } else {
                        json!(c)
                    }
                })
                .collect()
        })
        .collect()
}

/// True when typed-input parsing would make `c` a formula: `=` always, and `+`/`-`/`@` only
/// in the forms that start one (a plain `-5` stays the number it is). Mirrors the check in
/// `input_to_cell` (cmd/edit.rs).
fn would_be_formula(c: &str) -> bool {
    c.starts_with('=')
        || (c.len() > 1
            && c.starts_with(['+', '-', '@'])
            && gridcraft_core::parse::parse_number_text(c).is_none()
            && c.chars().nth(1).is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '('))
}

/// `data.webPreview`: the page's detected tables, each with a name, size and preview rows
/// (the Navigator's list and preview pane).
fn web_preview(s: &mut Session, p: &Json) -> Result<Json> {
    let req = request(p)?;
    let tables = fetch_tables(s, &req, p)?;
    let preview = |t: &WebTable| {
        json!({
            "name": t.name,
            "rows": t.rows.len(),
            "cols": t.rows.iter().map(Vec::len).max().unwrap_or(0),
            "preview": t.rows.iter().take(PREVIEW_ROWS).map(|r| r.iter().take(PREVIEW_COLS).map(|c| json!(c)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        })
    };
    Ok(json!({"url": req.url, "tables": tables.iter().map(preview).collect::<Vec<_>>()}))
}

/// `data.webRefresh`: re-fetch and rewrite the saved connections the params select (all by
/// default), reporting per-connection errors instead of failing the batch.
fn web_refresh(s: &mut Session, p: &Json) -> Result<Json> {
    let url = str_param(p, "url").map(str::trim).filter(|u| !u.is_empty());
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty());
    let queries: Vec<gridcraft_model::WebQuery> = {
        let d = s.doc()?;
        d.wb.web_queries
            .iter()
            .filter(|q| url.is_none_or(|u| q.url.eq_ignore_ascii_case(u)) && name.is_none_or(|n| q.name.eq_ignore_ascii_case(n)))
            .cloned()
            .collect()
    };
    if queries.is_empty() {
        return Err(EngineError::Other("No saved web table matches.".into()));
    }
    let mut refreshed = 0;
    let mut errors: Vec<Json> = Vec::new();
    for q in queries {
        // The URL comes from the saved connection, so a workbook the model deserialized could
        // hold one no fresh import would accept: validate before the fetch, like `request` does.
        if let Err(e) = crate::webquery::validate_url(&q.url) {
            errors.push(json!({"table": q.name, "error": e}));
            continue;
        }
        let req = WebRequest { url: q.url.clone(), max_rows: DEFAULT_MAX_ROWS, max_cols: MAX_COLS, max_bytes: DEFAULT_MAX_BYTES };
        // A background worker's downloads come through `fetch_tables` (keyed by URL); without
        // them, the fetch runs here.
        match fetch_tables(s, &req, p).and_then(|tables| {
            let table = pick_table(&tables, Some(&q.table), q.occurrence).ok_or_else(|| EngineError::Other(no_table_message(&tables)))?;
            rewrite_table(s, &q, table)
        }) {
            Ok((rows, _cols)) => {
                refreshed += 1;
                log::debug!("webRefresh {}: {} rows", q.name, rows);
            }
            Err(e) => errors.push(json!({"table": q.name, "error": e.to_string()})),
        }
    }
    Ok(json!({"refreshed": refreshed, "errors": errors}))
}

/// Re-fetch result over the connection's table: clear its range, write the new rows and
/// resize the table, as one undo step, at the table's own place. Formulas and formatting
/// outside the range are kept.
fn rewrite_table(s: &mut Session, q: &gridcraft_model::WebQuery, table: &WebTable) -> Result<(usize, usize)> {
    let d = s.doc()?;
    // Only the connection's own table. The stable id decides: a renamed table keeps refreshing,
    // and a different table created with the former name is never selected. Workbooks saved
    // before connections held the id fall back to the name match, as they always did.
    let by_id = |t: &gridcraft_model::Table| q.table_id.is_some_and(|id| t.id == id);
    let by_name = |t: &gridcraft_model::Table| q.table_id.is_none() && t.name.eq_ignore_ascii_case(&q.name);
    let Some((sheet, tname, old)) =
        d.wb.sheets.iter().enumerate().find_map(|(si, sh)| sh.tables.iter().find(|t| by_id(t) || by_name(t)).map(|t| (si, t.name.clone(), t.range)))
    else {
        return Err(EngineError::Other(format!("The sheet or table of the connection `{}` is gone.", q.name)));
    };
    let rows = json_rows(table);
    if rows.is_empty() {
        return Err(EngineError::Other("The table is empty.".into()));
    }
    let w = table.rows.iter().map(Vec::len).max().unwrap_or(1);
    // The fetched header names, as the edit below spells them.
    let page_headers: Vec<String> = (0..w)
        .map(|i| {
            table
                .rows
                .first()
                .and_then(|row| row.get(i))
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("Column{}", i + 1))
        })
        .collect();
    // Calculated columns the user added are theirs, not the page's: the ones with a column
    // formula whose name the fetched headers do not supply are kept, wherever the page's width
    // leaves room (a wider page moves them right, a narrower one left, like Excel). Selection is
    // by name, not position: a page that grows into a calculated column's old position must
    // move it, not overwrite it. Each one's old column is remembered so the clash check below
    // can tell its own cells from foreign content at the new place.
    let kept: Vec<(u32, TableColumn)> = {
        let sh = d.wb.sheet(sheet).ok_or_else(|| EngineError::Other(format!("The sheet of the connection `{}` is gone.", q.name)))?;
        let Some(t) = sh.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&tname)) else { return Err(EngineError::Other(format!("The table of the connection `{}` is gone.", q.name))) };
        t.columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.formula.is_some() && !page_headers.iter().any(|h| h.eq_ignore_ascii_case(&c.name)))
            .map(|(i, c)| (i as u32, c.clone()))
            .collect()
    };
    let total_w = w + kept.len();
    let (Some(end_row), Some(end_col)) = (
        old.start.row.checked_add(table.rows.len() as u32 - 1).filter(|r| *r < MAX_ROWS),
        old.start.col.checked_add(total_w as u32 - 1).filter(|c| *c < gridcraft_core::MAX_COLS),
    ) else {
        return Err(EngineError::Other("The refreshed table doesn't fit on the sheet below and right of the table's top-left cell.".into()));
    };
    let new_range = RangeRef::new(old.start, CellRef::new(end_row, end_col));
    // A refresh only owns the old table's own rectangle: refuse before any change if the
    // expanded destination would overwrite other content (a user formula, another table). The
    // totals row follows the resized table, so the row below the new data is checked too. The
    // kept calculated columns are expected content, not a clash: they are rewritten in place.
    if let Some(sh) = d.wb.sheet(sheet) {
        let totals_row = sh.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&tname)).is_some_and(|t| t.totals_row);
        let last_row = if totals_row { (new_range.end.row + 1).min(MAX_ROWS - 1) } else { new_range.end.row };
        // The kept calculated columns' own cells (at their old columns, within the old rows) are
        // expected, not a clash; the new places they move to must be empty, or the refresh
        // refuses rather than overwrite whatever sits there.
        let outside = new_range.height() > old.height() || new_range.width() > old.width() || totals_row;
        let clash = outside
            && (new_range.start.row..=last_row).any(|row| {
                sh.cells.row(row, new_range.start.col, new_range.end.col).any(|(c, cell)| {
                    (row < old.start.row || row > old.end.row || c < old.start.col || c > old.end.col)
                        && !(row <= old.end.row && kept.iter().any(|(i, _)| old.start.col + i == c))
                        && (!cell.value.is_empty() || cell.formula.is_some())
                })
            });
        if clash {
            return Err(EngineError::Other(format!(
                "The refreshed table would overwrite cells outside its old range {OVERWRITE_ERROR}: {}",
                new_range.a1()
            )));
        }
    }
    // Ordinary cell formulas the user typed inside the table (not calculated-column
    // metadata, which is restored by name below) are theirs like the kept columns are:
    // snapshot them before the clear and write them back after, so a refresh keeps them.
    let kept_cols: std::collections::HashSet<u32> = kept.iter().map(|(i, _)| old.start.col + i).collect();
    // The totals row's own row (when the table has one, it is the last of the old range) is
    // rewritten by the totals restore below, not by this snapshot: restoring its old
    // SUBTOTAL formulas at their old row would overwrite the page's resized data.
    let totals_old_row = d.wb.sheet(sheet).and_then(|sh| sh.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&tname))).filter(|t| t.totals_row).map(|_| old.end.row);
    let user_formulas: Vec<(CellRef, std::sync::Arc<gridcraft_model::Formula>)> = {
        let d = s.doc()?;
        let Some(sh) = d.wb.sheet(sheet) else { return Ok((table.rows.len(), w)) };
        sh.cells
            .iter_range(old)
            .filter(|(c, _)| !kept_cols.contains(&c.col) && totals_old_row.is_none_or(|r| c.row != r))
            .filter_map(|(c, x)| x.formula.clone().map(|f| (c, f)))
            .collect()
    };
    // The rewrite is one undo step over the table's own place: a failure mid-way rolls the
    // old table and its data back instead of leaving the sheet empty.
    s.grouped("Refresh Web Table", |s| {
        // Clear only the old table's rectangle and rewrite the data; the table itself stays:
        // its style, totals row, calculated-column and XML-binding metadata, filters and flags
        // are the user's settings, not the page's, and a refresh must not reset them. The
        // clear is the refresh's own, not `edit.clearContents`: that path unchecks checked
        // checkboxes to Value::Bool(false) on its first pass, and a cell left that way
        // outside a shrunk table would read as content and refuse the next grown refresh.
        edit(s, |cx| {
            let cleared: Vec<(CellRef, gridcraft_model::Cell)> = {
                let sh = cx.wb.sheet(sheet).ok_or_else(|| EngineError::Other("the sheet is gone.".into()))?;
                sh.cells.iter_range(old).map(|(c, x)| (c, x.clone())).collect()
            };
            for (c, _) in &cleared {
                cx.changed.push((sheet, *c));
            }
            let sh = cx.sheet_mut(sheet)?;
            for (c, x) in cleared {
                // The cell keeps its style; only the value and formula go.
                sh.set_cell(c, gridcraft_model::Cell { formula: None, value: gridcraft_core::Value::Empty, style: x.style });
            }
            cx.structural = true;
            Ok(())
        })?;
        // Write at the table's own origin, not A1: a moved table refreshes where it is.
        s.execute("range.setValues", json!({"range": old.start.a1(), "sheet": sheet, "values": rows}))?;
        edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            let Some(ti) = sh.tables.iter().position(|t| t.name.eq_ignore_ascii_case(&tname)) else { return Ok(()) };
            // The columns follow the new header row: an unchanged name keeps its metadata
            // (totals function and label, calculated-column formula, XML binding), a new one
            // starts plain. sync_headers (run by `edit`) then renames the columns to match.
            let old_cols = sh.tables[ti].columns.clone();
            // The names come from the fetched header row, not the cells: a numeric header
            // (`2024`) is a number in the cell, and reading it as text would miss the column
            // and drop its settings on every refresh. sync_headers writes them back as text.
            let mut columns: Vec<TableColumn> = page_headers
                .clone()
                .into_iter()
                .map(|name| {
                    old_cols.iter().find(|col| col.name.eq_ignore_ascii_case(&name)).cloned().unwrap_or(TableColumn {
                        name,
                        totals: TotalsFn::None,
                        totals_label: None,
                        formula: None,
                        unique_name: None,
                        xml: None,
                    })
                })
                .collect();
            // The user's calculated columns beyond the page's width stay, with their metadata.
            columns.extend(kept.iter().map(|(_, c)| c.clone()));
            let t = &mut sh.tables[ti];
            t.columns = columns;
            t.range = new_range;
            // Write each kept column's name into its new header cell before sync_headers runs:
            // an empty header there would be renamed to a generated Column{n}, breaking the
            // name the metadata (and the connection) still holds.
            for (i, (_, col)) in kept.iter().enumerate() {
                let c = CellRef::new(new_range.start.row, new_range.start.col + w as u32 + i as u32);
                let style = sh.style_id(c);
                sh.set_cell(c, gridcraft_model::Cell { value: gridcraft_core::Value::text(col.name.as_str()), style, ..Default::default() });
            }
            // A shrunken page leaves header cells outside the new range: clear them too, or
            // sync_headers regenerates a name ("Column3") in the dropped column, and the
            // next grown refresh reads that stale name as foreign content and refuses.
            for col in (new_range.end.col + 1)..=old.end.col {
                let at = CellRef::new(old.start.row, col);
                if sh.cell(at).is_some() {
                    let style = sh.style_id(at);
                    sh.set_cell(at, gridcraft_model::Cell { value: gridcraft_core::Value::Empty, style, ..Default::default() });
                }
            }
            cx.structural = true;
            Ok(())
        })?;
        // The kept calculated columns lost their cells to clearContents: refill each one's
        // formula down the resized data rows, relative-shifted per row like a fill would.
        if !kept.is_empty() {
            edit(s, |cx| {
                let sh = cx.sheet_mut(sheet)?;
                let Some(t) = sh.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&tname)).cloned() else { return Ok(()) };
                for (i, (_, col)) in kept.iter().enumerate() {
                    let Some(code) = col.formula.as_ref() else { continue };
                    let expr = gridcraft_formula::parse(code).map_err(|e| EngineError::Other(format!("The calculated column `{}` has a broken formula: {e}", col.name)))?;
                    let c0 = t.range.start.col + w as u32 + i as u32;
                    for row in (t.range.start.row + 1)..=t.range.end.row {
                        let at = CellRef::new(row, c0);
                        // Each cell keeps its own style (the header cell's format would be wrong
                        // for the data rows, and one shared style would erase per-cell formats).
                        let style = sh.style_id(at);
                        let shifted = gridcraft_formula::adjust::shift_relative(expr.clone(), row as i64 - (t.range.start.row as i64 + 1), 0);
                        sh.set_cell(at, gridcraft_model::Cell { formula: Some(std::sync::Arc::new(gridcraft_model::Formula::from_expr(shifted))), value: gridcraft_core::Value::Empty, style });
                    }
                }
                cx.structural = true;
                Ok(())
            })?;
        }
        // The user's ordinary formulas come back where the page did not overwrite them: only
        // inside the new range (a formula in a row or column the page dropped is gone with
        // the data it belonged to), never over a kept calculated column's refilled cell.
        if !user_formulas.is_empty() {
            edit(s, |cx| {
                let sh = cx.sheet_mut(sheet)?;
                // The kept columns' refilled cells sit at their new positions (page width plus
                // each kept index); a user formula restored over one of them would desync it
                // from its TableColumn.formula metadata.
                let kept_new: std::collections::HashSet<u32> = (0..kept.len() as u32).map(|i| new_range.start.col + w as u32 + i).collect();
                for (c, f) in &user_formulas {
                    if new_range.contains(*c) && !kept_new.contains(&c.col) {
                        let style = sh.style_id(*c);
                        sh.set_cell(*c, gridcraft_model::Cell { formula: Some(f.clone()), value: gridcraft_core::Value::Empty, style });
                    }
                }
                cx.structural = true;
                Ok(())
            })?;
        }
        // The totals row sits below the resized range: restore its SUBTOTAL formulas and
        // labels, which the clear took with the rest of the old rectangle.
        let restore_totals = {
            let d = s.doc()?;
            d.wb.sheet(sheet).and_then(|sh| sh.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&tname))).filter(|t| t.totals_row).is_some()
        };
        if restore_totals {
            edit(s, |cx| {
                let sh = cx.sheet_mut(sheet)?;
                let Some(ti) = sh.tables.iter().position(|t| t.name.eq_ignore_ascii_case(&tname)) else { return Ok(()) };
                sh.tables[ti].range.end.row = (sh.tables[ti].range.end.row + 1).min(MAX_ROWS - 1);
                write_totals(sh, ti);
                cx.structural = true;
                Ok(())
            })?;
        }
        let synced = now_ms();
        edit(s, |cx| {
            if let Some(qq) = cx.wb.web_queries.iter_mut().find(|x| x.sheet.eq_ignore_ascii_case(&q.sheet) && x.name.eq_ignore_ascii_case(&q.name)) {
                qq.last_synced = Some(synced);
            }
            cx.structural = true;
            Ok(())
        })?;
        Ok(())
    })?;
    Ok((table.rows.len(), w))
}

/// The `prefetched` param's value: tables a background worker already downloaded for this URL.
/// The UI thread runs the fetch off itself and feeds the result back through the command, so a
/// slow page never blocks the interface (Excel's queries run in the background too).
fn prefetched_tables(p: &Json) -> Result<Vec<WebTable>> {
    let raw = p.get("prefetched").ok_or_else(|| EngineError::Other("missing `prefetched`".into()))?;
    let mut out = Vec::new();
    // The same caps as the HTML path: the caller JSON is as hostile as the page it came from.
    let mut page_budget: usize = MAX_PAGE_CELLS;
    for t in raw.as_array().ok_or_else(|| EngineError::Other("`prefetched` must be an array of tables".into()))? {
        let name = t["name"].as_str().unwrap_or_default().to_string();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut cells: usize = 0;
        for r in t["rows"].as_array().into_iter().flatten() {
            if rows.len() >= DEFAULT_MAX_ROWS {
                break;
            }
            let mut row: Vec<String> = Vec::new();
            for c in r.as_array().into_iter().flatten() {
                if row.len() >= MAX_COLS {
                    break;
                }
                let text: String = c.as_str().unwrap_or_default().chars().take(MAX_TEXT).collect();
                cells += 1;
                row.push(text);
                if cells >= MAX_PAGE_CELLS {
                    break;
                }
            }
            // Empty rows are dropped, not kept as zero-cell rows: the widest row then always has
            // at least one cell, so the width arithmetic cannot underflow (an all-empty
            // table reaches the existing "The table is empty." refusal instead).
            if !row.is_empty() {
                rows.push(row);
            }
            if cells >= MAX_PAGE_CELLS {
                break;
            }
        }
        page_budget = page_budget.saturating_sub(cells);
        out.push(WebTable { name, rows });
        if page_budget == 0 || out.len() >= MAX_TABLES {
            break;
        }
    }
    Ok(out)
}

/// One entry of the `prefetchedByUrl` map: a page's tables (or its fetch `error`).
fn prefetched_for_url(p: &Json, url: &str) -> Result<Vec<WebTable>> {
    let entry = p.get("prefetchedByUrl").and_then(|m| m.get(url)).ok_or_else(|| EngineError::Other(format!("no prefetched page for {url}")))?;
    if let Some(e) = entry.get("error").and_then(Json::as_str) {
        return Err(EngineError::Other(e.to_string()));
    }
    prefetched_tables(&json!({"prefetched": entry["tables"]}))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use gridcraft_core::Value;

    /// A page with two tables: one named, one nameless.
    fn page() -> &'static str {
        "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>ABCD</td><td>12.50</td></tr></table>\
         <table><tr><td>x</td><td>1</td></tr></table>"
    }

    /// A session whose web source serves `page`.
    fn session_with_source() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            assert!(req.url.starts_with("https://"));
            Ok(crate::webquery::extract_tables(page(), req.max_rows, req.max_cols))
        }));
        s
    }

    /// The cell's displayed text on the active sheet.
    fn a1(s: &Session, at: &str) -> String {
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        crate::display::cell_text(&d.wb, sh, CellRef::parse(at).unwrap())
    }

    #[test]
    /// The default import: the first table, a new sheet, a saved connection.
    fn imports_the_first_table_into_a_new_sheet_and_saves_the_connection() {
        let mut s = session_with_source();
        let r = s.execute("data.fromWeb", json!({"url": "https://example.com/markets", "sheet": "Markets"})).unwrap();
        assert_eq!(r["sheet"], "Markets");
        assert_eq!(r["rows"], 2);
        assert_eq!(a1(&s, "A1"), "Sym");
        assert_eq!(a1(&s, "B2"), "12.5");
        let d = s.active().unwrap();
        assert_eq!(d.wb.web_queries.len(), 1);
        assert_eq!(d.wb.web_queries[0].url, "https://example.com/markets");
        assert!(d.wb.web_queries[0].last_synced.is_some());
        assert!(d.wb.active().unwrap().tables.iter().any(|t| t.name == "prices"));
    }

    #[test]
    /// A named table is picked by name; its cells keep their types.
    fn imports_a_named_table_and_keeps_text_and_numbers() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "table": "Table 2"})).unwrap();
        assert_eq!(a1(&s, "A1"), "x");
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        // The page name "Table 2" cannot be a table name (the space); the import still makes
        // the table, with a name the page table's was sanitized into.
        assert!(sh.tables.iter().any(|t| t.name == "Table_2"));
        // A one-row import is the table's header row, which is text.
        assert_eq!(sh.value(CellRef::parse("B1").unwrap()), Value::text("1"));
    }

    #[test]
    /// The preview lists every table with its name and size.
    fn preview_lists_the_page_tables() {
        let mut s = session_with_source();
        let r = s.execute("data.webPreview", json!({"url": "https://example.com"})).unwrap();
        let tables = r["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0]["name"], "prices");
        assert_eq!(tables[0]["rows"], 2);
        assert_eq!(tables[1]["name"], "Table 2");
    }

    #[test]
    /// A refresh rewrites the same rectangle with the new rows and resizes the table.
    fn refresh_rewrites_the_table_in_place() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // The source now serves a bigger table.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>A</td><td>1</td></tr><tr><td>B</td><td>2</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        assert!(r["errors"].as_array().unwrap().is_empty());
        assert_eq!(a1(&s, "A3"), "B");
        assert_eq!(a1(&s, "B3"), "2");
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(sh.tables.len(), 1);
        assert_eq!(sh.tables[0].range.height(), 3);
        // The old range's leftovers are gone: nothing beyond the new table.
        assert!(sh.value(CellRef::parse("C1").unwrap()).is_empty());
    }

    #[test]
    /// A refresh keeps the user's style and totals row instead of resetting them.
    fn refresh_keeps_the_table_s_settings_and_totals_row() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // The user restyles the table and adds a totals row.
        s.execute("table.style", json!({"table": "prices", "style": "TableStyleMedium9"})).unwrap();
        s.execute("table.totalRow", json!({"table": "prices", "on": true})).unwrap();
        let id = {
            let d = s.active().unwrap();
            d.wb.active().unwrap().tables[0].id
        };
        assert_eq!(a1(&s, "B3"), "12.5");
        // The source now serves a bigger table.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>A</td><td>1</td></tr><tr><td>B</td><td>2</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        assert!(r["errors"].as_array().unwrap().is_empty());
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let t = &sh.tables[0];
        // The table itself survived: same name, the user's style, the totals row with
        // its SUBTOTAL function, and the resized range including the totals row.
        assert_eq!(t.name, "prices");
        assert_eq!(t.id, id);
        assert_eq!(t.style, "TableStyleMedium9");
        assert!(t.totals_row);
        assert_eq!(t.range.height(), 4);
        assert_eq!(t.columns[1].totals, gridcraft_model::TotalsFn::Sum);
        // The refreshed data landed, and the totals row was rewritten below it.
        assert_eq!(a1(&s, "B3"), "2");
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        assert!(sh.cell(CellRef::parse("B4").unwrap()).unwrap().formula.is_some());
    }

    #[test]
    /// A numeric header's column keeps its settings across a refresh.
    fn refresh_keeps_the_settings_of_a_numeric_header_column() {
        // A numeric header (`2024`) lands in the cell as a number: the column lookup must use
        // the fetched header text, or the column's settings are dropped on every refresh.
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let page = "<table id=\"years\"><tr><th>Item</th><th>2024</th></tr><tr><td>A</td><td>1</td></tr></table>";
        s.web_source = Some(Arc::new(move |req: &WebRequest| Ok(crate::webquery::extract_tables(page, req.max_rows, req.max_cols))));
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // The user gives the numeric column a Sum totals function.
        s.execute("table.totalFunction", json!({"table": "years", "column": "2024", "function": "sum"})).unwrap();
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let t = &sh.tables[0];
        assert_eq!(t.columns[1].name, "2024");
        assert_eq!(t.columns[1].totals, gridcraft_model::TotalsFn::Sum);
    }

    #[test]
    /// The default sheet name is the site's own name, not the TLD or a subdomain.
    fn default_sheet_name_is_the_site_not_the_tld() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://www.example.com/markets"})).unwrap();
        let d = s.active().unwrap();
        assert_eq!(d.wb.active().unwrap().name, "example");
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://sub.example.co.uk/x?a=1"})).unwrap();
        let d = s.active().unwrap();
        assert_eq!(d.wb.active().unwrap().name, "example");
    }

    #[test]
    /// A second import adds a connection rather than replacing the first.
    fn a_second_import_keeps_the_first_connection() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "One"})).unwrap();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Two"})).unwrap();
        let d = s.active().unwrap();
        assert_eq!(d.wb.web_queries.len(), 2);
        assert_eq!(d.wb.web_queries[0].sheet, "One");
        assert_eq!(d.wb.web_queries[1].sheet, "Two");
    }

    #[test]
    /// A refresh that would grow over other content is refused before any change.
    fn refresh_refuses_to_overwrite_cells_outside_the_old_range() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // A user formula sits just below the old table; the source now serves more rows.
        s.execute("sheet.activate", json!({"sheet": "Web"})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": "=1+1"})).unwrap();
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>A</td><td>1</td></tr><tr><td>B</td><td>2</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 0);
        let err = r["errors"][0]["error"].as_str().unwrap();
        assert!(err.contains("already holds data"));
        // The formula and the old table are untouched.
        assert_eq!(a1(&s, "A3"), "2");
        assert_eq!(a1(&s, "A2"), "ABCD");
    }

    #[test]
    /// The clash check tests rows and columns independently (a formula beside the old rows).
    fn refresh_refuses_to_overwrite_a_new_column_inside_the_old_rows() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // The old table is A1:B2; the source now serves a wider table, and a user formula
        // sits in the new column inside the old rows — the tuple-ordering blind spot.
        s.execute("sheet.activate", json!({"sheet": "Web"})).unwrap();
        s.execute("cell.set", json!({"cell": "C2", "input": "=1+1"})).unwrap();
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th><th>Chg</th></tr><tr><td>A</td><td>1</td><td>2</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 0);
        assert!(r["errors"][0]["error"].as_str().unwrap().contains("already holds data"));
        assert_eq!(a1(&s, "C2"), "2");
    }

    #[test]
    /// A moved table refreshes where it is, not at its old origin.
    fn refresh_writes_at_the_moved_table_s_own_place() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // Move the sheet's contents down two rows (values plus the table range).
        s.execute("sheet.activate", json!({"sheet": "Web"})).unwrap();
        s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>A</td><td>1</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        // The header sits where the moved table is, and A1 stayed empty.
        assert_eq!(a1(&s, "A3"), "Sym");
        assert_eq!(a1(&s, "A1"), "");
    }

    #[test]
    /// The import result and the connection name the table that was actually created.
    fn the_result_and_connection_name_the_created_table() {
        let mut s = session_with_source();
        // "prices" is a valid name, so the first import takes it; the second, from another
        // sheet, must fall back to a generated name and report it.
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "One"})).unwrap();
        let r = s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Two"})).unwrap();
        let made = r["table"].as_str().unwrap();
        assert_ne!(made, "prices");
        assert!(made.starts_with("Web_Table"));
        let d = s.active().unwrap();
        assert!(d.wb.web_queries.iter().any(|q| q.name == made));
    }

    #[test]
    /// Page text that looks like a formula is imported as text, never live.
    fn page_content_never_becomes_a_formula() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>A</th><th>B</th></tr>\
                 <tr><td>=HYPERLINK(\"http://evil\")</td><td>-5</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        // The `=` cell is the text it came as, not a live formula.
        let c = sh.cell(CellRef::parse("A2").unwrap()).unwrap();
        assert!(c.formula.is_none());
        assert_eq!(c.value, gridcraft_core::Value::text("=HYPERLINK(\"http://evil\")"));
        // A plain negative number is still a number.
        assert_eq!(sh.value(CellRef::parse("B2").unwrap()), Value::number(-5.0));
    }

    #[test]
    /// With the connection's table gone, refresh reports it and touches nothing.
    fn refresh_never_falls_back_to_an_unrelated_table() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // Rename the imported table so the connection's name no longer matches any table.
        s.execute("sheet.activate", json!({"sheet": "Web"})).unwrap();
        let d = s.doc().unwrap();
        let tname = d.wb.active().unwrap().tables[0].name.clone();
        s.execute("table.rename", json!({"table": tname, "name": "Renamed"})).unwrap();
        // Convert the renamed table to a range: the connection's table is truly gone.
        s.execute("table.convertToRange", json!({"table": "Renamed"})).unwrap();
        // Another, unrelated table now sits on the sheet.
        s.execute("range.setValues", json!({"range": "E1", "sheet": "Web", "values": [["keep"]]})).unwrap();
        s.execute("insert.table", json!({"range": "E1", "header": true, "name": "Unrelated"})).unwrap();
        // The connection was dropped with its table: refresh reports nothing to do.
        let err = s.execute("data.webRefresh", json!({})).unwrap_err();
        assert!(err.to_string().contains("No saved web table matches"));
        assert_eq!(a1(&s, "E1"), "keep");
    }

    #[test]
    /// Duplicate page names: the connection re-imports the occurrence it started with.
    fn refresh_re_imports_the_same_occurrence_of_a_duplicate_table_name() {
        // The page holds two tables with the same id; the import takes the second one.
        let page = "<table id=\"prices\"><tr><th>Sym</th></tr><tr><td>first</td></tr></table>\
                    <table id=\"prices\"><tr><th>Sym</th></tr><tr><td>second</td></tr></table>";
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        s.web_source = Some(Arc::new(move |req: &WebRequest| Ok(crate::webquery::extract_tables(page, req.max_rows, req.max_cols))));
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web", "table": "prices", "occurrence": 1})).unwrap();
        assert_eq!(a1(&s, "A2"), "second");
        let d = s.active().unwrap();
        assert_eq!(d.wb.web_queries[0].occurrence, 1);
        // Refresh keeps importing the second one, not the first same-named table.
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        assert_eq!(a1(&s, "A2"), "second");
    }

    #[test]
    /// A vanished occurrence fails the refresh and keeps the sheet as it was.
    fn a_vanished_occurrence_fails_refresh_and_keeps_the_data() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let page = "<table id=\"prices\"><tr><td>first</td></tr></table>\
                    <table id=\"prices\"><tr><td>second</td></tr></table>";
        s.web_source = Some(Arc::new(move |req: &WebRequest| Ok(crate::webquery::extract_tables(page, req.max_rows, req.max_cols))));
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web", "table": "prices", "occurrence": 1})).unwrap();
        assert_eq!(a1(&s, "A1"), "second");
        // The page changes: only one table by that name is left.
        let changed = "<table id=\"prices\"><tr><td>changed</td></tr></table>";
        s.web_source = Some(Arc::new(move |req: &WebRequest| Ok(crate::webquery::extract_tables(changed, req.max_rows, req.max_cols))));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 0);
        assert!(!r["errors"].as_array().unwrap().is_empty());
        // The sheet still holds the table the user imported.
        assert_eq!(a1(&s, "A1"), "second");
    }

    #[test]
    /// No source, a non-http address and a missing table are errors, not imports.
    fn errors_without_source_bad_url_or_missing_table() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let err = s.execute("data.fromWeb", json!({"url": "https://x.com"})).unwrap_err();
        assert!(err.to_string().contains("not available"));
        assert!(s.execute("data.fromWeb", json!({"url": "ftp://x"})).is_err());
        assert!(s.execute("data.fromWeb", json!({})).is_ok()); // opens the dialog
        // A plain-http address is refused even with tables supplied (prefetched), keeping the
        // https-only contract; https with prefetched tables still imports.
        assert!(s.execute("data.fromWeb", json!({"url": "http://x.com", "prefetched": [{"name": "t", "rows": [["a"]]}]})).is_err());
        let mut s = session_with_source();
        let err = s.execute("data.fromWeb", json!({"url": "https://example.com", "table": "nope"})).unwrap_err();
        assert!(err.to_string().contains("no table by that name") || err.to_string().to_lowercase().contains("no table"));
        assert!(s.execute("data.webRefresh", json!({"name": "nothing"})).is_err());
    }

    #[test]
    /// Headless `data.fromWeb` without a URL fails instead of opening the dialog.
    fn headless_empty_params_fail_instead_of_opening_a_dialog() {
        let mut s = session_with_source();
        assert!(s.execute_headless("data.fromWeb", json!({})).is_err());
    }

    #[test]
    /// Every From Web command answers empty params with an error, not a panic.
    fn every_command_survives_empty_params() {
        let mut s = session_with_source();
        for id in ["data.fromWeb", "data.webPreview", "data.webRefresh"] {
            let _ = s.execute(id, json!({}));
        }
    }

    #[test]
    /// A renamed table keeps refreshing: the connection follows the rename by its stable id,
    /// and a different table created with the former name is never selected.
    fn refresh_follows_a_renamed_table_not_a_new_same_named_one() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        s.execute("sheet.activate", json!({"sheet": "Web"})).unwrap();
        let d = s.doc().unwrap();
        let tname = d.wb.active().unwrap().tables[0].name.clone();
        s.execute("table.rename", json!({"table": tname, "name": "Renamed"})).unwrap();
        // The connection's name followed the rename.
        {
            let d = s.active().unwrap();
            assert_eq!(d.wb.web_queries[0].name, "Renamed");
        }
        // A new, unrelated table takes the connection's former name.
        s.execute("range.setValues", json!({"range": "E1", "sheet": "Web", "values": [["keep"]]})).unwrap();
        s.execute("insert.table", json!({"range": "E1", "header": true, "name": tname})).unwrap();
        // The source now serves different rows: the refresh lands in the renamed table.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>Z</td><td>9</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        assert_eq!(a1(&s, "A2"), "Z");
        // The same-named decoy was not touched.
        assert_eq!(a1(&s, "E1"), "keep");
    }

    #[test]
    /// Renaming the imported sheet keeps the connection's sheet current.
    fn sheet_rename_keeps_the_connection_s_sheet() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        s.execute("sheet.rename", json!({"sheet": "Web", "name": "Data"})).unwrap();
        {
            let d = s.active().unwrap();
            assert_eq!(d.wb.web_queries[0].sheet, "Data");
        }
        // The Queries & Connections pane reports the current name too.
        let r = s.execute("data.queriesConnections", json!({})).unwrap();
        assert_eq!(r[0]["sheet"], "Data");
        // And a refresh still finds the table on the renamed sheet.
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
    }

    #[test]
    /// Deleting the imported sheet or converting its table to a range drops the connection.
    fn deleting_the_table_or_sheet_drops_the_connection() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        let tname = {
            let d = s.active().unwrap();
            d.wb.active().unwrap().tables[0].name.clone()
        };
        s.execute("table.convertToRange", json!({"table": tname})).unwrap();
        {
            let d = s.active().unwrap();
            assert!(d.wb.web_queries.is_empty());
        }
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        s.execute("home.deleteSheet", json!({"sheet": "Web"})).unwrap();
        let d = s.active().unwrap();
        assert!(d.wb.web_queries.is_empty());
    }

    #[test]
    /// Deleting the rows a web table spans drops its connection, whatever deleted the table
    /// (the choke-point prune in `commit`), so its id cannot later match a reused id.
    fn deleting_the_table_s_rows_drops_the_connection() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // Delete the rows the imported table spans: the table goes with them.
        s.execute("sheet.activate", json!({"sheet": "Web"})).unwrap();
        s.execute("home.deleteRows", json!({"rows": "1:2"})).unwrap();
        let d = s.active().unwrap();
        assert!(d.wb.active().unwrap().tables.is_empty());
        assert!(d.wb.web_queries.is_empty());
    }

    #[test]
    /// A calculated column the user added to the imported table survives a refresh: its
    /// metadata stays, its formulas are refilled down the resized data rows, and the page's own
    /// columns are rewritten around it (Excel keeps query-table calculated columns the same way).
    fn a_calculated_column_added_by_the_user_survives_a_refresh() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com/markets", "sheet": "Markets"})).unwrap();
        // The user's calculated column: cells with formulas, and the column metadata a workbook
        // save and load would round-trip as a `calculatedColumnFormula`.
        s.execute("cell.set", json!({"cell": "C2", "input": "=B2*2"})).unwrap();
        s.execute("cell.set", json!({"cell": "C3", "input": "=B3*2"})).unwrap();
        super::super::edit(&mut s, |cx| {
            let sh = cx.sheet_mut(cx.wb.sheet_index("Markets").ok_or_else(|| EngineError::Other("no Markets sheet".into()))?)?;
            let ti = sh.tables.iter().position(|t| t.name == "prices").expect("the table");
            while sh.tables[ti].columns.len() < 3 {
                let n = sh.tables[ti].columns.len();
                sh.tables[ti].columns.push(gridcraft_model::TableColumn { name: format!("Column{}", n + 1), totals: TotalsFn::None, totals_label: None, formula: Some("B2*2".into()), unique_name: None, xml: None });
            }
            cx.structural = true;
            Ok(())
        })
        .unwrap();
        s.execute("data.webRefresh", json!({})).unwrap();
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let t = sh.tables.iter().find(|t| t.name == "prices").unwrap();
        assert_eq!(t.range.width(), 3, "the calculated column stays in the table");
        assert_eq!(t.columns[2].formula.as_deref(), Some("B2*2"));
        // The page's data is refreshed and the formula recalculated against it.
        assert_eq!(sh.value(gridcraft_core::CellRef::parse("B2").unwrap()), gridcraft_core::Value::from(12.5));
        let c2 = sh.cell(gridcraft_core::CellRef::parse("C2").unwrap()).expect("the calculated column's cell");
        assert!(c2.formula.is_some(), "the formula is refilled");
        assert_eq!(c2.value, gridcraft_core::Value::from(25.0));
    }

    #[test]
    /// An ordinary formula the user typed inside the table survives a refresh: the clear
    /// takes the cell's value, the rewrite writes the page's data, and the formula is
    /// written back over the page's own value. The checkbox two-pass clear is not the
    /// refresh's clear: a checked box inside the old range, left as `Bool(false)` after the
    /// table shrinks away from it, would read as content and refuse the next grown refresh.
    fn a_refresh_keeps_the_user_s_ordinary_formulas() {
        let mut s = session_with_source();
        // A three-column import, so the user's formula and checkbox sit inside it.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th><th>Vol</th></tr><tr><td>A</td><td>12.5</td><td>3</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // The user's own formula inside the imported data (reads the page's price column).
        s.execute("cell.set", json!({"cell": "A2", "input": "=B2*10"})).unwrap();
        // A checked checkbox inside the table, in a column the next page drops.
        s.execute("cell.set", json!({"cell": "C2", "input": "TRUE", "style": "checkbox"})).unwrap();
        // The page shrinks to two columns: C2 leaves the table, and the refresh's clear
        // must leave it truly empty, not the checkbox pass's Bool(false).
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>A</td><td>1</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r0 = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r0["refreshed"], 1, "the shrunk refresh succeeds");
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let t0 = &sh.tables[0];
        assert_eq!(t0.range.width(), 2, "the stored range followed the shrunk page");
        // The user's formula survived the refresh, over the page's new value.
        let a2 = sh.cell(CellRef::parse("A2").unwrap()).expect("A2 exists");
        assert!(a2.formula.is_some(), "the user's formula is kept through the refresh");
        assert_eq!(a1(&s, "A2"), "10", "it recalculates against the new page value (B2=1)");
        // A later refresh that grows back over the checkbox cell must not be refused:
        // the refresh's clear left Empty, not Bool(false).
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th><th>Vol</th></tr><tr><td>A</td><td>1</td><td>7</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1, "the grown refresh is not refused by stale checkbox state");
        assert!(r["errors"].as_array().unwrap().is_empty());
    }

    #[test]
    /// A page that shrinks from four columns to two moves the kept calculated column onto a
    /// column that held a user formula: the refilled calculated column wins, and the user
    /// formula is not restored over it (that would desync the cell from its TableColumn
    /// formula metadata).
    fn a_shrunk_page_moves_the_kept_column_onto_a_user_formula_column() {
        let mut s = session_with_source();
        // A three-column import so the calculated column (added fourth) has room.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th><th>Vol</th></tr><tr><td>A</td><td>1</td><td>3</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        s.execute("data.fromWeb", json!({"url": "https://example.com", "sheet": "Web"})).unwrap();
        // A user calculated column, fourth of four.
        super::super::edit(&mut s, |cx| {
            let sh = cx.sheet_mut(cx.wb.sheet_index("Web").ok_or_else(|| EngineError::Other("no Web sheet".into()))?)?;
            let ti = sh.tables.iter().position(|t| t.name == "prices").expect("the table");
            sh.tables[ti].columns.push(gridcraft_model::TableColumn { name: "Total3".into(), totals: TotalsFn::None, totals_label: None, formula: Some("B2*2".into()), unique_name: None, xml: None });
            cx.structural = true;
            Ok(())
        })
        .unwrap();
        // A user formula in C2, which is where the kept column lands when the page shrinks
        // to two columns (new position: start.col + w + 0 = C).
        s.execute("cell.set", json!({"cell": "C2", "input": "=B2*999"})).unwrap();
        // The page shrinks to two columns: the calculated column moves from D to C.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th></tr><tr><td>A</td><td>1</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1, "the shrunk refresh succeeds (errors: {})", r["errors"]);
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let t = sh.tables.iter().find(|t| t.name == "prices").unwrap();
        // The kept column is third of three, at its new position C.
        assert_eq!(t.columns.len(), 3, "the page's two columns plus the kept one");
        assert_eq!(t.columns[2].name, "Total3");
        // Its cell holds the metadata's formula, not the user's C2 formula.
        let c2 = sh.cell(CellRef::parse("C2").unwrap()).expect("C2 exists");
        let f = c2.formula.as_ref().expect("the refilled formula");
        let code = gridcraft_formula::print(&f.expr().expect("a parsed formula"));
        assert!(code.contains("B2*2"), "the calculated column's formula is refilled (got {code})");
        assert!(!code.contains("999"), "the user formula was not restored over it");
        assert_eq!(a1(&s, "C2"), "2", "it recalculates against the new page value");
    }

    #[test]
    /// A wider page moves the user's calculated column right instead of overwriting it: the
    /// selection is by name, so the page's new third column takes position 3 and the calculated
    /// column (and its refilled formulas) moves to position 4.
    fn a_wider_page_moves_the_calculated_column_not_deletes_it() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com/markets", "sheet": "Markets"})).unwrap();
        // The user's calculated column, third of three.
        s.execute("cell.set", json!({"cell": "C2", "input": "=B2*2"})).unwrap();
        super::super::edit(&mut s, |cx| {
            let sh = cx.sheet_mut(cx.wb.sheet_index("Markets").ok_or_else(|| EngineError::Other("no Markets sheet".into()))?)?;
            let ti = sh.tables.iter().position(|t| t.name == "prices").expect("the table");
            while sh.tables[ti].columns.len() < 3 {
                let n = sh.tables[ti].columns.len();
                sh.tables[ti].columns.push(gridcraft_model::TableColumn { name: format!("Total{}", n), totals: TotalsFn::None, totals_label: None, formula: Some("B2*2".into()), unique_name: None, xml: None });
            }
            cx.structural = true;
            Ok(())
        })
        .unwrap();
        // A page three columns wide: the third column would sit where the calculated column is.
        s.web_source = Some(Arc::new(|req: &WebRequest| {
            Ok(crate::webquery::extract_tables(
                "<table id=\"prices\"><tr><th>Sym</th><th>Price</th><th>Volume</th></tr><tr><td>ABCD</td><td>12.50</td><td>100</td></tr></table>",
                req.max_rows,
                req.max_cols,
            ))
        }));
        s.execute("data.webRefresh", json!({})).unwrap();
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let t = sh.tables.iter().find(|t| t.name == "prices").unwrap();
        assert_eq!(t.range.width(), 4, "the page's three columns plus the calculated one");
        assert_eq!(t.columns[2].name, "Volume", "the page's third column takes position 3");
        assert_eq!(t.columns[3].formula.as_deref(), Some("B2*2"), "the calculated column moved right, metadata kept");
        assert_eq!(t.columns[3].name, "Total2", "sync_headers keeps the column's name");
        assert_eq!(sh.value(gridcraft_core::CellRef::parse("D1").unwrap()), gridcraft_core::Value::text("Total2"), "the moved column's header cell holds its name");
        // The page's data landed in its columns and the calculated column's cells refilled.
        assert_eq!(sh.value(gridcraft_core::CellRef::parse("C2").unwrap()), gridcraft_core::Value::from(100.0));
        let d2 = sh.cell(gridcraft_core::CellRef::parse("D2").unwrap()).expect("the calculated column's new cell");
        assert!(d2.formula.is_some(), "the formula is refilled at the new place");
        assert_eq!(d2.value, gridcraft_core::Value::from(25.0));
    }

    #[test]
    /// Userinfo in the URL (`user:pass@host`) is refused: ureq would send it as basic auth and
    /// the saved connection would store the secret verbatim in the workbook.
    fn a_url_with_userinfo_is_refused() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        for url in ["https://user:pass@example.com", "https://user@example.com"] {
            let err = s.execute("data.fromWeb", json!({"url": url})).unwrap_err();
            assert!(err.to_string().contains("can't be imported"), "{url}: {err}");
            assert!(s.active().unwrap().wb.web_queries.is_empty());
        }
    }

    #[test]
    /// A persisted connection whose deserialized URL holds userinfo — the JSON workbook model
    /// can carry one — is refused on refresh too: the guard lives in the shared fetch path, so
    /// the native provider never forwards the embedded credential, whatever stored the URL.
    fn a_persisted_userinfo_url_is_refused_on_refresh() {
        let mut s = session_with_source();
        s.execute("data.fromWeb", json!({"url": "https://example.com/markets", "sheet": "Markets"})).unwrap();
        // A deserialized workbook's connection: written as the JSON model would deliver it.
        super::super::edit(&mut s, |cx| {
            let q = cx.wb.web_queries.iter_mut().find(|q| q.sheet == "Markets").expect("the connection");
            q.url = "https://user:pass@example.com/markets".into();
            cx.structural = true;
            Ok(())
        })
        .unwrap();
        let r = s.execute("data.webRefresh", json!({})).unwrap();
        assert_eq!(r["refreshed"], 0, "the userinfo refresh must not run: {r}");
        assert!(r["errors"].as_array().is_some_and(|e| e.iter().any(|e| e["error"].as_str().is_some_and(|m| m.contains("can't be imported")))), "{r}");
    }

    #[test]
    /// A prefetched table of only empty rows is refused, not a zero-width import: the width
    /// arithmetic must never see an empty widest row (the u32 underflow CodeRabbit flagged).
    fn prefetched_tables_with_only_empty_rows_are_refused() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let err = s
            .execute("data.fromWeb", json!({"url": "https://example.com", "table": "t", "prefetched": [{"name": "t", "rows": [[], []]}]}))
            .unwrap_err();
        assert!(err.to_string().contains("The table is empty."));
    }

    #[test]
    /// A background worker's download re-enters through the command as `prefetched`.
    fn preview_and_refresh_run_on_prefetched_tables() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let r = s
            .execute(
                "data.webPreview",
                json!({
                    "url": "https://example.com",
                    "prefetched": [{"name": "prices", "rows": [["Sym", "Price"], ["A", "1"]]}]
                }),
            )
            .unwrap();
        assert_eq!(r["tables"][0]["name"], "prices");
        let r = s
            .execute(
                "data.fromWeb",
                json!({"url": "https://example.com", "table": "prices", "prefetched": [{"name": "prices", "rows": [["Sym", "Price"], ["A", "1"]]}]}),
            )
            .unwrap();
        assert_eq!(r["rows"], 2);
        // The prefetched pages keyed by URL refresh the saved connection.
        let r = s
            .execute(
                "data.webRefresh",
                json!({
                    "prefetchedByUrl": {
                        "https://example.com": {"tables": [{"name": "prices", "rows": [["Sym", "Price"], ["B", "2"]]}]}
                    }
                }),
            )
            .unwrap();
        assert_eq!(r["refreshed"], 1);
        assert_eq!(a1(&s, "A2"), "B");
    }
}
