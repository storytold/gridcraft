//! The database queries behind tables (Data › From Database) in an XLSX package.
//!
//! They go in a custom XML part of their own, `customXml/gridcraftDbQueries.xml` (or with a
//! number, when the file already keeps an unrelated part of that name), whose root
//! element is in GridCraft's namespace and holds the queries as JSON text (never a password).
//! Excel keeps custom XML parts when it saves, so a workbook keeps its queries through a round
//! trip; the tables themselves are ordinary tables. Table ids are renumbered on save and read, so
//! each query names its table by sheet and top-left cell, which both keep.

use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{DbQuery, MAX_DB_QUERIES, Preserved, Workbook};
use serde::{Deserialize, Serialize};

use crate::package::Rel;
use crate::read::Ctx;
use crate::write::{Out, Rels, XML_DECL};
use crate::xml::esc;

/// The namespace of the part.
pub const NS_DB_QUERIES: &str = "urn:gridcraft:dbqueries:1";
const ROOT: &str = "gridcraftDbQueries";
const CT_PROPS: &str = "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
const REL_PROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
/// The data store item id of the part (fixed, so saves are reproducible).
const ITEM_ID: &str = "{8E2B6C14-3A57-4F0D-B1C9-5D7E2A94F360}";
/// Largest part read.
const MAX_PART: u64 = 16 * 1024 * 1024;

/// A query as stored: its table by sheet name and top-left cell.
#[derive(Serialize, Deserialize)]
struct Stored {
    sheet: String,
    anchor: String,
    #[serde(flatten)]
    query: DbQuery,
}

/// Reads the queries if the workbook has the part; returns them and the part names that belong
/// to it (so pass-through doesn't keep a second copy).
pub fn read(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> (Vec<(String, String, DbQuery)>, Vec<String>) {
    for r in wb_rels.iter().filter(|r| r.kind.eq_ignore_ascii_case("customXml") && !r.external) {
        let Ok(Some(bytes)) = cx.pkg.read_limited(&r.target, MAX_PART) else { continue };
        // A quick look before parsing: other custom XML is left alone.
        let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(512)).unwrap_or(&[]));
        if !head.contains(ROOT) || !head.contains(NS_DB_QUERIES) {
            continue;
        }
        let mut parts = vec![r.target.clone()];
        if let Ok(item_rels) = cx.pkg.rels(&r.target) {
            parts.extend(item_rels.into_iter().filter(|x| !x.external).map(|x| x.target));
        }
        let stored = crate::xml::parse(&bytes)
            .ok()
            .filter(|root| root.name == ROOT)
            .and_then(|root| serde_json::from_str::<Vec<Stored>>(root.text.trim()).ok());
        return match stored {
            Some(stored) => (stored.into_iter().take(MAX_DB_QUERIES).map(|s| (s.sheet, s.anchor, s.query)).collect::<Vec<_>>(), parts),
            None => {
                cx.warn("the database queries of tables could not be read and were dropped");
                (vec![], parts)
            }
        };
    }
    (vec![], vec![])
}

/// Ties read queries to the tables of `wb` (sheet name and top-left cell); queries whose table
/// is gone are dropped.
pub fn attach(wb: &mut Workbook, read: Vec<(String, String, DbQuery)>) {
    wb.db_queries = read
        .into_iter()
        .filter_map(|(sheet, anchor, mut q)| {
            let at = CellRef::parse(&anchor)?;
            let sh = wb.sheet(wb.sheet_index(&sheet)?)?;
            q.table_id = sh.tables.iter().find(|t| t.range.start == at)?.id;
            Some(q)
        })
        .collect();
}

/// Removes the part (read into the model) from the workbook's kept content.
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

/// Writes the part when tables of the workbook have queries.
pub fn write(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) {
    let live: Vec<Stored> = wb
        .db_queries
        .iter()
        .filter_map(|q| {
            wb.sheets.iter().find_map(|sh| {
                let t = sh.tables.iter().find(|t| t.id == q.table_id)?;
                Some(Stored { sheet: sh.name.clone(), anchor: t.range.start.a1(), query: DbQuery { table_id: 0, ..q.clone() } })
            })
        })
        .collect();
    if live.is_empty() {
        return;
    }
    let Ok(json) = serde_json::to_string(&live) else { return };
    // Kept content may already hold parts of these names (another file's custom XML): take the
    // first numbered set that is free, since a package can't hold a name twice.
    let kept: Vec<&str> = wb.preserved.iter().flat_map(|p| p.parts.iter().chain(&p.vml).map(|x| x.name.as_str())).collect();
    let taken = |name: &str| kept.iter().any(|k| k.eq_ignore_ascii_case(name));
    let Some(n) = (0..=kept.len()).map(|n| if n == 0 { String::new() } else { n.to_string() }).find(|n| {
        ![
            format!("customXml/gridcraftDbQueries{n}.xml"),
            format!("customXml/gridcraftDbQueriesProps{n}.xml"),
            format!("customXml/_rels/gridcraftDbQueries{n}.xml.rels"),
        ]
        .iter()
        .any(|x| taken(x))
    }) else {
        return;
    };
    let mut x = String::from(XML_DECL);
    let _ = write!(x, "<{ROOT} xmlns=\"{NS_DB_QUERIES}\" version=\"1\">{}</{ROOT}>", esc(&json));
    out.default_type("xml", "application/xml");
    out.part(&format!("customXml/gridcraftDbQueries{n}.xml"), None, x.into_bytes());
    let props = format!(
        "{XML_DECL}<ds:datastoreItem ds:itemID=\"{ITEM_ID}\" xmlns:ds=\"http://schemas.openxmlformats.org/officeDocument/2006/customXml\"><ds:schemaRefs><ds:schemaRef ds:uri=\"{NS_DB_QUERIES}\"/></ds:schemaRefs></ds:datastoreItem>"
    );
    out.part(&format!("customXml/gridcraftDbQueriesProps{n}.xml"), Some(CT_PROPS), props.into_bytes());
    let mut item_rels = Rels::default();
    item_rels.add_uri(REL_PROPS, &format!("gridcraftDbQueriesProps{n}.xml"));
    out.part(&format!("customXml/_rels/gridcraftDbQueries{n}.xml.rels"), None, item_rels.xml().into_bytes());
    wb_rels.add("customXml", &format!("../customXml/gridcraftDbQueries{n}.xml"));
}
