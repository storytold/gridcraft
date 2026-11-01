//! Get & Transform Data (Power Query): evaluating the queries a workbook keeps
//! (`gridcraft_model::query`) into tables, and loading those into worksheets.
//!
//! A query is a source plus an ordered list of typed steps; evaluation runs the source, then
//! each step, and is deterministic for the same inputs (no hash-order dependence, no clock).
//! External sources (files, the web, databases) go through the injected [`DataAccess`] and are
//! refused while the document's external content is disabled (the Security Warning bar).

pub mod access;
pub mod formats;
pub mod html;
pub mod load;
pub mod m;
pub mod mtext;
pub mod ojson;
pub mod sources;
pub mod steps;
pub mod types;

use std::sync::Arc;

use gridcraft_core::Value;
use gridcraft_model::Workbook;
use gridcraft_model::query::{ColType, Query};

pub use access::{DataAccess, DbTable, FileEntry, HttpRequest, HttpResponse, StdAccess, check_url};

/// Most data rows a query table holds (what a worksheet can show under its header).
pub const MAX_ROWS: usize = 1_048_575;
/// Most columns.
pub const MAX_COLUMNS: usize = 16_384;
/// Deepest chain of queries referring to queries.
pub const MAX_QUERY_DEPTH: usize = 32;
/// Bytes of downloaded and read source content kept between previews.
const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;
/// Sources kept between previews.
const MAX_CACHE_ENTRIES: usize = 32;

pub type QResult<T> = std::result::Result<T, String>;

/// A query result: named, typed columns and rows of values (every row as wide as `columns`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QTable {
    pub columns: Vec<String>,
    pub types: Vec<ColType>,
    pub rows: Vec<Vec<Value>>,
}

impl QTable {
    pub fn with_columns(columns: Vec<String>) -> QTable {
        let n = columns.len();
        QTable { columns, types: vec![ColType::Any; n], rows: vec![] }
    }
    /// Adds a row, padded or cut to the table's width; rows past [`MAX_ROWS`] are dropped.
    pub fn push_row(&mut self, mut row: Vec<Value>) {
        if self.rows.len() >= MAX_ROWS {
            return;
        }
        row.resize(self.columns.len(), Value::Empty);
        self.rows.push(row);
    }
    /// A column's index, or Power Query's error for a missing column.
    pub fn col(&self, name: &str) -> QResult<usize> {
        self.columns.iter().position(|c| c == name).ok_or_else(|| format!("The column '{name}' of the table wasn't found."))
    }
    pub fn cols(&self, names: &[String]) -> QResult<Vec<usize>> {
        names.iter().map(|n| self.col(n)).collect()
    }
    /// The columns at `idx`, in that order.
    pub fn project(self, idx: &[usize]) -> QTable {
        let columns = idx.iter().filter_map(|&i| self.columns.get(i).cloned()).collect();
        let types = idx.iter().filter_map(|&i| self.types.get(i).copied()).collect();
        let rows = self.rows.into_iter().map(|r| idx.iter().map(|&i| r.get(i).cloned().unwrap_or_default()).collect()).collect();
        QTable { columns, types, rows }
    }
}

/// Something a source offers in the Navigator: an HTML table (`Table 1`), a JSON array path, an
/// XML element path, a sheet/table/name, an entity set or a database table.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct NavItem {
    pub name: String,
    pub kind: String,
    pub detail: String,
}

/// Makes column names unique the way Power Query does: a repeat of `Name` becomes `Name_1`.
pub fn unique_names(names: &mut [String]) {
    let mut seen: Vec<String> = Vec::with_capacity(names.len());
    let mut taken: std::collections::HashSet<String> = names.iter().cloned().collect();
    for n in names.iter_mut() {
        if seen.contains(n) {
            let base = n.clone();
            let mut k = 1usize;
            let mut cand = format!("{base}_{k}");
            while taken.contains(&cand) {
                k += 1;
                cand = format!("{base}_{k}");
            }
            taken.insert(cand.clone());
            *n = cand;
        }
        seen.push(n.clone());
    }
}

/// `base`, or `base_1`, `base_2`… when `existing` already has it.
pub fn unique_name(existing: &[String], base: &str) -> String {
    if !existing.iter().any(|c| c == base) {
        return base.to_string();
    }
    (1..).map(|k| format!("{base}_{k}")).find(|c| !existing.contains(c)).unwrap_or_else(|| base.to_string())
}

/// Source content kept between previews, so browsing the Navigator or clicking through Applied
/// Steps doesn't download or read a source again. Refresh clears it.
#[derive(Clone, Default)]
pub struct FetchCache {
    entries: Vec<(String, Fetched)>,
    bytes: usize,
}

