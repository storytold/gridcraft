//! Power Query queries authored in Excel, read (not run) so Data › Queries & Connections can list
//! them.
//!
//! Excel keeps its queries in a custom XML part whose root is `<DataMashup>`, holding base64 of a
//! binary stream described by Microsoft's public [MS-QDEFF] specification: a version, then
//! length-prefixed sections — an OPC package whose `Formulas/Section1.m` is the M section
//! document (`section Section1; shared Query1 = let … in …;`), permissions, metadata (an XML
//! document of per-query settings such as the table a query fills) and permission bindings.
//! `xl/connections.xml` reads each query through a Mashup OLE DB connection (`Location=<query>`),
//! and a query table hanging off the table the query loads into names that connection.
//!
//! Only names, formulas and load facts are read: GridCraft has no M interpreter. All of these parts
//! stay with the workbook's kept content (`passthrough`) and are written back byte for byte.
//! Everything here is hostile input: sizes are capped, offsets checked, and a damaged part only
//! costs the listing (with a warning), never the workbook.

use std::collections::HashMap;

use gridcraft_model::query::{MAX_M_TEXT, MAX_QUERIES};
use gridcraft_model::{ExcelQuery, Workbook};

use crate::package::{Package, Rel};
use crate::read::Ctx;
use crate::xml::{self, El};

/// Largest `DataMashup` custom XML part read (base64 text).
const MAX_PART: u64 = 64 * 1024 * 1024;
/// Largest `Formulas/Section1.m` read.
const MAX_SECTION: u64 = 16 * 1024 * 1024;
/// Largest table or query table part read while linking queries to tables.
const MAX_TABLE_PART: u64 = 16 * 1024 * 1024;
/// Most tables looked at while linking queries to tables.
const MAX_TABLES: usize = 4096;
/// The part of the mashup package holding the queries.
const SECTION_PART: &str = "Formulas/Section1.m";
/// Longest query name kept, in characters.
const MAX_MEMBER_NAME: usize = 1024;
/// Provider of the connections that read a query.
const MASHUP_PROVIDER: &str = "microsoft.mashup.oledb";

/// Reads the workbook's Excel queries, if it has a `DataMashup` part. `wb` must already hold the
/// sheets (to find the tables queries load into).
pub fn read(cx: &mut Ctx<'_>, wb_rels: &[Rel], wb: &Workbook) -> Vec<ExcelQuery> {
    let Some(text) = find_mashup(cx, wb_rels) else { return vec![] };
    let mut queries = match parse_mashup(&text) {
        Ok(q) => q,
        Err(e) => {
            cx.warn(format!("Excel's Power Query queries could not be read ({e}); they were kept unchanged"));
            return vec![];
        }
    };
    if queries.is_empty() {
        return vec![];
    }
    let connections = read_connections(cx, wb_rels);
    let tables = query_tables(cx, wb_rels);
    for Read { query: q, table_hint } in &mut queries {
        if let Some(c) = connections.iter().find(|c| c.location.eq_ignore_ascii_case(&q.name)) {
            q.connection = Some(c.name.clone());
            q.refresh_on_open = c.refresh_on_load;
            if let Some(t) = c.id.and_then(|id| tables.get(&id)) {
                q.table = Some(t.clone());
            }
        }
        // Without a query table (or with one GridCraft couldn't follow), the metadata's fill target
        // still names the table when it exists.
        if q.table.is_none()
            && let Some(t) = table_hint.take()
            && table_sheet(wb, &t).is_some()
        {
            q.table = Some(t);
        }
        q.sheet = q.table.as_deref().and_then(|t| table_sheet(wb, t));
    }
    queries.into_iter().map(|q| q.query).collect()
}

/// The sheet holding table `name`.
fn table_sheet(wb: &Workbook, name: &str) -> Option<String> {
    wb.sheets.iter().find(|s| s.tables.iter().any(|t| t.name.eq_ignore_ascii_case(name))).map(|s| s.name.clone())
}

