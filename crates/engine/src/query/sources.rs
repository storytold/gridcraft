//! The Source step: reading each kind of source into a table, the Navigator's item lists, and
//! the steps Power Query adds on its own when a query is created (Promoted Headers, Changed
//! Type).

use std::sync::Arc;

use super::ojson::Json;
use gridcraft_core::{DateSystem, Value};
use gridcraft_model::query::{ColType, ColumnType, QuerySource, QueryStep, StepOp, WorkbookItemKind};

use super::access::{self, HttpRequest, MAX_DOWNLOAD_BYTES, MAX_FILE_BYTES, MAX_FOLDER_FILES};
use super::{EvalCx, Fetched, MAX_ROWS, NavItem, QResult, QTable, formats, html, unique_names};

/// Most files From Folder's Combine reads.
const MAX_COMBINED_FILES: usize = 1_000;
/// Most pages of an OData feed followed.
const MAX_ODATA_PAGES: usize = 100;

/// What a downloaded or read document turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Html,
    Json,
    Xml,
    Csv,
}

impl EvalCx<'_> {
    /// A file's bytes (cached between previews).
    pub fn file(&mut self, path: &str) -> QResult<Fetched> {
        self.check_external()?;
        let key = format!("file:{path}");
        if !self.fresh
            && let Some(f) = self.cache.get(&key)
        {
            return Ok(f);
        }
        let bytes = self.access.read_file(path, MAX_FILE_BYTES)?;
        let f = Fetched { bytes: Arc::from(bytes), content_type: None };
        self.cache.put(key, f.clone());
        Ok(f)
    }

    /// A URL's content (cached between previews). Only http and https are requested.
    pub fn url(&mut self, url: &str, accept: Option<&str>) -> QResult<Fetched> {
        self.check_external()?;
        access::check_url(url)?;
        self.access.web_available()?;
        let key = format!("web:{}:{url}", accept.unwrap_or(""));
        if !self.fresh
            && let Some(f) = self.cache.get(&key)
        {
            return Ok(f);
        }
        let req = HttpRequest {
            url: url.trim().to_string(),
            accept: accept.map(str::to_string),
            timeout_ms: access::DEFAULT_TIMEOUT_MS,
            max_bytes: MAX_DOWNLOAD_BYTES,
        };
        let resp = self.access.http_get(&req)?;
        if resp.body.len() > MAX_DOWNLOAD_BYTES {
            return Err(format!("The response from {} is larger than {} MB.", access::url_host(url), MAX_DOWNLOAD_BYTES / (1024 * 1024)));
        }
        if !(200..300).contains(&resp.status) {
            return Err(format!("{} answered with HTTP status {}.", access::url_host(url), resp.status));
        }
        let f = Fetched { bytes: Arc::from(resp.body), content_type: resp.content_type };
        self.cache.put(key, f.clone());
        Ok(f)
    }
}

