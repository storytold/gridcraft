//! Data › Get & Transform Data and Data › Queries & Connections (Power Query), plus the `query.*`
//! primitives the Power Query Editor, the CLI and MCP use to build and inspect queries.
//!
//! Creating a query reads its source once, adds the steps Power Query adds by itself (Promoted
//! Headers, Changed Type) and loads the result as a table on a new worksheet (or only keeps the
//! connection). Refresh evaluates queries first, outside the edit, then writes every result in one
//! undoable step. Sources outside the workbook stay disabled while the document's external
//! content is (the Security Warning bar) or the Trust Center disables data connections.

use gridcraft_core::CellRef;
use gridcraft_model::query::{LoadTo, MAX_NAME, MAX_QUERIES, MAX_STEPS, Query, QuerySource, QueryStep, StepOp, WorkbookItemKind};
use serde_json::{Value as Json, json};

use super::*;
use crate::query::{self as q, EvalCx, QTable, access, load, m, mtext, sources};

/// Rows a preview returns unless asked for fewer.
const PREVIEW_ROWS: usize = 1_000;
/// Most entries in Recent Sources.
const MAX_RECENT: usize = 20;

const PDF: &str = "From PDF isn't available in GridCraft: reading tables out of PDF pages isn't implemented yet.";
const SHAREPOINT: &str = "SharePoint sources need a Microsoft 365 sign-in, which GridCraft doesn't offer.";
const AZURE: &str = "Azure sources need a Microsoft cloud sign-in, which GridCraft doesn't offer.";
const POWER_PLATFORM: &str = "Power Platform sources (Power BI, Dataverse, Dataflows) need a Microsoft cloud sign-in, which GridCraft doesn't offer.";
const ONLINE_SERVICES: &str =
    "Online services (SharePoint Online lists, Exchange Online, Salesforce…) need a cloud sign-in, which GridCraft doesn't offer.";
const ANALYSIS_SERVICES: &str = "Analysis Services (OLAP cube) connections aren't supported in GridCraft.";
const MICROSOFT_QUERY: &str = "Microsoft Query is a separate Windows program GridCraft can't host; use From ODBC instead.";
const OLEDB: &str = "OLE DB providers are Windows components GridCraft can't load; use From ODBC instead.";
const NO_REFRESH_RUNNING: &str = "No refresh is in progress: GridCraft refreshes queries in the foreground.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // ---- Get & Transform Data
        cmd!(
            "data.getData",
            "Get Data",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text | url, source?: csv|excel|xml|json|folder|web|odata, name?, load?: table|connectionOnly|none} (CSV, TSV, JSON, XML or Excel by extension) → {query, sheet, table, rows, cols}",
            has_doc,
            get_data
        ),
        cmd!(
            "data.fromExcelWorkbook",
            "From Excel Workbook",
            ["Data", "Get & Transform Data", "Get Data", "From File"],
            None,
            "{path, item: sheet/table/name, itemKind?: sheet|table|definedName, name?, load?, sheet?, cell?} → query",
            can_files,
            |s, p| file_source(s, p, "excel")
        ),
        cmd!(
            "data.fromTextCsv",
            "From Text/CSV",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text, delimiter?: \",\", encoding?: utf-8|utf-16|windows-1252, name?, load?: table|connectionOnly|none, sheet?: existing sheet, cell?} → imports as a query (new sheet table)",
            has_doc,
            from_text
        ),
        cmd!(
            "data.fromXml",
            "From XML",
            ["Data", "Get & Transform Data", "Get Data", "From File"],
            None,
            "{path, item?: element path (catalog/book), name?, load?, sheet?, cell?} → query",
            can_files,
            |s, p| file_source(s, p, "xml")
        ),
        cmd!(
            "data.fromJson",
            "From JSON",
            ["Data", "Get & Transform Data", "Get Data", "From File"],
            None,
            "{path, item?: property path (data.items), name?, load?, sheet?, cell?} → query",
            can_files,
            |s, p| file_source(s, p, "json")
        ),
        cmd!("data.fromPdf", "From PDF", ["Data", "Get & Transform Data", "Get Data", "From File"], None, "{}", |_| Err(PDF.into()), |_, _| ok()),
        cmd!(
            "data.fromFolder",
            "From Folder",
            ["Data", "Get & Transform Data", "Get Data", "From File"],
            None,
            "{path, combine?: bool, extension?: csv, name?, load?} → query (one row per file, or the files combined)",
            can_files,
            |s, p| file_source(s, p, "folder")
        ),
        cmd!(
            "data.fromSharePointFolder",
            "From SharePoint Folder",
            ["Data", "Get & Transform Data", "Get Data", "From File"],
            None,
            "{}",
            |_| Err(SHAREPOINT.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.fromSqlServer",
            "From SQL Server Database",
            ["Data", "Get & Transform Data", "Get Data", "From Database"],
            None,
            "{server, database?, table? | sql?, name?, load?} → query through the SQL Server ODBC driver",
            can_odbc,
            from_sql_server
        ),
        cmd!(
            "data.fromAccess",
            "From Microsoft Access Database",
            ["Data", "Get & Transform Data", "Get Data", "From Database"],
            None,
            "{path, table? | sql?, name?, load?} → query through the Access ODBC driver",
            can_odbc,
            from_access
        ),
        cmd!(
            "data.fromAnalysisServices",
            "From Analysis Services",
            ["Data", "Get & Transform Data", "Get Data", "From Database"],
            None,
            "{}",
            |_| Err(ANALYSIS_SERVICES.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.fromAnalysisServicesImport",
            "From SQL Server Analysis Services Database (Import)",
            ["Data", "Get & Transform Data", "Get Data", "From Database"],
            None,
            "{}",
            |_| Err(ANALYSIS_SERVICES.into()),
            |_, _| ok()
        ),
        cmd!("data.fromAzure", "From Azure", ["Data", "Get & Transform Data", "Get Data"], None, "{service?}", |_| Err(AZURE.into()), |_, _| ok()),
        cmd!(
            "data.fromPowerPlatform",
            "From Power Platform",
            ["Data", "Get & Transform Data", "Get Data"],
            None,
            "{service?}",
            |_| Err(POWER_PLATFORM.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.fromOnlineServices",
            "From Online Services",
            ["Data", "Get & Transform Data", "Get Data"],
            None,
            "{service?}",
            |_| Err(ONLINE_SERVICES.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.fromTableRange",
            "From Table/Range",
            ["Data", "Get & Transform Data"],
            None,
            "{table? | range?, name?, load?: table|connectionOnly|none} (a range becomes a table first) → query",
            has_doc,
            from_table_range
        ),
        cmd!(
            "data.fromWeb",
            "From Web",
            ["Data", "Get & Transform Data"],
            None,
            "{url, item?: \"Table 1\" | JSON path | XML path, name?, load?: table|connectionOnly|none, sheet?, cell?} → query",
            can_web,
            from_web
        ),
        cmd!(
            "data.fromMicrosoftQuery",
            "From Microsoft Query",
            ["Data", "Get & Transform Data", "Get Data", "From Other Sources"],
            None,
            "{}",
            |_| Err(MICROSOFT_QUERY.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.fromODataFeed",
            "From OData Feed",
            ["Data", "Get & Transform Data", "Get Data", "From Other Sources"],
            None,
            "{url, item?: entity set, name?, load?} → query",
            can_web,
            from_odata
        ),
        cmd!(
            "data.fromOdbc",
            "From ODBC",
            ["Data", "Get & Transform Data", "Get Data", "From Other Sources"],
            None,
            "{connection: \"DSN=…\" | \"Driver={…};…\", table? | sql?, name?, load?} → query",
            can_odbc,
            from_odbc
        ),
        cmd!(
            "data.fromOledb",
            "From OLEDB",
            ["Data", "Get & Transform Data", "Get Data", "From Other Sources"],
            None,
            "{}",
            |_| Err(OLEDB.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.blankQuery",
            "Blank Query",
            ["Data", "Get & Transform Data", "Get Data", "From Other Sources"],
            None,
            "{name?, load?: none|table|connectionOnly} → a new empty query (opens the Power Query Editor from the menu)",
            has_doc,
            blank_query
        ),
        cmd!(
            "data.mergeQueries",
            "Merge",
            ["Data", "Get & Transform Data", "Get Data", "Combine Queries"],
            None,
            "{left, right, leftColumns: [..], rightColumns: [..], join?: leftOuter|rightOuter|fullOuter|inner|leftAnti|rightAnti, columns?, name?, load?} → new query",
            has_queries,
            merge_queries
        ),
        cmd!(
            "data.appendQueries",
            "Append",
            ["Data", "Get & Transform Data", "Get Data", "Combine Queries"],
            None,
            "{queries: [first, second, …], name?, load?} → new query",
            has_queries,
            append_queries
        ),
        cmd!(
            noundo "data.powerQueryEditor",
            "Launch Power Query Editor…",
            ["Data", "Get & Transform Data", "Get Data"],
            None,
            "{query?} opens the editor (menu) → {queries}",
            has_doc,
            power_query_editor
        ),
        cmd!(
            "data.dataSourceSettings",
            "Data Source Settings…",
            ["Data", "Get & Transform Data", "Get Data"],
            None,
            "{from?, to?} (Change Source: every query reading `from` reads `to`) → sources",
            has_doc,
            data_source_settings
        ),
        cmd!(
            noundo "data.queryOptions",
            "Query Options",
            ["Data", "Get & Transform Data", "Get Data"],
            None,
            "{loadToWorksheet?: bool} → options",
            always,
            query_options
        ),
        cmd!(noundo "data.recentSources", "Recent Sources", ["Data", "Get & Transform Data"], None, "{clear?: bool} → sources", has_doc, recent_sources),
        cmd!(
            query "data.existingConnections",
            "Existing Connections",
            ["Data", "Get & Transform Data"],
            None,
            "{} → this workbook's connections",
            has_doc,
            existing_connections
        ),
        // ---- Queries & Connections
        cmd!(
            "data.refreshAll",
            "Refresh All",
            ["Data", "Queries & Connections"],
            Some("Cmd+Alt+F5"),
            "{replace?} refreshes every query, then every PivotTable → {queries, refreshed, errors}",
            has_doc,
            refresh_all
        ),
        cmd!(
            "data.refresh",
            "Refresh",
            ["Data", "Queries & Connections", "Refresh All"],
            Some("Alt+F5"),
            "{query?} (default: the query table or PivotTable at the active cell) → status",
            has_doc,
            refresh_one
        ),
        cmd!(query "data.refreshStatus", "Refresh Status", ["Data", "Queries & Connections", "Refresh All"], None, "{} → last refresh of each query", has_doc, refresh_status),
        cmd!(
            noundo "data.cancelRefresh",
            "Cancel Refresh",
            ["Data", "Queries & Connections", "Refresh All"],
            None,
            "{}",
            |_| Err(NO_REFRESH_RUNNING.into()),
            |_, _| ok()
        ),
        cmd!(
            "data.connectionProperties",
            "Connection Properties…",
            ["Data", "Queries & Connections", "Refresh All"],
            None,
            "{query?, description?, refreshOnOpen?, everyMinutes?, background?, inRefreshAll?} → properties",
            has_queries,
            connection_properties
        ),
        cmd!(
            noundo "data.queriesConnections",
            "Queries & Connections",
            ["Data", "Queries & Connections"],
            None,
            "{on?: bool} toggles the task pane → {on, queries}",
            has_doc,
            queries_connections
        ),
        cmd!(
            "data.properties",
            "Properties",
            ["Data", "Queries & Connections"],
            None,
            "{query?, …as data.connectionProperties} (the query table at the active cell) → properties",
            in_query_table,
            properties
        ),
        cmd!(
            noundo "data.enableContent",
            "Enable Content",
            [],
            None,
            "{} answers the Security Warning: external data connections are enabled for this workbook (refresh-on-open queries refresh)",
            blocked,
            enable_content
        ),
        cmd!(
            noundo "file.trustCenter",
            "Trust Center",
            ["File", "Options", "Trust Center"],
            None,
            "{externalContent?: enableAll|prompt|disableAll} → settings",
            always,
            trust_center
        ),
        // ---- query primitives
        cmd!(query "query.list", "List Queries", [], None, "{} → [{name, source, steps, load, status}]", has_doc, list),
        cmd!(query "query.get", "Get Query", [], None, "{name} → {query, steps: [names], m} (an Excel query: {query, m, excel: true, refreshable: false})", has_doc, get),
        cmd!(
            query "query.listExcel",
            "List Excel Queries",
            [],
            None,
            "{} → [{name, formula, description?, connection?, table?, sheet?, load: table|dataModel|connectionOnly, rows?, lastRefreshed?, refreshOnOpen}]: Power Query queries authored in Excel, read-only (M isn't evaluated)",
            has_doc,
            list_excel
        ),
        cmd!(
            "query.set",
            "Edit Query",
            [],
            None,
            "{query: {name, source: {kind, …}, steps: [{op, …}], load?, refresh?}, replace?: old name, refresh?: bool} creates or replaces a query",
            has_doc,
            set
        ),
        cmd!("query.delete", "Delete Query", [], None, "{name, deleteTable?: bool}", has_doc, delete),
        cmd!("query.rename", "Rename Query", [], None, "{name, newName} (references in other queries follow)", has_doc, rename),
        cmd!("query.duplicate", "Duplicate Query", [], None, "{name, newName?}", has_doc, duplicate),
        cmd!("query.reference", "Reference Query", [], None, "{name, newName?}", has_doc, reference),
        cmd!(
            "query.addStep",
            "Add Applied Step",
            [],
            None,
            "{name, step: {op: removeColumns, columns: [..]} | …, at?: index (1 = after Source)}",
            has_doc,
            add_step
        ),
        cmd!("query.removeStep", "Delete Applied Step", [], None, "{name, step: index (1 = first after Source) | step name}", has_doc, remove_step),
        cmd!("query.renameStep", "Rename Applied Step", [], None, "{name, step, newName}", has_doc, rename_step),
        cmd!("query.moveStep", "Move Applied Step", [], None, "{name, step, to: index}", has_doc, move_step),
        cmd!(
            query "query.preview",
            "Preview Query",
            [],
            None,
            "{name | query: {…}, step?: index (0 = Source), rows?: 1000} → {columns, types, rows, rowCount, error?}",
            has_doc,
            preview
        ),
        cmd!(
            query "query.navigator",
            "Navigator",
            [],
            None,
            "{source: {kind, path | url | connection, …}} → items the source offers (tables, sheets, arrays, entity sets)",
            has_doc,
            navigator
        ),
        cmd!(
            "query.load",
            "Close & Load To…",
            [],
            None,
            "{name, to?: table|connectionOnly, sheet?: existing sheet (else a new one), cell?: \"A1\"} → status",
            has_doc,
            load_to
        ),
        cmd!(query "query.mText", "Advanced Editor", [], None, "{name | query: {…}} → M-like text (read-only)", has_doc, m_text),
    ]
}

// ---------------------------------------------------------------- enabled

fn can_files(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if cfg!(target_arch = "wasm32") { Err(access::FILES_UNAVAILABLE.into()) } else { Ok(()) }
}
fn can_web(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    s.data_access().web_available()
}
fn can_odbc(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    s.data_access().odbc_available()
}
fn has_queries(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| !d.wb.queries.is_empty()) { Ok(()) } else { Err("This workbook has no queries yet (Data › Get Data).".into()) }
}
fn blocked(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.external_blocked) { Ok(()) } else { Err("External content is already enabled for this workbook.".into()) }
}
fn in_query_table(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if active_query(s).is_some() { Ok(()) } else { Err("Select a cell in a table that a query loaded.".into()) }
}

// ---------------------------------------------------------------- helpers

fn other(e: String) -> EngineError {
    EngineError::Other(e)
}

/// The index of the query whose loaded table holds the active cell.
fn active_query(s: &Session) -> Option<usize> {
    let d = s.active()?;
    let sh = d.wb.active()?;
    let t = sh.tables.iter().find(|t| t.range.contains(d.selection.active))?;
    d.wb.queries.iter().position(|q| q.load.table.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&t.name)))
}

