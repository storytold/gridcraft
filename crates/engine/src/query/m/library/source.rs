//! Data sources and binaries: `Excel.CurrentWorkbook`, `Excel.Workbook`, `File.Contents`,
//! `Csv.Document`, `Web.Contents`, `Web.Page`, `Json.Document`, `Folder.Files`, `OData.Feed`,
//! `Odbc.Query`, and the binary functions Enter Data tables use (`Binary.FromText`,
//! `Binary.Decompress`). Files, the web and databases go through the query engine's data access,
//! so they are cached between previews and refused while external content is disabled.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::query::{ColType, QuerySource};
use gridcraft_model::{Sheet, StyleId, Visibility, Workbook};

use super::super::eval::{Ev, limit};
use super::super::value::{Bin, Lazy, MError, MRecord, MResult, MTable, MValue};
use super::{Args, cant};
use crate::query::access::{MAX_FILE_BYTES, MAX_FOLDER_FILES};
use crate::query::ojson::Json;
use crate::query::{MAX_COLUMNS, MAX_ROWS, formats, html, sources, unique_names};

/// Deepest JSON nesting turned into M values.
const MAX_JSON_DEPTH: usize = 256;

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Excel.CurrentWorkbook" => current_workbook(ev)?,
        "Excel.Workbook" => {
            let b = binary(a.at(0))?;
            let (wb, _) = crate::io::open_bytes(&b.origin, &b.bytes)
                .map_err(|e| MError::format(format!("The file isn't a workbook GridCraft can read: {e}")))?;
            let headers = matches!(a.at(1), MValue::Logical(true));
            workbook_navigation(&wb, headers)?
        }
        "File.Contents" => {
            let path = a.text(0)?;
            let f = ev.cx.file(&path).map_err(MError::source)?;
            MValue::Binary(Rc::new(Bin { bytes: f.bytes, content_type: f.content_type, origin: path.to_string() }))
        }
        "Csv.Document" => csv_document(a)?,
        "Web.Contents" => {
            let url = web_url(&a.text(0)?, a.opt_record(1)?.as_deref())?;
            let f = ev.cx.url(&url, None).map_err(MError::source)?;
            MValue::Binary(Rc::new(Bin { bytes: f.bytes, content_type: f.content_type, origin: url }))
        }
        "Web.Page" => {
            let text = text_or_binary(a.at(0))?;
            let mut t = MTable::new(["Caption", "Source", "ClassName", "Id", "Data"].iter().map(|s| Rc::from(*s)).collect());
            for ht in html::tables(&text) {
                let caption = if ht.caption.is_empty() { MValue::Null } else { MValue::text(&ht.caption) };
                let data = super::super::from_qtable(&ht.to_qtable(), ev.sys);
                t.push(vec![caption, MValue::text("Table"), MValue::Null, MValue::Null, MValue::table(data)])?;
            }
            MValue::table(t)
        }
        "Json.Document" => {
            let text = match a.at(0) {
                MValue::Binary(b) => formats::decode(&b.bytes, encoding_name(a.at(1))),
                MValue::Text(t) => t.to_string(),
                other => return Err(cant(other, "Binary")),
            };
            let doc = crate::query::ojson::parse(&text)
                .map_err(|e| MError::format(format!("We found extra characters at the end of JSON input, or the JSON isn't valid. {e}")))?;
            json_value(&doc, 0)?
        }
        "Folder.Files" => folder_files(ev, &a.text(0)?)?,
        "OData.Feed" => odata_feed(ev, &a.text(0)?)?,
        "Odbc.Query" => {
            let src = QuerySource::Odbc { connection: a.text(0)?.to_string(), table: None, sql: Some(a.text(1)?.to_string()) };
            let t = sources::table(ev.cx, &src).map_err(MError::source)?;
            MValue::table(super::super::from_qtable(&t, ev.sys))
        }
        "Text.FromBinary" => MValue::text(formats::decode(&binary(a.at(0))?.bytes, encoding_name(a.at(1)))),
        "Lines.FromBinary" => {
            let text = formats::decode(&binary(a.at(0))?.bytes, encoding_name(a.at(3)));
            let lines: Vec<MValue> = text.lines().map(MValue::text).collect();
            if lines.len() > MAX_ROWS {
                return Err(limit("The text has too many lines."));
            }
            MValue::list(lines)
        }
        "Binary.FromText" => {
            let t = a.text(0)?;
            let hex = matches!(a.at(1), MValue::Number(n) if *n == 1.0);
            let bytes = if hex { from_hex(&t) } else { crate::io::base64_decode(&t) }
                .ok_or_else(|| MError::format("The text isn't valid base64 (or hex) binary data."))?;
            bin(bytes, "")
        }
        "#binary" => match a.at(0) {
            MValue::Text(t) => bin(crate::io::base64_decode(t).ok_or_else(|| MError::format("The text isn't valid base64."))?, ""),
            MValue::List(l) => {
                let mut bytes = Vec::with_capacity(l.len());
                for x in l.iter() {
                    match x {
                        MValue::Number(n) if (0.0..=255.0).contains(n) && n.fract() == 0.0 => bytes.push(*n as u8),
                        other => return Err(cant(other, "Byte")),
                    }
                }
                bin(bytes, "")
            }
            other => return Err(cant(other, "List")),
        },
        "Binary.Decompress" => {
            let b = binary(a.at(0))?;
            let gzip = matches!(a.at(1), MValue::Number(n) if *n == 0.0);
            let body = if gzip { gzip_body(&b.bytes).ok_or_else(|| MError::format("The data isn't in the GZip format."))? } else { &b.bytes[..] };
            let out = miniz_oxide::inflate::decompress_to_vec_with_limit(body, MAX_FILE_BYTES)
                .map_err(|_| MError::format("The compressed data is damaged or too large."))?;
            bin(out, &b.origin)
        }
        "Binary.Buffer" => MValue::Binary(binary(a.at(0))?),
        _ => return Ok(None),
    };
    Ok(Some(r))
}