/// Reads a source into a table.
pub fn table(cx: &mut EvalCx<'_>, src: &QuerySource) -> QResult<QTable> {
    match src {
        QuerySource::Csv { path, delimiter, encoding } => {
            let f = cx.file(path)?;
            formats::csv_table(&f.bytes, delimiter.as_deref(), encoding.as_deref())
        }
        QuerySource::Text { text, delimiter } => formats::csv_table(text.as_bytes(), delimiter.as_deref(), None),
        QuerySource::Workbook { path, item, item_kind } => {
            let f = cx.file(path)?;
            let (wb, _) = crate::io::open_bytes(path, &f.bytes).map_err(|e| e.to_string())?;
            formats::workbook_table(&wb, item, *item_kind)
        }
        QuerySource::Xml { path, item } => {
            let f = cx.file(path)?;
            formats::xml_table(&f.bytes, item.as_deref())
        }
        QuerySource::Json { path, item } => {
            let f = cx.file(path)?;
            formats::json_table(&f.bytes, item.as_deref())
        }
        QuerySource::Folder { path, combine, extension } => {
            cx.check_external()?;
            let files = cx.access.list_folder(path, MAX_FOLDER_FILES)?;
            if !combine {
                return Ok(formats::folder_table(&files));
            }
            combine_files(cx, &files, extension.as_deref())
        }
        QuerySource::Web { url, item } => {
            let f = cx.url(url, None)?;
            document_table(&f, url, item.as_deref())
        }
        QuerySource::OData { url, item } => odata(cx, url, item.as_deref()),
        QuerySource::Odbc { connection, table, sql } => {
            cx.check_external()?;
            cx.access.odbc_available()?;
            let db = match (sql.as_deref().filter(|s| !s.trim().is_empty()), table.as_deref()) {
                (Some(sql), _) => cx.access.odbc_query(connection, sql, MAX_ROWS)?,
                (None, Some(t)) => {
                    // Only a table the connection lists can be read, so the name can't carry SQL.
                    let tables = cx.access.odbc_tables(connection)?;
                    let t = tables.iter().find(|x| x.eq_ignore_ascii_case(t)).ok_or_else(|| format!("The database has no table '{t}'."))?;
                    cx.access.odbc_table(connection, t, MAX_ROWS)?
                }
                (None, None) => return Err("Choose a table or enter a SQL statement.".into()),
            };
            let mut columns = db.columns;
            unique_names(&mut columns);
            let mut t = QTable::with_columns(columns);
            for row in db.rows {
                t.push_row(row);
            }
            // Drivers hand dates and times over as text: typed like Power Query types them.
            let sys = cx.wb.date_system;
            let cul = super::types::Culture::of(None);
            for i in 0..t.columns.len() {
                let ty = super::types::detect(t.rows.iter().filter_map(|r| r.get(i)), sys);
                if matches!(ty, ColType::Date | ColType::DateTime | ColType::Time) {
                    for row in t.rows.iter_mut() {
                        if let Some(v) = row.get_mut(i) {
                            *v = super::types::convert(v, ty, &cul, sys);
                        }
                    }
                }
                if let Some(slot) = t.types.get_mut(i) {
                    *slot = ty;
                }
            }
            Ok(t)
        }
        QuerySource::CurrentWorkbook { name } => {
            let wb = cx.wb;
            if wb.table(name).is_some() {
                formats::workbook_table(wb, name, WorkbookItemKind::Table)
            } else if let Some(n) = wb.names.iter().find(|n| n.name.eq_ignore_ascii_case(name)) {
                let (si, r) = formats::name_range(wb, &n.formula).ok_or_else(|| format!("The name '{name}' doesn't refer to a range."))?;
                let sh = wb.sheet(si).ok_or("The name's sheet is missing.")?;
                Ok(formats::range_table(sh, r, true))
            } else {
                Err(format!("Couldn't find a table or range named '{name}' in this workbook."))
            }
        }
        QuerySource::Query { name } => cx.evaluate_named(name),
        QuerySource::Blank => Ok(QTable::default()),
    }
}

/// What a fetched document is: the content type first, then a look at the bytes.
pub fn doc_kind(f: &Fetched, url: &str) -> DocKind {
    let ct = f.content_type.as_deref().unwrap_or("").to_ascii_lowercase();
    let head: String = String::from_utf8_lossy(f.bytes.get(..f.bytes.len().min(2048)).unwrap_or(&[]))
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    let looks_html = head.starts_with("<!doctype html") || head.starts_with("<html") || head.contains("<table") || head.contains("<body");
    if ct.contains("html") {
        return DocKind::Html;
    }
    if ct.contains("json") {
        return DocKind::Json;
    }
    if ct.contains("xml") {
        return if looks_html { DocKind::Html } else { DocKind::Xml };
    }
    if ct.contains("csv") || ct.contains("tab-separated") {
        return DocKind::Csv;
    }
    if head.starts_with('{') || head.starts_with('[') {
        return DocKind::Json;
    }
    if head.starts_with('<') {
        return if looks_html { DocKind::Html } else { DocKind::Xml };
    }
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".json") {
        DocKind::Json
    } else if path.ends_with(".xml") {
        DocKind::Xml
    } else {
        DocKind::Csv
    }
}

/// A downloaded document as a table: the HTML table `item` (`Table 1`… or its caption; the first
/// table without one), or the document read as JSON, XML or CSV.
pub fn document_table(f: &Fetched, url: &str, item: Option<&str>) -> QResult<QTable> {
    match doc_kind(f, url) {
        DocKind::Html => {
            let text = formats::decode(&f.bytes, None);
            let tables = html::tables(&text);
            if tables.is_empty() {
                return Err(format!("The page at {} has no tables.", access::url_host(url)));
            }
            let pick = match item {
                None | Some("") => 0,
                Some(name) => {
                    let by_number = name.strip_prefix("Table ").and_then(|n| n.trim().parse::<usize>().ok()).and_then(|n| n.checked_sub(1));
                    by_number
                        .filter(|i| *i < tables.len())
                        .or_else(|| tables.iter().position(|t| t.caption.eq_ignore_ascii_case(name)))
                        .ok_or_else(|| format!("The page has no table '{name}'."))?
                }
            };
            tables.get(pick).map(html::HtmlTable::to_qtable).ok_or_else(|| "The table is missing.".to_string())
        }
        DocKind::Json => formats::json_table(&f.bytes, item.filter(|i| !i.is_empty())),
        DocKind::Xml => formats::xml_table(&f.bytes, item.filter(|i| !i.is_empty())),
        DocKind::Csv => formats::csv_table(&f.bytes, None, None),
    }
}