fn query_index(s: &Session, name: &str) -> Result<usize> {
    let wb = &s.doc()?.wb;
    if let Some(i) = wb.queries.iter().position(|q| q.name.eq_ignore_ascii_case(name)) {
        return Ok(i);
    }
    match wb.excel_queries.iter().find(|q| q.name.eq_ignore_ascii_case(name)) {
        Some(q) => Err(excel_query_error(&q.name)),
        None => Err(bad("query", format!("no query named `{name}`"))),
    }
}

/// The Excel query called `name` when no GridCraft query is (its name as Excel spells it).
fn excel_only(wb: &gridcraft_model::Workbook, name: &str) -> Option<String> {
    if wb.queries.iter().any(|q| q.name.eq_ignore_ascii_case(name)) {
        return None;
    }
    wb.excel_queries
        .iter()
        .find(|q| q.name == name)
        .or_else(|| wb.excel_queries.iter().find(|q| q.name.eq_ignore_ascii_case(name)))
        .map(|q| q.name.clone())
}

/// Whether one of Excel's queries is called `name` (query names are shared with them).
fn excel_named(wb: &gridcraft_model::Workbook, name: &str) -> bool {
    wb.excel_queries.iter().any(|q| q.name.eq_ignore_ascii_case(name))
}

/// Why an Excel query can't be edited.
fn excel_query_error(name: &str) -> EngineError {
    EngineError::Other(format!(
        "\"{name}\" is a Power Query query made in Excel. GridCraft refreshes it from its M formula, but editing M isn't supported yet: edit it in Excel."
    ))
}

/// The Excel query loading into the table at the active cell.
fn active_excel_query(s: &Session) -> Option<String> {
    let d = s.active()?;
    let sh = d.wb.active()?;
    let t = sh.tables.iter().find(|t| t.range.contains(d.selection.active))?;
    d.wb.excel_queries.iter().find(|q| q.table.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&t.name))).map(|q| q.name.clone())
}

fn name_param<'a>(p: &'a Json, id: &str) -> Result<&'a str> {
    str_param(p, "name").or_else(|| str_param(p, "query")).ok_or_else(|| bad(id, "missing `name`"))
}

/// Checks a query or step name (not empty, not too long, no control characters).
fn check_name(name: &str) -> Result<()> {
    let n = name.trim();
    if n.is_empty() {
        return Err(EngineError::Other("A query name can't be empty.".into()));
    }
    if n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(EngineError::Other(format!("A name can have at most {MAX_NAME} characters and no control characters.")));
    }
    Ok(())
}

/// `base`, or `base (2)`, `base (3)`… when a query has that name already.
fn unique_query_name(wb: &gridcraft_model::Workbook, base: &str) -> String {
    let base: String = base.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME - 6).collect();
    let base = if base.is_empty() { "Query1".to_string() } else { base };
    let taken = |n: &str| wb.queries.iter().any(|q| q.name.eq_ignore_ascii_case(n)) || excel_named(wb, n);
    if !taken(&base) {
        return base;
    }
    (2..).map(|k| format!("{base} ({k})")).find(|n| !taken(n)).unwrap_or(base)
}

