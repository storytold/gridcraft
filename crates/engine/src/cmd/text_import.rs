//! Get Data › From Text/CSV: the import preview (delimiter, columns with their detected data type,
//! sample rows) and the import itself, with a column mapping (keep, rename, reorder, data type)
//! and an optional PivotTable built from the imported table.

use gridcraft_core::date::serial_from_ymd;
use gridcraft_core::{CellRef, DateSystem, RangeRef, Value};
use serde_json::{Value as Json, json};

use super::*;

/// The most rows `data.previewText` returns.
const MAX_PREVIEW_ROWS: usize = 200;
/// Rows a column's data type is guessed from by default (Excel's "Based on first 200 rows").
const KIND_SAMPLE_ROWS: usize = 200;
/// The most rows an import takes (a sheet's height).
const MAX_IMPORT_ROWS: usize = 1_048_576;
/// The error a workbook (XLSX, XLS, XLSB, ODS, templates…) gets from the text import: the UI opens
/// it as File › Open would.
pub const WORKBOOK_NOT_TEXT: &str = "This file is a workbook, not text. Open it with File > Open.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "data.previewText",
            "Preview Text/CSV",
            [],
            None,
            "{path | text | base64, delimiter?: \"auto\"|\",\"|\";\"|\"\\t\", header?: bool (default true), decimal?: auto|dot|comma, detect?: rows200 (default)|all|none, limit?: rows (default 20)} → {delimiter, decimal, rows, columns: [{index, name, type: text|number|date, samples}], preview: [[…]]}",
            has_doc,
            preview_text
        ),
        cmd!(
            "data.fromTextCsv",
            "From Text/CSV",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text | base64, delimiter?: \"auto\"|\",\"|\";\"|\"\\t\", header?: bool (default true), decimal?: auto|dot|comma, detect?: rows200 (default)|all|none (the rows `auto` columns take their type from; none keeps text), sheet?: new sheet name, columns?: [{source: index, name?, type?: auto|text|number|date|skip, skip?: bool}] (order = sheet order; default every column, auto), pivot?: true (recommended layout) | {rows?, columns?, values?: [field | {field, func?}], filters?} using the imported names} → imports into a new sheet as a table, then the PivotTable on its own sheet",
            has_doc,
            from_text
        ),
        cmd!(
            "data.getData",
            "Get Data",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text | base64} (CSV/TSV/JSON array of objects) → new sheet table; takes data.fromTextCsv's mapping and pivot params too",
            has_doc,
            from_text
        ),
    ]
}

/// A column's data type in the mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
    Date,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Number => "number",
            Kind::Date => "date",
        }
    }
}

/// The parsed file: field text by record, plus how it was read.
struct Records {
    delimiter: char,
    decimal_comma: bool,
    /// Rows a column's type is detected from: `Some(0)` detects nothing, `None` reads them all.
    sample: Option<usize>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

fn source_bytes(cmd: &str, p: &Json) -> Result<Option<Vec<u8>>> {
    if let Some(t) = str_param(p, "text") {
        Ok(Some(t.as_bytes().to_vec()))
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path).map(Some)
    } else if let Some(b) = str_param(p, "base64") {
        crate::io::base64_decode(b).map(Some).ok_or_else(|| bad(cmd, "`base64` is not valid base64"))
    } else {
        Ok(None)
    }
}

fn delimiter_param(p: &Json) -> Option<u8> {
    match str_param(p, "delimiter")? {
        "" | "auto" => None,
        "tab" | "\\t" => Some(b'\t'),
        "semicolon" => Some(b';'),
        "comma" => Some(b','),
        d => d.chars().next().and_then(|c| u8::try_from(c).ok()),
    }
}

/// A JSON array of objects, the one JSON shape Get Data reads; anything else is delimited text
/// (a CSV whose first field is `[tag]` starts with `[` too).
fn json_objects(text: &str) -> Option<Vec<Json>> {
    if !text.trim_start().starts_with('[') {
        return None;
    }
    serde_json::from_str::<Vec<Json>>(text).ok().filter(|arr| !arr.is_empty() && arr.iter().all(Json::is_object))
}

