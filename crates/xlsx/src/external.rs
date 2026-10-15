//! Cached values of linked workbooks (`xl/externalLinks/externalLinkN.xml`).
//!
//! A formula reaching into another workbook (`[1]Prices!B2`, `'[1]P&L'!A7:A99`) names it by its
//! position in `<externalReferences>`. Excel stores the values it last read from each linked
//! workbook in the link part (`<externalBook><sheetDataSet>`) and calculates with them while the
//! source is closed. The parts themselves are carried through unchanged by `passthrough`.

use std::fmt::Write as _;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{ExternalBook, ExternalName, ExternalSheet, Workbook};

use crate::IoError;
use crate::package::Rel;
use crate::read::Ctx;
use crate::write::{NS_MAIN, NS_REL, Out, Rels, XML_DECL};
use crate::xml::{self, El, esc, esc_attr};

const CT_EXTERNAL_LINK: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml";

/// One book per `<externalReference>`, in order; a link that can't be read (or isn't to a
/// workbook: DDE, OLE) is an empty book so that the later ones keep their positions.
pub(crate) fn read(cx: &mut Ctx<'_>, wb_xml: &El, wb_rels: &[Rel]) -> Result<Vec<ExternalBook>, IoError> {
    let Some(refs) = wb_xml.child("externalReferences") else { return Ok(vec![]) };
    let mut books = vec![];
    for r in refs.kids("externalReference") {
        let target = r.attr("id").and_then(|id| wb_rels.iter().find(|x| x.id == id)).map(|x| x.target.clone());
        let book = match target {
            Some(t) => match cx.optional_xml(&t)?.as_ref().and_then(|x| x.child("externalBook")) {
                Some(x) => {
                    let mut b = book(x);
                    if let Some(id) = x.attr("id") {
                        b.path = cx.pkg.rels(&t)?.iter().find(|r| r.id == id && r.external).map(|r| link_path(&r.target)).unwrap_or_default();
                    }
                    b
                }
                None => ExternalBook::default(),
            },
            None => ExternalBook::default(),
        };
        books.push(book);
    }
    Ok(books)
}

fn book(x: &El) -> ExternalBook {
    let mut sheets: Vec<ExternalSheet> = x
        .child("sheetNames")
        .map(|n| {
            n.kids("sheetName")
                .map(|s| ExternalSheet { name: s.attr("val").map(xml::decode_xstring).unwrap_or_default(), ..Default::default() })
                .collect()
        })
        .unwrap_or_default();
    for data in x.child("sheetDataSet").into_iter().flat_map(|d| d.kids("sheetData")) {
        let Some(sheet) = data.attr_u32("sheetId").and_then(|i| sheets.get_mut(i as usize)) else { continue };
        sheet.refresh_error = data.flag("refreshError", false);
        for c in data.kids("row").flat_map(|r| r.kids("cell")) {
            let Some(pos) = c.attr("r").and_then(CellRef::parse).filter(|p| p.row < MAX_ROWS && p.col < MAX_COLS) else { continue };
            let v = value(c);
            if !v.is_empty() {
                sheet.cells.push((pos, v));
            }
        }
        sheet.cells.sort_by_key(|(p, _)| *p);
        sheet.cells.dedup_by_key(|(p, _)| *p);
    }
    let names = x
        .child("definedNames")
        .into_iter()
        .flat_map(|d| d.kids("definedName"))
        .filter_map(|d| {
            let refers_to = d.attr("refersTo")?;
            Some(ExternalName {
                name: d.attr("name")?.to_string(),
                sheet: d.attr_u32("sheetId").map(|i| i as usize),
                refers_to: refers_to.strip_prefix('=').unwrap_or(refers_to).to_string(),
            })
        })
        .collect();
    ExternalBook { sheets, names, ..Default::default() }
}

/// The linked file of an `externalLinkPath` target, as Excel shows it:
/// `file:///C:\links\My%20Files\Source.xlsx` is `C:\links\My Files\Source.xlsx`,
/// `file:///Users/me/Source.xlsx` is `/Users/me/Source.xlsx`, and a relative target
/// (`Source.xlsx`) stays relative to the workbook.
fn link_path(target: &str) -> String {
    let path = match target.strip_prefix("file:///") {
        Some(p) if p.starts_with("\\\\") || p.as_bytes().get(1) == Some(&b':') => p.to_string(),
        Some(p) => format!("/{p}"),
        None => target.to_string(),
    };
    decode(&path)
}

/// [`link_path`] the other way round. Separators and a drive's colon stay as they are; the rest
/// is percent-encoded where a URI needs it (`My Files` → `My%20Files`).
fn link_target(path: &str) -> String {
    let encoded = encode(path);
    if path.starts_with("\\\\") || path.as_bytes().get(1) == Some(&b':') {
        format!("file:///{encoded}")
    } else if path.starts_with('/') {
        format!("file://{encoded}")
    } else {
        encoded.replace('\\', "/")
    }
}

/// Percent-encodes what a URI path can't hold as it is; non-ASCII characters are written as they
/// are, as Excel does.
fn encode(path: &str) -> String {
    let mut s = String::with_capacity(path.len());
    for c in path.chars() {
        if !c.is_ascii() || c.is_ascii_alphanumeric() || "-._~!$&'()+,;=@\\/:".contains(c) {
            s.push(c);
        } else {
            let _ = write!(s, "%{:02X}", c as u32);
        }
    }
    s
}