fn bin(bytes: Vec<u8>, origin: &str) -> MValue {
    MValue::Binary(Rc::new(Bin { bytes: Arc::from(bytes), content_type: None, origin: origin.to_string() }))
}

fn binary(v: &MValue) -> MResult<Rc<Bin>> {
    match v {
        MValue::Binary(b) => Ok(b.clone()),
        MValue::Text(t) => Ok(Rc::new(Bin { bytes: Arc::from(t.as_bytes()), content_type: None, origin: String::new() })),
        other => Err(cant(other, "Binary")),
    }
}

fn text_or_binary(v: &MValue) -> MResult<String> {
    match v {
        MValue::Text(t) => Ok(t.to_string()),
        MValue::Binary(b) => Ok(formats::decode(&b.bytes, None)),
        other => Err(cant(other, "Binary")),
    }
}

/// A code page (`65001`, `1252`…) as the decoder's encoding name.
fn encoding_name(v: &MValue) -> Option<&'static str> {
    match v {
        MValue::Number(n) => match *n as i64 {
            65001 | 20127 => Some("utf-8"),
            1200 => Some("utf-16le"),
            1201 => Some("utf-16be"),
            1252 | 28591 => Some("windows-1252"),
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------- workbooks

/// Whether cells of a style show dates (their numbers become date-times, as Power Query reads
/// them).
struct DateStyles(HashMap<StyleId, bool>);

impl DateStyles {
    fn is_date(&mut self, wb: &Workbook, id: StyleId) -> bool {
        *self.0.entry(id).or_insert_with(|| crate::display::number_format(wb.styles.get(id).num_fmt.as_str()).is_date())
    }
}

/// A block of cells as a table; `headers` takes the first row as column names.
fn range_table(wb: &Workbook, sh: &Sheet, r: RangeRef, headers: bool, ds: &mut DateStyles) -> MResult<MTable> {
    let width = (r.width() as usize).min(MAX_COLUMNS);
    let mut names: Vec<String> = if headers {
        (0..width).map(|i| sh.value(CellRef::new(r.start.row, r.start.col + i as u32)).display()).collect()
    } else {
        (1..=width).map(|i| format!("Column{i}")).collect()
    };
    for (i, n) in names.iter_mut().enumerate() {
        if n.is_empty() {
            *n = format!("Column{}", i + 1);
        }
    }
    unique_names(&mut names);
    let mut t = MTable::new(names.into_iter().map(Rc::from).collect());
    let first = r.start.row.saturating_add(u32::from(headers));
    if first > r.end.row {
        return Ok(t);
    }
    for row in first..=r.end.row {
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            let c = CellRef::new(row, r.start.col + i as u32);
            let v = sh.value(c);
            let date = matches!(v, Value::Number(_)) && ds.is_date(wb, sh.style_id(c));
            cells.push(MValue::from_cell(&v, if date { ColType::DateTime } else { ColType::Any }, wb.date_system));
        }
        t.push(cells)?;
    }
    Ok(t)
}

fn table_content(wb: &Workbook, name: &str, ds: &mut DateStyles) -> MResult<Option<MTable>> {
    let Some((si, ti)) = wb.table(name) else { return Ok(None) };
    let Some(sh) = wb.sheet(si) else { return Ok(None) };
    let Some(t) = sh.tables.get(ti) else { return Ok(None) };
    let names: Vec<Rc<str>> = t.columns.iter().map(|c| Rc::from(c.name.as_str())).collect();
    let mut out = match t.data_range() {
        Some(r) => range_table(wb, sh, r, false, ds)?,
        None => MTable::default(),
    };
    if out.width() != names.len() {
        out = MTable::new(names.clone());
    }
    out.names = names.into();
    Ok(Some(out))
}

fn defined_names(wb: &Workbook) -> impl Iterator<Item = &gridcraft_model::DefinedName> {
    wb.names.iter().filter(|n| !n.hidden && n.scope.is_none() && !n.name.starts_with('_'))
}

fn current_workbook(ev: &mut Ev<'_, '_>) -> MResult<MValue> {
    let wb = ev.cx.wb;
    let mut ds = DateStyles(HashMap::new());
    let mut out = MTable::new(vec!["Content".into(), "Name".into()]);
    for name in wb.sheets.iter().flat_map(|s| s.tables.iter().map(|t| t.name.clone())).collect::<Vec<_>>() {
        if let Some(t) = table_content(wb, &name, &mut ds)? {
            out.push(vec![MValue::table(t), MValue::text(&name)])?;
        }
    }
    for n in defined_names(wb) {
        let Some((si, r)) = formats::name_range(wb, &n.formula) else { continue };
        let Some(sh) = wb.sheet(si) else { continue };
        out.push(vec![MValue::table(range_table(wb, sh, r, false, &mut ds)?), MValue::text(&n.name)])?;
    }
    Ok(MValue::table(out))
}

/// `Excel.Workbook`'s navigation table: sheets, tables and defined names.
fn workbook_navigation(wb: &Workbook, headers: bool) -> MResult<MValue> {
    let mut ds = DateStyles(HashMap::new());
    let mut out = MTable::new(["Name", "Data", "Item", "Kind", "Hidden"].iter().map(|s| Rc::from(*s)).collect());
    for sh in &wb.sheets {
        let data = match sh.used_range() {
            Some(r) => range_table(wb, sh, r, headers, &mut ds)?,
            None => MTable::default(),
        };
        out.push(vec![
            MValue::text(&sh.name),
            MValue::table(data),
            MValue::text(&sh.name),
            MValue::text("Sheet"),
            MValue::Logical(sh.visibility != Visibility::Visible),
        ])?;
    }
    for name in wb.sheets.iter().flat_map(|s| s.tables.iter().map(|t| t.name.clone())).collect::<Vec<_>>() {
        if let Some(t) = table_content(wb, &name, &mut ds)? {
            out.push(vec![MValue::text(&name), MValue::table(t), MValue::text(&name), MValue::text("Table"), MValue::Logical(false)])?;
        }
    }
    for n in defined_names(wb) {
        let Some((si, r)) = formats::name_range(wb, &n.formula) else { continue };
        let Some(sh) = wb.sheet(si) else { continue };
        let data = range_table(wb, sh, r, headers, &mut ds)?;
        out.push(vec![MValue::text(&n.name), MValue::table(data), MValue::text(&n.name), MValue::text("DefinedName"), MValue::Logical(false)])?;
    }
    Ok(MValue::table(out))
}

// ---------------------------------------------------------------- text, web, JSON

fn csv_document(a: &Args<'_>) -> MResult<MValue> {
    let b = binary(a.at(0))?;
    let (mut columns, mut delimiter, mut encoding) = (a.at(1).clone(), a.at(2).clone(), a.at(4).clone());
    if let MValue::Record(opts) = a.at(1) {
        columns = opts.get("Columns").cloned().unwrap_or(MValue::Null);
        delimiter = opts.get("Delimiter").cloned().unwrap_or(MValue::Null);
        encoding = opts.get("Encoding").cloned().unwrap_or(MValue::Null);
    }
    let delim = match &delimiter {
        MValue::Null => ",".to_string(),
        MValue::Text(t) => t.to_string(),
        other => return Err(cant(other, "Text")),
    };
    let q = formats::csv_table(&b.bytes, Some(&delim), encoding_name(&encoding)).map_err(MError::format)?;
    let mut t = MTable::new(q.columns.iter().map(|c| Rc::from(c.as_str())).collect());
    for row in &q.rows {
        t.push(row.iter().map(|v| if let Value::Text(s) = v { MValue::Text(Rc::from(&**s)) } else { MValue::Null }).collect())?;
    }
    let names: Option<Vec<Rc<str>>> = match &columns {
        MValue::Null => None,
        MValue::Number(n) if *n >= 1.0 && n.fract() == 0.0 && *n <= MAX_COLUMNS as f64 => {
            Some((1..=*n as usize).map(|i| Rc::from(format!("Column{i}"))).collect())
        }
        v @ MValue::List(_) => Some(super::names_of(v)?),
        other => return Err(cant(other, "Number")),
    };
    if let Some(names) = names {
        let n = names.len();
        for r in t.rows.iter_mut() {
            r.resize(n, MValue::Null);
        }
        t.types = vec![ColType::Any; n];
        t.names = names.into();
    }
    Ok(MValue::table(t))
}

/// A URL with `Web.Contents`' `RelativePath` and `Query` options.
fn web_url(base: &str, opts: Option<&MRecord>) -> MResult<String> {
    let mut url = base.trim().to_string();
    let Some(o) = opts else { return Ok(url) };
    if let Some(MValue::Text(rel)) = o.get("RelativePath")
        && !rel.is_empty()
    {
        if !url.ends_with('/') {
            url.push('/');
        }
        url.push_str(rel.trim_start_matches('/'));
    }
    if let Some(MValue::Record(q)) = o.get("Query") {
        let mut parts = Vec::new();
        for (k, v) in q.names.iter().zip(q.values.iter()) {
            match v {
                MValue::Text(t) => parts.push(format!("{}={}", encode(k), encode(t))),
                MValue::List(l) => {
                    for x in l.iter() {
                        if let MValue::Text(t) = x {
                            parts.push(format!("{}={}", encode(k), encode(t)));
                        }
                    }
                }
                other => return Err(cant(other, "Text")),
            }
        }
        if !parts.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&parts.join("&"));
        }
    }
    Ok(url)
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn json_value(j: &Json, depth: usize) -> MResult<MValue> {
    if depth > MAX_JSON_DEPTH {
        return Err(limit("The JSON nests too deeply."));
    }
    Ok(match j {
        Json::Null => MValue::Null,
        Json::Bool(b) => MValue::Logical(*b),
        Json::Number(n) => MValue::Number(*n),
        Json::String(s) => MValue::text(s),
        Json::Array(a) => {
            if a.len() > super::super::eval::MAX_LIST {
                return Err(limit("The JSON array is too long."));
            }
            MValue::list(a.iter().map(|x| json_value(x, depth + 1)).collect::<MResult<_>>()?)
        }
        Json::Object(m) => {
            let mut fields = Vec::new();
            for (k, v) in m.iter() {
                fields.push((Rc::from(k.as_str()), json_value(v, depth + 1)?));
            }
            MValue::record(MRecord::new(fields))
        }
    })
}

