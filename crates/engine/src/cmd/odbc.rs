use super::*;
use crate::odbc::Request;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{QuerySource, Sheet, Table, TableColumn, TotalsFn};
use gridcraft_odbc::{Connection, DataSet, Limits};
use serde_json::{Value as Json, json};
use std::{io::Write, sync::Arc};

fn other(message: impl Into<String>) -> EngineError {
    EngineError::Other(message.into())
}

pub fn specs() -> Vec<CommandSpec> {
    let mut apply = cmd!(query "data.odbcApply", "Apply ODBC Results", [], None, "{jobId, replace?: bool} → one undoable edit", has_doc, apply);
    apply.undoable = true;
    vec![
        cmd!(query "data.odbcSources", "ODBC Sources", ["Data", "Get Data"], None, "{} → {dataSources, drivers}", always, sources),
        cmd!(query "data.odbcTest", "Test ODBC Connection", ["Data", "Get Data"], None, "{dsn | connectionString, user?, password?} → {jobId}", has_doc, |s, p| submit(s, p, "test")),
        cmd!(query "data.odbcPreview", "Preview ODBC Query", ["Data", "Get Data"], None, "{dsn | connectionString, user?, password?, sql} → {jobId}", has_doc, |s, p| submit(s, p, "preview")),
        cmd!(query "data.fromOdbc", "Load ODBC Query", ["Data", "Get Data"], None, "{dsn | connectionString, user?, password?, sql, maxRows?: 1..100000} → {jobId}", has_doc, |s, p| submit(s, p, "load")),
        cmd!(query "data.odbcStatus", "ODBC Job Status", [], None, "{jobId} → {state, error?, rows?, columns?, truncated}", always, status),
        cmd!(query "data.odbcCancel", "Cancel ODBC Job", [], None, "{jobId}", always, cancel),
        apply,
        cmd!(query "data.refreshQuery", "Refresh ODBC Query", ["Data", "Queries & Connections"], None, "{table, dsn? | connectionString?, user?, password?, replace?: bool} → {jobId}", has_doc, refresh),
    ]
}

fn sources(s: &mut Session, _: &Json) -> Result<Json> {
    serde_json::to_value(s.odbc.sources().map_err(|e| other(e.to_string()))?).map_err(|e| other(e.to_string()))
}

fn settings(p: &Json, mut base: Connection) -> Result<Connection> {
    for key in ["dsn", "connectionString", "user", "password"] {
        if p.get(key).is_some_and(|v| !v.is_string()) {
            return Err(bad("ODBC", format!("`{key}` must be text")));
        }
        let limit = match key {
            "dsn" => 255,
            "user" => 512,
            _ => 16384,
        };
        if str_param(p, key).is_some_and(|s| s.len() > limit) {
            return Err(bad("ODBC", format!("`{key}` is too long")));
        }
    }
    if p.get("dsn").is_some() || p.get("connectionString").is_some() {
        base.dsn = str_param(p, "dsn").map(str::to_string);
        base.connection_string = str_param(p, "connectionString").map(str::to_string);
    }
    if let Some(user) = str_param(p, "user") {
        base.user = user.into();
    }
    if let Some(password) = str_param(p, "password") {
        base.password = password.into();
    }
    base.validate().map_err(|e| bad("ODBC", e.to_string()))?;
    Ok(base)
}

fn limits(p: &Json, preview: bool) -> Result<Limits> {
    let mut value = Limits::default();
    if let Some(rows) = p.get("maxRows") {
        value.rows = rows.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("ODBC", "`maxRows` must be a positive integer"))?;
    }
    value = value.validate().map_err(|e| bad("ODBC", e.to_string()))?;
    if preview {
        value.rows = value.rows.min(100);
        value.bytes = 2 * 1024 * 1024;
    }
    value.validate().map_err(|e| bad("ODBC", e.to_string()))
}