/// Percent-escapes decoded; the text as written when they don't decode to UTF-8.
fn decode(path: &str) -> String {
    let b = path.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) if c == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

fn value(c: &El) -> Value {
    let Some(v) = c.child("v").map(|v| v.text.as_str()) else { return Value::Empty };
    match c.attr("t").unwrap_or("n") {
        "str" | "s" | "inlineStr" => Value::text(xml::decode_xstring(v)),
        "b" => Value::Bool(matches!(v.trim(), "1" | "true" | "TRUE")),
        "e" => Value::Error(CellError::parse(v.trim()).unwrap_or(CellError::Value)),
        _ => match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Value::Number(if n == 0.0 { 0.0 } else { n }),
            _ => Value::Empty,
        },
    }
}

/// Adds a link part for each link added in GridCraft (a typed reference to another workbook)
/// after the ones read from the file, which `passthrough` writes as they were. `kept` is the kept
/// links' `<externalReferences>`; returns it with the new links in it, so `[n]` in formulas stays
/// the n-th link. When links read from the file couldn't be kept, none are added after them.
pub(crate) fn write(wb: &Workbook, kept: String, wb_rels: &mut Rels, out: &mut Out) -> String {
    let known = kept.matches("<externalReference ").count();
    let new = wb.external_books.get(known..).unwrap_or_default();
    if !new.iter().all(|b| b.added) {
        return kept;
    }
    let mut added = String::new();
    for (i, b) in new.iter().enumerate().map(|(i, b)| (i + known, b)) {
        let name = out.kept.claim(&format!("xl/externalLinks/externalLink{}.xml", i + 1));
        let mut rels = Rels::default();
        let id = rels.add_external("externalLinkPath", &link_target(&b.path));
        out.part(&name, Some(CT_EXTERNAL_LINK), part_xml(b, &id).into_bytes());
        out.part(&crate::passthrough::rels_part_name(&name), None, rels.xml().into_bytes());
        let rid = wb_rels.add("externalLink", &format!("/{name}"));
        let _ = write!(added, "<externalReference r:id=\"{rid}\"/>");
    }
    match kept.rfind("</externalReferences>") {
        _ if added.is_empty() => kept,
        Some(end) => format!("{}{added}{}", kept.get(..end).unwrap_or(""), kept.get(end..).unwrap_or("")),
        None => format!("{kept}<externalReferences>{added}</externalReferences>"),
    }
}

/// An `externalLink` part for `b`: its sheets, names and saved values; a sheet whose values
/// couldn't be read (none yet, for a new link) is marked `refreshError`.
fn part_xml(b: &ExternalBook, rid: &str) -> String {
    let mut x = format!("{XML_DECL}<externalLink xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"><externalBook r:id=\"{rid}\">");
    if !b.sheets.is_empty() {
        x.push_str("<sheetNames>");
        for s in &b.sheets {
            let _ = write!(x, "<sheetName val=\"{}\"/>", esc_attr(&xml::encode_xstring(&s.name)));
        }
        x.push_str("</sheetNames>");
    }
    if !b.names.is_empty() {
        x.push_str("<definedNames>");
        for n in &b.names {
            let _ = write!(x, "<definedName name=\"{}\"", esc_attr(&n.name));
            if !n.refers_to.is_empty() {
                let _ = write!(x, " refersTo=\"={}\"", esc_attr(&n.refers_to));
            }
            if let Some(i) = n.sheet {
                let _ = write!(x, " sheetId=\"{i}\"");
            }
            x.push_str("/>");
        }
        x.push_str("</definedNames>");
    }
    if !b.sheets.is_empty() {
        x.push_str("<sheetDataSet>");
        for (i, s) in b.sheets.iter().enumerate() {
            let _ = write!(x, "<sheetData sheetId=\"{i}\"{}", if s.refresh_error { " refreshError=\"1\"" } else { "" });
            if s.cells.is_empty() {
                x.push_str("/>");
                continue;
            }
            x.push('>');
            let mut row = None;
            for (c, v) in &s.cells {
                let (t, v) = match v {
                    Value::Number(n) => ("", xml::num(*n)),
                    Value::Text(t) => (" t=\"str\"", esc(&xml::encode_xstring(t))),
                    Value::Bool(b) => (" t=\"b\"", (*b as u8).to_string()),
                    Value::Error(e) => (" t=\"e\"", e.as_str().to_string()),
                    Value::Empty | Value::Array(_) => continue,
                };
                if row != Some(c.row) {
                    x.push_str(if row.is_some() { "</row>" } else { "" });
                    let _ = write!(x, "<row r=\"{}\">", c.row + 1);
                    row = Some(c.row);
                }
                let _ = write!(x, "<cell r=\"{}\"{t}><v>{v}</v></cell>", c.a1());
            }
            x.push_str(if row.is_some() { "</row></sheetData>" } else { "</sheetData>" });
        }
        x.push_str("</sheetDataSet>");
    }
    x.push_str("</externalBook></externalLink>");
    x
}