/// Reads the file into records: a JSON array of objects (keys are the header), else delimited
/// text, every field kept as text so the mapping decides the types. A workbook (by its content,
/// whatever its name) is refused with [`WORKBOOK_NOT_TEXT`] so the caller can open it instead.
fn read_records(bytes: &[u8], p: &Json) -> Result<Records> {
    if !matches!(gridcraft_xlsx::sniff(bytes), gridcraft_xlsx::Format::Csv | gridcraft_xlsx::Format::Unknown) {
        return Err(EngineError::Other(WORKBOOK_NOT_TEXT.into()));
    }
    let header_row = bool_param(p, "header").unwrap_or(true);
    let text = String::from_utf8_lossy(bytes);
    let (delimiter, mut all) = if let Some(arr) = json_objects(&text) {
        let mut keys: Vec<String> = Vec::new();
        for o in &arr {
            if let Some(m) = o.as_object() {
                for k in m.keys() {
                    if !keys.contains(k) {
                        keys.push(k.clone());
                    }
                }
            }
        }
        let field = |v: Option<&Json>| match v {
            None | Some(Json::Null) => String::new(),
            Some(Json::String(s)) => s.clone(),
            Some(Json::Bool(b)) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Some(v) => v.to_string(),
        };
        let mut out = vec![keys.clone()];
        out.extend(arr.iter().take(MAX_IMPORT_ROWS).map(|o| keys.iter().map(|k| field(o.get(k))).collect()));
        (',', out)
    } else {
        let opts = gridcraft_xlsx::CsvOptions { delimiter: delimiter_param(p).unwrap_or(0), parse_values: false, ..Default::default() };
        let wb = gridcraft_xlsx::read_csv(bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
        let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
        let Some(u) = sh.used_range() else { return Err(EngineError::Other("The file is empty.".into())) };
        let rows = (u.start.row..=u.end.row)
            .map(|r| {
                (0..=u.end.col)
                    .map(|c| match sh.value(CellRef::new(r, c)) {
                        Value::Empty => String::new(),
                        v => v.display(),
                    })
                    .collect()
            })
            .collect();
        (wb.csv_delimiter.map_or(',', char::from), rows)
    };
    let width = all.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return Err(EngineError::Other("Nothing to import.".into()));
    }
    for r in &mut all {
        r.resize(width, String::new());
    }
    let header = if header_row && !all.is_empty() { all.remove(0) } else { vec![String::new(); width] };
    let decimal_comma = match str_param(p, "decimal") {
        Some("comma") => true,
        Some("dot") => false,
        // A `;` file is the European convention: decimal commas.
        _ => delimiter == ';',
    };
    let sample = match str_param(p, "detect") {
        Some("all") => None,
        Some("none") => Some(0),
        _ => Some(KIND_SAMPLE_ROWS),
    };
    Ok(Records { delimiter, decimal_comma, sample, header: unique_names(&header), rows: all })
}

/// Header names as the table will have them: blanks become `Column{n}` and repeats get ` 2`, ` 3`…
fn unique_names(header: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(header.len());
    for (i, h) in header.iter().enumerate() {
        let base = match h.trim() {
            "" => format!("Column{}", i + 1),
            t => t.to_string(),
        };
        let mut name = base.clone();
        let mut n = 2;
        while out.iter().any(|o| o.to_lowercase() == name.to_lowercase()) {
            name = format!("{base} {n}");
            n += 1;
        }
        out.push(name);
    }
    out
}

/// A number written with either decimal convention: `1 234,56` (comma) or `1,234.56` (dot),
/// optionally negative or a percentage.
fn parse_number(s: &str, decimal_comma: bool) -> Option<f64> {
    let t: String = s.trim().chars().filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}')).collect();
    let (t, scale) = match t.strip_suffix('%') {
        Some(rest) => (rest.to_string(), 0.01),
        None => (t, 1.0),
    };
    let (thousands, decimal) = if decimal_comma { ('.', ',') } else { (',', '.') };
    // A thousands separator splits digits into threes: `21.10.2026` is not a number.
    let whole = t.split(decimal).next().unwrap_or_default();
    if t.matches(decimal).count() > 1 || whole.split(thousands).skip(1).any(|g| g.len() != 3) {
        return None;
    }
    let t = t.replace(thousands, "").replace(decimal, ".");
    let digits = t.chars().filter(char::is_ascii_digit).count();
    if digits == 0 || !t.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E')) {
        return None;
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| n * scale)
}

