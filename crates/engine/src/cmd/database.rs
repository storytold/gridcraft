//! Data › Get Data › From Database: run a query through the host's [`crate::datasource::DataSource`]
//! and put the result on a new sheet as a table. The query is kept with the table
//! ([`gridcraft_model::DbQuery`]), so Data › Refresh and Refresh All run it again in place.

use gridcraft_core::date::serial_from_ymd;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, DbQuery, MAX_DB_QUERIES, TableColumn, TotalsFn};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Result, always, bad, bool_param, cmd, edit, has_doc, str_param};
use crate::datasource::{DEFAULT_MAX_ROWS, DEFAULT_TIMEOUT_SECS, DRIVERS, DbOp, DbRequest, DbTable, DbValue, driver_for_connection, select_sql};
use crate::{EngineError, Session};

/// Longest text a cell holds (Excel's limit).
const MAX_TEXT: usize = 32_767;
/// Integers beyond this lose digits as numbers, so they are kept as text.
const MAX_EXACT_INT: i64 = 1 << 53;
/// Timeout of a look at tables or columns, in seconds.
const CATALOG_TIMEOUT_SECS: u64 = 15;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.fromDatabase",
            "From Database",
            ["Data", "Get & Transform Data"],
            None,
            "{driver: postgres|mysql|sqlite|odbc, connection: URL, file path or ODBC connection string (no password; TLS: PostgreSQL `sslmode=disable|prefer|require|verify-ca|verify-full`, MySQL `ssl-mode=DISABLED|PREFERRED|REQUIRED|VERIFY_CA|VERIFY_IDENTITY`, default prefer), query: SQL | table + schema? + columns?: [names] (blank = all), user?, password?, sheet?: new sheet name, maxRows?: 1..1048575, timeout?: seconds 1..3600} → imports the result into a new sheet as a table that Data › Refresh runs again",
            has_doc,
            from_database
        ),
        cmd!(query
            "data.databaseTables",
            "Database Tables",
            ["Data", "Get & Transform Data"],
            None,
            "{driver, connection, user?, password?} → [{schema, name, type}]: the database's tables and views",
            always,
            database_tables
        ),
        cmd!(query
            "data.databaseColumns",
            "Database Columns",
            ["Data", "Get & Transform Data"],
            None,
            "{driver, connection, table, schema?, user?, password?} → [{name, type}]: the table's columns",
            always,
            database_columns
        ),
        cmd!(
            "data.refresh",
            "Refresh",
            ["Data", "Queries & Connections"],
            None,
            "{table?: name (default: the table at the selection), password?, trust?: true enables the workbook's database queries read from the file} → runs the database query behind the table again and replaces its data; on a PivotTable, refreshes it. A query read from a file runs only once trusted ({needTrust: true} until then); without a needed password, asks for it ({needPassword: true})",
            has_doc,
            refresh
        ),
    ]
}

/// Driver, connection, user and password of a request, with the password remembered for the
/// session (never saved) so Refresh doesn't ask again.
struct Conn {
    driver: String,
    connection: String,
    user: Option<String>,
    password: Option<String>,
}

fn conn_params(s: &mut Session, id: &'static str, p: &Json) -> Result<Conn> {
    let driver = str_param(p, "driver").map(|d| d.trim().to_ascii_lowercase()).unwrap_or_default();
    if !DRIVERS.contains(&driver.as_str()) {
        return Err(bad(id, format!("`driver` must be one of {}", DRIVERS.join(", "))));
    }
    let connection = str_param(p, "connection").map(str::trim).filter(|c| !c.is_empty()).ok_or_else(|| bad(id, "missing `connection`"))?.to_string();
    // A `mysql://` URL sent to the PostgreSQL driver fails with an obscure parse error; say what is wrong.
    if let Some(named) = driver_for_connection(&connection)
        && named != driver
    {
        return Err(bad(id, format!("the connection is a {named} URL but `driver` is {driver}; choose {named}")));
    }
    check_connection(id, &driver, &connection)?;
    let user = str_param(p, "user").map(str::trim).filter(|x| !x.is_empty()).map(str::to_string);
    let key = password_key(&driver, &connection, user.as_deref());
    let password = match str_param(p, "password").filter(|x| !x.is_empty()) {
        Some(pw) => {
            s.db_passwords.insert(key, pw.to_string());
            Some(pw.to_string())
        }
        None => s.db_passwords.get(&key).cloned(),
    };
    Ok(Conn { driver, connection, user, password })
}