/// Fills empty step names with Power Query's (`Removed Columns`), numbered when the name is taken
/// (`Removed Columns1`). Names given keep priority over generated ones.
fn name_steps(q: &mut Query) {
    let mut used: Vec<String> = vec!["Source".into()];
    let given: Vec<bool> = q.steps.iter().map(|s| !s.name.trim().is_empty()).collect();
    for pass_given in [true, false] {
        for (st, g) in q.steps.iter_mut().zip(&given) {
            if *g != pass_given {
                continue;
            }
            let base: String =
                if pass_given { st.name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect() } else { st.op.default_name().to_string() };
            st.name = if used.iter().any(|u| u.eq_ignore_ascii_case(&base)) {
                (1..).map(|k| format!("{base}{k}")).find(|n| !used.iter().any(|u| u.eq_ignore_ascii_case(n))).unwrap_or(base)
            } else {
                base
            };
            used.push(st.name.clone());
        }
    }
}

fn parse_query(v: &Json) -> Result<Query> {
    let mut q: Query = serde_json::from_value(v.clone()).map_err(|e| bad("query", format!("not a query definition: {e}")))?;
    check_name(&q.name)?;
    q.name = q.name.trim().to_string();
    if q.steps.len() > MAX_STEPS {
        return Err(bad("query", format!("a query can have at most {MAX_STEPS} steps")));
    }
    name_steps(&mut q);
    Ok(q)
}

/// Whether evaluating the query reaches outside the workbook, following references.
fn reaches_outside(wb: &gridcraft_model::Workbook, q: &Query, depth: usize) -> bool {
    if depth > q::MAX_QUERY_DEPTH {
        return false;
    }
    let refs = || {
        let mut names: Vec<&str> = q.steps.iter().flat_map(|s| s.op.referenced_queries()).collect();
        if let QuerySource::Query { name } = &q.source {
            names.push(name);
        }
        names
    };
    q.source.is_external()
        || refs().into_iter().any(|n| wb.queries.iter().find(|x| x.name.eq_ignore_ascii_case(n)).is_some_and(|x| reaches_outside(wb, x, depth + 1)))
}

/// Runs `f` with an evaluation context over the active document (its fetch cache, external
/// content setting and the injected data access).
fn with_eval<R>(s: &mut Session, fresh: bool, drafts: &[Query], f: impl FnOnce(&mut EvalCx<'_>) -> R) -> Result<R> {
    let access = s.data_access();
    let prefs = s.prefs.clone();
    let d = s.doc_mut()?;
    let wb = d.wb.clone();
    let external = d.external_allowed(&prefs);
    let mut cache = std::mem::take(&mut d.fetch_cache);
    if fresh {
        cache.clear();
    }
    let r = {
        let mut cx = EvalCx::new(&wb, access.as_ref(), external, &mut cache);
        cx.fresh = fresh;
        cx.drafts = drafts;
        f(&mut cx)
    };
    if let Ok(d) = s.doc_mut() {
        d.fetch_cache = cache;
    }
    Ok(r)
}

/// Adds a source location to Recent Sources.
fn remember(s: &mut Session, src: &QuerySource) {
    let Some(loc) = src.location() else { return };
    if matches!(src, QuerySource::Odbc { .. }) {
        // Connection strings may hold credentials: not kept.
        return;
    }
    let loc = loc.to_string();
    s.prefs.recent_sources.retain(|r| *r != loc);
    s.prefs.recent_sources.insert(0, loc);
    s.prefs.recent_sources.truncate(MAX_RECENT);
}

/// Unix milliseconds now (off wasm, where the clock traps).
fn now_ms() -> f64 {
    gridcraft_core::date::unix_now_ms().unwrap_or(0.0)
}

/// Applies Load To params (`load`, `sheet`, `cell`) to a query. `None` from `load: none`.
fn load_params(s: &Session, p: &Json, q: &mut Query) -> Result<bool> {
    match str_param(p, "load").or_else(|| str_param(p, "to")) {
        Some("none") => return Ok(false),
        Some("connectionOnly" | "connection") => q.load.to = LoadTo::ConnectionOnly,
        Some("table") => q.load.to = LoadTo::Table,
        Some(other) => return Err(bad("load", format!("`{other}` isn't table, connectionOnly or none"))),
        None => q.load.to = if s.prefs.load_to_worksheet { LoadTo::Table } else { LoadTo::ConnectionOnly },
    }
    if let Some(sheet) = str_param(p, "sheet") {
        if s.doc()?.wb.sheet_index(sheet).is_none() {
            return Err(bad("sheet", format!("no sheet named `{sheet}`")));
        }
        q.load.sheet = Some(sheet.to_string());
        let cell = str_param(p, "cell").unwrap_or("A1");
        CellRef::parse(&cell.replace('$', "")).ok_or_else(|| bad("cell", format!("`{cell}` isn't a cell")))?;
        q.load.cell = Some(cell.to_string());
    }
    Ok(true)
}

fn table_json(t: &QTable, limit: usize) -> Json {
    let rows: Vec<Json> = t.rows.iter().take(limit).map(|r| Json::Array(r.iter().map(value_json).collect())).collect();
    json!({
        "columns": t.columns,
        "types": t.types.iter().map(|ty| serde_json::to_value(ty).unwrap_or(Json::Null)).collect::<Vec<_>>(),
        "rows": rows,
        "rowCount": t.rows.len(),
    })
}

fn value_json(v: &gridcraft_core::Value) -> Json {
    use gridcraft_core::Value;
    match v {
        Value::Empty => Json::Null,
        Value::Number(n) => json!(n),
        Value::Bool(b) => json!(b),
        Value::Text(t) => json!(t.as_ref()),
        Value::Error(e) => json!({"error": e.as_str()}),
        Value::Array(_) => json!({"error": "#VALUE!"}),
    }
}

/// Writes evaluated results into the workbook in one edit: tables for queries loaded as tables,
/// the status of each (Excel's queries too, into the tables they load). Returns a summary per
/// query.
fn store(
    s: &mut Session,
    results: Vec<(usize, q::QResult<QTable>)>,
    excel: Vec<(String, q::QResult<m::Refreshed>)>,
    pivots: Option<bool>,
) -> Result<Json> {
    let mut new_sheets: Vec<usize> = Vec::new();
    let out = edit(s, |cx| {
        let mut summary = Vec::new();
        for (qi, r) in results {
            let Some(name) = cx.wb.queries.get(qi).map(|q| q.name.clone()) else { continue };
            let status = match r {
                Ok(t) => {
                    let to = cx.wb.queries.get(qi).map(|q| q.load.to).unwrap_or_default();
                    // A load that fails (the table would overlap data, a protected sheet) leaves the
                    // workbook as it was, the old table included.
                    let before = cx.wb.clone();
                    let loaded = match to {
                        LoadTo::Table => match load::write(cx, qi, &t) {
                            Ok(l) => {
                                new_sheets.extend(l.new_sheet);
                                Ok(Some(l))
                            }
                            Err(e) => {
                                cx.wb = before;
                                Err(e)
                            }
                        },
                        LoadTo::ConnectionOnly => {
                            let old = cx.wb.queries.get_mut(qi).and_then(|q| q.load.table.take());
                            match old {
                                Some(tn) => load::remove_table(cx, &tn).map(|_| None),
                                None => Ok(None),
                            }
                        }
                    };
                    match loaded {
                        Ok(l) => {
                            let st = gridcraft_model::query::QueryStatus {
                                rows: Some(t.rows.len() as u64),
                                columns: Some(t.columns.len() as u64),
                                error: None,
                            };
                            let mut j = json!({"query": name, "rows": t.rows.len(), "cols": t.columns.len()});
                            if let Some(l) = l {
                                j["table"] = json!(l.table);
                                j["range"] = json!(l.range.a1());
                                j["sheet"] = json!(cx.wb.sheet(l.sheet).map(|sh| sh.name.clone()));
                            }
                            (st, j)
                        }
                        Err(e) => (
                            gridcraft_model::query::QueryStatus { rows: None, columns: None, error: Some(e.clone()) },
                            json!({"query": name, "error": e}),
                        ),
                    }
                }
                Err(e) => {
                    (gridcraft_model::query::QueryStatus { rows: None, columns: None, error: Some(e.clone()) }, json!({"query": name, "error": e}))
                }
            };
            if let Some(q) = cx.wb.queries.get_mut(qi) {
                q.status = status.0;
            }
            summary.push(status.1);
        }
        let mut res = json!({"queries": summary});
        if !excel.is_empty() {
            res["excelQueries"] = Json::Array(excel.into_iter().map(|(name, r)| store_excel(cx, &name, r, &mut new_sheets)).collect());
        }
        if let Some(replace) = pivots {
            let (n, errors) = super::pivot::refresh_pivots(cx, replace);
            res["refreshed"] = json!(n);
            res["errors"] = json!(errors);
        }
        Ok(res)
    })?;
    let d = s.doc_mut()?;
    for at in new_sheets {
        d.sheet_selections.insert(at.min(d.sheet_selections.len()), crate::Selection::default());
    }
    Ok(out)
}

/// Loads an Excel query's result into the table it loads (a query loading only to a
/// connection or the Data Model keeps just its status); returns its summary.
fn store_excel(cx: &mut super::Ctx<'_>, name: &str, r: q::QResult<m::Refreshed>, new_sheets: &mut Vec<usize>) -> Json {
    let Some(ei) = cx.wb.excel_queries.iter().position(|q| q.name == name) else { return json!({"query": name, "error": "The query is gone."}) };
    let target = cx.wb.excel_queries.get(ei).map(|q| (q.table.clone(), q.sheet.clone())).unwrap_or_default();
    let r = r.and_then(|rf| {
        let loaded = match target {
            (Some(table), sheet) => {
                // A load that fails leaves the workbook as it was, the old table included.
                let before = cx.wb.clone();
                let load = gridcraft_model::query::QueryLoad { to: LoadTo::Table, sheet, cell: None, table: Some(table) };
                match load::write_load(cx, name, &load, &rf.table) {
                    Ok(l) => Some(l),
                    Err(e) => {
                        cx.wb = before;
                        return Err(e);
                    }
                }
            }
            (None, _) => None,
        };
        Ok((rf, loaded))
    });
    match r {
        Ok((rf, loaded)) => {
            let mut j = json!({"query": name, "rows": rf.table.rows.len(), "cols": rf.table.columns.len(), "errors": rf.errors});
            let sheet = loaded.as_ref().and_then(|l| cx.wb.sheet(l.sheet).map(|sh| sh.name.clone()));
            if let Some(q) = cx.wb.excel_queries.get_mut(ei) {
                q.rows = Some(rf.table.rows.len() as u64);
                q.error = None;
                if let Some(l) = &loaded {
                    q.table = Some(l.table.clone());
                    q.sheet = sheet.clone();
                }
            }
            if let Some(l) = loaded {
                new_sheets.extend(l.new_sheet);
                j["table"] = json!(l.table);
                j["range"] = json!(l.range.a1());
                j["sheet"] = json!(sheet);
            }
            j
        }
        Err(e) => {
            if let Some(q) = cx.wb.excel_queries.get_mut(ei) {
                q.error = Some(e.clone());
            }
            json!({"query": name, "error": e})
        }
    }
}

/// Evaluates the queries at `idx` and Excel's queries `excel` (fresh reads when `fresh`) and
/// stores their results in one edit.
fn refresh_indices(s: &mut Session, idx: Vec<usize>, excel: Vec<String>, fresh: bool, pivots: Option<bool>) -> Result<Json> {
    let d = s.doc()?;
    let prefs = s.prefs.clone();
    if let Err(why) = d.external_allowed(&prefs)
        && idx.iter().filter_map(|&i| d.wb.queries.get(i)).any(|q| reaches_outside(&d.wb, q, 0))
    {
        return Err(EngineError::Other(why));
    }
    let wb = d.wb.clone();
    let (results, excel_results) = with_eval(s, fresh, &[], |cx| {
        let r: Vec<(usize, q::QResult<QTable>)> = idx.iter().filter_map(|&i| wb.queries.get(i).map(|q| (i, cx.evaluate(q, None)))).collect();
        let e: Vec<(String, q::QResult<m::Refreshed>)> = excel.iter().cloned().zip(m::evaluate_excel_queries(cx, &excel)).collect();
        (r, e)
    })?;
    let mut names: Vec<String> = idx.iter().filter_map(|&i| wb.queries.get(i).map(|q| q.name.clone())).collect();
    names.extend(excel);
    let r = store(s, results, excel_results, pivots)?;
    let now = now_ms();
    let d = s.doc_mut()?;
    for n in names {
        d.refreshed_at.retain(|(x, _)| !x.eq_ignore_ascii_case(&n));
        d.refreshed_at.push((n, now));
    }
    Ok(r)
}

/// Adds a new query (named uniquely) and, unless `load: none`, loads it. With `load: none` the
/// definition is returned unsaved (the Power Query Editor opens it as a draft).
fn add_query(s: &mut Session, p: &Json, mut q: Query) -> Result<Json> {
    if s.doc()?.wb.queries.len() >= MAX_QUERIES {
        return Err(EngineError::Other(format!("A workbook can hold at most {MAX_QUERIES} queries.")));
    }
    q.name = unique_query_name(&s.doc()?.wb, &q.name);
    name_steps(&mut q);
    let load = load_params(s, p, &mut q)?;
    remember(s, &q.source);
    if !load {
        let preview = with_eval(s, false, std::slice::from_ref(&q), |cx| cx.evaluate(&q, None))?;
        let mut r = json!({"query": q, "saved": false});
        match preview {
            Ok(t) => r["preview"] = table_json(&t, 50),
            Err(e) => r["error"] = json!(e),
        }
        return Ok(r);
    }
    let result = with_eval(s, false, std::slice::from_ref(&q), |cx| cx.evaluate(&q, None))?;
    let name = q.name.clone();
    let mut new_sheets = Vec::new();
    let out = edit(s, |cx| {
        cx.wb.queries.push(q);
        let qi = cx.wb.queries.len() - 1;
        let (status, mut j) = match result {
            Ok(t) => {
                let st = gridcraft_model::query::QueryStatus { rows: Some(t.rows.len() as u64), columns: Some(t.columns.len() as u64), error: None };
                let mut j = json!({"query": name, "rows": t.rows.len(), "cols": t.columns.len()});
                if cx.wb.queries.get(qi).is_some_and(|q| q.load.to == LoadTo::Table) {
                    let l = load::write(cx, qi, &t).map_err(EngineError::Other)?;
                    new_sheets.extend(l.new_sheet);
                    j["table"] = json!(l.table);
                    j["range"] = json!(l.range.a1());
                    j["sheet"] = json!(cx.wb.sheet(l.sheet).map(|sh| sh.name.clone()));
                }
                (st, j)
            }
            // A query whose source can't be read isn't added (Excel shows the error in the
            // Navigator before anything is created).
            Err(e) => return Err(EngineError::Other(e)),
        };
        if let Some(q) = cx.wb.queries.get_mut(qi) {
            q.status = status;
        }
        j["saved"] = json!(true);
        Ok(j)
    })?;
    let now = now_ms();
    let d = s.doc_mut()?;
    for at in new_sheets {
        d.sheet_selections.insert(at.min(d.sheet_selections.len()), crate::Selection::default());
    }
    d.refreshed_at.push((name, now));
    Ok(out)
}

/// A new query from a source: reads it once, adds Power Query's automatic steps, then adds it.
fn create(s: &mut Session, p: &Json, src: QuerySource, default_name: String) -> Result<Json> {
    let d = s.doc()?;
    if src.is_external() {
        d.external_allowed(&s.prefs).map_err(EngineError::Other)?;
    }
    let sys = d.wb.date_system;
    let raw = with_eval(s, false, &[], |cx| sources::table(cx, &src))?.map_err(other)?;
    let mut q = Query::new(str_param(p, "name").map(str::to_string).unwrap_or(default_name), src);
    check_name(&q.name)?;
    q.steps = sources::auto_steps(&q.source, &raw, sys);
    add_query(s, p, q)
}

/// A name from a path's file stem (`C:\data\Sales.csv` → `Sales`).
fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).find(|x| !x.is_empty()).unwrap_or(path);
    let stem = file.rsplit_once('.').map(|(a, _)| a).filter(|a| !a.is_empty()).unwrap_or(file);
    stem.to_string()
}