/// The base64 text of the first `DataMashup` custom XML part.
fn find_mashup(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> Option<String> {
    for r in wb_rels.iter().filter(|r| r.kind.eq_ignore_ascii_case("customXml") && !r.external) {
        let bytes = match cx.pkg.read_limited(&r.target, MAX_PART) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => {
                cx.warn(format!("{} could not be read: {e}", r.target));
                continue;
            }
        };
        // A cheap look before parsing (UTF-8 or UTF-16): most custom XML parts aren't the mashup.
        if !contains(&bytes, b"DataMashup") && !contains_utf16(&bytes, "DataMashup") {
            continue;
        }
        let Ok(root) = xml::parse(&bytes) else { continue };
        if root.name == "DataMashup" {
            return Some(root.text);
        }
    }
    None
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn contains_utf16(hay: &[u8], needle: &str) -> bool {
    let le: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let be: Vec<u8> = needle.encode_utf16().flat_map(u16::to_be_bytes).collect();
    contains(hay, &le) || contains(hay, &be)
}

/// A query read from the mashup, before it is linked to connections and tables.
struct Read {
    query: ExcelQuery,
    /// The metadata's `FillTarget`: the table the query fills.
    table_hint: Option<String>,
}

/// Decodes the part's base64 text and reads the queries out of the binary stream.
fn parse_mashup(text: &str) -> Result<Vec<Read>, String> {
    let bin = base64(text).ok_or("the DataMashup part isn't valid base64")?;
    let stream = parse_stream(&bin)?;
    let mut pkg = Package::open(stream.package).map_err(|e| format!("its package: {e}"))?;
    let section = match pkg.read_limited(SECTION_PART, MAX_SECTION) {
        Ok(Some(b)) => b,
        Ok(None) => return Ok(vec![]),
        Err(e) => return Err(format!("{SECTION_PART}: {e}")),
    };
    let section = String::from_utf8_lossy(section.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&section)).into_owned();
    let meta = stream.metadata_xml.and_then(|x| xml::parse(x).ok()).map(|root| metadata(&root)).unwrap_or_default();
    Ok(section_members(&section)
        .into_iter()
        .map(|m| {
            let facts = meta.get(&m.name.to_lowercase());
            let fill = facts.is_some_and(|f| f.fill_enabled);
            Read {
                query: ExcelQuery {
                    name: m.name,
                    formula: m.formula,
                    description: m.description,
                    data_model: facts.is_some_and(|f| f.data_model),
                    rows: facts.and_then(|f| f.rows),
                    last_refreshed: facts.and_then(|f| f.last_updated.clone()),
                    ..ExcelQuery::default()
                },
                table_hint: facts.and_then(|f| f.fill_target.clone()).filter(|_| fill),
            }
        })
        .collect())
}

/// The sections of the binary stream GridCraft reads.
struct Stream<'a> {
    package: &'a [u8],
    metadata_xml: Option<&'a [u8]>,
}

/// [MS-QDEFF] top level: `Version` (0), then length-prefixed package parts, permissions, metadata
/// and permission bindings (each length a little-endian u32). Metadata is itself a version, a
/// length-prefixed XML document and a length-prefixed content package.
fn parse_stream(bin: &[u8]) -> Result<Stream<'_>, String> {
    let mut r = Bytes { b: bin, pos: 0 };
    let version = r.u32().ok_or("it is too short")?;
    if version != 0 {
        return Err(format!("unknown DataMashup version {version}"));
    }
    let package = r.block().ok_or("its package is cut short")?;
    // Permissions, then metadata; both optional as far as GridCraft is concerned.
    let metadata_xml = r.block().and_then(|_| r.block()).and_then(|meta| {
        let mut m = Bytes { b: meta, pos: 0 };
        m.u32()?; // metadata version
        m.block()
    });
    Ok(Stream { package, metadata_xml })
}

/// A bounds-checked reader over a byte slice.
struct Bytes<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.b.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
    }
    /// A u32 length followed by that many bytes.
    fn block(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u32()?).ok()?;
        self.take(n)
    }
}