/// Refuses a connection before any driver sees it: one with a password written into it (the
/// connection is saved with the workbook and journaled), a SQLite path that reaches another
/// host (a UNC path sends the user's network credentials to it) or a URI, and an ODBC string
/// that loads a driver library or a file DSN by path.
fn check_connection(id: &'static str, driver: &str, connection: &str) -> Result<()> {
    if crate::datasource::redact(connection) != connection {
        return Err(bad(id, "the connection contains a password; leave it out and give it in `password` (it is never saved)"));
    }
    match driver {
        "sqlite" => {
            let path = connection.strip_prefix("sqlite://").or_else(|| connection.strip_prefix("sqlite:")).unwrap_or(connection);
            let uri = path.contains("://") || path.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("file:"));
            if path.starts_with("\\\\") || path.starts_with("//") || uri {
                return Err(bad(id, "a SQLite database must be a local file, not a network path or a URI"));
            }
        }
        "odbc" => {
            for (key, value) in connection.split(';').filter_map(|kv| kv.split_once('=')) {
                let key = key.trim().to_ascii_lowercase();
                if key == "filedsn" || key == "savefile" {
                    return Err(bad(
                        id,
                        format!("`{}` is not allowed in an ODBC connection; use DSN= or Driver= with a driver name", key.to_ascii_uppercase()),
                    ));
                }
                if key == "driver" && value.contains(['/', '\\']) {
                    return Err(bad(id, "an ODBC `Driver=` must name an installed driver, not a library path"));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn password_key(driver: &str, connection: &str, user: Option<&str>) -> String {
    format!("{driver}\n{connection}\n{}", user.unwrap_or(""))
}

fn source(s: &Session) -> Result<crate::datasource::DataSource> {
    s.data_source.clone().ok_or_else(|| EngineError::Other("Database connections are not available in this build.".into()))
}

fn run_op(s: &Session, c: &Conn, op: DbOp, query: String, max_rows: usize, timeout_secs: u64) -> Result<DbTable> {
    let req = DbRequest {
        driver: c.driver.clone(),
        connection: c.connection.clone(),
        password: c.password.clone(),
        user: c.user.clone(),
        query,
        max_rows,
        timeout_secs,
        op,
    };
    source(s)?(&req).map_err(EngineError::Other)
}

/// The text of a catalog cell.
fn cell_text(v: Option<&DbValue>) -> String {
    match v {
        Some(DbValue::Text(t)) => t.clone(),
        Some(DbValue::Null) | None => String::new(),
        Some(DbValue::Int(i)) => i.to_string(),
        Some(other) => format!("{other:?}"),
    }
}

fn database_tables(s: &mut Session, p: &Json) -> Result<Json> {
    let c = conn_params(s, "data.databaseTables", p)?;
    let t = run_op(s, &c, DbOp::Tables, String::new(), 100_000, CATALOG_TIMEOUT_SECS)?;
    Ok(Json::Array(
        t.rows.iter().map(|r| json!({"schema": cell_text(r.first()), "name": cell_text(r.get(1)), "type": cell_text(r.get(2))})).collect(),
    ))
}

fn database_columns(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "data.databaseColumns";
    let c = conn_params(s, ID, p)?;
    let table = str_param(p, "table").filter(|t| !t.is_empty()).ok_or_else(|| bad(ID, "missing `table`"))?.to_string();
    let schema = str_param(p, "schema").filter(|x| !x.is_empty()).map(str::to_string);
    let t = run_op(s, &c, DbOp::Columns { schema, table }, String::new(), 100_000, CATALOG_TIMEOUT_SECS)?;
    Ok(Json::Array(t.rows.iter().map(|r| json!({"name": cell_text(r.first()), "type": cell_text(r.get(1))})).collect()))
}

fn from_database(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "data.fromDatabase";
    if ["driver", "connection", "query", "table"].iter().all(|k| str_param(p, k).is_none_or(|v| v.trim().is_empty())) {
        s.ui_requests.push(crate::UiRequest::Dialog("fromDatabase".into(), json!({})));
        return Ok(Json::Null);
    }
    let c = conn_params(s, ID, p)?;
    let table_name = str_param(p, "table").map(str::trim).filter(|t| !t.is_empty());
    let query = match table_name {
        Some(t) => {
            let columns: Vec<String> = p
                .get("columns")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(Json::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            select_sql(&c.driver, str_param(p, "schema").map(str::trim), t, &columns)
        }
        None => str_param(p, "query").map(str::trim).filter(|q| !q.is_empty()).ok_or_else(|| bad(ID, "missing `query` or `table`"))?.to_string(),
    };
    let max_rows_param = p.get("maxRows").and_then(Json::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX).clamp(1, DEFAULT_MAX_ROWS));
    let max_rows = max_rows_param.unwrap_or(DEFAULT_MAX_ROWS);
    let timeout_param = p.get("timeout").and_then(Json::as_u64).map(|t| t.clamp(1, 3600));
    source(s)?;
    let table = run_op(s, &c, DbOp::Query, query.clone(), max_rows, timeout_param.unwrap_or(DEFAULT_TIMEOUT_SECS))?;
    let name = str_param(p, "sheet")
        .filter(|n| !n.trim().is_empty())
        .or(table_name)
        .map(|n| n.chars().take(31).collect::<String>())
        .unwrap_or_else(|| "Query".into());
    let mut r = put_table(s, &table, &name, max_rows)?;
    // Keep the query with the table it filled (the last table of the new sheet).
    let d = s.doc()?;
    if let Some(tid) = d.wb.active().and_then(|sh| sh.tables.last()).map(|t| t.id) {
        let q = DbQuery {
            table_id: tid,
            driver: c.driver,
            connection: c.connection,
            user: c.user,
            query,
            password_required: c.password.is_some(),
            max_rows: max_rows_param,
            timeout_secs: timeout_param,
            trusted: true,
        };
        edit(s, |cx| {
            cx.wb.db_queries.retain(|x| x.table_id != tid);
            if cx.wb.db_queries.len() < MAX_DB_QUERIES {
                cx.wb.db_queries.push(q);
            }
            Ok(())
        })?;
        r["table"] = json!(s.doc()?.wb.active().and_then(|sh| sh.tables.last()).map(|t| t.name.clone()));
    }
    Ok(r)
}

/// Header names: unique (case-insensitively) and non-empty, so a table keeps every column.
fn headers(table: &DbTable, w: usize) -> Vec<String> {
    let mut headers: Vec<String> = Vec::with_capacity(w);
    for (i, c) in table.columns.iter().take(w).enumerate() {
        let base = if c.trim().is_empty() { format!("Column{}", i + 1) } else { c.chars().take(255).collect() };
        let mut h = base.clone();
        let mut n = 2;
        while headers.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
            h = format!("{base}{n}");
            n += 1;
        }
        headers.push(h);
    }
    headers
}

/// Date/time number format per column, from its first non-null value.
fn date_formats(rows: &[Vec<DbValue>], w: usize) -> Vec<Option<&'static str>> {
    (0..w)
        .map(|ci| {
            rows.iter().filter_map(|row| row.get(ci)).find(|v| **v != DbValue::Null).and_then(|v| match v {
                DbValue::Date(..) => Some("yyyy-mm-dd"),
                DbValue::DateTime(..) => Some("yyyy-mm-dd hh:mm:ss"),
                DbValue::Time(..) => Some("hh:mm:ss"),
                _ => None,
            })
        })
        .collect()
}

/// Writes the header row and `rows` with their top-left at `at`, keeping each cell's style.
fn write_cells(sh: &mut gridcraft_model::Sheet, at: CellRef, headers: &[String], rows: &[Vec<DbValue>], sys: gridcraft_core::date::DateSystem) {
    let mut put = |r: u32, c: u32, value: Value| {
        let cell = CellRef::new(at.row + r, at.col + c);
        let style = sh.cell(cell).map(|x| x.style).unwrap_or_default();
        if value == Value::Empty && style == Default::default() {
            sh.remove_cell(cell);
        } else {
            sh.set_cell(cell, Cell { value, formula: None, style });
        }
    };
    for (ci, h) in headers.iter().enumerate() {
        put(0, ci as u32, Value::text(h.as_str()));
    }
    for (ri, row) in rows.iter().enumerate() {
        for ci in 0..headers.len() {
            put(ri as u32 + 1, ci as u32, row.get(ci).map_or(Value::Empty, |v| cell_value(v, sys)));
        }
    }
}

/// Applies the date/time formats of `formats` to the data rows below `at`.
fn format_dates(s: &mut Session, sheet: usize, at: CellRef, formats: &[Option<&str>], rows: u32) -> Result<()> {
    if rows == 0 {
        return Ok(());
    }
    let name = s.doc()?.wb.sheet(sheet).map(|sh| sh.name.clone()).unwrap_or_default();
    for (ci, f) in formats.iter().enumerate() {
        if let Some(code) = f {
            let col = at.col + ci as u32;
            let range = RangeRef::new(CellRef::new(at.row + 1, col), CellRef::new(at.row + rows, col));
            s.execute("home.numberFormat", json!({"range": format!("'{}'!{}", name.replace('\'', "''"), range.a1()), "code": code}))?;
        }
    }
    Ok(())
}

/// Writes `table` to a new sheet (header row + rows), formats date columns and makes it a table.
pub(crate) fn put_table(s: &mut Session, table: &DbTable, name: &str, max_rows: usize) -> Result<Json> {
    let w = table.columns.len().min(MAX_COLS as usize);
    if w == 0 {
        return Err(EngineError::Other("The query returned no columns.".into()));
    }
    let rows = &table.rows[..table.rows.len().min(max_rows).min(DEFAULT_MAX_ROWS)];
    let truncated = table.truncated || rows.len() < table.rows.len();
    let r = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    let doc = s.doc()?;
    let sheet = doc.wb.active_sheet.get();
    let sys = doc.wb.date_system;
    let headers = headers(table, w);
    let formats = date_formats(rows, w);
    edit(s, |cx| {
        write_cells(cx.sheet_mut(sheet)?, CellRef::new(0, 0), &headers, rows, sys);
        cx.structural = true;
        Ok(())
    })?;
    let last_row = rows.len() as u32;
    format_dates(s, sheet, CellRef::new(0, 0), &formats, last_row)?;
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(last_row, w as u32 - 1));
    let _ = s.execute("insert.table", json!({"range": range.a1(), "header": true}));
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, w as u32 - 1).a1()}));
    Ok(json!({"sheet": r["name"], "rows": rows.len(), "cols": w, "truncated": truncated}))
}