fn submit(s: &mut Session, p: &Json, kind: &str) -> Result<Json> {
    if kind != "load"
        && let Some(table) = str_param(p, "table")
    {
        let mut request = refresh_request(s, p, table)?;
        request.kind = kind.into();
        if kind == "preview" {
            request.limits.rows = 100;
            request.limits.bytes = 2 * 1024 * 1024;
        }
        return start(s, request);
    }
    let doc = s.doc()?;
    let sql = if kind == "test" {
        String::new()
    } else {
        let sql = str_param(p, "sql").ok_or_else(|| bad("ODBC", "`sql` is required"))?;
        gridcraft_odbc::validate_query(sql).map_err(|e| bad("ODBC", e.to_string()))?;
        sql.to_string()
    };
    let request = Request {
        uid: doc.uid,
        revision: doc.revision,
        kind: kind.into(),
        connection: settings(p, Connection::default())?,
        sql,
        limits: limits(p, kind == "preview")?,
        table: None,
        connection_ref: None,
        replace: false,
        batch: vec![],
    };
    start(s, request)
}

fn start(s: &mut Session, request: Request) -> Result<Json> {
    retain_profiles(s);
    let id = s.odbc.submit(request).map_err(other)?;
    Ok(json!({"jobId": id, "state": "running"}))
}

fn retain_profiles(s: &mut Session) {
    let references =
        s.documents()
            .iter()
            .flat_map(|doc| {
                doc.wb.sheets.iter().flat_map(|sheet| &sheet.tables).filter_map(move |table| {
                    table.query.as_ref().filter(|q| q.connection_ref.len() <= 128).map(|q| (doc.uid, q.connection_ref.clone()))
                })
            })
            .collect();
    s.odbc.retain_profiles(&references);
}

fn job_id(p: &Json) -> Result<&str> {
    str_param(p, "jobId").filter(|s| s.len() <= 128).ok_or_else(|| bad("ODBC", "`jobId` is required"))
}

fn status(s: &mut Session, p: &Json) -> Result<Json> {
    let info = s.odbc.status(job_id(p)?).ok_or_else(|| bad("ODBC", "ODBC job was not found."))?;
    serde_json::to_value(info).map_err(|e| other(e.to_string()))
}

fn cancel(s: &mut Session, p: &Json) -> Result<Json> {
    s.odbc.cancel(job_id(p)?).map_err(other)?;
    Ok(json!({"cancellationRequested": true}))
}

fn apply(s: &mut Session, p: &Json) -> Result<Json> {
    let id = job_id(p)?.to_string();
    let doc = s.doc()?;
    if s.odbc.status(&id).is_some_and(|j| j.kind == "refreshAll") {
        let items = s.odbc.ready_batch(&id, doc.uid, doc.revision).map_err(other)?;
        let mut applied_tables = Vec::new();
        let result = edit(s, |cx| {
            let mut refreshed = 0;
            let mut errors = Vec::new();
            let mut truncated = Vec::new();
            let mut replace = false;
            for item in &items {
                replace = bool_param(p, "replace").unwrap_or(item.request.replace);
                let snapshot = cx.wb.clone();
                let structural = cx.structural;
                let result = match &item.data {
                    Ok(data) => refresh_table(cx, &id, &item.request, data, replace),
                    Err(error) => Err(other(error.clone())),
                };
                match result {
                    Ok(result) => {
                        refreshed += 1;
                        if let Some(table) = &item.request.table {
                            applied_tables.push(table.clone());
                        }
                        if result["truncated"] == true {
                            truncated.push(item.request.table.clone());
                        }
                    }
                    Err(error) => {
                        cx.wb = snapshot;
                        cx.structural = structural;
                        errors.push(json!({"query": item.request.table, "error": error.to_string()}));
                    }
                }
            }
            let pivots = super::pivot::refresh_pivots(cx, replace);
            let pivot_count = pivots["refreshed"].as_u64().unwrap_or(0);
            if let Some(pivot_errors) = pivots["errors"].as_array() {
                errors.extend(pivot_errors.iter().cloned());
            }
            Ok(
                json!({"refreshed": refreshed + pivot_count, "queriesRefreshed": refreshed, "pivotsRefreshed": pivot_count, "errors": errors, "truncated": !truncated.is_empty(), "truncatedQueries": truncated}),
            )
        })?;
        retain_profiles(s);
        s.odbc.complete(&id, &applied_tables);
        return Ok(result);
    }
    let (data, request) = s.odbc.ready(&id, doc.uid, doc.revision).map_err(other)?;
    let result = match request.kind.as_str() {
        "preview" => serde_json::to_value(&data).map_err(|e| other(e.to_string()))?,
        "load" => edit(s, |cx| import(cx, &id, &request, &data))?,
        "refresh" => edit(s, |cx| refresh_table(cx, &id, &request, &data, bool_param(p, "replace").unwrap_or(request.replace)))?,
        _ => return Err(bad("ODBC", "This job has no data to apply.")),
    };
    retain_profiles(s);
    s.odbc.complete(&id, &[]);
    Ok(result)
}