/// Standard base64 (RFC 4648), whitespace ignored. `None` on any other character or bad padding.
fn base64(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut pad) = (0u32, 0u32, 0usize);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return None;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (pad <= 2).then_some(out)
}

/// What the metadata says about one query.
#[derive(Default)]
struct Facts {
    fill_enabled: bool,
    data_model: bool,
    fill_target: Option<String>,
    rows: Option<u64>,
    last_updated: Option<String>,
}

/// The metadata XML's `Formula` items (`Section1/<query>`), by lower-case query name. Entry values
/// carry a one-letter type prefix (`l` number, `s` string, `d` date…).
fn metadata(root: &El) -> HashMap<String, Facts> {
    let mut out = HashMap::new();
    let Some(items) = root.child("Items") else { return out };
    for item in items.kids("Item") {
        let Some(loc) = item.child("ItemLocation") else { continue };
        if loc.child("ItemType").map(|t| t.text.trim()) != Some("Formula") {
            continue;
        }
        let path = loc.child("ItemPath").map(|p| p.text.trim()).unwrap_or("");
        let mut segs = path.split('/');
        let (Some(_section), Some(name), None) = (segs.next(), segs.next(), segs.next()) else { continue };
        let name = percent_decode(name);
        let mut f = Facts::default();
        for e in item.child("StableEntries").into_iter().flat_map(|s| s.kids("Entry")) {
            let (Some(ty), Some(v)) = (e.attr("Type"), e.attr("Value")) else { continue };
            let v = v.get(1..).unwrap_or("");
            match ty {
                "FillEnabled" => f.fill_enabled = v == "1",
                "FillToDataModelEnabled" => f.data_model = v == "1",
                "FillTarget" if !v.is_empty() => f.fill_target = Some(v.to_string()),
                "FillCount" => f.rows = v.parse().ok(),
                "FillLastUpdated" if !v.is_empty() => f.last_updated = Some(v.to_string()),
                _ => {}
            }
        }
        if out.len() < MAX_QUERIES {
            out.insert(name.to_lowercase(), f);
        }
    }
    out
}

/// `%XX` escapes decoded as UTF-8 (item paths are URL-encoded).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (c, hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One `shared` member of the section document.
#[derive(Debug, PartialEq)]
struct Member {
    name: String,
    formula: String,
    description: String,
}

/// The members of an M section document: `section Name;` then `[attributes] shared Name = expr;`
/// for each query. `;` only ends a member outside text, quoted identifiers and comments.
fn section_members(src: &str) -> Vec<Member> {
    let mut out = vec![];
    let mut first = true;
    for chunk in split_members(src) {
        if std::mem::take(&mut first) && keyword_at(chunk, skip_trivia(chunk, 0), "section") {
            continue;
        }
        if let Some(m) = member(chunk) {
            if out.len() >= MAX_QUERIES {
                break;
            }
            out.push(m);
        }
    }
    out
}