/// The table (sheet index, table index) `data.refresh` acts on: the named one, or the one at
/// the selection.
fn target_table(s: &Session, p: &Json) -> Result<Option<(usize, usize)>> {
    let d = s.doc()?;
    if let Some(n) = str_param(p, "table").filter(|n| !n.is_empty()) {
        for (si, sh) in d.wb.sheets.iter().enumerate() {
            if let Some(ti) = sh.tables.iter().position(|t| t.name.eq_ignore_ascii_case(n)) {
                return Ok(Some((si, ti)));
            }
        }
        return Err(bad("data.refresh", format!("no table named `{n}`")));
    }
    let si = d.wb.active_sheet.get();
    Ok(d.wb.sheet(si).and_then(|sh| sh.tables.iter().position(|t| t.range.contains(d.selection.active))).map(|ti| (si, ti)))
}

fn refresh(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "data.refresh";
    if bool_param(p, "trust") == Some(true) {
        trust_queries(s)?;
    }
    let Some((si, ti)) = target_table(s, p)? else {
        // Excel's Data › Refresh also refreshes PivotTables (the one at the selection, as `pivot.refresh` picks).
        if s.doc()?.wb.active().is_some_and(|sh| !sh.pivots.is_empty()) {
            return s.execute("pivot.refresh", json!({}));
        }
        return Err(bad(ID, "select a cell in a table imported from a database"));
    };
    let (tid, tname) = {
        let t = &s.doc()?.wb.sheets[si].tables[ti];
        (t.id, t.name.clone())
    };
    let Some(q) = s.doc()?.wb.db_queries.iter().find(|q| q.table_id == tid).cloned() else {
        return Err(bad(ID, format!("table `{tname}` was not imported from a database")));
    };
    refresh_query(s, &q, &tname, str_param(p, "password"))
}

/// Enables every database query of the active workbook (the user trusts the file's source).
fn trust_queries(s: &mut Session) -> Result<()> {
    let d = s.doc_mut()?;
    if d.wb.db_queries.iter().any(|q| !q.trusted) {
        for q in &mut std::sync::Arc::make_mut(&mut d.wb).db_queries {
            q.trusted = true;
        }
    }
    Ok(())
}

/// Asks the host to confirm the workbook's untrusted database queries before any runs: the
/// dialog lists each distinct connection (without passwords, with the user it connects as) and
/// the SQL it would run, and OK refreshes `tname` with `trust`.
fn ask_trust(s: &mut Session, tname: &str) -> Result<()> {
    let mut connections: Vec<String> = vec![];
    let mut queries: Vec<Json> = vec![];
    for q in s.doc()?.wb.db_queries.iter().filter(|q| !q.trusted) {
        // The drivers connect as `user` when it is set, whatever user the connection names.
        let user = q.user.as_deref().filter(|u| !u.is_empty()).map(|u| format!(" (user: {u})")).unwrap_or_default();
        let c = format!("{}: {}{user}", q.driver, crate::datasource::redact(&q.connection));
        let entry = json!({"connection": c, "sql": q.query});
        if !queries.contains(&entry) {
            queries.push(entry);
        }
        if !connections.contains(&c) {
            connections.push(c);
        }
    }
    s.ui_requests.push(crate::UiRequest::Dialog("dbTrust".into(), json!({"table": tname, "connections": connections, "queries": queries})));
    Ok(())
}