impl std::fmt::Debug for FetchCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchCache").field("entries", &self.entries.len()).field("bytes", &self.bytes).finish()
    }
}

/// Bytes read from a file or downloaded, with the response's content type.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub bytes: Arc<[u8]>,
    pub content_type: Option<String>,
}

impl FetchCache {
    pub fn get(&self, key: &str) -> Option<Fetched> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, f)| f.clone())
    }
    pub fn put(&mut self, key: String, f: Fetched) {
        if f.bytes.len() > MAX_CACHE_BYTES {
            return;
        }
        self.entries.retain(|(k, _)| *k != key);
        self.bytes = self.entries.iter().map(|(_, f)| f.bytes.len()).sum();
        while !self.entries.is_empty() && (self.bytes + f.bytes.len() > MAX_CACHE_BYTES || self.entries.len() >= MAX_CACHE_ENTRIES) {
            let (_, old) = self.entries.remove(0);
            self.bytes = self.bytes.saturating_sub(old.bytes.len());
        }
        self.bytes += f.bytes.len();
        self.entries.push((key, f));
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What evaluating queries needs: the workbook (for its tables and other queries), data access,
/// whether external content is enabled, and the fetch cache.
pub struct EvalCx<'a> {
    pub wb: &'a Workbook,
    pub access: &'a dyn DataAccess,
    /// `Ok` when this document may reach external sources; else the reason it may not.
    pub external: Result<(), String>,
    pub cache: &'a mut FetchCache,
    /// Read sources again even when cached (Refresh).
    pub fresh: bool,
    /// Definitions that take precedence over the workbook's queries of the same name (the
    /// Power Query Editor's unsaved drafts).
    pub drafts: &'a [Query],
    stack: Vec<String>,
}

impl<'a> EvalCx<'a> {
    pub fn new(wb: &'a Workbook, access: &'a dyn DataAccess, external: Result<(), String>, cache: &'a mut FetchCache) -> EvalCx<'a> {
        EvalCx { wb, access, external, cache, fresh: false, drafts: &[], stack: vec![] }
    }

    /// A query by name: a draft first, then the workbook's.
    pub fn find(&self, name: &str) -> Option<&'a Query> {
        self.drafts.iter().find(|q| q.name.eq_ignore_ascii_case(name)).or_else(|| self.wb.queries.iter().find(|q| q.name.eq_ignore_ascii_case(name)))
    }

    /// Evaluates `q` through its first `steps` applied steps (all of them when `None`; 0 is the
    /// Source step alone).
    pub fn evaluate(&mut self, q: &Query, steps: Option<usize>) -> QResult<QTable> {
        if self.stack.len() >= MAX_QUERY_DEPTH {
            return Err("Queries refer to each other too deeply.".into());
        }
        if self.stack.iter().any(|n| n.eq_ignore_ascii_case(&q.name)) {
            return Err(format!("A cyclic reference was encountered: the query '{}' refers to itself.", q.name));
        }
        self.stack.push(q.name.clone());
        let r = self.run(q, steps);
        self.stack.pop();
        r
    }

    fn run(&mut self, q: &Query, steps: Option<usize>) -> QResult<QTable> {
        let mut t = sources::table(self, &q.source)?;
        for step in q.steps.iter().take(steps.unwrap_or(usize::MAX)) {
            let name = if step.name.is_empty() { step.op.default_name() } else { step.name.as_str() };
            t = steps::apply(self, t, &step.op).map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(t)
    }

    /// Evaluates the query called `name` (Reference, Merge and Append read other queries).
    pub fn evaluate_named(&mut self, name: &str) -> QResult<QTable> {
        let q = self.find(name).ok_or_else(|| format!("The query '{name}' wasn't found."))?;
        self.evaluate(q, None)
    }

    /// Whether external sources may be read.
    pub fn check_external(&self) -> QResult<()> {
        self.external.clone()
    }
}

/// WEBSERVICE's fetcher over `access`: an http(s) GET with a short timeout and a small size
/// cap, returning the text of a 2xx response.
pub fn web_service_fetcher(access: Arc<dyn DataAccess>) -> gridcraft_calc::WebFetch {
    gridcraft_calc::WebFetch(Arc::new(move |url: &str| {
        check_url(url)?;
        let req = HttpRequest { url: url.trim().to_string(), accept: None, timeout_ms: access::WEBSERVICE_TIMEOUT_MS, max_bytes: 256 * 1024 };
        let resp = access.http_get(&req)?;
        if !(200..300).contains(&resp.status) {
            return Err(format!("HTTP status {}", resp.status));
        }
        Ok(formats::decode(&resp.body, None))
    }))
}

#[cfg(test)]
mod tests;
