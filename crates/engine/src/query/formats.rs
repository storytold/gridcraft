//! Reading the formats sources deliver into query tables: delimited text, JSON, XML, Excel
//! workbooks and folder listings. Each reader also lists what the Navigator offers (the arrays of
//! a JSON document, the repeating elements of an XML file, the sheets/tables/names of a workbook).

use super::ojson::Json;
use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::Workbook;
use gridcraft_model::query::{ColType, WorkbookItemKind};

use super::access::FileEntry;
use super::{MAX_COLUMNS, MAX_ROWS, NavItem, QResult, QTable, unique_names};

/// Most nested levels of JSON records flattened into columns (`a.b.c`).
const JSON_FLATTEN_DEPTH: usize = 4;
/// Most arrays listed for one JSON document.
const MAX_NAV_ITEMS: usize = 500;
/// Windows-1252 code points for bytes 0x80-0x9F (the others are Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

// ---------------------------------------------------------------- delimited text

/// Delimited text as raw text fields, `Column1`… (headers are promoted by a later step).
pub fn csv_table(bytes: &[u8], delimiter: Option<&str>, encoding: Option<&str>) -> QResult<QTable> {
    let text = decode(bytes, encoding);
    let delim = match delimiter {
        None | Some("") => None,
        Some(d) if d.eq_ignore_ascii_case("tab") || d == "\\t" => Some('\t'),
        Some(d) => {
            let mut cs = d.chars();
            match (cs.next(), cs.next()) {
                (Some(c), None) if c != '"' && c != '\n' && c != '\r' => Some(c),
                _ => return Err(format!("The delimiter must be a single character (got '{d}').")),
            }
        }
    };
    let mut records = gridcraft_xlsx::csv_records(&text, delim, MAX_ROWS + 1, MAX_COLUMNS);
    // Blank lines at the end are not rows (blank lines between rows are).
    while records.last().is_some_and(|r| r.len() == 1 && r.first().is_some_and(String::is_empty)) {
        records.pop();
    }
    let width = records.iter().map(Vec::len).max().unwrap_or(0);
    let mut t = QTable::with_columns((1..=width).map(|i| format!("Column{i}")).collect());
    for rec in records {
        t.push_row(rec.into_iter().map(Value::text).collect());
    }
    for ty in t.types.iter_mut() {
        *ty = ColType::Text;
    }
    Ok(t)
}

/// Text in the File Origin's encoding: `utf-8` (default, BOMs honoured), `utf-16`/`utf-16le`,
/// `utf-16be`, `windows-1252`/`latin1`. `None` detects like Open does.
pub fn decode(bytes: &[u8], encoding: Option<&str>) -> String {
    let enc = encoding.map(|e| e.trim().to_ascii_lowercase().replace('_', "-"));
    let utf16 = |b: &[u8], le: bool| -> String {
        let units = b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
    };
    match enc.as_deref() {
        Some("utf-16" | "utf-16le" | "1200") => utf16(bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes), true),
        Some("utf-16be" | "1201") => utf16(bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes), false),
        Some("windows-1252" | "1252" | "latin1" | "iso-8859-1" | "cp1252") => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => CP1252.get(usize::from(b - 0x80)).and_then(|c| char::from_u32(*c)).unwrap_or('\u{FFFD}'),
                _ => char::from(b),
            })
            .collect(),
        _ => gridcraft_xlsx::decode_text(bytes),
    }
}

// ---------------------------------------------------------------- JSON

/// The arrays of records (or values) a JSON document holds, as Navigator items (`.`-separated
/// property paths; the empty path is the document itself).
pub fn json_items(doc: &Json) -> Vec<NavItem> {
    let mut out = Vec::new();
    collect_json_arrays(doc, String::new(), 0, &mut out);
    if out.is_empty() && doc.is_object() {
        out.push(NavItem { name: String::new(), kind: "record".into(), detail: "The document as one row".into() });
    }
    out
}