fn refresh(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "table").ok_or_else(|| bad("data.refreshQuery", "`table` is required"))?;
    let request = refresh_request(s, p, name)?;
    start(s, request)
}

fn refresh_request(s: &Session, p: &Json, name: &str) -> Result<Request> {
    if name.len() > 255 {
        return Err(bad("ODBC", "Query table name is too long."));
    }
    let doc = s.doc()?;
    let (si, ti) = doc.wb.table(name).ok_or_else(|| bad("data.refreshQuery", "Query table was not found."))?;
    let table = doc.wb.sheet(si).and_then(|s| s.tables.get(ti)).ok_or_else(|| bad("data.refreshQuery", "Query table was not found."))?;
    let source = table.query.as_ref().filter(|q| q.valid()).ok_or_else(|| bad("data.refreshQuery", "Table has no valid ODBC query definition."))?;
    let base = s.odbc.profile(doc.uid, &source.connection_ref).unwrap_or_else(|| Connection { dsn: source.dsn.clone(), ..Default::default() });
    Ok(Request {
        uid: doc.uid,
        revision: doc.revision,
        kind: "refresh".into(),
        connection: settings(p, base)?,
        sql: source.sql.clone(),
        limits: Limits { rows: source.max_rows as usize, ..Default::default() },
        table: Some(table.name.clone()),
        connection_ref: Some(source.connection_ref.clone()),
        replace: bool_param(p, "replace").unwrap_or(false),
        batch: vec![],
    })
}

pub(crate) fn refresh_all(s: &mut Session, p: &Json) -> Result<Json> {
    let doc = s.doc()?;
    let mut request = Request {
        uid: doc.uid,
        revision: doc.revision,
        kind: "refreshAll".into(),
        connection: Connection::default(),
        sql: String::new(),
        limits: Limits::default(),
        table: None,
        connection_ref: None,
        replace: false,
        batch: vec![],
    };
    for table in doc.wb.sheets.iter().flat_map(|sh| &sh.tables).filter(|t| t.query.is_some()).take(65) {
        let item = refresh_request(s, p, &table.name).unwrap_or_else(|_| Request {
            uid: doc.uid,
            revision: doc.revision,
            kind: "refresh".into(),
            connection: Connection::default(),
            sql: String::new(),
            limits: Limits::default(),
            table: Some(table.name.chars().take(255).collect()),
            connection_ref: None,
            replace: bool_param(p, "replace").unwrap_or(false),
            batch: vec![],
        });
        request.batch.push(item);
    }
    start(s, request)
}

fn column_names(data: &DataSet) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    data.columns
        .iter()
        .enumerate()
        .map(|(i, column)| {
            let base = if column.name.trim().is_empty() { format!("Column{}", i + 1) } else { column.name.chars().take(240).collect::<String>() };
            let mut name = base.clone();
            let mut suffix = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base}_{suffix}");
                suffix += 1;
            }
            name
        })
        .collect()
}

fn result_range(start: CellRef, data: &DataSet) -> Result<RangeRef> {
    let row =
        start.row.checked_add(data.rows.len() as u32).filter(|r| *r < MAX_ROWS).ok_or_else(|| bad("ODBC", "Result does not fit in the sheet."))?;
    let col = data
        .columns
        .len()
        .checked_sub(1)
        .and_then(|n| start.col.checked_add(n as u32))
        .filter(|c| *c < MAX_COLS)
        .ok_or_else(|| bad("ODBC", "Result does not fit in the sheet."))?;
    Ok(RangeRef::new(start, CellRef::new(row, col)))
}