// ---------------------------------------------------------------- sources

fn get_data(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(src) = str_param(p, "source") {
        return match src {
            "csv" | "text" => from_text(s, p),
            "excel" | "xml" | "json" | "folder" => file_source(s, p, src),
            "web" => from_web(s, p),
            "odata" => from_odata(s, p),
            "odbc" => from_odbc(s, p),
            "table" => from_table_range(s, p),
            "blank" => blank_query(s, p),
            other => Err(bad("data.getData", format!("unknown source `{other}`"))),
        };
    }
    if str_param(p, "url").is_some() {
        return from_web(s, p);
    }
    if let Some(path) = str_param(p, "path") {
        let lower = path.to_ascii_lowercase();
        let kind = if lower.ends_with(".json") {
            "json"
        } else if lower.ends_with(".xml") {
            "xml"
        } else if lower.ends_with(".xlsx")
            || lower.ends_with(".xlsm")
            || lower.ends_with(".xls")
            || lower.ends_with(".xlsb")
            || lower.ends_with(".ods")
        {
            "excel"
        } else {
            return from_text(s, p);
        };
        return file_source(s, p, kind);
    }
    if let Some(text) = str_param(p, "text") {
        // JSON pasted as text: an array or a record.
        let t = text.trim_start();
        if t.starts_with('[') || t.starts_with('{') {
            let doc = q::ojson::parse(text).map_err(|e| bad("data.getData", e))?;
            let table = q::formats::json_doc_table(&doc, None).map_err(other)?;
            // Kept as delimited text so the query is self-contained (Enter Data).
            let mut csv = String::new();
            let esc = |v: &str| if v.contains([',', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() };
            csv.push_str(&table.columns.iter().map(|c| esc(c)).collect::<Vec<_>>().join(","));
            for row in &table.rows {
                csv.push('\n');
                csv.push_str(&row.iter().map(|v| esc(&v.display())).collect::<Vec<_>>().join(","));
            }
            let name = str_param(p, "name").or_else(|| str_param(p, "sheet")).unwrap_or("Table1").to_string();
            return create(s, p, QuerySource::Text { text: csv, delimiter: Some(",".into()) }, name);
        }
        return from_text(s, p);
    }
    s.ui_requests.push(crate::UiRequest::Dialog("importFile".into(), json!({"kind": "csv"})));
    ok()
}

fn from_text(s: &mut Session, p: &Json) -> Result<Json> {
    let delimiter = str_param(p, "delimiter").map(str::to_string);
    let encoding = str_param(p, "encoding").map(str::to_string);
    let (src, name) = if let Some(t) = str_param(p, "text") {
        (QuerySource::Text { text: t.to_string(), delimiter }, str_param(p, "sheet").unwrap_or("Table1").to_string())
    } else if let Some(path) = str_param(p, "path") {
        can_files(s).map_err(EngineError::Other)?;
        (QuerySource::Csv { path: path.to_string(), delimiter, encoding }, str_param(p, "sheet").map(str::to_string).unwrap_or_else(|| stem(path)))
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("importFile".into(), json!({"kind": "csv"})));
        return ok();
    };
    // `sheet` names the new sheet in the old API; it is the query's name here (the sheet is named
    // after the query), unless it names an existing sheet to load into.
    let mut p = p.clone();
    if let Some(sh) = str_param(&p, "sheet").map(str::to_string)
        && s.doc()?.wb.sheet_index(&sh).is_none()
        && let Some(o) = p.as_object_mut()
    {
        o.remove("sheet");
        o.entry("name").or_insert(json!(sh));
    }
    create(s, &p, src, name)
}