/// What refreshing a query needs before it can run.
enum Ready {
    /// Its connection and row limit.
    Run(Conn, usize),
    /// An answer from the user first (the host was asked); the command's response.
    Ask(Json),
}

/// Checks that `q` may run: a query read from a file and not yet trusted asks the host first
/// (`{needTrust: true}`), and so does one without a password it needs (`{needPassword: true}`).
fn ready(s: &mut Session, q: &DbQuery, tname: &str, password: Option<&str>) -> Result<Ready> {
    const ID: &str = "data.refresh";
    if !q.trusted {
        ask_trust(s, tname)?;
        return Ok(Ready::Ask(json!({"table": tname, "needTrust": true})));
    }
    let mut params = json!({"driver": q.driver, "connection": q.connection, "user": q.user});
    if let Some(pw) = password.filter(|x| !x.is_empty()) {
        params["password"] = json!(pw);
    }
    let c = conn_params(s, ID, &params)?;
    if q.password_required && c.password.is_none() {
        s.ui_requests.push(crate::UiRequest::Dialog(
            "dbPassword".into(),
            json!({"table": tname, "connection": crate::datasource::redact(&q.connection), "user": q.user}),
        ));
        return Ok(Ready::Ask(json!({"table": tname, "needPassword": true})));
    }
    Ok(Ready::Run(c, q.max_rows.unwrap_or(DEFAULT_MAX_ROWS).clamp(1, DEFAULT_MAX_ROWS)))
}

fn fetch(s: &Session, q: &DbQuery, c: &Conn, max_rows: usize) -> Result<DbTable> {
    run_op(s, c, DbOp::Query, q.query.clone(), max_rows, q.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS))
}

/// The error of a request the data source is still running in the background.
fn is_pending(e: &EngineError) -> bool {
    matches!(e, EngineError::Other(m) if m == crate::datasource::PENDING)
}

/// Runs `q` again and replaces the data of its table, or asks the host for what it needs first
/// (see [`ready`]).
fn refresh_query(s: &mut Session, q: &DbQuery, tname: &str, password: Option<&str>) -> Result<Json> {
    match ready(s, q, tname, password)? {
        Ready::Ask(r) => Ok(r),
        Ready::Run(c, max_rows) => {
            let result = fetch(s, q, &c, max_rows)?;
            refill(s, q.table_id, tname, &result, max_rows)
        }
    }
}

/// Replaces a table's header and data with `table`, growing or shrinking it from its top-left
/// cell. The total row, when on, moves below the new data.
fn refill(s: &mut Session, tid: u32, tname: &str, table: &DbTable, max_rows: usize) -> Result<Json> {
    let w = table.columns.len().min(MAX_COLS as usize);
    if w == 0 {
        return Err(EngineError::Other("The query returned no columns.".into()));
    }
    let d = s.doc()?;
    let sys = d.wb.date_system;
    let (si, ti) =
        d.wb.sheets
            .iter()
            .enumerate()
            .find_map(|(si, sh)| sh.tables.iter().position(|t| t.id == tid).map(|ti| (si, ti)))
            .ok_or_else(|| bad("data.refresh", format!("table `{tname}` no longer exists")))?;
    let sh = &d.wb.sheets[si];
    let old = sh.tables[ti].range;
    let totals = sh.tables[ti].totals_row;
    let at = old.start;
    let room = (MAX_ROWS - at.row - 1 - u32::from(totals)) as usize;
    let rows = &table.rows[..table.rows.len().min(max_rows).min(room)];
    let truncated = table.truncated || rows.len() < table.rows.len();
    let w = w.min((MAX_COLS - at.col) as usize);
    let new = RangeRef::new(at, CellRef::new(at.row + rows.len() as u32 + u32::from(totals), at.col + w as u32 - 1));
    // Never write over content next to the table.
    for r in new.start.row..=new.end.row {
        for c in new.start.col..=new.end.col {
            let cell = CellRef::new(r, c);
            if !old.contains(cell) && sh.cell(cell).is_some_and(|x| x.value != Value::Empty || x.formula.is_some()) {
                return Err(EngineError::Other(format!(
                    "Refreshing `{tname}` would overwrite {} next to it; make room for the new rows and columns.",
                    cell.a1()
                )));
            }
            if !old.contains(cell) && sh.tables.iter().enumerate().any(|(i, t)| i != ti && t.range.contains(cell)) {
                return Err(EngineError::Other(format!("Refreshing `{tname}` would overlap another table at {}.", cell.a1())));
            }
        }
    }
    let headers = headers(table, w);
    let formats = date_formats(rows, w);
    edit(s, |cx| {
        let sheet = cx.sheet_mut(si)?;
        let totals_cells: Vec<Option<Cell>> =
            (0..w as u32).map(|c| if totals { sheet.cell(CellRef::new(old.end.row, at.col + c)).cloned() } else { None }).collect();
        // Clear the old content, keeping cell formats.
        for r in old.start.row..=old.end.row {
            for c in old.start.col..=old.end.col {
                let cell = CellRef::new(r, c);
                match sheet.cell(cell).map(|x| x.style) {
                    Some(style) if style != Default::default() => sheet.set_cell(cell, Cell { value: Value::Empty, formula: None, style }),
                    _ => {
                        sheet.remove_cell(cell);
                    }
                }
            }
        }
        write_cells(sheet, at, &headers, rows, sys);
        if totals {
            for (c, cell) in totals_cells.into_iter().enumerate() {
                if let Some(cell) = cell {
                    sheet.set_cell(CellRef::new(new.end.row, at.col + c as u32), cell);
                }
            }
        }
        let t = sheet.tables.get_mut(ti).ok_or_else(|| bad("table", "gone"))?;
        let mut cols = Vec::with_capacity(w);
        for h in &headers {
            cols.push(t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(h)).cloned().unwrap_or(TableColumn {
                name: h.clone(),
                totals: TotalsFn::None,
                totals_label: None,
                formula: None,
                unique_name: None,
                xml: None,
            }));
        }
        t.columns = cols;
        t.range = new;
        cx.structural = true;
        Ok(())
    })?;
    format_dates(s, si, at, &formats, rows.len() as u32)?;
    Ok(json!({"table": tname, "rows": rows.len(), "cols": w, "truncated": truncated}))
}