fn write_data(sheet: &mut Sheet, range: RangeRef, data: &DataSet, names: &[String]) {
    for (col, name) in names.iter().enumerate() {
        sheet.set_value(CellRef::new(range.start.row, range.start.col + col as u32), Value::text(name.clone()));
    }
    for (row, values) in data.rows.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            sheet.set_value(CellRef::new(range.start.row + row as u32 + 1, range.start.col + col as u32), value.clone());
        }
    }
}

fn source(id: &str, request: &Request, sheet: &Sheet, range: RangeRef, data: &DataSet) -> Result<QuerySource> {
    Ok(QuerySource {
        dsn: request.connection.dsn.clone(),
        connection_ref: request.connection_ref.clone().unwrap_or_else(|| id.into()),
        sql: request.sql.clone(),
        max_rows: request.limits.rows as u32,
        fingerprint: fingerprint(sheet, range)?,
        column_types: data.columns.iter().map(|c| format!("{:?}", c.kind)).collect(),
    })
}

fn import(cx: &mut Ctx<'_>, id: &str, request: &Request, data: &DataSet) -> Result<Json> {
    if cx.wb.protected_structure {
        return Err(other("Workbook structure is protected."));
    }
    let range = result_range(CellRef::new(0, 0), data)?;
    let name = (1u32..).map(|n| format!("Query{n}")).find(|n| cx.wb.table(n).is_none()).ok_or_else(|| other("No query table name is available."))?;
    let mut sheet = Sheet::new(cx.wb.next_sheet_name());
    let names = column_names(data);
    write_data(&mut sheet, range, data, &names);
    let query = source(id, request, &sheet, range, data)?;
    let table_id = cx
        .wb
        .sheets
        .iter()
        .flat_map(|s| &s.tables)
        .map(|t| t.id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| other("Table identifiers are exhausted."))?;
    sheet.tables.push(Table {
        id: table_id,
        name: name.clone(),
        range,
        header_row: true,
        totals_row: false,
        columns: names
            .into_iter()
            .map(|name| TableColumn { name, totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None })
            .collect(),
        style: "TableStyleMedium2".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
        query: Some(query),
    });
    let sheet_name = sheet.name.clone();
    cx.wb.active_sheet = cx.wb.sheets.len();
    cx.wb.sheets.push(Arc::new(sheet));
    *cx.sel = Selection::at(range.start);
    cx.structural = true;
    Ok(json!({"table": name, "sheet": sheet_name, "range": range.a1(), "rows": data.rows.len(), "truncated": data.truncated}))
}