fn file_source(s: &mut Session, p: &Json, kind: &str) -> Result<Json> {
    let Some(path) = str_param(p, "path").map(str::to_string) else {
        s.ui_requests.push(crate::UiRequest::Dialog("importFile".into(), json!({"kind": kind})));
        return ok();
    };
    can_files(s).map_err(EngineError::Other)?;
    let item = str_param(p, "item").map(str::to_string);
    let (src, name) = match kind {
        "excel" => {
            let item_kind = match str_param(p, "itemKind") {
                Some("table") => WorkbookItemKind::Table,
                Some("definedName" | "name") => WorkbookItemKind::DefinedName,
                Some("sheet") | None => WorkbookItemKind::Sheet,
                Some(o) => return Err(bad("itemKind", format!("`{o}` isn't sheet, table or definedName"))),
            };
            // Without an item, the Navigator's list says what to pick.
            let Some(item) = item else {
                let src = QuerySource::Workbook { path, item: String::new(), item_kind };
                let items = with_eval(s, false, &[], |cx| sources::items(cx, &src))?.map_err(other)?;
                let names: Vec<String> = items.iter().take(20).map(|i| format!("{} ({})", i.name, i.kind)).collect();
                return Err(bad("data.fromExcelWorkbook", format!("missing `item`; the workbook offers: {}", names.join(", "))));
            };
            (QuerySource::Workbook { path, item: item.clone(), item_kind }, item)
        }
        "xml" => (
            QuerySource::Xml { path: path.clone(), item: item.clone() },
            item.map(|i| i.rsplit('/').next().unwrap_or(&i).to_string()).unwrap_or_else(|| stem(&path)),
        ),
        "json" => (QuerySource::Json { path: path.clone(), item }, stem(&path)),
        "folder" => (
            QuerySource::Folder {
                path: path.clone(),
                combine: bool_param(p, "combine").unwrap_or(false),
                extension: str_param(p, "extension").map(str::to_string),
            },
            stem(&path),
        ),
        _ => return Err(bad("source", format!("unknown file source `{kind}`"))),
    };
    create(s, p, src, name)
}

fn from_web(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(url) = str_param(p, "url").map(str::to_string) else {
        s.ui_requests.push(crate::UiRequest::Dialog("fromWeb".into(), json!({})));
        return ok();
    };
    access::check_url(&url).map_err(|e| bad("data.fromWeb", e))?;
    can_web(s).map_err(EngineError::Other)?;
    let item = str_param(p, "item").map(str::to_string);
    let name = item.clone().filter(|i| !i.is_empty()).unwrap_or_else(|| access::url_host(&url).to_string());
    create(s, p, QuerySource::Web { url, item }, name)
}

fn from_odata(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(url) = str_param(p, "url").map(str::to_string) else {
        s.ui_requests.push(crate::UiRequest::Dialog("fromWeb".into(), json!({"odata": true})));
        return ok();
    };
    access::check_url(&url).map_err(|e| bad("data.fromODataFeed", e))?;
    can_web(s).map_err(EngineError::Other)?;
    let item = str_param(p, "item").map(str::to_string);
    let name = item.clone().filter(|i| !i.is_empty()).unwrap_or_else(|| "OData Feed".to_string());
    create(s, p, QuerySource::OData { url, item }, name)
}

fn odbc_source(s: &mut Session, p: &Json, connection: String, default_name: &str) -> Result<Json> {
    can_odbc(s).map_err(EngineError::Other)?;
    let table = str_param(p, "table").or_else(|| str_param(p, "item")).map(str::to_string);
    let sql = str_param(p, "sql").map(str::to_string);
    if table.is_none() && sql.is_none() {
        let src = QuerySource::Odbc { connection, table: None, sql: None };
        let items = with_eval(s, false, &[], |cx| sources::items(cx, &src))?.map_err(other)?;
        let names: Vec<&str> = items.iter().take(30).map(|i| i.name.as_str()).collect();
        return Err(bad("table", format!("missing `table` or `sql`; the database offers: {}", names.join(", "))));
    }
    let name = table.clone().unwrap_or_else(|| default_name.to_string());
    create(s, p, QuerySource::Odbc { connection, table, sql }, name)
}

fn from_odbc(s: &mut Session, p: &Json) -> Result<Json> {
    let conn = str_param(p, "connection").ok_or_else(|| bad("data.fromOdbc", "missing `connection`"))?.to_string();
    odbc_source(s, p, conn, "Query1")
}

/// ODBC connection-string value, braced when it holds separators.
fn conn_value(v: &str) -> String {
    if v.contains([';', '{', '}', '=']) { format!("{{{}}}", v.replace('}', "}}")) } else { v.to_string() }
}

/// The ODBC connection string From SQL Server Database uses: the SQL Server driver Windows ships,
/// Windows authentication.
pub fn sql_server_connection(server: &str, database: Option<&str>) -> String {
    let mut conn = format!("Driver={{SQL Server}};Server={};Trusted_Connection=Yes;", conn_value(server));
    if let Some(db) = database.filter(|d| !d.trim().is_empty()) {
        conn.push_str(&format!("Database={};", conn_value(db)));
    }
    conn
}

/// The ODBC connection string From Microsoft Access Database uses.
pub fn access_connection(path: &str) -> String {
    format!("Driver={{Microsoft Access Driver (*.mdb, *.accdb)}};Dbq={};", conn_value(path))
}

fn from_sql_server(s: &mut Session, p: &Json) -> Result<Json> {
    let server = str_param(p, "server").ok_or_else(|| bad("data.fromSqlServer", "missing `server`"))?;
    let conn = sql_server_connection(server, str_param(p, "database"));
    odbc_source(s, p, conn, "Query1")
}

fn from_access(s: &mut Session, p: &Json) -> Result<Json> {
    let path = str_param(p, "path").ok_or_else(|| bad("data.fromAccess", "missing `path`"))?;
    let conn = access_connection(path);
    odbc_source(s, p, conn, &stem(path))
}

fn from_table_range(s: &mut Session, p: &Json) -> Result<Json> {
    // A table by name, the table at the selection, else the range (or its current region) made a
    // table first, as Excel's Create Table step does.
    let table = match str_param(p, "table") {
        Some(t) => {
            s.doc()?.wb.table(t).ok_or_else(|| bad("table", format!("no table named `{t}`")))?;
            t.to_string()
        }
        None => {
            let r = target_range(s, p)?;
            let d = s.doc()?;
            let si = target_sheet(s, p)?;
            let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
            match sh.tables.iter().find(|t| t.range.contains(r.start)) {
                Some(t) => t.name.clone(),
                None => {
                    let r = if r.is_single() { crate::selection::current_region(sh, r.start) } else { r };
                    if sh.used_range().is_none_or(|u| !u.intersects(&r)) {
                        return Err(EngineError::Other("Select a cell in a table or a range of data first.".into()));
                    }
                    let range = format!("{}!{}", gridcraft_formula::quote_sheet(&sh.name), r.a1());
                    let made = s.execute("insert.table", json!({"range": range}))?;
                    made.get("table").and_then(Json::as_str).map(str::to_string).ok_or_else(|| other("couldn't create the table".into()))?
                }
            }
        }
    };
    create(s, p, QuerySource::CurrentWorkbook { name: table.clone() }, table)
}

fn blank_query(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").unwrap_or("Query1").to_string();
    check_name(&name)?;
    let mut p = p.clone();
    if let Some(o) = p.as_object_mut() {
        // A blank query isn't loaded until it has content, unless asked.
        o.entry("load").or_insert(json!("none"));
    }
    add_query(s, &p, Query::new(name, QuerySource::Blank))
}

fn merge_queries(s: &mut Session, p: &Json) -> Result<Json> {
    let left = str_param(p, "left").ok_or_else(|| bad("data.mergeQueries", "missing `left`"))?.to_string();
    let right = str_param(p, "right").ok_or_else(|| bad("data.mergeQueries", "missing `right`"))?.to_string();
    query_index(s, &left)?;
    query_index(s, &right)?;
    let strings = |k: &str| -> Result<Vec<String>> {
        let v = p.get(k).ok_or_else(|| bad("data.mergeQueries", format!("missing `{k}`")))?;
        serde_json::from_value(v.clone()).map_err(|_| bad(k, "a list of column names"))
    };
    let join = match p.get("join") {
        Some(j) => serde_json::from_value(j.clone()).map_err(|_| bad("join", "leftOuter, rightOuter, fullOuter, inner, leftAnti or rightAnti"))?,
        None => Default::default(),
    };
    let columns: Vec<String> = match p.get("columns") {
        Some(c) => serde_json::from_value(c.clone()).map_err(|_| bad("columns", "a list of column names"))?,
        None => vec![],
    };
    let mut q = Query::new(str_param(p, "name").unwrap_or("Merge1"), QuerySource::Query { name: left });
    q.steps.push(QueryStep {
        name: String::new(),
        op: StepOp::MergeQueries {
            query: right.clone(),
            left: strings("leftColumns")?,
            right: strings("rightColumns")?,
            join,
            columns,
            prefix: Some(right),
        },
    });
    add_query(s, p, q)
}

