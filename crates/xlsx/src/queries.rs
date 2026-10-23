//! GridCraft's Get & Transform queries in an XLSX package.
//!
//! Excel keeps its own Power Query definitions in a `DataMashup` custom XML part (a binary
//! package) with connections and query tables; GridCraft passes those through untouched. Its own
//! query definitions (typed steps, `gridcraft_model::query`) go in a separate custom XML part,
//! `customXml/gridcraftQueries.xml`, whose root element is in GridCraft's namespace and holds the
//! definitions as JSON text. Custom XML parts are kept by Excel when it saves, so a workbook keeps
//! its queries through a round trip; the loaded tables are ordinary tables Excel opens as such.

use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_model::{Preserved, Query, Workbook};

use crate::package::Rel;
use crate::read::Ctx;
use crate::write::{Out, Rels, XML_DECL};
use crate::xml::esc;

/// The namespace of GridCraft's query part.
pub const NS_QUERIES: &str = "urn:gridcraft:queries:1";
const ROOT: &str = "gridcraftQueries";
const PART: &str = "customXml/gridcraftQueries.xml";
const PROPS_PART: &str = "customXml/gridcraftQueriesProps.xml";
const CT_PROPS: &str = "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
const REL_PROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
/// The data store item id of the part (fixed, so saves are reproducible).
const ITEM_ID: &str = "{3C1F8A52-7D4E-4B19-9F2A-6E0B5D8C1A47}";
/// Largest query part read.
const MAX_PART: u64 = 64 * 1024 * 1024;

/// Reads GridCraft's query part if the workbook has one; returns the queries and the part names
/// that belong to it (so pass-through doesn't keep a second copy).
pub fn read(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> (Vec<Query>, Vec<String>) {
    for r in wb_rels.iter().filter(|r| r.kind.eq_ignore_ascii_case("customXml") && !r.external) {
        let Ok(Some(bytes)) = cx.pkg.read_limited(&r.target, MAX_PART) else { continue };
        // A quick look before parsing: Excel's DataMashup and other custom XML are left alone.
        let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(512)).unwrap_or(&[]));
        if !head.contains(ROOT) || !head.contains(NS_QUERIES) {
            continue;
        }
        let Ok(root) = crate::xml::parse(&bytes) else {
            cx.warn("GridCraft's query definitions could not be read and were dropped");
            continue;
        };
        if root.name != ROOT {
            continue;
        }
        let mut parts = vec![r.target.clone()];
        if let Ok(item_rels) = cx.pkg.rels(&r.target) {
            parts.extend(item_rels.into_iter().filter(|x| !x.external).map(|x| x.target));
        }
        // Each query on its own: one this version can't read (a newer GridCraft's step, say)
        // doesn't take the others with it.
        let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(root.text.trim()) else {
            // Not even a list: the part is kept as it is (written back unchanged) and nothing is read.
            cx.warn("GridCraft's query definitions could not be read; they are kept in the file unchanged");
            return (vec![], vec![]);
        };
        let total = items.len();
        let mut queries: Vec<Query> = items.into_iter().filter_map(|q| serde_json::from_value(q).ok()).collect();
        let unreadable = total - queries.len();
        if unreadable > 0 {
            cx.warn(format!("{unreadable} of GridCraft's {total} queries could not be read and were dropped"));
        }
        if queries.len() > gridcraft_model::query::MAX_QUERIES {
            cx.warn(format!("The workbook has {} queries; GridCraft keeps the first {}", queries.len(), gridcraft_model::query::MAX_QUERIES));
            queries.truncate(gridcraft_model::query::MAX_QUERIES);
        }
        return (queries, parts);
    }
    (vec![], vec![])
}

/// Removes the query part (read into the model) from the workbook's kept content.
pub fn unkeep(preserved: &mut Option<Arc<Preserved>>, parts: &[String]) {
    if parts.is_empty() {
        return;
    }
    let Some(p) = preserved.as_mut() else { return };
    let p = Arc::make_mut(p);
    p.rels.retain(|r| r.external || !parts.iter().any(|x| x.eq_ignore_ascii_case(&r.target)));
    p.parts.retain(|x| !parts.iter().any(|n| n.eq_ignore_ascii_case(&x.name)));
    if p.is_empty() {
        *preserved = None;
    }
}

/// Writes the query part when the workbook has queries.
pub fn write(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) {
    if wb.queries.is_empty() {
        return;
    }
    let Ok(json) = serde_json::to_string(&wb.queries) else { return };
    let mut x = String::from(XML_DECL);
    let _ = write!(x, "<{ROOT} xmlns=\"{NS_QUERIES}\" version=\"1\">{}</{ROOT}>", esc(&json));
    out.default_type("xml", "application/xml");
    out.part(PART, None, x.into_bytes());
    let props = format!(
        "{XML_DECL}<ds:datastoreItem ds:itemID=\"{ITEM_ID}\" xmlns:ds=\"http://schemas.openxmlformats.org/officeDocument/2006/customXml\"><ds:schemaRefs><ds:schemaRef ds:uri=\"{NS_QUERIES}\"/></ds:schemaRefs></ds:datastoreItem>"
    );
    out.part(PROPS_PART, Some(CT_PROPS), props.into_bytes());
    let mut item_rels = Rels::default();
    item_rels.add_uri(REL_PROPS, "gridcraftQueriesProps.xml");
    out.part("customXml/_rels/gridcraftQueries.xml.rels", None, item_rels.xml().into_bytes());
    wb_rels.add("customXml", "../customXml/gridcraftQueries.xml");
}