fn refresh_table(cx: &mut Ctx<'_>, id: &str, request: &Request, data: &DataSet, replace: bool) -> Result<Json> {
    let name = request.table.as_deref().ok_or_else(|| bad("ODBC", "Refresh has no target table."))?;
    let (si, ti) = cx.wb.table(name).ok_or_else(|| bad("ODBC", "Query table was deleted."))?;
    let sheet = cx.wb.sheet(si).ok_or_else(|| other("Query sheet was deleted."))?;
    let table = sheet.tables.get(ti).ok_or_else(|| other("Query table was deleted."))?;
    let query = table.query.as_ref().filter(|q| q.valid()).ok_or_else(|| bad("ODBC", "Query definition is invalid."))?;
    let old = table.range;
    let names = column_names(data);
    let kinds = data.columns.iter().map(|c| format!("{:?}", c.kind)).collect::<Vec<_>>();
    if old.start.row > old.end.row
        || old.start.col > old.end.col
        || old.end.row >= MAX_ROWS
        || old.end.col >= MAX_COLS
        || old.height() > 100_001
        || old.width() > 256
        || !table.header_row
        || table.totals_row
        || table.columns.iter().any(|c| c.formula.is_some())
        || table.columns.iter().map(|c| &c.name).ne(names.iter())
        || (!query.column_types.is_empty() && query.column_types != kinds)
    {
        return Err(bad("ODBC", "Query table schema changed; load the query into a new table."));
    }
    if sheet.is_protected() {
        return Err(other(PROTECTED));
    }
    let filter = sheet.autofilter.as_ref().filter(|af| af.range == old).map(|af| af.criteria.clone()).unwrap_or_else(|| table.filter.clone());
    if filter.len() > 64 || filter.iter().any(|(col, _)| *col >= old.width()) {
        return Err(bad("ODBC", "Query table has an invalid filter."));
    }
    let range = result_range(old.start, data)?;
    let owned = RangeRef::new(old.start, CellRef::new(old.end.row.max(range.end.row), old.end.col));
    let shares_rows = |other: RangeRef| other.start.row <= owned.end.row && other.end.row >= owned.start.row;
    if !filter.is_empty()
        && (sheet.autofilter.as_ref().is_some_and(|af| af.range != old && !af.criteria.is_empty() && shares_rows(af.range))
            || sheet.tables.iter().enumerate().any(|(index, table)| index != ti && !table.filter.is_empty() && shares_rows(table.range)))
    {
        return Err(bad("ODBC", "Refresh would change another active row filter. Clear that filter first."));
    }
    let hidden = sheet
        .rows
        .keys()
        .copied()
        .filter(|row| !filter.is_empty() && *row > old.start.row && *row <= old.end.row && sheet.is_row_filtered(*row))
        .collect::<Vec<_>>();
    if sheet.cells.iter_range(owned).any(|(c, cell)| cell.formula.is_some() || (!old.contains(c) && !cell.value.is_empty()))
        || sheet.cell_pictures.keys().any(|c| owned.contains(*c))
        || sheet.merges.iter().any(|r| owned.intersects(r))
        || sheet.spill_ranges.values().any(|r| owned.intersects(r))
        || sheet.pivots.iter().any(|p| p.last_range.is_some_and(|r| owned.intersects(&r)))
        || sheet.tables.iter().enumerate().any(|(index, t)| index != ti && owned.intersects(&t.range))
    {
        return Err(bad("ODBC", "Refresh would overwrite a formula, merged cell or adjacent data."));
    }
    if (query.fingerprint.is_empty() || query.fingerprint != fingerprint(sheet, old)?) && !replace {
        return Err(bad("ODBC", "Query data was edited. Set `replace: true` to replace it."));
    }
    let sheet = cx.wb.sheet_mut(si).ok_or_else(|| other("Query sheet was deleted."))?;
    for row in hidden {
        if let Some(info) = sheet.rows.get_mut(&row) {
            info.hidden = false;
        }
    }
    let cells = sheet.cells.iter_range(old).map(|(c, _)| c).collect::<Vec<_>>();
    for cell in cells {
        sheet.set_value(cell, Value::Empty);
    }
    write_data(sheet, range, data, &names);
    let query = source(id, request, sheet, range, data)?;
    let table = sheet.tables.get_mut(ti).ok_or_else(|| other("Query table was deleted."))?;
    table.range = range;
    table.query = Some(query);
    let original_filter = sheet.autofilter.clone();
    let matching_filter = original_filter.as_ref().is_some_and(|af| af.range == old);
    if !filter.is_empty() || matching_filter {
        let reapply = !filter.is_empty();
        sheet.autofilter = Some(gridcraft_model::AutoFilter { range, criteria: filter });
        if reapply {
            super::data::apply_filter(&mut cx.wb, si);
        }
        if !matching_filter && let Some(sheet) = cx.wb.sheet_mut(si) {
            sheet.autofilter = original_filter;
        }
    }
    cx.structural = true;
    Ok(json!({"table": name, "range": range.a1(), "rows": data.rows.len(), "truncated": data.truncated}))
}

fn fingerprint(sheet: &Sheet, range: RangeRef) -> Result<String> {
    struct Hash(u64);
    impl Write for Hash {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            for byte in bytes {
                self.0 ^= u64::from(*byte);
                self.0 = self.0.wrapping_mul(1_099_511_628_211);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hash = Hash(14_695_981_039_346_656_037);
    serde_json::to_writer(&mut hash, &(range.height(), range.width())).map_err(|e| other(e.to_string()))?;
    for (at, cell) in sheet.cells.iter_range(range).filter(|(_, cell)| !cell.value.is_empty()) {
        serde_json::to_writer(&mut hash, &(at.row - range.start.row, at.col - range.start.col, &cell.value)).map_err(|e| other(e.to_string()))?;
    }
    Ok(format!("{:016x}", hash.0))
}