/// The Navigator's items for a downloaded document.
pub fn document_items(f: &Fetched, url: &str) -> QResult<Vec<NavItem>> {
    Ok(match doc_kind(f, url) {
        DocKind::Html => {
            let text = formats::decode(&f.bytes, None);
            html::tables(&text)
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let rows = t.rows.len();
                    let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
                    let detail = if t.caption.is_empty() { format!("{rows} rows, {cols} columns") } else { t.caption.clone() };
                    NavItem { name: format!("Table {}", i + 1), kind: "table".into(), detail }
                })
                .collect()
        }
        DocKind::Json => {
            let doc = super::ojson::parse(&formats::decode(&f.bytes, None))?;
            formats::json_items(&doc)
        }
        DocKind::Xml => {
            let root = gridcraft_xlsx::parse_xml(&f.bytes).map_err(|e| format!("The XML couldn't be read: {e}"))?;
            formats::xml_items(&root)
        }
        DocKind::Csv => vec![NavItem { name: String::new(), kind: "table".into(), detail: "Delimited text".into() }],
    })
}

/// Everything a source offers to pick from (the Navigator's left side).
pub fn items(cx: &mut EvalCx<'_>, src: &QuerySource) -> QResult<Vec<NavItem>> {
    match src {
        QuerySource::Web { url, .. } => {
            let f = cx.url(url, None)?;
            document_items(&f, url)
        }
        QuerySource::Xml { path, .. } => {
            let f = cx.file(path)?;
            let root = gridcraft_xlsx::parse_xml(&f.bytes).map_err(|e| format!("The XML couldn't be read: {e}"))?;
            Ok(formats::xml_items(&root))
        }
        QuerySource::Json { path, .. } => {
            let f = cx.file(path)?;
            let doc = super::ojson::parse(&formats::decode(&f.bytes, None))?;
            Ok(formats::json_items(&doc))
        }
        QuerySource::Workbook { path, .. } => {
            let f = cx.file(path)?;
            let (wb, _) = crate::io::open_bytes(path, &f.bytes).map_err(|e| e.to_string())?;
            Ok(formats::workbook_items(&wb))
        }
        QuerySource::OData { url, .. } => {
            let f = cx.url(url, Some("application/json"))?;
            let doc = odata_doc(&f)?;
            Ok(match entity_sets(&doc) {
                Some(sets) => sets.into_iter().map(|(name, _)| NavItem { name, kind: "entitySet".into(), detail: String::new() }).collect(),
                None => vec![NavItem { name: String::new(), kind: "table".into(), detail: "The feed".into() }],
            })
        }
        QuerySource::Odbc { connection, .. } => {
            cx.check_external()?;
            cx.access.odbc_available()?;
            Ok(cx.access.odbc_tables(connection)?.into_iter().map(|name| NavItem { name, kind: "table".into(), detail: String::new() }).collect())
        }
        QuerySource::CurrentWorkbook { .. } => Ok(formats::workbook_items(cx.wb).into_iter().filter(|i| i.kind != "sheet").collect()),
        _ => Ok(vec![NavItem { name: String::new(), kind: "table".into(), detail: String::new() }]),
    }
}

fn odata_doc(f: &Fetched) -> QResult<Json> {
    super::ojson::parse(&formats::decode(&f.bytes, None)).map_err(|e| format!("The OData feed isn't JSON. {e}"))
}

/// A service document's entity sets (name, relative URL), if `doc` is one.
fn entity_sets(doc: &Json) -> Option<Vec<(String, String)>> {
    let context = doc.get("@odata.context").and_then(Json::as_str).unwrap_or("");
    let list = doc.get("value")?.as_array()?;
    let sets: Vec<(String, String)> = list
        .iter()
        .filter_map(|e| {
            let o = e.as_object()?;
            let looks = o.keys().all(|k| matches!(k.as_str(), "name" | "url" | "kind" | "title"));
            let kind_ok = o.get("kind").and_then(Json::as_str).is_none_or(|k| k == "EntitySet");
            if !(looks && kind_ok) {
                return None;
            }
            Some((o.get("name")?.as_str()?.to_string(), o.get("url")?.as_str()?.to_string()))
        })
        .collect();
    // A service document lists only entity sets; its context (when given) ends in `$metadata`.
    let context_ok = context.is_empty() || context.ends_with("$metadata");
    (!list.is_empty() && sets.len() == list.len() && context_ok).then_some(sets)
}