/// What Refresh All did with the workbook's database queries.
pub(crate) struct RefreshAll {
    pub refreshed: usize,
    pub errors: Vec<Json>,
    /// Tables that wait for a password (the host was asked for the first one).
    pub need_password: Vec<String>,
    /// Tables whose queries came from the file and are not trusted yet (the host was asked once).
    pub need_trust: Vec<String>,
}

/// Refreshes every database query of the workbook (Data › Refresh All). `trust` enables the
/// queries read from the file first; without it, they wait and the host is asked once. Every
/// query runs before any table changes, so a request still running in the background
/// ([`crate::datasource::PENDING`]) fails the command with nothing changed (and the other
/// requests started meanwhile).
pub(crate) fn refresh_all(s: &mut Session, trust: bool) -> Result<RefreshAll> {
    if trust {
        trust_queries(s)?;
    }
    let queries = s.doc()?.wb.db_queries.clone();
    let asked_before = s.ui_requests.len();
    let mut out = RefreshAll { refreshed: 0, errors: vec![], need_password: vec![], need_trust: vec![] };
    let mut fetched = vec![];
    let mut pending = false;
    for q in queries {
        let Some(tname) = s.doc()?.wb.sheets.iter().flat_map(|sh| sh.tables.iter()).find(|t| t.id == q.table_id).map(|t| t.name.clone()) else {
            continue;
        };
        if !q.trusted {
            if out.need_trust.is_empty() {
                ask_trust(s, &tname)?;
                if let Some(crate::UiRequest::Dialog(_, p)) = s.ui_requests.last_mut() {
                    p["then"] = json!("refreshAll");
                }
            }
            out.need_trust.push(tname);
            continue;
        }
        let asked = s.ui_requests.len();
        match ready(s, &q, &tname, None) {
            Ok(Ready::Ask(_)) => {
                // Ask for one password at a time (and not while the trust question is open);
                // after it, Refresh All carries on.
                if out.need_password.is_empty() && out.need_trust.is_empty() {
                    if let Some(crate::UiRequest::Dialog(_, p)) = s.ui_requests.last_mut() {
                        p["then"] = json!("refreshAll");
                    }
                } else {
                    s.ui_requests.truncate(asked);
                }
                out.need_password.push(tname);
            }
            Ok(Ready::Run(c, max_rows)) => match fetch(s, &q, &c, max_rows) {
                Err(e) if is_pending(&e) => pending = true,
                r => fetched.push((q, tname, max_rows, r)),
            },
            Err(e) => out.errors.push(json!({"table": tname, "error": e.to_string()})),
        }
    }
    if pending {
        s.ui_requests.truncate(asked_before);
        return Err(EngineError::Other(crate::datasource::PENDING.into()));
    }
    for (q, tname, max_rows, r) in fetched {
        match r.and_then(|t| refill(s, q.table_id, &tname, &t, max_rows)) {
            Ok(_) => out.refreshed += 1,
            Err(e) => out.errors.push(json!({"table": tname, "error": e.to_string()})),
        }
    }
    Ok(out)
}