// ---------------------------------------------------------------- folders and feeds

fn folder_files(ev: &mut Ev<'_, '_>, path: &str) -> MResult<MValue> {
    ev.cx.check_external().map_err(MError::source)?;
    let files = ev.cx.access.list_folder(path, MAX_FOLDER_FILES).map_err(MError::source)?;
    let names = ["Content", "Name", "Extension", "Date accessed", "Date modified", "Date created", "Attributes", "Folder Path"];
    let mut t = MTable::new(names.iter().map(|s| Rc::from(*s)).collect());
    t.types = vec![ColType::Any, ColType::Text, ColType::Text, ColType::DateTime, ColType::DateTime, ColType::DateTime, ColType::Any, ColType::Text];
    let when = |v: Option<f64>| v.map(|s| MValue::DateTime(s - 25_569.0)).unwrap_or(MValue::Null);
    for f in files.iter().take(MAX_ROWS) {
        let full = f.path();
        let origin = full.clone();
        let content = Lazy::value(Box::new(move |ev: &mut Ev<'_, '_>| {
            let got = ev.cx.file(&full).map_err(MError::source)?;
            Ok(MValue::Binary(Rc::new(Bin { bytes: got.bytes, content_type: got.content_type, origin })))
        }));
        let attributes = MRecord::new(vec![
            ("Kind".into(), MValue::text(f.extension.trim_start_matches('.').to_uppercase() + " File")),
            ("Size".into(), MValue::Number(f.size as f64)),
            ("ReadOnly".into(), MValue::Logical(f.read_only)),
            ("Hidden".into(), MValue::Logical(f.hidden)),
            ("System".into(), MValue::Logical(false)),
            ("Directory".into(), MValue::Logical(false)),
        ]);
        t.push(vec![
            content,
            MValue::text(&f.name),
            MValue::text(&f.extension),
            when(f.accessed),
            when(f.modified),
            when(f.created),
            MValue::record(attributes),
            MValue::text(&f.folder),
        ])?;
    }
    Ok(MValue::table(t))
}