fn collect_json_arrays(v: &Json, path: String, depth: usize, out: &mut Vec<NavItem>) {
    if depth > 6 || out.len() >= MAX_NAV_ITEMS {
        return;
    }
    match v {
        Json::Array(a) => {
            out.push(NavItem { name: path, kind: "list".into(), detail: format!("{} items", a.len()) });
        }
        Json::Object(o) => {
            for (k, child) in o.iter() {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                collect_json_arrays(child, p, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// The value at a `.`-separated path (array elements by index: `items.0`).
fn json_at<'a>(doc: &'a Json, path: &str) -> Option<&'a Json> {
    if path.is_empty() {
        return Some(doc);
    }
    let mut v = doc;
    for part in path.split('.') {
        v = match v {
            Json::Object(o) => o.get(part)?,
            Json::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(v)
}

/// A JSON document as a table: the array at `item` (else the document if it is an array, else
/// its first array) with records flattened into columns (`address.city`); scalars give one
/// `Column1`. A lone record gives one row.
pub fn json_table(bytes: &[u8], item: Option<&str>) -> QResult<QTable> {
    let doc = super::ojson::parse(&decode(bytes, None))?;
    json_doc_table(&doc, item)
}

pub fn json_doc_table(doc: &Json, item: Option<&str>) -> QResult<QTable> {
    let target = match item {
        Some(p) => json_at(doc, p).ok_or_else(|| format!("The JSON has nothing at '{p}'."))?,
        None => match doc {
            Json::Array(_) => doc,
            _ => json_items(doc).into_iter().find(|i| i.kind == "list").and_then(|i| json_at(doc, &i.name)).unwrap_or(doc),
        },
    };
    let rows: Vec<&Json> = match target {
        Json::Array(a) => a.iter().take(MAX_ROWS).collect(),
        other => vec![other],
    };
    if rows.iter().all(|r| !r.is_object()) {
        let mut t = QTable::with_columns(vec!["Column1".into()]);
        for r in rows {
            t.push_row(vec![json_value(r)]);
        }
        return Ok(t);
    }
    let mut columns: Vec<String> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut flat_rows: Vec<Vec<(usize, Value)>> = Vec::with_capacity(rows.len());
    for r in rows {
        let mut fields = Vec::new();
        flatten(r, "", 0, &mut |name, v| {
            let i = match index.get(name) {
                Some(i) => *i,
                None if columns.len() < MAX_COLUMNS => {
                    columns.push(name.to_string());
                    index.insert(name.to_string(), columns.len() - 1);
                    columns.len() - 1
                }
                None => return,
            };
            fields.push((i, v));
        });
        flat_rows.push(fields);
    }
    let mut t = QTable::with_columns(columns);
    let width = t.columns.len();
    for fields in flat_rows {
        let mut row = vec![Value::Empty; width];
        for (i, v) in fields {
            if let Some(slot) = row.get_mut(i) {
                *slot = v;
            }
        }
        t.push_row(row);
    }
    Ok(t)
}

/// Calls `f` with each leaf of a record: nested records become `parent.child` columns, lists
/// stay as JSON text.
fn flatten(v: &Json, prefix: &str, depth: usize, f: &mut dyn FnMut(&str, Value)) {
    match v {
        Json::Object(o) if depth < JSON_FLATTEN_DEPTH => {
            for (k, child) in o.iter() {
                let name = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(child, &name, depth + 1, f);
            }
        }
        _ => f(if prefix.is_empty() { "Value" } else { prefix }, json_value(v)),
    }
}

/// A JSON scalar as a cell value; lists and records as their JSON text.
pub fn json_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Empty,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::number(*n),
        Json::String(s) => Value::text(s.as_str()),
        other => Value::text(other.text(32_767).chars().take(32_767).collect::<String>()),
    }
}

// ---------------------------------------------------------------- XML

/// The element paths whose elements repeat under one parent (the tables of an XML document).
pub fn xml_items(root: &gridcraft_xlsx::XmlElement) -> Vec<NavItem> {
    let mut out = Vec::new();
    collect_xml_tables(root, &root.name, 0, &mut out);
    if out.is_empty() {
        out.push(NavItem { name: root.name.clone(), kind: "table".into(), detail: "The root element".into() });
    }
    out
}

fn collect_xml_tables(el: &gridcraft_xlsx::XmlElement, path: &str, depth: usize, out: &mut Vec<NavItem>) {
    if depth > 32 || out.len() >= MAX_NAV_ITEMS {
        return;
    }
    let mut names: Vec<&str> = Vec::new();
    for c in &el.children {
        if !names.contains(&c.name.as_str()) {
            names.push(&c.name);
        }
    }
    for n in names {
        let count = el.kids(n).count();
        let child_path = format!("{path}/{n}");
        let first = el.kids(n).next();
        let flat_record = first.is_some_and(|f| !f.children.is_empty() && f.children.iter().all(|g| g.children.is_empty()));
        if (count > 1 || flat_record) && !out.iter().any(|i| i.name == child_path) {
            out.push(NavItem { name: child_path.clone(), kind: "table".into(), detail: format!("{count} rows") });
        }
        // Look deeper under the first element of each name (siblings share a shape).
        if let Some(f) = first {
            collect_xml_tables(f, &child_path, depth + 1, out);
        }
    }
}

/// An XML document as a table: the elements at `item` (else the first table found) are rows;
/// their attributes (`Attribute:name`) and child elements' text are columns.
pub fn xml_table(bytes: &[u8], item: Option<&str>) -> QResult<QTable> {
    let root = gridcraft_xlsx::parse_xml(bytes).map_err(|e| format!("The XML couldn't be read: {e}"))?;
    let path = match item {
        Some(p) => p.to_string(),
        None => xml_items(&root).into_iter().next().map(|i| i.name).unwrap_or_else(|| root.name.clone()),
    };
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let Some((first, rest)) = parts.split_first() else { return Err("The XML table path is empty.".into()) };
    if *first != root.name {
        return Err(format!("The XML has no element '{path}'."));
    }
    // All elements at the path (every parent along the way, not only the first).
    let mut level: Vec<&gridcraft_xlsx::XmlElement> = vec![&root];
    for name in rest {
        level = level.iter().flat_map(|e| e.kids(name)).take(MAX_ROWS).collect();
    }
    if level.is_empty() {
        return Err(format!("The XML has no element '{path}'."));
    }
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<(usize, Value)>> = Vec::new();
    fn col(columns: &mut Vec<String>, name: String) -> Option<usize> {
        match columns.iter().position(|c| *c == name) {
            Some(i) => Some(i),
            None if columns.len() < MAX_COLUMNS => {
                columns.push(name);
                Some(columns.len() - 1)
            }
            None => None,
        }
    }
    for e in level {
        let mut fields = Vec::new();
        for (k, v) in &e.attrs {
            if let Some(i) = col(&mut columns, format!("Attribute:{k}")) {
                fields.push((i, Value::text(v.as_str())));
            }
        }
        if e.children.is_empty()
            && let Some(i) = col(&mut columns, "Element:Text".into())
        {
            fields.push((i, text_value(&e.text)));
        }
        for c in &e.children {
            let text = if c.children.is_empty() { c.text.clone() } else { all_text(c) };
            // A repeated child element keeps its first value.
            if let Some(i) = col(&mut columns, c.name.clone())
                && !fields.iter().any(|(j, _)| *j == i)
            {
                fields.push((i, text_value(&text)));
            }
            for (k, v) in &c.attrs {
                if let Some(i) = col(&mut columns, format!("{}.Attribute:{k}", c.name))
                    && !fields.iter().any(|(j, _)| *j == i)
                {
                    fields.push((i, Value::text(v.as_str())));
                }
            }
        }
        rows.push(fields);
    }
    let mut t = QTable::with_columns(columns);
    let width = t.columns.len();
    for fields in rows {
        let mut row = vec![Value::Empty; width];
        for (i, v) in fields {
            if let Some(slot) = row.get_mut(i) {
                *slot = v;
            }
        }
        t.push_row(row);
    }
    for ty in t.types.iter_mut() {
        *ty = ColType::Text;
    }
    Ok(t)
}

fn text_value(s: &str) -> Value {
    if s.is_empty() { Value::Empty } else { Value::text(s) }
}

/// The text of an element and its descendants, in order, bounded.
fn all_text(e: &gridcraft_xlsx::XmlElement) -> String {
    let mut out = e.text.clone();
    let mut stack: Vec<&gridcraft_xlsx::XmlElement> = e.children.iter().rev().collect();
    while let Some(c) = stack.pop() {
        if out.len() > 32_767 {
            break;
        }
        out.push_str(&c.text);
        stack.extend(c.children.iter().rev());
    }
    out
}

// ---------------------------------------------------------------- Excel workbooks

/// The sheets, tables and defined names of a workbook, as Navigator items.
pub fn workbook_items(wb: &Workbook) -> Vec<NavItem> {
    let mut out = Vec::new();
    for t in wb.sheets.iter().flat_map(|s| s.tables.iter()) {
        out.push(NavItem { name: t.name.clone(), kind: "table".into(), detail: format!("{} rows", t.range.height().saturating_sub(1)) });
    }
    for n in wb.names.iter().filter(|n| !n.hidden && n.scope.is_none() && !n.name.starts_with('_')) {
        out.push(NavItem { name: n.name.clone(), kind: "definedName".into(), detail: n.formula.clone() });
    }
    for s in &wb.sheets {
        out.push(NavItem { name: s.name.clone(), kind: "sheet".into(), detail: s.used_range().map(|r| r.a1()).unwrap_or_default() });
    }
    out
}

/// A sheet's used range, a table, or a defined name's range of `wb` as a table. Sheets and names
/// give `Column1`… (Power Query promotes their headers in a later step); tables keep theirs.
pub fn workbook_table(wb: &Workbook, item: &str, kind: WorkbookItemKind) -> QResult<QTable> {
    match kind {
        WorkbookItemKind::Table => {
            let (si, ti) = wb.table(item).ok_or_else(|| format!("The workbook has no table named '{item}'."))?;
            let sh = wb.sheet(si).ok_or("The table's sheet is missing.")?;
            let t = sh.tables.get(ti).ok_or("The table is missing.")?;
            let mut out = QTable::with_columns(t.columns.iter().map(|c| c.name.clone()).collect());
            if let Some(data) = t.data_range() {
                for r in data.start.row..=data.end.row {
                    if out.rows.len() >= MAX_ROWS {
                        break;
                    }
                    out.push_row((data.start.col..=data.end.col).map(|c| sh.value(CellRef::new(r, c))).collect());
                }
            }
            Ok(out)
        }
        WorkbookItemKind::Sheet => {
            let si = wb.sheet_index(item).ok_or_else(|| format!("The workbook has no sheet named '{item}'."))?;
            let sh = wb.sheet(si).ok_or("The sheet is missing.")?;
            match sh.used_range() {
                Some(r) => Ok(range_table(sh, r, false)),
                None => Ok(QTable::default()),
            }
        }
        WorkbookItemKind::DefinedName => {
            let n = wb.names.iter().find(|n| n.name.eq_ignore_ascii_case(item)).ok_or_else(|| format!("The workbook has no name '{item}'."))?;
            let (sheet, range) = name_range(wb, &n.formula).ok_or_else(|| format!("The name '{item}' doesn't refer to a range."))?;
            let sh = wb.sheet(sheet).ok_or("The name's sheet is missing.")?;
            Ok(range_table(sh, range, false))
        }
    }
}

/// `Sheet1!$A$1:$C$9` → (sheet index, range).
pub fn name_range(wb: &Workbook, formula: &str) -> Option<(usize, RangeRef)> {
    let f = formula.trim().trim_start_matches('=');
    let (sheet, body) = f.rsplit_once('!')?;
    let sheet = sheet.trim_matches('\'').replace("''", "'");
    let si = wb.sheet_index(&sheet)?;
    let range = RangeRef::parse(&body.replace('$', ""))?;
    Some((si, range))
}

/// A block of cells as a table; `headers` takes the first row as column names.
pub fn range_table(sh: &gridcraft_model::Sheet, r: RangeRef, headers: bool) -> QTable {
    let width = (r.width() as usize).min(MAX_COLUMNS);
    let mut columns: Vec<String> = if headers {
        (0..width).map(|i| sh.value(CellRef::new(r.start.row, r.start.col + i as u32)).display()).collect()
    } else {
        (1..=width).map(|i| format!("Column{i}")).collect()
    };
    for (i, c) in columns.iter_mut().enumerate() {
        if c.is_empty() {
            *c = format!("Column{}", i + 1);
        }
    }
    unique_names(&mut columns);
    let mut t = QTable::with_columns(columns);
    let first = r.start.row + u32::from(headers);
    for row in first..=r.end.row {
        if t.rows.len() >= MAX_ROWS || row < first {
            break;
        }
        t.push_row((0..width).map(|i| sh.value(CellRef::new(row, r.start.col + i as u32))).collect());
    }
    t
}

// ---------------------------------------------------------------- folders

/// From Folder's columns, one row per file: Power Query's, with its Attributes record expanded
/// into the fields people use (size, hidden, read-only). The Content column (the file's bytes)
/// isn't shown; Combine reads the files instead.
pub fn folder_table(files: &[FileEntry]) -> QTable {
    let mut t = QTable::with_columns(
        [
            "Name",
            "Extension",
            "Date accessed",
            "Date modified",
            "Date created",
            "Attributes.Size",
            "Attributes.Hidden",
            "Attributes.ReadOnly",
            "Folder Path",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    );
    t.types = vec![
        ColType::Text,
        ColType::Text,
        ColType::DateTime,
        ColType::DateTime,
        ColType::DateTime,
        ColType::Whole,
        ColType::Logical,
        ColType::Logical,
        ColType::Text,
    ];
    let num = |v: Option<f64>| v.map(Value::number).unwrap_or(Value::Empty);
    for f in files.iter().take(MAX_ROWS) {
        t.push_row(vec![
            Value::text(f.name.as_str()),
            Value::text(f.extension.as_str()),
            num(f.accessed),
            num(f.modified),
            num(f.created),
            Value::number(f.size as f64),
            Value::Bool(f.hidden),
            Value::Bool(f.read_only),
            Value::text(f.folder.as_str()),
        ]);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_reads_raw_fields_with_any_delimiter() {
        let t = csv_table(b"a;b\n1;\"x;y\"\n\n", None, None).unwrap();
        assert_eq!(t.columns, vec!["Column1", "Column2"]);
        assert_eq!(t.rows, vec![vec![Value::text("a"), Value::text("b")], vec![Value::text("1"), Value::text("x;y")]]);
        let t = csv_table(b"a|b\n1|2", Some("|"), None).unwrap();
        assert_eq!(t.rows[1], vec![Value::text("1"), Value::text("2")]);
        assert!(csv_table(b"a", Some("ab"), None).is_err());
        assert_eq!(decode(&[0x63, 0x61, 0x66, 0xE9], Some("windows-1252")), "café");
    }

    #[test]
    fn json_arrays_flatten_into_columns() {
        let doc = br#"{"meta":{"n":2},"items":[{"id":1,"name":"A","address":{"city":"Berlin"}},{"id":2,"tags":["x"],"name":null}]}"#;
        let t = json_table(doc, None).unwrap();
        assert_eq!(t.columns, vec!["id", "name", "address.city", "tags"]);
        assert_eq!(t.rows[0], vec![Value::number(1.0), Value::text("A"), Value::text("Berlin"), Value::Empty]);
        assert_eq!(t.rows[1][3], Value::text(r#"["x"]"#));
        let items = json_items(&crate::query::ojson::parse(std::str::from_utf8(doc).unwrap()).unwrap());
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["items"]);
        let t = json_table(b"[1,2,3]", None).unwrap();
        assert_eq!(t.columns, vec!["Column1"]);
        assert_eq!(t.rows.len(), 3);
        assert!(json_table(b"{bad", None).is_err());
        assert!(json_table(b"{}", Some("nope")).is_err());
        // Deep nesting is refused by the parser's recursion limit, not a stack overflow.
        let deep = "[".repeat(100_000);
        assert!(json_table(deep.as_bytes(), None).is_err());
    }

    #[test]
    fn xml_repeating_elements_become_rows() {
        let doc = br#"<?xml version="1.0"?><catalog><book id="b1"><title>Rust</title><price>35</price></book><book id="b2"><title>XML</title><price currency="EUR">20</price></book></catalog>"#;
        let root = gridcraft_xlsx::parse_xml(doc).unwrap();
        assert_eq!(xml_items(&root)[0].name, "catalog/book");
        let t = xml_table(doc, None).unwrap();
        assert_eq!(t.columns, vec!["Attribute:id", "title", "price", "price.Attribute:currency"]);
        assert_eq!(t.rows[1], vec![Value::text("b2"), Value::text("XML"), Value::text("20"), Value::text("EUR")]);
        assert!(xml_table(doc, Some("catalog/nothing")).is_err());
        // Damaged XML is an error or a partial table, never a panic.
        let _ = xml_table(b"<a><b>", None);
    }
}