fn append_queries(s: &mut Session, p: &Json) -> Result<Json> {
    let list: Vec<String> = p
        .get("queries")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| bad("data.appendQueries", "missing `queries` (two or more query names)"))?;
    if list.len() < 2 {
        return Err(bad("queries", "name two or more queries"));
    }
    for n in &list {
        query_index(s, n)?;
    }
    let (first, rest) = list.split_first().ok_or_else(|| bad("queries", "empty"))?;
    let mut q = Query::new(str_param(p, "name").unwrap_or("Append1"), QuerySource::Query { name: first.clone() });
    q.steps.push(QueryStep { name: String::new(), op: StepOp::AppendQueries { queries: rest.to_vec() } });
    add_query(s, p, q)
}

fn power_query_editor(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(n) = str_param(p, "query") {
        query_index(s, n)?;
    }
    s.ui_requests.push(crate::UiRequest::Dialog("powerQueryEditor".into(), json!({"query": str_param(p, "query")})));
    list(s, p)
}

fn data_source_settings(s: &mut Session, p: &Json) -> Result<Json> {
    if let (Some(from), Some(to)) = (str_param(p, "from"), str_param(p, "to")) {
        let (from, to) = (from.to_string(), to.to_string());
        let changed = edit(s, |cx| {
            let mut n = 0;
            for q in cx.wb.queries.iter_mut() {
                let loc = match &mut q.source {
                    QuerySource::Csv { path, .. }
                    | QuerySource::Workbook { path, .. }
                    | QuerySource::Xml { path, .. }
                    | QuerySource::Json { path, .. }
                    | QuerySource::Folder { path, .. } => path,
                    QuerySource::Web { url, .. } | QuerySource::OData { url, .. } => url,
                    QuerySource::Odbc { connection, .. } => connection,
                    _ => continue,
                };
                if *loc == from {
                    loc.clone_from(&to);
                    n += 1;
                }
            }
            Ok(n)
        })?;
        if changed == 0 {
            return Err(bad("from", format!("no query reads `{from}`")));
        }
    } else if p.as_object().is_none_or(|o| o.is_empty()) {
        s.ui_requests.push(crate::UiRequest::Dialog("dataSourceSettings".into(), json!({})));
    }
    let wb = &s.doc()?.wb;
    let mut sources: Vec<Json> = Vec::new();
    for q in &wb.queries {
        let Some(loc) = q.source.location() else { continue };
        let kind = serde_json::to_value(&q.source).ok().and_then(|v| v.get("kind").cloned()).unwrap_or(Json::Null);
        match sources.iter_mut().find(|x| x["location"] == json!(loc)) {
            Some(x) => {
                if let Some(a) = x["queries"].as_array_mut() {
                    a.push(json!(q.name));
                }
            }
            None => sources.push(json!({"location": loc, "kind": kind, "queries": [q.name]})),
        }
    }
    Ok(json!({"sources": sources}))
}

fn query_options(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(b) = bool_param(p, "loadToWorksheet") {
        s.prefs.load_to_worksheet = b;
    }
    Ok(json!({"loadToWorksheet": s.prefs.load_to_worksheet}))
}

fn recent_sources(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "clear") == Some(true) {
        s.prefs.recent_sources.clear();
    } else if p.as_object().is_none_or(|o| o.is_empty()) {
        s.ui_requests.push(crate::UiRequest::Dialog("recentSources".into(), json!({})));
    }
    Ok(json!({"sources": s.prefs.recent_sources}))
}

fn existing_connections(s: &mut Session, _: &Json) -> Result<Json> {
    let wb = &s.doc()?.wb;
    Ok(json!({"connections": wb.queries.iter().map(|q| json!({
        "name": format!("Query - {}", q.name),
        "query": q.name,
        "description": q.description,
        "location": q.source.location(),
        "loaded": q.load.table,
    })).collect::<Vec<_>>()}))
}

fn refresh_all(s: &mut Session, p: &Json) -> Result<Json> {
    let replace = bool_param(p, "replace").unwrap_or(false);
    let idx: Vec<usize> = s.doc()?.wb.queries.iter().enumerate().filter(|(_, q)| q.refresh.in_refresh_all).map(|(i, _)| i).collect();
    // Excel's queries refresh too (their M formulas run).
    let excel: Vec<String> = s.doc()?.wb.excel_queries.iter().map(|q| q.name.clone()).collect();
    refresh_indices(s, idx, excel, true, Some(replace))
}

fn refresh_one(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(n) = str_param(p, "query").or_else(|| str_param(p, "name"))
        && let Some(name) = excel_only(&s.doc()?.wb, n)
    {
        return refresh_indices(s, vec![], vec![name], true, None);
    }
    let qi = match str_param(p, "query").or_else(|| str_param(p, "name")) {
        Some(n) => query_index(s, n)?,
        None => match active_query(s) {
            Some(i) => i,
            None => {
                let d = s.doc()?;
                let in_pivot =
                    d.wb.active().is_some_and(|sh| sh.pivots.iter().any(|pt| pt.last_range.is_some_and(|r| r.contains(d.selection.active))));
                if in_pivot {
                    return s.execute("pivot.refresh", json!({}));
                }
                if let Some(name) = active_excel_query(s) {
                    return refresh_indices(s, vec![], vec![name], true, None);
                }
                return Err(EngineError::Other("Select a cell in a query's table or a PivotTable to refresh it.".into()));
            }
        },
    };
    refresh_indices(s, vec![qi], vec![], true, None)
}

fn refresh_status(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(json!({"running": false, "queries": d.wb.queries.iter().map(|q| {
        let at = d.refreshed_at.iter().find(|(n, _)| n.eq_ignore_ascii_case(&q.name)).map(|(_, t)| *t);
        json!({"query": q.name, "rows": q.status.rows, "error": q.status.error, "refreshedAt": at})
    }).collect::<Vec<_>>()}))
}

fn properties_json(q: &Query) -> Json {
    json!({
        "query": q.name,
        "description": q.description,
        "refreshOnOpen": q.refresh.on_open,
        "everyMinutes": q.refresh.every_minutes,
        "background": q.refresh.background,
        "inRefreshAll": q.refresh.in_refresh_all,
        "load": q.load,
    })
}

fn set_properties(s: &mut Session, p: &Json, qi: usize) -> Result<Json> {
    let keys = ["description", "refreshOnOpen", "everyMinutes", "background", "inRefreshAll", "newName"];
    if !keys.iter().any(|k| p.get(*k).is_some()) {
        let q = s.doc()?.wb.queries.get(qi).ok_or_else(|| bad("query", "gone"))?;
        return Ok(properties_json(q));
    }
    if let Some(n) = str_param(p, "newName") {
        let old = s.doc()?.wb.queries.get(qi).map(|q| q.name.clone()).unwrap_or_default();
        rename(s, &json!({"name": old, "newName": n}))?;
    }
    edit(s, |cx| {
        let q = cx.wb.queries.get_mut(qi).ok_or_else(|| bad("query", "gone"))?;
        if let Some(d) = str_param(p, "description") {
            q.description = d.chars().take(4096).collect();
        }
        if let Some(b) = bool_param(p, "refreshOnOpen") {
            q.refresh.on_open = b;
        }
        if let Some(m) = u32_param(p, "everyMinutes") {
            q.refresh.every_minutes = m.min(32_767);
        }
        if let Some(b) = bool_param(p, "background") {
            q.refresh.background = b;
        }
        if let Some(b) = bool_param(p, "inRefreshAll") {
            q.refresh.in_refresh_all = b;
        }
        Ok(properties_json(q))
    })
}

fn connection_properties(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = match str_param(p, "query").or_else(|| str_param(p, "name")) {
        Some(n) => query_index(s, n)?,
        None => match active_query(s) {
            Some(i) => i,
            None => {
                if p.as_object().is_none_or(|o| o.is_empty()) {
                    s.ui_requests.push(crate::UiRequest::Dialog("existingConnections".into(), json!({})));
                    return existing_connections(s, p);
                }
                return Err(bad("data.connectionProperties", "missing `query`"));
            }
        },
    };
    if p.as_object().is_some_and(|o| o.keys().all(|k| k == "query" || k == "name")) {
        let name = s.doc()?.wb.queries.get(qi).map(|q| q.name.clone());
        s.ui_requests.push(crate::UiRequest::Dialog("queryProperties".into(), json!({"query": name})));
    }
    set_properties(s, p, qi)
}

fn properties(s: &mut Session, p: &Json) -> Result<Json> {
    connection_properties(s, p)
}

fn queries_connections(s: &mut Session, p: &Json) -> Result<Json> {
    s.queries_pane = bool_param(p, "on").unwrap_or(!s.queries_pane);
    let l = list(s, p)?;
    Ok(json!({"on": s.queries_pane, "queries": l}))
}

fn enable_content(s: &mut Session, _: &Json) -> Result<Json> {
    {
        let d = s.doc_mut()?;
        d.external_blocked = false;
    }
    // WEBSERVICE calls recalculate with the fetcher; refresh-on-open queries refresh.
    s.sync_web_service();
    let has_web = s
        .doc()?
        .wb
        .sheets
        .iter()
        .any(|sh| sh.cells.iter().any(|(_, c)| c.formula.as_ref().is_some_and(|f| f.text.to_ascii_uppercase().contains("WEBSERVICE("))));
    if has_web {
        let d = s.doc_mut()?;
        let mut wb = (*d.wb).clone();
        d.calc.recalc_all(&mut wb);
        d.wb = std::sync::Arc::new(wb);
    }
    let (idx, excel) = refresh_on_open(&s.doc()?.wb);
    let refreshed = if idx.is_empty() && excel.is_empty() { json!(null) } else { refresh_indices(s, idx, excel, true, None)? };
    Ok(json!({"enabled": true, "refreshed": refreshed}))
}

