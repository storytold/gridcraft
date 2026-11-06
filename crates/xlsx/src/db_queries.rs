//! The database queries behind tables (Data › From Database) in an XLSX package.
//!
//! They go in a custom XML part of their own, `customXml/gridcraftDbQueries.xml` (or with a
//! number, when the file already keeps an unrelated part of that name), whose root
//! element is in GridCraft's namespace and holds the queries as JSON text (never a password).
//! Excel keeps custom XML parts when it saves, so a workbook keeps its queries through a round
//! trip; the tables themselves are ordinary tables. Table ids are renumbered on save and read, so
//! each query names its table by its name as written (`displayName`), which Excel keeps when the
//! sheet is renamed or the table moves. Files written before the name was stored, or with a name
//! two tables share, name it by sheet and top-left cell instead.

use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{DbQuery, MAX_DB_QUERIES, Preserved, Workbook};
use serde::{Deserialize, Serialize};

use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

use crate::IoError;
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
/// Largest part, read or written (a bigger one could not be opened again).
pub(crate) const MAX_PART: u64 = 16 * 1024 * 1024;

/// A query as stored: its table by name, and by sheet name and top-left cell.
#[derive(Serialize, Deserialize)]
pub struct Stored {
    /// The table's name as written; absent in files from before it was stored, and when two
    /// tables of the workbook were written with the same name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) table: Option<String>,
    pub(crate) sheet: String,
    pub(crate) anchor: String,
    #[serde(flatten)]
    pub(crate) query: DbQuery,
}

/// Whether the part's root element is `gridcraftDbQueries` in GridCraft's namespace, as the
/// namespace declarations in scope resolve it (whatever the prefix).
fn is_ours(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes));
    let mut r = NsReader::from_str(&text);
    loop {
        match r.read_resolved_event() {
            Ok((ns, Event::Start(e) | Event::Empty(e))) => {
                return e.local_name().as_ref() == ROOT.as_bytes() && matches!(ns, ResolveResult::Bound(n) if n.as_ref() == NS_DB_QUERIES.as_bytes());
            }
            Ok((_, Event::Eof)) | Err(_) => return false,
            Ok(_) => {}
        }
    }
}

/// Reads the queries if the workbook has the part; returns them and the part names that belong
/// to it (so pass-through doesn't keep a second copy). Only a part that is ours and was read
/// whole is taken: any other custom XML, or ours when it can't be read, stays kept as it is.
pub fn read(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> (Vec<Stored>, Vec<String>) {
    for r in wb_rels.iter().filter(|r| r.kind.eq_ignore_ascii_case("customXml") && !r.external) {
        let Ok(Some(bytes)) = cx.pkg.read_limited(&r.target, MAX_PART) else { continue };
        // A quick look before parsing: other custom XML is left alone.
        let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(512)).unwrap_or(&[]));
        if !head.contains(ROOT) || !head.contains(NS_DB_QUERIES) || !is_ours(&bytes) {
            continue;
        }
        let Some(stored) = crate::xml::parse(&bytes).ok().and_then(|root| serde_json::from_str::<Vec<Stored>>(root.text.trim()).ok()) else {
            cx.warn(format!("the database queries of tables in {} could not be read; the part is kept as it is", r.target));
            continue;
        };
        let mut parts = vec![r.target.clone()];
        if let Ok(item_rels) = cx.pkg.rels(&r.target) {
            parts.extend(item_rels.into_iter().filter(|x| !x.external).map(|x| x.target));
        }
        return (stored.into_iter().take(MAX_DB_QUERIES).collect(), parts);
    }
    (vec![], vec![])
}

/// Ties read queries to the tables of `wb`: by the table's name when the query has one (the one
/// table of the workbook with that name, or none), else by sheet name and top-left cell. A
/// query whose table can't be found is dropped.
pub fn attach(wb: &mut Workbook, read: Vec<Stored>) {
    wb.db_queries = read
        .into_iter()
        .filter_map(|Stored { table, sheet, anchor, mut query }| {
            query.table_id = match table {
                Some(name) => {
                    let mut named = wb.sheets.iter().flat_map(|sh| sh.tables.iter()).filter(|t| t.name.eq_ignore_ascii_case(&name));
                    match (named.next(), named.next()) {
                        (Some(t), None) => t.id,
                        _ => return None,
                    }
                }
                None => {
                    let at = CellRef::parse(&anchor)?;
                    wb.sheet(wb.sheet_index(&sheet)?)?.tables.iter().find(|t| t.range.start == at)?.id
                }
            };
            Some(query)
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

/// Writes the part when tables of the workbook have queries (after the sheets, whose tables
/// record the names they were written with). A part too big to be read again is an error.
pub fn write(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) -> Result<(), IoError> {
    // A name written for two tables (sheets can each hold one) can't tell them apart.
    let shared = |name: &str| out.table_names.values().filter(|n| n.eq_ignore_ascii_case(name)).count() > 1;
    let live: Vec<Stored> = wb
        .db_queries
        .iter()
        .filter_map(|q| {
            wb.sheets.iter().find_map(|sh| {
                let t = sh.tables.iter().find(|t| t.id == q.table_id)?;
                let table = out.table_names.get(&t.id).filter(|n| !shared(n)).cloned();
                Some(Stored { table, sheet: sh.name.clone(), anchor: t.range.start.a1(), query: DbQuery { table_id: 0, ..q.clone() } })
            })
        })
        .collect();
    if live.is_empty() {
        return Ok(());
    }
    let Ok(json) = serde_json::to_string(&live) else { return Ok(()) };
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
        return Ok(());
    };
    let mut x = String::from(XML_DECL);
    let _ = write!(x, "<{ROOT} xmlns=\"{NS_DB_QUERIES}\" version=\"1\">{}</{ROOT}>", esc(&json));
    if x.len() as u64 > MAX_PART {
        return Err(IoError::Format(format!(
            "the database queries of the tables take {} MiB, more than the {} MiB a workbook can hold; shorten the SQL or remove some database tables",
            x.len().div_ceil(1024 * 1024),
            MAX_PART / (1024 * 1024)
        )));
    }
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
    Ok(())
}