/// The text of each `;`-terminated member (text after the last `;` is not a member).
fn split_members(src: &str) -> Vec<&str> {
    let b = src.as_bytes();
    let mut out = vec![];
    let (mut start, mut i) = (0, 0);
    while let Some(&c) = b.get(i) {
        match c {
            b'"' => i = skip_quoted(b, i),
            b'/' if b.get(i + 1) == Some(&b'/') => i = skip_line_comment(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_block_comment(b, i),
            b';' => {
                out.push(src.get(start..i).unwrap_or(""));
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    out
}

/// Past the text literal (or quoted identifier) whose opening `"` is at `i`; `""` is an escaped quote.
fn skip_quoted(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while let Some(&c) = b.get(i) {
        if c == b'"' {
            if b.get(i + 1) == Some(&b'"') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

fn skip_line_comment(b: &[u8], i: usize) -> usize {
    b.get(i..).and_then(|r| r.iter().position(|&c| c == b'\n' || c == b'\r')).map_or(b.len(), |p| i + p)
}

fn skip_block_comment(b: &[u8], i: usize) -> usize {
    b.get(i + 2..).and_then(|r| r.windows(2).position(|w| w == b"*/")).map_or(b.len(), |p| i + 2 + p + 2)
}

/// Past whitespace and comments from `i`.
fn skip_trivia(s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    loop {
        match (b.get(i), b.get(i + 1)) {
            (Some(c), _) if c.is_ascii_whitespace() => i += 1,
            (Some(b'/'), Some(b'/')) => i = skip_line_comment(b, i),
            (Some(b'/'), Some(b'*')) => i = skip_block_comment(b, i),
            _ => {
                // Non-ASCII whitespace (a no-break space, a BOM left in the text).
                match s.get(i..).and_then(|r| r.chars().next()) {
                    Some(c) if c.is_whitespace() || c == '\u{FEFF}' => i += c.len_utf8(),
                    _ => return i,
                }
            }
        }
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Whether keyword `kw` starts at `i` (and isn't the start of a longer identifier).
fn keyword_at(s: &str, i: usize, kw: &str) -> bool {
    s.get(i..).is_some_and(|r| r.starts_with(kw) && !r.get(kw.len()..).and_then(|x| x.chars().next()).is_some_and(is_ident_char))
}

/// One member: optional `[…]` attributes, optional `shared`, the name, `=`, the expression.
fn member(chunk: &str) -> Option<Member> {
    let b = chunk.as_bytes();
    let mut i = skip_trivia(chunk, 0);
    let mut description = String::new();
    if b.get(i) == Some(&b'[') {
        let end = skip_brackets(b, i)?;
        description = attribute_description(chunk.get(i + 1..end.saturating_sub(1))?);
        i = skip_trivia(chunk, end);
    }
    if keyword_at(chunk, i, "shared") {
        i = skip_trivia(chunk, i + "shared".len());
    }
    let (name, after) = if chunk.get(i..)?.starts_with("#\"") {
        let end = skip_quoted(b, i + 1);
        (m_text(chunk.get(i + 2..end.checked_sub(1)?)?), end)
    } else {
        let len: usize = chunk.get(i..)?.chars().take_while(|&c| is_ident_char(c)).map(char::len_utf8).sum();
        (chunk.get(i..i + len)?.to_string(), i + len)
    };
    let i = skip_trivia(chunk, after);
    if name.is_empty() || b.get(i) != Some(&b'=') {
        return None;
    }
    let mut formula = chunk.get(i + 1..)?.trim().to_string();
    if formula.len() > MAX_M_TEXT {
        let mut cut = MAX_M_TEXT;
        while !formula.is_char_boundary(cut) {
            cut -= 1;
        }
        formula.truncate(cut);
        formula.push_str("\n…");
    }
    Some(Member { name: name.chars().take(MAX_MEMBER_NAME).collect(), formula, description })
}

/// Past the balanced `[…]` starting at `i` (text and comments inside skipped).
fn skip_brackets(b: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0usize;
    while let Some(&c) = b.get(i) {
        match c {
            b'"' => {
                i = skip_quoted(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = skip_line_comment(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = skip_block_comment(b, i);
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `Description = "…"` in a member's attribute record, if present.
fn attribute_description(attrs: &str) -> String {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        i = skip_trivia(attrs, i);
        if keyword_at(attrs, i, "Description") {
            let j = skip_trivia(attrs, i + "Description".len());
            if b.get(j) == Some(&b'=') {
                let k = skip_trivia(attrs, j + 1);
                if b.get(k) == Some(&b'"') {
                    let end = skip_quoted(b, k);
                    return attrs.get(k + 1..end.saturating_sub(1)).map(m_text).unwrap_or_default();
                }
            }
        }
        match b.get(i) {
            Some(b'"') => i = skip_quoted(b, i),
            Some(_) => i += attrs.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8),
            None => break,
        }
    }
    String::new()
}

/// The value of M text between its quotes: `""` is `"`, and `#(…)` escapes (`cr`, `lf`, `tab`,
/// `#`, 4 or 8 hex digits, comma-separated) are decoded.
fn m_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '"' && rest.starts_with("\"\"") {
            out.push('"');
            rest = rest.get(2..).unwrap_or("");
            continue;
        }
        if c == '#'
            && rest.starts_with("#(")
            && let Some(close) = rest.find(')')
            && let Some(decoded) = rest.get(2..close).and_then(m_escapes)
        {
            out.push_str(&decoded);
            rest = rest.get(close + 1..).unwrap_or("");
            continue;
        }
        out.push(c);
        rest = rest.get(c.len_utf8()..).unwrap_or("");
    }
    out
}

fn m_escapes(list: &str) -> Option<String> {
    list.split(',')
        .map(|e| match e.trim() {
            "cr" => Some('\r'),
            "lf" => Some('\n'),
            "tab" => Some('\t'),
            "#" => Some('#'),
            h if (h.len() == 4 || h.len() == 8) && h.bytes().all(|c| c.is_ascii_hexdigit()) => {
                u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
            }
            _ => None,
        })
        .collect()
}

/// A connection reading a query.
struct Connection {
    id: Option<u32>,
    name: String,
    /// The query it reads.
    location: String,
    refresh_on_load: bool,
}

/// The Mashup OLE DB connections of `xl/connections.xml`.
fn read_connections(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> Vec<Connection> {
    let Some(r) = wb_rels.iter().find(|r| r.kind.eq_ignore_ascii_case("connections") && !r.external) else { return vec![] };
    let root = match cx.pkg.read_limited(&r.target, MAX_TABLE_PART) {
        Ok(Some(b)) => match xml::parse(&b) {
            Ok(x) => x,
            Err(_) => return vec![],
        },
        _ => return vec![],
    };
    root.kids("connection")
        .filter_map(|c| {
            let location = mashup_location(c, 0)?;
            Some(Connection {
                id: c.attr_u32("id"),
                name: c.attr("name").unwrap_or("").to_string(),
                location,
                refresh_on_load: c.flag("refreshOnLoad", false),
            })
        })
        .take(MAX_QUERIES * 4)
        .collect()
}

/// `Location` of the first Mashup OLE DB connection string in `e` (`dbPr`, or the Data Model's
/// `x15:oledbPr` in an extension).
fn mashup_location(e: &El, depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    if let Some(cs) = e.attr("connection")
        && cs.to_ascii_lowercase().contains(MASHUP_PROVIDER)
        && let Some(l) = connection_string_value(cs, "Location")
    {
        return Some(l);
    }
    e.children.iter().find_map(|c| mashup_location(c, depth + 1))
}

/// The value of `key` in an OLE DB connection string (`k=v;k="v;v"`; quotes double to escape).
fn connection_string_value(cs: &str, key: &str) -> Option<String> {
    let mut rest = cs;
    loop {
        let eq = rest.find('=')?;
        let k = rest.get(..eq)?.trim().trim_start_matches(';').trim();
        let after = rest.get(eq + 1..)?.trim_start();
        let (value, next) = match after.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let mut v = String::new();
                let mut it = after.char_indices().skip(1).peekable();
                let mut end = after.len();
                while let Some((i, c)) = it.next() {
                    if c == q {
                        if it.peek().is_some_and(|&(_, n)| n == q) {
                            it.next();
                            v.push(q);
                            continue;
                        }
                        end = i + 1;
                        break;
                    }
                    v.push(c);
                }
                (v, after.get(end..).unwrap_or(""))
            }
            _ => {
                let end = after.find(';').unwrap_or(after.len());
                (after.get(..end).unwrap_or("").trim().to_string(), after.get(end..).unwrap_or(""))
            }
        };
        if k.eq_ignore_ascii_case(key) {
            return Some(value);
        }
        rest = next.strip_prefix(';').unwrap_or(next);
        if rest.trim().is_empty() {
            return None;
        }
    }
}

/// Connection id → name of the table a query table loads (tables' `queryTable` relationships).
fn query_tables(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let mut seen = 0usize;
    for ws in wb_rels.iter().filter(|r| r.kind.eq_ignore_ascii_case("worksheet") && !r.external) {
        let Ok(sheet_rels) = cx.pkg.rels(&ws.target) else { continue };
        for t in sheet_rels.iter().filter(|r| r.kind.eq_ignore_ascii_case("table") && !r.external) {
            seen += 1;
            if seen > MAX_TABLES {
                return out;
            }
            let Ok(table_rels) = cx.pkg.rels(&t.target) else { continue };
            let Some(qt) = table_rels.iter().find(|r| r.kind.eq_ignore_ascii_case("queryTable") && !r.external) else { continue };
            let Some(id) = small_xml(cx, &qt.target).and_then(|x| x.attr_u32("connectionId")) else { continue };
            let Some(name) = small_xml(cx, &t.target).and_then(|x| x.attr("displayName").or_else(|| x.attr("name")).map(str::to_string)) else {
                continue;
            };
            out.entry(id).or_insert(name);
        }
    }
    out
}

fn small_xml(cx: &mut Ctx<'_>, part: &str) -> Option<El> {
    cx.pkg.read_limited(part, MAX_TABLE_PART).ok().flatten().and_then(|b| xml::parse(&b).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_members_read_names_formulas_and_descriptions() {
        let src = "section Section1;\r\n\r\nshared Sales = let\r\n    Source = Excel.CurrentWorkbook(){[Name=\"Table1\"]}[Content]\r\nin\r\n    Source;\r\n\r\n[ Description = \"Totals; by \"\"region\"\"\" ]\r\nshared #\"Region Totals\" = let\r\n    // a comment; with a semicolon\r\n    S = Sales /* and; here */\r\nin\r\n    S;\r\nshared #\"Tab#(tab)Name\" = 1;\r\n";
        let m = section_members(src);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].name, "Sales");
        assert!(m[0].formula.starts_with("let") && m[0].formula.ends_with("Source"));
        assert_eq!(m[1].name, "Region Totals");
        assert_eq!(m[1].description, "Totals; by \"region\"");
        assert!(m[1].formula.contains("/* and; here */"));
        assert_eq!(m[2].name, "Tab\tName");
        assert_eq!(m[2].formula, "1");
    }

    #[test]
    fn hostile_section_text_never_panics() {
        for src in [
            "",
            ";",
            "section",
            "section S;",
            "shared",
            "shared =",
            "shared #\"",
            "shared #\"a",
            "[",
            "[[[]",
            "]];",
            "\"",
            "/*",
            "//",
            "#(",
            "shared #\"#(\" = 1;",
            "shared a = \"unterminated;",
            "[Description=\"é\"] shared é = 1;",
            "shared x = #(0041;",
            "\u{FEFF};;;",
        ] {
            let _ = section_members(src);
        }
        assert_eq!(section_members("shared é.1 = 2;")[0].name, "é.1");
    }

    #[test]
    fn base64_decodes_and_rejects_garbage() {
        assert_eq!(base64("aGVs\nbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64("").as_deref(), Some(&b""[..]));
        assert!(base64("aGVs*").is_none());
        assert!(base64("aG=Vs").is_none());
    }

    #[test]
    fn truncated_streams_are_errors() {
        assert!(parse_stream(&[]).is_err());
        assert!(parse_stream(&[1, 0, 0, 0]).is_err());
        assert!(parse_stream(&[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]).is_err());
        let s = parse_stream(&[0, 0, 0, 0, 1, 0, 0, 0, 7]).ok();
        assert!(s.is_some_and(|s| s.package == [7] && s.metadata_xml.is_none()));
    }

    #[test]
    fn connection_strings() {
        let cs = "Provider=Microsoft.Mashup.OleDb.1;Data Source=$Workbook$;Location=\"My \"\"Q\"\"; 2\";Extended Properties=\"\"";
        assert_eq!(connection_string_value(cs, "location").as_deref(), Some("My \"Q\"; 2"));
        assert_eq!(connection_string_value("Location=Sales;X=1", "Location").as_deref(), Some("Sales"));
        assert_eq!(connection_string_value("Location=Sales", "Missing"), None);
        assert_eq!(connection_string_value("=;=;\"", "Location"), None);
        assert_eq!(percent_decode("My%20Query%2"), "My Query%2");
    }
}