fn trust_center(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(v) = p.get("externalContent") {
        s.prefs.external_content = serde_json::from_value(v.clone()).map_err(|_| bad("externalContent", "enableAll, prompt or disableAll"))?;
        if s.prefs.external_content == crate::ExternalContent::EnableAll
            && let Some(d) = s.active_mut()
        {
            d.external_blocked = false;
        }
    }
    Ok(json!({"externalContent": s.prefs.external_content}))
}

// ---------------------------------------------------------------- primitives

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.queries
            .iter()
            .map(|q| {
                let kind = serde_json::to_value(&q.source).ok().and_then(|v| v.get("kind").cloned()).unwrap_or(Json::Null);
                json!({
                    "name": q.name,
                    "description": q.description,
                    "source": kind,
                    "location": q.source.location(),
                    "steps": q.steps.len(),
                    "load": q.load,
                    "rows": q.status.rows,
                    "error": q.status.error,
                    "external": q.source.is_external(),
                })
            })
            .collect(),
    ))
}

fn get(s: &mut Session, p: &Json) -> Result<Json> {
    let name = name_param(p, "query.get")?;
    let d = s.doc()?;
    if !d.wb.queries.iter().any(|q| q.name.eq_ignore_ascii_case(name))
        && let Some(q) = d.wb.excel_queries.iter().find(|q| q.name.eq_ignore_ascii_case(name))
    {
        let j = excel_json(&d.wb, q);
        let refreshable = j["refreshable"].clone();
        return Ok(json!({"query": j, "m": q.formula, "excel": true, "refreshable": refreshable}));
    }
    let qi = query_index(s, name)?;
    let q = s.doc()?.wb.queries.get(qi).ok_or_else(|| bad("query", "gone"))?;
    Ok(json!({"query": q, "steps": mtext::step_names(q), "m": mtext::query(q)}))
}

/// An Excel query as JSON, with what GridCraft can't run in its M formula (`unsupported`: M
/// functions it lacks and unknown names; `syntaxError` when the formula doesn't parse).
fn excel_json(wb: &gridcraft_model::Workbook, q: &gridcraft_model::ExcelQuery) -> Json {
    let mut v = serde_json::to_value(q).unwrap_or(Json::Null);
    let is_query = |n: &str| wb.excel_queries.iter().any(|x| x.name == n) || wb.queries.iter().any(|x| x.name.eq_ignore_ascii_case(n));
    let (unsupported, syntax) = match m::unsupported(&q.formula, &is_query) {
        Ok(u) => (u, None),
        Err(e) => (vec![], Some(e.to_string())),
    };
    if let Json::Object(o) = &mut v {
        o.insert("load".into(), json!(q.load_kind()));
        o.insert("refreshable".into(), json!(unsupported.is_empty() && syntax.is_none()));
        o.insert("unsupported".into(), json!(unsupported));
        if let Some(e) = syntax {
            o.insert("syntaxError".into(), json!(e));
        }
    }
    v
}

fn list_excel(s: &mut Session, _: &Json) -> Result<Json> {
    let wb = &s.doc()?.wb;
    Ok(Json::Array(wb.excel_queries.iter().map(|q| excel_json(wb, q)).collect()))
}

/// References to query `old` in other queries, renamed to `new`.
fn rename_refs(queries: &mut [Query], old: &str, new: &str) {
    for q in queries.iter_mut() {
        if let QuerySource::Query { name } = &mut q.source
            && name.eq_ignore_ascii_case(old)
        {
            *name = new.to_string();
        }
        for st in q.steps.iter_mut() {
            match &mut st.op {
                StepOp::MergeQueries { query, .. } if query.eq_ignore_ascii_case(old) => *query = new.to_string(),
                StepOp::AppendQueries { queries } => {
                    for x in queries.iter_mut().filter(|x| x.eq_ignore_ascii_case(old)) {
                        *x = new.to_string();
                    }
                }
                _ => {}
            }
        }
    }
}

fn set(s: &mut Session, p: &Json) -> Result<Json> {
    let v = p.get("query").ok_or_else(|| bad("query.set", "missing `query`"))?;
    let q = parse_query(v)?;
    let replace = str_param(p, "replace").map(str::to_string);
    let existing = {
        let wb = &s.doc()?.wb;
        let key = replace.as_deref().unwrap_or(&q.name);
        wb.queries.iter().position(|x| x.name.eq_ignore_ascii_case(key))
    };
    {
        let wb = &s.doc()?.wb;
        if wb.queries.iter().enumerate().any(|(i, x)| Some(i) != existing && x.name.eq_ignore_ascii_case(&q.name)) || excel_named(wb, &q.name) {
            return Err(EngineError::Other(format!("A query named '{}' already exists.", q.name)));
        }
        if existing.is_none() && wb.queries.len() >= MAX_QUERIES {
            return Err(EngineError::Other(format!("A workbook can hold at most {MAX_QUERIES} queries.")));
        }
        if q.source.is_external() {
            s.doc()?.external_allowed(&s.prefs).map_err(EngineError::Other)?;
        }
    }
    remember(s, &q.source);
    let name = q.name.clone();
    let qi = edit(s, |cx| {
        Ok(match existing {
            Some(i) => {
                let old = cx.wb.queries.get(i).map(|x| x.name.clone()).unwrap_or_default();
                let mut q = q;
                // The loaded table and its place stay with the query unless the definition moves it.
                if let Some(prev) = cx.wb.queries.get(i)
                    && q.load.table.is_none()
                {
                    q.load.table.clone_from(&prev.load.table);
                }
                if let Some(slot) = cx.wb.queries.get_mut(i) {
                    *slot = q;
                }
                if !old.eq_ignore_ascii_case(&name) {
                    rename_refs(&mut cx.wb.queries, &old, &name);
                }
                i
            }
            None => {
                cx.wb.queries.push(q);
                cx.wb.queries.len() - 1
            }
        })
    })?;
    let mut r = json!({"query": name, "index": qi});
    if bool_param(p, "refresh").unwrap_or(false) {
        r["refresh"] = refresh_indices(s, vec![qi], vec![], false, None)?;
    }
    Ok(r)
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.delete")?)?;
    let delete_table = bool_param(p, "deleteTable").unwrap_or(false);
    edit(s, |cx| {
        if qi >= cx.wb.queries.len() {
            return Err(bad("query", "gone"));
        }
        let q = cx.wb.queries.remove(qi);
        if delete_table && let Some(t) = &q.load.table {
            load::remove_table(cx, t).map_err(EngineError::Other)?;
        }
        Ok(json!({"deleted": q.name}))
    })
}

fn rename(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.rename")?)?;
    let new = str_param(p, "newName").ok_or_else(|| bad("query.rename", "missing `newName`"))?.trim().to_string();
    check_name(&new)?;
    let wb = &s.doc()?.wb;
    if wb.queries.iter().enumerate().any(|(i, q)| i != qi && q.name.eq_ignore_ascii_case(&new)) || excel_named(wb, &new) {
        return Err(EngineError::Other(format!("A query named '{new}' already exists.")));
    }
    edit(s, |cx| {
        let old = cx.wb.queries.get(qi).map(|q| q.name.clone()).ok_or_else(|| bad("query", "gone"))?;
        if let Some(q) = cx.wb.queries.get_mut(qi) {
            q.name.clone_from(&new);
        }
        rename_refs(&mut cx.wb.queries, &old, &new);
        Ok(json!({"query": new}))
    })
}

fn duplicate(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.duplicate")?)?;
    let mut q = s.doc()?.wb.queries.get(qi).cloned().ok_or_else(|| bad("query", "gone"))?;
    q.name = str_param(p, "newName").map(str::to_string).unwrap_or_else(|| q.name.clone());
    q.load = Default::default();
    q.load.to = LoadTo::ConnectionOnly;
    q.status = Default::default();
    insert_new(s, q)
}

fn reference(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.reference")?)?;
    let base = s.doc()?.wb.queries.get(qi).map(|q| q.name.clone()).ok_or_else(|| bad("query", "gone"))?;
    let mut q = Query::new(str_param(p, "newName").map(str::to_string).unwrap_or_else(|| base.clone()), QuerySource::Query { name: base });
    q.load.to = LoadTo::ConnectionOnly;
    insert_new(s, q)
}

/// Adds a query (named uniquely) without loading it.
fn insert_new(s: &mut Session, mut q: Query) -> Result<Json> {
    if s.doc()?.wb.queries.len() >= MAX_QUERIES {
        return Err(EngineError::Other(format!("A workbook can hold at most {MAX_QUERIES} queries.")));
    }
    check_name(&q.name)?;
    q.name = unique_query_name(&s.doc()?.wb, &q.name);
    let name = q.name.clone();
    edit(s, |cx| {
        cx.wb.queries.push(q);
        Ok(json!({"query": name}))
    })
}

/// A step position from `step`: an index (1 = the first step after Source) or a step name.
fn step_index(q: &Query, v: Option<&Json>) -> Result<usize> {
    match v {
        Some(Json::Number(n)) => {
            let i = n.as_u64().ok_or_else(|| bad("step", "a step number"))? as usize;
            if i == 0 || i > q.steps.len() {
                return Err(bad("step", format!("the query has steps 1 to {} after Source", q.steps.len())));
            }
            Ok(i - 1)
        }
        Some(Json::String(name)) => {
            q.steps.iter().position(|s| s.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad("step", format!("no step named `{name}`")))
        }
        _ => Err(bad("step", "missing `step` (number or name)")),
    }
}