/// A date (with an optional `hh:mm[:ss]` time): `2026-10-21`, `21.10.2026`, `21-10-2026`, and
/// `21/10/2026` day-first in comma-decimal files, `10/21/2026` month-first otherwise. Returns the
/// serial and the number format to show it with.
fn parse_date(s: &str, decimal_comma: bool, sys: DateSystem) -> Option<(f64, &'static str)> {
    let t = s.trim();
    let (date, time) = match t.split_once(['T', ' ']) {
        Some((d, rest)) => (d, Some(rest.trim())),
        None => (t, None),
    };
    let nums = |sep: char| -> Option<[i64; 3]> {
        let parts: Vec<&str> = date.split(sep).collect();
        if parts.len() != 3 || parts.iter().any(|x| x.is_empty() || x.len() > 4 || !x.chars().all(|c| c.is_ascii_digit())) {
            return None;
        }
        Some([parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?])
    };
    let (y, m, d) = if let Some([a, b, c]) = nums('-') {
        if date.split('-').next().is_some_and(|x| x.len() == 4) { (a, b, c) } else { (c, b, a) }
    } else if let Some([a, b, c]) = nums('.') {
        (c, b, a)
    } else if let Some([a, b, c]) = nums('/') {
        if date.split('/').next().is_some_and(|x| x.len() == 4) {
            (a, b, c)
        } else if decimal_comma {
            (c, b, a)
        } else {
            (c, a, b)
        }
    } else {
        return None;
    };
    if !(1000..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    let first = serial_from_ymd(sys, y, m, 1)?;
    let next = serial_from_ymd(sys, y, m + 1, 1)?;
    if (d as f64) > next - first {
        return None;
    }
    let day = serial_from_ymd(sys, y, m, d)?;
    let Some(time) = time.filter(|x| !x.is_empty()) else { return Some((day, "yyyy-mm-dd")) };
    let hms: Vec<&str> = time.trim_end_matches('Z').split(':').collect();
    if !(2..=3).contains(&hms.len()) {
        return None;
    }
    let part = |i: usize, max: f64| -> Option<f64> { hms.get(i).map_or(Some(0.0), |x| x.parse::<f64>().ok().filter(|v| (0.0..max).contains(v))) };
    let secs = part(0, 24.0)? * 3600.0 + part(1, 60.0)? * 60.0 + part(2, 60.0)?;
    Some((day + secs / 86_400.0, "yyyy-mm-dd hh:mm"))
}

/// The data type a column's values suggest: number or date when every non-blank sample is one.
fn detect_kind(rec: &Records, col: usize, sys: DateSystem) -> Kind {
    let decimal_comma = rec.decimal_comma;
    let values: Vec<&str> =
        rec.rows.iter().take(rec.sample.unwrap_or(usize::MAX)).filter_map(|r| r.get(col)).map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if values.is_empty() {
        Kind::Text
    } else if values.iter().all(|v| parse_number(v, decimal_comma).is_some()) {
        Kind::Number
    } else if values.iter().all(|v| parse_date(v, decimal_comma, sys).is_some()) {
        Kind::Date
    } else {
        Kind::Text
    }
}

fn date_system(s: &Session) -> DateSystem {
    s.doc().map(|d| d.wb.date_system).unwrap_or_default()
}

fn delimiter_name(d: char) -> String {
    if d == '\t' { "\\t".into() } else { d.to_string() }
}

fn preview_text(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(bytes) = source_bytes("data.previewText", p)? else { return Err(bad("data.previewText", "missing `path`, `text` or `base64`")) };
    let rec = read_records(&bytes, p)?;
    let sys = date_system(s);
    let limit = u32_param(p, "limit").map_or(20, |n| n as usize).min(MAX_PREVIEW_ROWS);
    let columns: Vec<Json> = rec
        .header
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let samples: Vec<&str> = rec.rows.iter().filter_map(|r| r.get(i)).map(String::as_str).filter(|v| !v.trim().is_empty()).take(5).collect();
            json!({"index": i, "name": name, "type": detect_kind(&rec, i, sys).name(), "samples": samples})
        })
        .collect();
    Ok(json!({
        "delimiter": delimiter_name(rec.delimiter),
        "decimal": if rec.decimal_comma { "comma" } else { "dot" },
        "rows": rec.rows.len(),
        "columns": columns,
        "preview": rec.rows.iter().take(limit).collect::<Vec<_>>(),
    }))
}

/// One column of the imported sheet.
struct Target {
    source: usize,
    name: String,
    kind: Kind,
}