/// Joins a feed URL and a relative entity-set URL.
fn join_url(base: &str, rel: &str) -> String {
    if rel.to_ascii_lowercase().starts_with("http://") || rel.to_ascii_lowercase().starts_with("https://") {
        return rel.to_string();
    }
    let base = base.split(['?', '#']).next().unwrap_or(base);
    format!("{}/{}", base.trim_end_matches('/'), rel.trim_start_matches('/'))
}

/// From OData Feed: the entity set `item` of a service root, or the feed itself; pages are
/// followed through `@odata.nextLink`.
fn odata(cx: &mut EvalCx<'_>, url: &str, item: Option<&str>) -> QResult<QTable> {
    let f = cx.url(url, Some("application/json"))?;
    let doc = odata_doc(&f)?;
    let (mut doc, mut page_url) = match (entity_sets(&doc), item.filter(|i| !i.is_empty())) {
        (Some(sets), Some(name)) => {
            let (_, rel) = sets.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).ok_or_else(|| format!("The feed has no entity set '{name}'."))?;
            let u = join_url(url, rel);
            let f = cx.url(&u, Some("application/json"))?;
            (odata_doc(&f)?, u)
        }
        (Some(sets), None) => {
            let names: Vec<&str> = sets.iter().map(|(n, _)| n.as_str()).take(10).collect();
            return Err(format!("This is a service root: choose one of its entity sets ({}…).", names.join(", ")));
        }
        (None, _) => (doc, url.to_string()),
    };
    let mut records: Vec<Json> = Vec::new();
    for page in 0..MAX_ODATA_PAGES {
        let list = doc
            .get("value")
            .or_else(|| doc.get("d").and_then(|d| d.get("results").or(Some(d))))
            .and_then(Json::as_array)
            .ok_or("The OData feed has no list of records ('value').")?;
        records.extend(list.iter().take(MAX_ROWS.saturating_sub(records.len())).cloned());
        let next = doc
            .get("@odata.nextLink")
            .or_else(|| doc.get("odata.nextLink"))
            .or_else(|| doc.get("d").and_then(|d| d.get("__next")))
            .and_then(Json::as_str);
        let Some(next) = next.filter(|_| records.len() < MAX_ROWS && page + 1 < MAX_ODATA_PAGES) else { break };
        page_url = join_url(&page_url, next);
        let f = cx.url(&page_url, Some("application/json"))?;
        doc = odata_doc(&f)?;
    }
    // Annotations aren't data.
    for r in records.iter_mut() {
        if let Some(o) = r.as_object_mut() {
            o.retain(|k| !k.starts_with("@odata.") && !k.starts_with("odata.") && k != "__metadata");
        }
    }
    let mut t = formats::json_doc_table(&Json::Array(records), None)?;
    for i in 0..t.columns.len() {
        let ty = super::types::detect(t.rows.iter().filter_map(|r| r.get(i)), cx.wb.date_system);
        if let Some(slot) = t.types.get_mut(i) {
            *slot = ty;
        }
    }
    Ok(t)
}

/// From Folder › Combine: the files with the chosen extension, read like their own sources
/// (headers promoted for delimited text), appended with a `Source.Name` column first.
fn combine_files(cx: &mut EvalCx<'_>, files: &[access::FileEntry], extension: Option<&str>) -> QResult<QTable> {
    let ext = match extension {
        Some(e) if !e.is_empty() => format!(".{}", e.trim_start_matches('.').to_ascii_lowercase()),
        _ => files
            .iter()
            .find(|f| !f.hidden && matches!(f.extension.as_str(), ".csv" | ".txt" | ".tsv" | ".json" | ".xml"))
            .map(|f| f.extension.clone())
            .ok_or("The folder has no CSV, text, JSON or XML files to combine.")?,
    };
    let mut out: Option<QTable> = None;
    for f in files.iter().filter(|f| !f.hidden && f.extension == ext).take(MAX_COMBINED_FILES) {
        let bytes = cx.file(&f.path())?;
        let mut t = match ext.as_str() {
            ".json" => formats::json_table(&bytes.bytes, None),
            ".xml" => formats::xml_table(&bytes.bytes, None),
            _ => formats::csv_table(&bytes.bytes, None, None).map(promote_first_row),
        }
        .map_err(|e| format!("{}: {e}", f.name))?;
        t.columns.insert(0, super::unique_name(&t.columns, "Source.Name"));
        t.types.insert(0, ColType::Text);
        for row in t.rows.iter_mut() {
            row.insert(0, Value::text(f.name.as_str()));
        }
        out = Some(match out {
            None => t,
            Some(acc) => append_tables(acc, t),
        });
    }
    out.ok_or_else(|| format!("The folder has no {ext} files."))
}