fn odata_feed(ev: &mut Ev<'_, '_>, url: &str) -> MResult<MValue> {
    let src = QuerySource::OData { url: url.to_string(), item: None };
    match sources::table(ev.cx, &src) {
        Ok(t) => Ok(MValue::table(super::super::from_qtable(&t, ev.sys))),
        Err(e) => {
            // A service root: its entity sets, each read when navigated to.
            let items = sources::items(ev.cx, &src).map_err(|_| MError::source(e.clone()))?;
            if items.is_empty() {
                return Err(MError::source(e));
            }
            let mut t = MTable::new(vec!["Name".into(), "Data".into(), "Signature".into()]);
            for it in items {
                let (u, name) = (url.to_string(), it.name.clone());
                let data = Lazy::value(Box::new(move |ev: &mut Ev<'_, '_>| {
                    let src = QuerySource::OData { url: u, item: Some(name) };
                    let t = sources::table(ev.cx, &src).map_err(MError::source)?;
                    Ok(MValue::table(super::super::from_qtable(&t, ev.sys)))
                }));
                t.push(vec![MValue::text(&it.name), data, MValue::text("table")])?;
            }
            Ok(MValue::table(t))
        }
    }
}

// ---------------------------------------------------------------- binary encodings

fn from_hex(s: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits.chunks(2).map(|p| std::str::from_utf8(p).ok().and_then(|h| u8::from_str_radix(h, 16).ok())).collect()
}