fn targets(rec: &Records, p: &Json, sys: DateSystem) -> Result<Vec<Target>> {
    let kind_of = |t: Option<&str>, col: usize| -> Result<Kind> {
        Ok(match t.unwrap_or("auto") {
            "auto" | "" => detect_kind(rec, col, sys),
            "text" => Kind::Text,
            "number" => Kind::Number,
            "date" => Kind::Date,
            other => return Err(bad("data.fromTextCsv", format!("unknown type `{other}` (auto|text|number|date|skip)"))),
        })
    };
    let Some(list) = p.get("columns").and_then(Json::as_array) else {
        return rec.header.iter().enumerate().map(|(i, n)| Ok(Target { source: i, name: n.clone(), kind: kind_of(None, i)? })).collect();
    };
    let mut out: Vec<Target> = Vec::new();
    for c in list {
        if bool_param(c, "skip").unwrap_or(false) || str_param(c, "type") == Some("skip") {
            continue;
        }
        let source = c
            .get("source")
            .and_then(Json::as_u64)
            .map(|n| n as usize)
            .ok_or_else(|| bad("data.fromTextCsv", "each column needs `source` (its index in the file)"))?;
        let Some(original) = rec.header.get(source) else {
            return Err(bad("data.fromTextCsv", format!("the file has no column {source} (it has {})", rec.header.len())));
        };
        let name = str_param(c, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or(original).to_string();
        out.push(Target { source, name, kind: kind_of(str_param(c, "type"), source)? });
    }
    if out.is_empty() {
        return Err(bad("data.fromTextCsv", "every column is skipped: nothing to import"));
    }
    let names = unique_names(&out.iter().map(|t| t.name.clone()).collect::<Vec<_>>());
    for (t, n) in out.iter_mut().zip(names) {
        t.name = n;
    }
    Ok(out)
}

/// A field as the cell input `range.setValues` takes: numbers as numbers, text with a leading
/// apostrophe so it stays exactly as in the file (`=…` is not a formula, `007` keeps its zeros).
fn cell_value(field: &str, kind: Kind, decimal_comma: bool, sys: DateSystem) -> Json {
    if field.trim().is_empty() {
        return Json::Null;
    }
    let number = match kind {
        Kind::Number => parse_number(field, decimal_comma),
        Kind::Date => parse_date(field, decimal_comma, sys).map(|(n, _)| n),
        Kind::Text => None,
    };
    number.map_or_else(|| json!(format!("'{field}")), |n| json!(n))
}

pub(crate) fn from_text(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(bytes) = source_bytes("data.fromTextCsv", p)? else {
        s.ui_requests.push(crate::UiRequest::Dialog("importText".into(), json!({})));
        return ok();
    };
    let rec = read_records(&bytes, p)?;
    if rec.rows.len() >= MAX_IMPORT_ROWS {
        return Err(EngineError::Other("The file has more rows than a sheet holds.".into()));
    }
    let sys = date_system(s);
    let cols = targets(&rec, p, sys)?;
    let mut values: Vec<Vec<Json>> = Vec::with_capacity(rec.rows.len() + 1);
    values.push(cols.iter().map(|t| json!(format!("'{}", t.name))).collect());
    for r in &rec.rows {
        values.push(cols.iter().map(|t| cell_value(r.get(t.source).map_or("", String::as_str), t.kind, rec.decimal_comma, sys)).collect());
    }
    let name = str_param(p, "sheet").map(str::to_string).or_else(|| {
        str_param(p, "path").and_then(|x| std::path::Path::new(x).file_stem().and_then(|f| f.to_str()).map(|f| f.chars().take(31).collect()))
    });
    let sheet = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    s.execute("range.setValues", json!({"range": "A1", "values": values}))?;
    let last_row = values.len() as u32 - 1;
    let last_col = cols.len() as u32 - 1;
    for (i, t) in cols.iter().enumerate() {
        if t.kind == Kind::Date && last_row > 0 {
            let with_time =
                rec.rows.iter().filter_map(|r| r.get(t.source)).filter_map(|f| parse_date(f, rec.decimal_comma, sys)).any(|(n, _)| n.fract() != 0.0);
            let range = RangeRef::new(CellRef::new(1, i as u32), CellRef::new(last_row, i as u32));
            s.execute("home.numberFormat", json!({"range": range.a1(), "code": if with_time { "yyyy-mm-dd hh:mm" } else { "yyyy-mm-dd" }}))?;
        }
    }
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(last_row, last_col));
    let table = s.execute("insert.table", json!({"range": range.a1(), "header": true})).ok().and_then(|t| t.get("table").cloned());
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, last_col).a1()}));
    let pivot = match p.get("pivot") {
        None | Some(Json::Null) | Some(Json::Bool(false)) => Json::Null,
        Some(spec) => {
            let source = match &table {
                Some(t) => t.clone(),
                None => json!(format!("'{}'!{}", sheet["name"].as_str().unwrap_or_default().replace('\'', "''"), range.a1())),
            };
            if last_row == 0 {
                return Err(EngineError::Other("A PivotTable needs at least one row of data.".into()));
            }
            if spec.as_bool() == Some(true) {
                s.execute("insert.recommendedPivotTables", json!({"source": source, "apply": 0}))?
            } else {
                let mut q = json!({"source": source, "destination": "new"});
                for area in ["rows", "columns", "values", "filters"] {
                    if let Some(v) = spec.get(area) {
                        q[area] = v.clone();
                    }
                }
                s.execute("insert.pivotTable", q)?
            }
        }
    };
    Ok(
        json!({"sheet": sheet["name"], "table": table, "rows": rec.rows.len(), "cols": cols.len(), "delimiter": delimiter_name(rec.delimiter), "pivot": pivot}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        s
    }

    const SALES: &str =
        "Region;Produkt;Kwota;Data\nPółnoc;Panel A;1 234,50;21.10.2026\nPołudnie;Panel B;99,5;2026-10-22\nPółnoc;Panel B;10;03.01.2026\n";

    #[test]
    fn preview_detects_semicolons_decimal_commas_and_types() {
        let mut s = session();
        let r = s.execute("data.previewText", json!({"text": SALES})).unwrap();
        assert_eq!(r["delimiter"], ";");
        assert_eq!(r["decimal"], "comma");
        assert_eq!(r["rows"], 3);
        let types: Vec<&str> = r["columns"].as_array().unwrap().iter().map(|c| c["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["text", "text", "number", "date"]);
        assert_eq!(r["columns"][2]["samples"][0], "1 234,50");
        assert_eq!(r["preview"][1][0], "Południe");
    }

    #[test]
    fn mapping_renames_reorders_skips_and_types_columns() {
        let mut s = session();
        let cols = json!([{"source": 2, "name": "Wartość"}, {"source": 0}, {"source": 1, "skip": true}, {"source": 3, "type": "text"}]);
        let r = s.execute("data.fromTextCsv", json!({"text": SALES, "sheet": "Dane", "columns": cols})).unwrap();
        assert_eq!(r["cols"], 3);
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(d.wb.sheet_index("Dane").unwrap()).unwrap();
        let v = |a: &str| sh.value(CellRef::parse(a).unwrap());
        assert_eq!(v("A1"), Value::text("Wartość"));
        assert_eq!(v("B1"), Value::text("Region"));
        assert_eq!(v("A2"), Value::number(1234.5));
        assert_eq!(v("A3"), Value::number(99.5));
        assert_eq!(v("C2"), Value::text("21.10.2026"));
        assert_eq!(sh.tables.len(), 1);
    }

    #[test]
    fn dates_become_serials_with_a_date_format() {
        let mut s = session();
        s.execute("data.fromTextCsv", json!({"text": SALES, "sheet": "Dane"})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(d.wb.sheet_index("Dane").unwrap()).unwrap();
        let at = CellRef::parse("D2").unwrap();
        assert_eq!(sh.value(at), Value::number(serial_from_ymd(DateSystem::D1900, 2026, 10, 21).unwrap()));
        assert_eq!(crate::display::cell_text(&d.wb, sh, at), "2026-10-21");
    }

    #[test]
    fn text_stays_text_even_when_it_looks_like_a_formula() {
        let mut s = session();
        s.execute("data.fromTextCsv", json!({"text": "a,b\n=1+1,007\n", "columns": [{"source": 0}, {"source": 1, "type": "text"}]})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(sh.value(CellRef::parse("A2").unwrap()), Value::text("=1+1"));
        assert_eq!(sh.value(CellRef::parse("B2").unwrap()), Value::text("007"));
    }

    #[test]
    fn import_builds_the_requested_pivot_table() {
        let mut s = session();
        let pivot = json!({"rows": ["Region"], "columns": ["Produkt"], "values": [{"field": "Kwota", "func": "sum"}]});
        let r = s.execute("data.fromTextCsv", json!({"text": SALES, "sheet": "Dane", "pivot": pivot})).unwrap();
        let pv = &r["pivot"];
        assert!(pv["name"].as_str().is_some_and(|n| n.starts_with("PivotTable")), "{r}");
        let d = s.doc().unwrap();
        let si = d.wb.sheet_index(pv["sheet"].as_str().unwrap()).unwrap();
        let text = crate::display::range_text(&d.wb, si, RangeRef::parse(pv["range"].as_str().unwrap()).unwrap());
        assert!(text.contains("Północ") && text.contains("1244.5"), "{text}");
    }

    #[test]
    fn pivot_true_applies_the_first_recommendation() {
        let mut s = session();
        let r = s.execute("data.fromTextCsv", json!({"text": SALES, "pivot": true})).unwrap();
        assert!(r["pivot"].is_object(), "{r}");
        assert!(s.doc().unwrap().wb.sheets.iter().any(|sh| !sh.pivots.is_empty()));
    }

    #[test]
    fn type_detection_reads_200_rows_the_whole_file_or_nothing() {
        let mut s = session();
        let mut text = String::from("n\n");
        for i in 0..200 {
            text.push_str(&format!("{i}\n"));
        }
        text.push_str("x\n");
        let kind = |s: &mut Session, detect: &str| {
            s.execute("data.previewText", json!({"text": text, "detect": detect})).unwrap()["columns"][0]["type"].clone()
        };
        assert_eq!(kind(&mut s, "rows200"), "number");
        assert_eq!(kind(&mut s, "all"), "text");
        assert_eq!(kind(&mut s, "none"), "text");
    }

    #[test]
    fn only_an_array_of_objects_is_read_as_json() {
        let mut s = session();
        // A CSV whose first field starts with `[` is still CSV.
        let r = s.execute("data.previewText", json!({"text": "[tag],value\n[a],1\n"})).unwrap();
        assert_eq!(r["columns"][0]["name"], "[tag]");
        assert_eq!(r["columns"][1]["type"], "number");
        let r = s.execute("data.previewText", json!({"text": "[1,2]\n"})).unwrap();
        assert_eq!(r["columns"].as_array().unwrap().len(), 2);
        let r = s.execute("data.previewText", json!({"text": r#"[{"a": 1, "b": "x"}, {"a": 2}]"#})).unwrap();
        assert_eq!(r["columns"][0]["name"], "a");
        assert_eq!(r["preview"][0], json!(["1", "x"]));
    }

    #[test]
    fn a_workbook_is_refused_by_content_whatever_its_name() {
        let mut s = session();
        let xlsx = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
        let err = s.execute("data.previewText", json!({"base64": crate::io::base64_encode(&xlsx)})).unwrap_err();
        assert!(err.to_string().contains(WORKBOOK_NOT_TEXT), "{err}");
    }

    #[test]
    fn duplicate_and_blank_headers_get_unique_names() {
        assert_eq!(unique_names(&["a".into(), "".into(), "A".into(), "a".into()]), ["a", "Column2", "A 2", "a 3"]);
    }

    #[test]
    fn numbers_and_dates_in_both_conventions() {
        assert_eq!(parse_number("1 234,56", true), Some(1234.56));
        assert_eq!(parse_number("1,234.56", false), Some(1234.56));
        assert_eq!(parse_number("-12,5%", true), Some(-0.125));
        assert_eq!(parse_number("inf", false), None);
        assert_eq!(parse_number("12a", false), None);
        assert_eq!(parse_number("21.10.2026", true), None);
        assert_eq!(parse_number("1.234.567,5", true), Some(1_234_567.5));
        assert_eq!(parse_number("1,5", false), None);
        let sys = DateSystem::D1900;
        let day = serial_from_ymd(sys, 2026, 10, 21);
        assert_eq!(parse_date("21.10.2026", true, sys).map(|x| x.0), day);
        assert_eq!(parse_date("10/21/2026", false, sys).map(|x| x.0), day);
        assert_eq!(parse_date("21/10/2026", true, sys).map(|x| x.0), day);
        assert_eq!(parse_date("2026-10-21T12:00", false, sys).map(|x| x.0), day.map(|d| d + 0.5));
        assert_eq!(parse_date("31.02.2026", true, sys), None);
        assert_eq!(parse_date("1.2", true, sys), None);
    }

    #[test]
    fn programmatic_calls_without_a_file_do_not_import() {
        let mut s = session();
        assert!(s.execute("data.previewText", json!({})).is_err());
        let sheets = s.doc().unwrap().wb.sheets.len();
        s.execute("data.fromTextCsv", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheets.len(), sheets);
        assert!(s.execute("data.fromTextCsv", json!({"text": "a\n1\n", "columns": [{"source": 5}]})).is_err());
    }
}