fn add_step(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.addStep")?)?;
    let v = p.get("step").ok_or_else(|| bad("query.addStep", "missing `step`"))?;
    let step: QueryStep = serde_json::from_value(v.clone()).map_err(|e| bad("step", format!("not a step: {e}")))?;
    edit(s, |cx| {
        let q = cx.wb.queries.get_mut(qi).ok_or_else(|| bad("query", "gone"))?;
        if q.steps.len() >= MAX_STEPS {
            return Err(EngineError::Other(format!("A query can have at most {MAX_STEPS} steps.")));
        }
        let at = match p.get("at").and_then(Json::as_u64) {
            Some(i) => (i as usize).saturating_sub(1).min(q.steps.len()),
            None => q.steps.len(),
        };
        q.steps.insert(at, step);
        name_steps(q);
        Ok(json!({"query": q.name, "step": q.steps.get(at).map(|s| s.name.clone()), "index": at + 1}))
    })
}

fn remove_step(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.removeStep")?)?;
    edit(s, |cx| {
        let q = cx.wb.queries.get_mut(qi).ok_or_else(|| bad("query", "gone"))?;
        let i = step_index(q, p.get("step"))?;
        let removed = q.steps.remove(i);
        Ok(json!({"query": q.name, "removed": removed.name}))
    })
}

fn rename_step(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.renameStep")?)?;
    let new = str_param(p, "newName").ok_or_else(|| bad("query.renameStep", "missing `newName`"))?.trim().to_string();
    check_name(&new)?;
    edit(s, |cx| {
        let q = cx.wb.queries.get_mut(qi).ok_or_else(|| bad("query", "gone"))?;
        let i = step_index(q, p.get("step"))?;
        if new.eq_ignore_ascii_case("Source") || q.steps.iter().enumerate().any(|(j, s)| j != i && s.name.eq_ignore_ascii_case(&new)) {
            return Err(EngineError::Other(format!("A step named '{new}' already exists.")));
        }
        if let Some(st) = q.steps.get_mut(i) {
            st.name.clone_from(&new);
        }
        Ok(json!({"query": q.name, "step": new}))
    })
}

fn move_step(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.moveStep")?)?;
    let to = p.get("to").and_then(Json::as_u64).ok_or_else(|| bad("query.moveStep", "missing `to`"))? as usize;
    edit(s, |cx| {
        let q = cx.wb.queries.get_mut(qi).ok_or_else(|| bad("query", "gone"))?;
        let i = step_index(q, p.get("step"))?;
        if to == 0 || to > q.steps.len() {
            return Err(bad("to", format!("a position from 1 to {}", q.steps.len())));
        }
        let st = q.steps.remove(i);
        q.steps.insert(to - 1, st);
        Ok(json!({"query": q.name}))
    })
}

/// The query a call names (`name`) or defines inline (`query`).
fn named_or_inline(s: &Session, p: &Json, id: &str) -> Result<Query> {
    if let Some(v) = p.get("query").filter(|v| v.is_object()) {
        return parse_query(v);
    }
    let qi = query_index(s, name_param(p, id)?)?;
    s.doc()?.wb.queries.get(qi).cloned().ok_or_else(|| bad("query", "gone"))
}

fn preview(s: &mut Session, p: &Json) -> Result<Json> {
    let q = named_or_inline(s, p, "query.preview")?;
    let steps = p.get("step").and_then(Json::as_u64).map(|n| n as usize);
    let rows = p.get("rows").and_then(Json::as_u64).map_or(PREVIEW_ROWS, |n| (n as usize).min(100_000));
    let drafts: Vec<Query> = p.get("drafts").and_then(|v| serde_json::from_value::<Vec<Query>>(v.clone()).ok()).unwrap_or_default();
    let mut all = drafts;
    all.push(q.clone());
    let r = with_eval(s, false, &all, |cx| cx.evaluate(&q, steps))?;
    let step = steps.unwrap_or(q.steps.len()).min(q.steps.len());
    let mut out = match r {
        Ok(t) => table_json(&t, rows),
        Err(e) => json!({"columns": [], "types": [], "rows": [], "rowCount": 0, "error": e}),
    };
    out["formula"] = json!(mtext::step_formula(&q, step));
    out["steps"] = json!(mtext::step_names(&q));
    Ok(out)
}

fn navigator(s: &mut Session, p: &Json) -> Result<Json> {
    let src: QuerySource = p
        .get("source")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| bad("query.navigator", "missing `source` ({kind: web, url} | {kind: xml, path} …)"))?;
    if let QuerySource::Web { url, .. } | QuerySource::OData { url, .. } = &src {
        access::check_url(url).map_err(|e| bad("url", e))?;
    }
    if src.is_external() {
        s.doc()?.external_allowed(&s.prefs).map_err(EngineError::Other)?;
    }
    let items = with_eval(s, false, &[], |cx| sources::items(cx, &src))?.map_err(other)?;
    Ok(json!({"items": items}))
}

fn load_to(s: &mut Session, p: &Json) -> Result<Json> {
    let qi = query_index(s, name_param(p, "query.load")?)?;
    let mut q = s.doc()?.wb.queries.get(qi).cloned().ok_or_else(|| bad("query", "gone"))?;
    let moved = str_param(p, "sheet").is_some() || str_param(p, "newSheet").is_some() || bool_param(p, "newSheet") == Some(true);
    load_params(s, p, &mut q)?;
    if bool_param(p, "newSheet") == Some(true) {
        q.load.sheet = None;
        q.load.cell = None;
    }
    let old_table = q.load.table.clone();
    // A new place for the table: the old one goes.
    edit(s, |cx| {
        if moved && let Some(t) = &old_table {
            load::remove_table(cx, t).map_err(EngineError::Other)?;
        }
        if let Some(slot) = cx.wb.queries.get_mut(qi) {
            let mut q = q;
            if moved {
                q.load.table = None;
            }
            *slot = q;
        }
        Ok(())
    })?;
    refresh_indices(s, vec![qi], vec![], false, None)
}

fn m_text(s: &mut Session, p: &Json) -> Result<Json> {
    let q = named_or_inline(s, p, "query.mText")?;
    Ok(json!({"query": q.name, "m": mtext::query(&q)}))
}

// ---------------------------------------------------------------- opening and scheduled refresh

/// Whether a workbook holds external queries or WEBSERVICE calls (what the Security Warning is
/// about).
pub fn has_external_content(wb: &gridcraft_model::Workbook) -> bool {
    wb.queries.iter().any(|q| reaches_outside(wb, q, 0))
        || wb.excel_queries.iter().any(|q| m::reaches_outside(&q.formula))
        || wb.sheets.iter().any(|sh| {
            sh.cells
                .iter()
                .any(|(_, c)| c.formula.as_ref().is_some_and(|f| f.text.len() >= 10 && f.text.to_ascii_uppercase().contains("WEBSERVICE(")))
        })
}

/// The queries set to refresh when the file opens: GridCraft's (by index) and Excel's (by name).
fn refresh_on_open(wb: &gridcraft_model::Workbook) -> (Vec<usize>, Vec<String>) {
    let idx = wb.queries.iter().enumerate().filter(|(_, q)| q.refresh.on_open).map(|(i, _)| i).collect();
    let excel = wb.excel_queries.iter().filter(|q| q.refresh_on_open).map(|q| q.name.clone()).collect();
    (idx, excel)
}

/// After a workbook is opened from a file: blocks its external content under the Prompt
/// setting, or refreshes its refresh-on-open queries when connections are enabled. Returns
/// whether the Security Warning should show.
pub fn after_open(s: &mut Session) -> Result<bool> {
    let prefs = s.prefs.clone();
    let external = has_external_content(&s.doc()?.wb);
    if !external {
        return Ok(false);
    }
    match prefs.external_content {
        crate::ExternalContent::Prompt => {
            s.doc_mut()?.external_blocked = true;
            Ok(true)
        }
        crate::ExternalContent::DisableAll => Ok(false),
        crate::ExternalContent::EnableAll => {
            s.sync_web_service();
            let d = s.doc_mut()?;
            if d.calc.web.is_some() {
                let mut wb = (*d.wb).clone();
                d.calc.recalc_all(&mut wb);
                d.wb = std::sync::Arc::new(wb);
            }
            let (idx, excel) = refresh_on_open(&s.doc()?.wb);
            if !idx.is_empty() || !excel.is_empty() {
                // A failed refresh shows in the queries' status, not as a failed open.
                let _ = refresh_indices(s, idx, excel, true, None);
            }
            Ok(false)
        }
    }
}

/// Queries of the active document due for "refresh every N minutes" at `now` (Unix ms). The
/// app calls this from its frame loop and runs `data.refresh` for each. A query not refreshed in
/// this session starts its interval now.
pub fn due_refreshes(s: &mut Session, now: f64) -> Vec<String> {
    let prefs = s.prefs.clone();
    let Some(d) = s.active_mut() else { return vec![] };
    if d.external_allowed(&prefs).is_err() {
        return vec![];
    }
    let mut due = Vec::new();
    for q in d.wb.queries.iter().filter(|q| q.refresh.every_minutes > 0) {
        match d.refreshed_at.iter().find(|(n, _)| n.eq_ignore_ascii_case(&q.name)).map(|(_, t)| *t) {
            Some(t) if now - t >= f64::from(q.refresh.every_minutes) * 60_000.0 => due.push(q.name.clone()),
            Some(_) => {}
            None => d.refreshed_at.push((q.name.clone(), now)),
        }
    }
    due
}