fn cell_value(v: &DbValue, sys: gridcraft_core::date::DateSystem) -> Value {
    let text = |t: &str| Value::text(t.chars().take(MAX_TEXT).collect::<String>());
    let time = |h: u32, m: u32, sec: f64| (f64::from(h) * 3600.0 + f64::from(m) * 60.0 + sec) / 86_400.0;
    match v {
        DbValue::Null => Value::Empty,
        DbValue::Bool(b) => Value::Bool(*b),
        DbValue::Int(i) if (-MAX_EXACT_INT..=MAX_EXACT_INT).contains(i) => Value::number(*i as f64),
        DbValue::Int(i) => text(&i.to_string()),
        DbValue::Float(f) if f.is_finite() => Value::number(*f),
        DbValue::Float(f) => text(&f.to_string()),
        DbValue::Text(t) => text(t),
        DbValue::Date(y, m, d) => {
            serial_from_ymd(sys, i64::from(*y), i64::from(*m), i64::from(*d)).map_or_else(|| text(&format!("{y:04}-{m:02}-{d:02}")), Value::number)
        }
        DbValue::DateTime(y, mo, d, h, mi, sec) => serial_from_ymd(sys, i64::from(*y), i64::from(*mo), i64::from(*d))
            .map_or_else(|| text(&format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{sec:02.0}")), |day| Value::number(day + time(*h, *mi, *sec))),
        DbValue::Time(h, m, sec) => Value::number(time(*h, *m, *sec)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::datasource::{DataSource, DbOp};

    fn session_with(table: DbTable) -> (Session, Arc<Mutex<Vec<DbRequest>>>) {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let source: DataSource = Arc::new(move |r: &DbRequest| {
            log.lock().unwrap().push(r.clone());
            Ok(table.clone())
        });
        s.data_source = Some(source);
        (s, seen)
    }

    fn sample() -> DbTable {
        DbTable {
            columns: vec!["id".into(), "name".into(), "born".into(), "".into(), "name".into()],
            rows: vec![
                vec![DbValue::Int(1), DbValue::Text("00123".into()), DbValue::Date(2024, 1, 5), DbValue::Bool(true), DbValue::Null],
                vec![DbValue::Int(i64::MAX), DbValue::Text("b".into()), DbValue::Null, DbValue::Float(f64::NAN), DbValue::Float(2.5)],
            ],
            truncated: false,
        }
    }

    fn text_at(s: &Session, a1: &str) -> String {
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        crate::display::cell_text(&d.wb, sh, CellRef::parse(a1).unwrap())
    }

    #[test]
    fn imports_rows_into_a_new_table_sheet() {
        let (mut s, seen) = session_with(sample());
        let r = s
            .execute(
                "data.fromDatabase",
                json!({"driver": "SQLite", "connection": "/x.db", "query": "select 1", "password": "pw", "sheet": "People"}),
            )
            .unwrap();
        assert_eq!(r["sheet"], "People");
        assert_eq!(r["rows"], 2);
        assert_eq!(r["cols"], 5);
        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.driver, "sqlite");
        assert_eq!(req.password.as_deref(), Some("pw"));
        let sh = s.active().unwrap().wb.active().unwrap();
        assert_eq!(sh.name, "People");
        assert_eq!(sh.tables.len(), 1);
        // Text stays text, even when it looks like a number.
        assert_eq!(sh.value(CellRef::parse("B2").unwrap()), Value::text("00123"));
        assert_eq!(text_at(&s, "C2"), "2024-01-05");
        // Blank and duplicate headers get unique names.
        assert_eq!(text_at(&s, "D1"), "Column4");
        assert_eq!(text_at(&s, "E1"), "name2");
        // Integers beyond 2^53 keep every digit.
        assert_eq!(sh.value(CellRef::parse("A3").unwrap()), Value::text(i64::MAX.to_string()));
        assert_eq!(sh.value(CellRef::parse("E3").unwrap()), Value::number(2.5));
    }

    #[test]
    fn password_is_left_out_of_the_journal() {
        let (mut s, _) = session_with(sample());
        s.execute("data.fromDatabase", json!({"driver": "postgres", "connection": "postgres://u@h/db", "query": "select 1", "password": "secret"}))
            .unwrap();
        let journaled = s.journal.iter().find(|(id, _)| id == "data.fromDatabase").unwrap();
        assert!(journaled.1.get("password").is_none());
    }

    #[test]
    fn max_rows_truncates_and_reports_it() {
        let (mut s, seen) = session_with(sample());
        let r = s.execute("data.fromDatabase", json!({"driver": "mysql", "connection": "mysql://u@h/db", "query": "q", "maxRows": 1})).unwrap();
        assert_eq!(r["rows"], 1);
        assert_eq!(r["truncated"], true);
        assert_eq!(seen.lock().unwrap()[0].max_rows, 1);
    }

    #[test]
    fn errors_without_host_bad_driver_or_driver_failure() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let p = json!({"driver": "sqlite", "connection": "/x.db", "query": "select 1"});
        assert!(s.execute("data.fromDatabase", p.clone()).unwrap_err().to_string().contains("not available"));
        assert!(s.execute("data.fromDatabase", json!({"driver": "oracle", "connection": "x", "query": "q"})).is_err());
        assert!(s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db"})).is_err());
        let e = s.execute("data.fromDatabase", json!({"driver": "postgres", "connection": "mysql://u@h/db", "query": "q"})).unwrap_err();
        assert!(e.to_string().contains("mysql URL"));
        s.data_source = Some(Arc::new(|_: &DbRequest| Err("connection refused".to_string())));
        assert!(s.execute("data.fromDatabase", p).unwrap_err().to_string().contains("connection refused"));
        let (mut s, _) = session_with(DbTable::default());
        assert!(s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q"})).is_err());
    }

    #[test]
    fn empty_params_open_the_dialog_but_headless_calls_fail() {
        let (mut s, seen) = session_with(sample());
        s.execute("data.fromDatabase", json!({})).unwrap();
        assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "fromDatabase")));
        assert!(s.execute_headless("data.fromDatabase", json!({})).is_err());
        assert!(seen.lock().unwrap().is_empty());
    }

    /// A session, the answer its data source gives next, and the requests it has received.
    type Swappable = (Session, Arc<Mutex<DbTable>>, Arc<Mutex<Vec<DbRequest>>>);

    /// A session whose data source answers with whatever `next` holds and records each request.
    fn swappable() -> Swappable {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let next = Arc::new(Mutex::new(DbTable::default()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (n, log) = (next.clone(), seen.clone());
        s.data_source = Some(Arc::new(move |r: &DbRequest| {
            log.lock().unwrap().push(r.clone());
            Ok(n.lock().unwrap().clone())
        }));
        (s, next, seen)
    }

    fn ints(cols: &[&str], rows: &[&[i64]]) -> DbTable {
        DbTable {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            rows: rows.iter().map(|r| r.iter().map(|v| DbValue::Int(*v)).collect()).collect(),
            truncated: false,
        }
    }

    #[test]
    fn table_mode_selects_the_chosen_columns_and_names_the_sheet() {
        let (mut s, next, seen) = swappable();
        *next.lock().unwrap() = ints(&["id", "qty"], &[&[1, 2]]);
        let r = s
            .execute(
                "data.fromDatabase",
                json!({"driver": "mysql", "connection": "mysql://h:3306/db", "table": "orders", "schema": "db", "columns": ["id", "qty"]}),
            )
            .unwrap();
        assert_eq!(seen.lock().unwrap()[0].query, "SELECT `id`, `qty` FROM `db`.`orders`");
        assert_eq!(r["sheet"], "orders");
        let wb = &s.active().unwrap().wb;
        let q = &wb.db_queries[0];
        assert_eq!(q.table_id, wb.active().unwrap().tables[0].id);
        assert_eq!(q.query, "SELECT `id`, `qty` FROM `db`.`orders`");
        assert!(!q.password_required);
    }

    #[test]
    fn catalog_commands_list_tables_and_columns() {
        let (mut s, next, seen) = swappable();
        *next.lock().unwrap() = DbTable {
            columns: vec!["schema".into(), "name".into(), "type".into()],
            rows: vec![vec![DbValue::Text("public".into()), DbValue::Text("t".into()), DbValue::Text("BASE TABLE".into())]],
            truncated: false,
        };
        let r = s.execute("data.databaseTables", json!({"driver": "postgres", "connection": "postgres://h/db"})).unwrap();
        assert_eq!(r, json!([{"schema": "public", "name": "t", "type": "BASE TABLE"}]));
        assert_eq!(seen.lock().unwrap()[0].op, DbOp::Tables);
        *next.lock().unwrap() = DbTable {
            columns: vec!["name".into(), "type".into()],
            rows: vec![vec![DbValue::Text("id".into()), DbValue::Text("integer".into())]],
            truncated: false,
        };
        let r = s
            .execute("data.databaseColumns", json!({"driver": "postgres", "connection": "postgres://h/db", "table": "t", "schema": "public"}))
            .unwrap();
        assert_eq!(r, json!([{"name": "id", "type": "integer"}]));
        assert_eq!(seen.lock().unwrap()[1].op, DbOp::Columns { schema: Some("public".into()), table: "t".into() });
        // Catalog looks don't change the workbook or the journal.
        assert!(s.journal.iter().all(|(id, _)| !id.starts_with("data.database")));
    }

    #[test]
    fn refresh_grows_and_shrinks_the_table_in_place() {
        let (mut s, next, _) = swappable();
        *next.lock().unwrap() = ints(&["a", "b"], &[&[1, 2], &[3, 4]]);
        s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q", "sheet": "Data"})).unwrap();
        s.execute("range.setValues", json!({"range": "E1", "values": [["note"]]})).unwrap();
        *next.lock().unwrap() = ints(&["a", "b", "c"], &[&[5, 6, 7], &[8, 9, 10], &[11, 12, 13]]);
        let r = s.execute("data.refresh", json!({})).unwrap();
        assert_eq!(r["rows"], 3);
        let t = s.active().unwrap().wb.active().unwrap().tables[0].clone();
        assert_eq!(t.range.a1(), "A1:C4");
        assert_eq!(t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(text_at(&s, "C4"), "13");
        assert_eq!(text_at(&s, "E1"), "note");
        *next.lock().unwrap() = ints(&["a"], &[&[1]]);
        s.execute("data.refresh", json!({"table": t.name})).unwrap();
        let t = s.active().unwrap().wb.active().unwrap().tables[0].clone();
        assert_eq!(t.range.a1(), "A1:A2");
        assert_eq!(text_at(&s, "A3"), "");
        assert_eq!(text_at(&s, "B2"), "");
        // Content next to the table is never overwritten.
        s.execute("range.setValues", json!({"range": "A5", "values": [["keep"]]})).unwrap();
        *next.lock().unwrap() = ints(&["a"], &[&[1], &[2], &[3], &[4], &[5]]);
        let e = s.execute("data.refresh", json!({"table": t.name})).unwrap_err();
        assert!(e.to_string().contains("A5"), "{e}");
        assert_eq!(text_at(&s, "A5"), "keep");
    }

    #[test]
    fn refresh_asks_for_a_password_it_does_not_know() {
        let (mut s, next, seen) = swappable();
        *next.lock().unwrap() = ints(&["a"], &[&[1]]);
        s.execute("data.fromDatabase", json!({"driver": "mysql", "connection": "mysql://h/db", "user": "bob", "password": "pw", "query": "q"}))
            .unwrap();
        // Remembered for the session.
        s.execute("data.refresh", json!({})).unwrap();
        assert_eq!(seen.lock().unwrap()[1].password.as_deref(), Some("pw"));
        // A new session (the file reopened) asks.
        s.db_passwords.clear();
        s.take_ui_requests();
        let r = s.execute("data.refresh", json!({})).unwrap();
        assert_eq!(r["needPassword"], true);
        assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "dbPassword")));
        assert_eq!(seen.lock().unwrap().len(), 2);
        s.execute("data.refresh", json!({"password": "pw2"})).unwrap();
        assert_eq!(seen.lock().unwrap()[2].password.as_deref(), Some("pw2"));
        assert!(s.journal.iter().all(|(_, p)| p.get("password").is_none()));
        // Refresh All reports what waits for a password.
        s.db_passwords.clear();
        let r = s.execute("data.refreshAll", json!({})).unwrap();
        assert_eq!(r["needPassword"], json!([s.active().unwrap().wb.active().unwrap().tables[0].name.clone()]));
    }

    #[test]
    fn refresh_all_runs_every_query_and_refresh_needs_an_imported_table() {
        let (mut s, next, seen) = swappable();
        *next.lock().unwrap() = ints(&["a"], &[&[1]]);
        s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q1"})).unwrap();
        s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q2"})).unwrap();
        let r = s.execute("data.refreshAll", json!({})).unwrap();
        assert_eq!(r["refreshed"], 2);
        assert_eq!(seen.lock().unwrap().iter().map(|q| q.query.as_str()).collect::<Vec<_>>(), ["q1", "q2", "q1", "q2"]);
        s.execute("home.insertSheet", json!({})).unwrap();
        assert!(s.execute("data.refresh", json!({})).is_err());
        s.execute("range.setValues", json!({"range": "A1", "values": [["h"], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        assert!(s.execute("data.refresh", json!({})).unwrap_err().to_string().contains("not imported"));
    }

    /// Marks the workbook's queries as read from a file, as opening an .xlsx leaves them.
    fn as_if_opened(s: &mut Session) {
        for q in &mut Arc::make_mut(&mut s.doc_mut().unwrap().wb).db_queries {
            q.trusted = false;
        }
    }

    #[test]
    fn security_warning_names_the_user_each_query_connects_as() {
        let (mut s, next, _) = swappable();
        *next.lock().unwrap() = ints(&["a"], &[&[1]]);
        let url = "postgres://reader@h/db";
        for user in [Some("admin"), Some("other"), None, None] {
            let mut p = json!({"driver": "postgres", "connection": url, "query": "q"});
            if let Some(u) = user {
                p["user"] = json!(u);
            }
            s.execute("data.fromDatabase", p).unwrap();
        }
        as_if_opened(&mut s);
        s.take_ui_requests();
        s.execute("data.refresh", json!({})).unwrap();
        let asked = s.take_ui_requests();
        let Some(crate::UiRequest::Dialog(_, p)) = asked.first() else { panic!("no dialog: {asked:?}") };
        // The user field overrides the URL's user, so it is shown; queries that differ only by
        // user stay apart, identical ones are listed once.
        assert_eq!(
            p["queries"],
            json!([
                {"connection": "postgres: postgres://reader@h/db (user: admin)", "sql": "q"},
                {"connection": "postgres: postgres://reader@h/db (user: other)", "sql": "q"},
                {"connection": "postgres: postgres://reader@h/db", "sql": "q"},
            ])
        );
    }

    #[test]
    fn queries_from_a_file_run_only_once_trusted() {
        let (mut s, next, seen) = swappable();
        *next.lock().unwrap() = ints(&["a"], &[&[1]]);
        s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q1"})).unwrap();
        s.execute("data.fromDatabase", json!({"driver": "postgres", "connection": "postgres://u@h/db", "query": "q2"})).unwrap();
        as_if_opened(&mut s);
        s.take_ui_requests();
        // Refresh asks once and runs nothing.
        let r = s.execute("data.refresh", json!({})).unwrap();
        assert_eq!(r["needTrust"], true);
        let asked = s.take_ui_requests();
        let Some(crate::UiRequest::Dialog(name, p)) = asked.first() else { panic!("no dialog: {asked:?}") };
        assert_eq!(name, "dbTrust");
        assert_eq!(p["connections"], json!(["sqlite: /x.db", "postgres: postgres://u@h/db"]));
        assert_eq!(p["queries"], json!([{"connection": "sqlite: /x.db", "sql": "q1"}, {"connection": "postgres: postgres://u@h/db", "sql": "q2"}]));
        // Refresh All waits for the same answer, asks once, and still refreshes nothing.
        let r = s.execute("data.refreshAll", json!({})).unwrap();
        assert_eq!(r["refreshed"], 0);
        assert_eq!(r["needTrust"].as_array().map(Vec::len), Some(2));
        assert_eq!(s.take_ui_requests().iter().filter(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "dbTrust")).count(), 1);
        assert_eq!(seen.lock().unwrap().len(), 2);
        // Programmatic calls fail instead of waiting for a dialog.
        assert!(s.execute_headless("data.refresh", json!({})).unwrap_err().to_string().contains("trust"));
        assert_eq!(seen.lock().unwrap().len(), 2);
        // `trust` enables every query of the workbook.
        let r = s.execute("data.refreshAll", json!({"trust": true})).unwrap();
        assert_eq!(r["refreshed"], 2);
        s.execute("data.refresh", json!({})).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 5);
    }

    #[test]
    fn connections_with_passwords_network_paths_or_library_paths_are_refused() {
        let (mut s, seen) = session_with(sample());
        for (driver, connection) in [
            ("postgres", "postgres://u:secret@h/db"),
            ("postgres", "host=h user=u password=secret"),
            ("postgres", "postgres://u@h/db?password=secret"),
            ("postgres", "postgres://u@h/db?sslmode=disable&password=secret"),
            ("odbc", "DSN=sales;UID=u;PWD=secret"),
            ("sqlite", "\\\\attacker\\share\\x.db"),
            ("sqlite", "//attacker/share/x.db"),
            ("sqlite", "sqlite:////attacker/share/x.db"),
            ("sqlite", "file:x.db?vfs=unix-none"),
            ("odbc", "Driver=/tmp/evil.so;Server=h"),
            ("odbc", "DRIVER={C:\\evil\\evil.dll}"),
            ("odbc", "FILEDSN=/tmp/x.dsn"),
            ("odbc", "DSN=sales;SAVEFILE=/tmp/x.dsn"),
        ] {
            let r = s.execute("data.fromDatabase", json!({"driver": driver, "connection": connection, "query": "select 1"}));
            assert!(r.is_err(), "{driver} {connection} was accepted");
        }
        assert!(seen.lock().unwrap().is_empty());
        // Ordinary connections still work.
        for (driver, connection) in [
            ("sqlite", "/home/u/x.db"),
            ("sqlite", "sqlite:///home/u/x.db"),
            ("sqlite", "C:\\data\\x.db"),
            ("odbc", "DSN=sales;UID=u"),
            ("odbc", "Driver={PostgreSQL Unicode};Server=h;Database=d"),
            ("postgres", "postgres://u@h:5432/db"),
        ] {
            s.execute("data.fromDatabase", json!({"driver": driver, "connection": connection, "query": "select 1"})).unwrap();
        }
    }

    #[test]
    fn a_request_still_running_changes_nothing() {
        let (mut s, next, seen) = swappable();
        *next.lock().unwrap() = ints(&["a"], &[&[1]]);
        s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q1", "sheet": "One"})).unwrap();
        s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q2", "sheet": "Two"})).unwrap();
        let sheets = s.active().unwrap().wb.sheets.len();
        let undo = s.active().unwrap().undo.len();
        // The host answers "still running" to every request.
        let inner = s.data_source.clone().unwrap();
        s.data_source = Some(Arc::new(|_: &DbRequest| Err(crate::datasource::PENDING.to_string())));
        *next.lock().unwrap() = ints(&["a"], &[&[1], &[2]]);
        for (id, p) in [
            ("data.refreshAll", json!({})),
            ("data.refresh", json!({})),
            ("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q3"})),
        ] {
            let e = s.execute(id, p).unwrap_err();
            assert_eq!(e.to_string(), crate::datasource::PENDING, "{id}");
        }
        assert_eq!(s.active().unwrap().wb.sheets.len(), sheets);
        assert_eq!(s.active().unwrap().undo.len(), undo, "no table changed");
        assert!(s.take_ui_requests().is_empty());
        // The answers are in: the same command runs to the end.
        s.data_source = Some(inner);
        let r = s.execute("data.refreshAll", json!({})).unwrap();
        assert_eq!(r["refreshed"], 2);
        assert_eq!(seen.lock().unwrap().len(), 4);
    }
}