/// The deflate stream inside a GZip member (header fields skipped).
fn gzip_body(b: &[u8]) -> Option<&[u8]> {
    if b.get(..3)? != [0x1f, 0x8b, 8] {
        return None;
    }
    let flags = *b.get(3)?;
    let mut at = 10usize;
    if flags & 4 != 0 {
        let len = usize::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]));
        at = at.checked_add(2 + len)?;
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            at += b.get(at..)?.iter().position(|c| *c == 0)? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    b.get(at..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_encodings() {
        assert_eq!(from_hex("0aFF").unwrap(), vec![10, 255]);
        assert!(from_hex("0").is_none());
        assert!(gzip_body(&[0x1f, 0x8b, 8, 0x1f]).is_none());
        assert!(gzip_body(&[0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 0, 0xff, 0xff]).is_none());
    }

    #[test]
    fn web_urls() {
        let q = MRecord::new(vec![("q".into(), MValue::text("a b&c")), ("n".into(), MValue::list(vec![MValue::text("1"), MValue::text("2")]))]);
        let opts = MRecord::new(vec![("RelativePath".into(), MValue::text("api/v1")), ("Query".into(), MValue::record(q))]);
        assert_eq!(web_url("https://x.test", Some(&opts)).unwrap(), "https://x.test/api/v1?q=a%20b%26c&n=1&n=2");
    }
}