/// `b`'s rows after `a`'s, columns matched by name.
fn append_tables(a: QTable, b: QTable) -> QTable {
    let mut a = a;
    let width_before = a.columns.len();
    let map: Vec<usize> = b
        .columns
        .iter()
        .map(|c| match a.columns.iter().position(|x| x == c) {
            Some(i) => i,
            None => {
                a.columns.push(c.clone());
                a.types.push(ColType::Any);
                a.columns.len() - 1
            }
        })
        .collect();
    let width = a.columns.len();
    if width > width_before {
        for row in a.rows.iter_mut() {
            row.resize(width, Value::Empty);
        }
    }
    for row in b.rows {
        if a.rows.len() >= MAX_ROWS {
            break;
        }
        let mut out = vec![Value::Empty; width];
        for (v, &i) in row.into_iter().zip(&map) {
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        a.rows.push(out);
    }
    a
}

fn promote_first_row(t: QTable) -> QTable {
    let mut rows = t.rows.into_iter();
    let Some(first) = rows.next() else { return QTable::default() };
    let mut columns: Vec<String> = first
        .iter()
        .enumerate()
        .map(|(i, v)| if v.is_empty() || v.as_text() == Some("") { format!("Column{}", i + 1) } else { v.display() })
        .collect();
    unique_names(&mut columns);
    let n = columns.len();
    QTable { columns, types: vec![ColType::Any; n], rows: rows.collect() }
}

/// Whether the first row of a raw table looks like column names: all non-empty, distinct text
/// that isn't a number or a date, above at least one row of data.
pub fn looks_like_headers(t: &QTable, sys: DateSystem) -> bool {
    let Some(first) = t.rows.first() else { return false };
    if t.rows.len() < 2 || first.is_empty() {
        return false;
    }
    let cul = super::types::Culture::of(None);
    let mut seen = std::collections::HashSet::new();
    first.iter().all(|v| match v {
        Value::Text(s) => {
            let s = s.trim();
            !s.is_empty()
                && seen.insert(s.to_string())
                && super::types::parse_number(s, &cul).is_none()
                && super::types::parse_datetime(s, &cul, sys).is_none()
        }
        _ => false,
    })
}

/// The steps Power Query adds by itself to a new query: Promoted Headers where the source gives
/// raw rows whose first row holds names, then Changed Type with the types detected from the
/// first 200 rows (Data Type Detection).
pub fn auto_steps(src: &QuerySource, raw: &QTable, sys: DateSystem) -> Vec<QueryStep> {
    let raw_rows = matches!(
        src,
        QuerySource::Csv { .. }
            | QuerySource::Text { .. }
            | QuerySource::Workbook { item_kind: WorkbookItemKind::Sheet | WorkbookItemKind::DefinedName, .. }
    ) || matches!(src, QuerySource::Web { .. }) && raw.columns.first().is_some_and(|c| c == "Column1");
    let typed_source = matches!(
        src,
        QuerySource::Folder { .. } | QuerySource::Odbc { .. } | QuerySource::OData { .. } | QuerySource::Blank | QuerySource::Query { .. }
    );
    if typed_source || raw.columns.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut data: Vec<&Vec<Value>> = raw.rows.iter().collect();
    let mut names = raw.columns.clone();
    if raw_rows && looks_like_headers(raw, sys) {
        out.push(QueryStep { name: "Promoted Headers".into(), op: StepOp::PromoteHeaders });
        if let Some(first) = raw.rows.first() {
            names = first.iter().enumerate().map(|(i, v)| if v.is_empty() { format!("Column{}", i + 1) } else { v.display() }).collect();
            unique_names(&mut names);
        }
        data.remove(0);
    }
    let columns: Vec<ColumnType> = names
        .iter()
        .enumerate()
        .map(|(i, n)| ColumnType { column: n.clone(), col_type: super::types::detect(data.iter().filter_map(|r| r.get(i)), sys) })
        .map(|mut ct| {
            if ct.col_type == ColType::Any {
                ct.col_type = ColType::Text;
            }
            ct
        })
        .collect();
    if !columns.is_empty() {
        out.push(QueryStep { name: "Changed Type".into(), op: StepOp::ChangeType { columns, locale: None } });
    }
    out
}
