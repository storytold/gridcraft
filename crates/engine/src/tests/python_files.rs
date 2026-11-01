//! Python cells in files: native JSON, and xlsx as `_xlfn._xlws.PY(index,0|1)` with the code in
//! the Python part ([MS-XLSX] 2.1.23, 2.3.10) and cached values. Nothing here runs Python: results
//! are cached ones, as a file would hold them.

use std::io::{Read, Write};
use std::sync::Arc;

use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_model::{Cell, PyCode, PyOutput, Workbook};
use serde_json::json;

use super::{s, v};
use crate::{Session, Trust};

fn py(s: &mut Session, cell: &str, code: &str) {
    s.execute("python.set", json!({"cell": cell, "code": code})).unwrap();
}

/// Gives a Python cell a result, as running it (or a file's cache) would.
fn cache(s: &mut Session, cell: &str, value: Value, type_name: &str) {
    let d = s.doc_mut().unwrap();
    let wb = Arc::make_mut(&mut d.wb);
    let sh = wb.sheet_mut(0).unwrap();
    let c = sh.cells.get_mut(CellRef::parse(cell).unwrap()).unwrap();
    let p = Arc::make_mut(c.py.as_mut().unwrap());
    p.output = Some(PyOutput { value, type_name: type_name.into(), preview: String::new(), convertible: true });
}

fn array(rows: usize, cols: usize, data: Vec<Value>) -> Value {
    Value::Array(Arc::new(Array::new(rows, cols, data).unwrap()))
}

fn save(s: &mut Session, format: &str) -> Vec<u8> {
    let r = s.execute("file.saveBytes", json!({"format": format})).unwrap();
    crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap()
}

fn open(s: &mut Session, name: &str, bytes: &[u8]) {
    s.execute("file.open", json!({"name": name, "base64": crate::io::base64_encode(bytes)})).unwrap();
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    z.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
    xml
}

fn sheet_xml(bytes: &[u8]) -> String {
    part(bytes, "xl/worksheets/sheet1.xml")
}

/// The package with `edit` applied to each XML part (by name), and `add` added.
fn with_parts(bytes: &[u8], edit: impl Fn(&str, &str) -> String, add: &[(&str, &str)]) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if let Ok(text) = std::str::from_utf8(&data) {
            data = edit(&name, text).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    for (name, text) in add {
        out.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// The package with `edit` applied to the first sheet's XML.
fn with_sheet_xml(bytes: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
    with_parts(bytes, |name, text| if name == "xl/worksheets/sheet1.xml" { edit(text) } else { text.to_string() }, &[])
}

/// `xml` with its `<sheetData>` replaced by `data`.
fn replace_sheet_data(xml: &str, data: &str) -> String {
    match (xml.find("<sheetData"), xml.find("</sheetData>"), xml.find("<sheetData/>")) {
        (Some(a), Some(b), _) => format!("{}{data}{}", &xml[..a], &xml[b + "</sheetData>".len()..]),
        (_, _, Some(a)) => format!("{}{data}{}", &xml[..a], &xml[a + "<sheetData/>".len()..]),
        _ => panic!("no sheetData in {xml}"),
    }
}

/// An empty workbook's package with `data` as its first sheet's cells and, when `scripts` is
/// given, a Python part holding them (the relationship and the content type included), as
/// [MS-XLSX] 2.1.23 describes it.
fn excel_shaped(data: &str, scripts: Option<&[&str]>) -> Vec<u8> {
    let mut s = s();
    let plain = save(&mut s, "xlsx");
    let Some(scripts) = scripts else { return with_sheet_xml(&plain, |xml| replace_sheet_data(xml, data)) };
    let codes: String = scripts.iter().map(|c| format!("<pythonScript><code>{c}</code></pythonScript>")).collect();
    let python = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<python xmlns=\"http://schemas.microsoft.com/office/spreadsheetml/2023/python\"><environmentDefinition id=\"{{00000000-0000-0000-0000-000000000000}}\"/><pythonScripts>{codes}</pythonScripts></python>"
    );
    const REL: &str =
        "<Relationship Id=\"rIdPy\" Type=\"http://schemas.microsoft.com/office/2023/09/relationships/Python\" Target=\"python/python1.xml\"/>";
    const TYPE: &str = "<Override PartName=\"/xl/python/python1.xml\" ContentType=\"application/vnd.ms-excel.python+xml\"/>";
    let edit = |name: &str, text: &str| match name {
        "xl/worksheets/sheet1.xml" => replace_sheet_data(text, data),
        "xl/_rels/workbook.xml.rels" => text.replace("</Relationships>", &format!("{REL}</Relationships>")),
        "[Content_Types].xml" => text.replace("</Types>", &format!("{TYPE}</Types>")),
        _ => text.to_string(),
    };
    with_parts(&plain, edit, &[("xl/python/python1.xml", &python)])
}

fn code(s: &Session, cell: &str) -> Option<PyCode> {
    s.doc().unwrap().wb.active().unwrap().cell(CellRef::parse(cell).unwrap()).and_then(|c| c.py.as_deref()).cloned()
}

/// A workbook with a value, a formula over a Python cell, a spilling table, an object, long code
/// with quotes and line breaks, and an error.
fn build() -> (Session, String) {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "21"})).unwrap();
    py(&mut s, "B1", "xl('A1') * 2");
    cache(&mut s, "B1", Value::Number(42.0), "int");
    s.execute("cell.set", json!({"cell": "A5", "input": "=B1+1"})).unwrap();
    py(&mut s, "C1", "[[1, 'two'], [3, \"four\"]]");
    cache(&mut s, "C1", array(2, 2, vec![1.0.into(), Value::text("two"), 3.0.into(), Value::text("four")]), "list");
    s.execute("python.set", json!({"cell": "E1", "code": "[1, 2]", "output": "object"})).unwrap();
    cache(&mut s, "E1", array(2, 1, vec![1.0.into(), 2.0.into()]), "list");
    let long = format!("# {}\ntext = \"say \\\"hi\\\"\"\nlen(text)", "x".repeat(7_900));
    assert!(PyCode::new(&long).fits());
    py(&mut s, "F1", &long);
    cache(&mut s, "F1", Value::Number(8.0), "int");
    py(&mut s, "G1", "1/0");
    {
        let d = s.doc_mut().unwrap();
        let sh = Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap();
        sh.cells.get_mut(CellRef::parse("G1").unwrap()).unwrap().value = Value::Error(CellError::Python);
    }
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Number(42.0));
    assert_eq!(v(&s, "A5"), Value::Number(43.0));
    assert_eq!(v(&s, "D2"), Value::text("four"));
    assert_eq!(v(&s, "E1"), Value::text("list"), "a Python object shows its type");
    assert_eq!(v(&s, "E2"), Value::Empty, "and doesn't spill");
    (s, long)
}

#[test]
fn xlsx_stores_the_code_in_the_python_part_with_cached_values() {
    let (mut s, long) = build();
    let bytes = save(&mut s, "xlsx");
    let xml = sheet_xml(&bytes);
    // The formula names the cell's script by index, in the order the cells are written: B1, C1,
    // E1, F1, G1.
    assert!(xml.contains("<f>_xlfn._xlws.PY(0,0)</f><v>42</v>"), "{xml}");
    assert!(xml.contains(">_xlfn._xlws.PY(2,1)</f>"), "the Python object cell has return type 1: {xml}");
    assert!(!xml.contains("xl('A1')"), "the code is not in the sheet");
    let python = part(&bytes, "xl/python.xml");
    assert!(
        python.contains("<python xmlns=\"http://schemas.microsoft.com/office/spreadsheetml/2023/python\"><pythonScripts><pythonScript><code>xl('A1') * 2</code></pythonScript>"),
        "{python}"
    );
    assert_eq!(python.matches("<pythonScript>").count(), 5);
    assert!(
        part(&bytes, "[Content_Types].xml").contains("<Override PartName=\"/xl/python.xml\" ContentType=\"application/vnd.ms-excel.python+xml\"/>")
    );
    assert!(
        part(&bytes, "xl/_rels/workbook.xml.rels")
            .contains("Type=\"http://schemas.microsoft.com/office/2023/09/relationships/Python\" Target=\"python.xml\"")
    );
    assert!(xml.contains("t=\"array\" ref=\"C1:D2\""), "the spill is saved like a dynamic array's: {xml}");
    assert!(xml.contains("#VALUE!") && !xml.contains("#PYTHON!"), "Python errors are cached as #VALUE!");

    open(&mut s, "book.xlsx", &bytes);
    assert_eq!(s.doc().unwrap().trust, Trust::FromFile);
    // Cached values show, spills included, and the code is back.
    assert_eq!(v(&s, "B1"), Value::Number(42.0));
    assert_eq!(v(&s, "A5"), Value::Number(43.0));
    assert_eq!(
        (v(&s, "C1"), v(&s, "D1"), v(&s, "C2"), v(&s, "D2")),
        (Value::Number(1.0), Value::text("two"), Value::Number(3.0), Value::text("four"))
    );
    assert_eq!(code(&s, "B1").unwrap().code, "xl('A1') * 2");
    assert!(code(&s, "E1").unwrap().object);
    assert_eq!(code(&s, "F1").unwrap().code, long, "long code with quotes and line breaks survives");
    assert_eq!(v(&s, "G1"), Value::Error(CellError::Value));
    // A blocker typed into the spill area gives #SPILL!; clearing it spills again, without
    // running anything.
    s.execute("cell.set", json!({"cell": "D2", "input": "x"})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Error(CellError::Spill));
    s.execute("edit.clearContents", json!({"range": "D2"})).unwrap();
    assert_eq!(v(&s, "D2"), Value::text("four"));
    // Saved again, the file holds the same formulas and scripts.
    let again = save(&mut s, "xlsx");
    assert!(sheet_xml(&again).contains("<f>_xlfn._xlws.PY(0,0)</f>"));
    assert_eq!(part(&again, "xl/python.xml"), python);
    // A workbook without Python cells has no Python part.
    let mut plain = super::s();
    assert!(!save(&mut plain, "xlsx").windows(10).any(|w| w == b"python.xml"));
}

#[test]
fn excel_authored_python_cells_are_read_as_python() {
    // A workbook as [MS-XLSX] 2.3.10 describes it: `_xlfn._xlws.PY(index,0|1,params…)` naming a
    // script of the Python part, whose code names the parameters as `%Px%`, with a spilled result
    // cached.
    let data = "<sheetData><row r=\"1\"><c r=\"A1\" cm=\"1\"><f t=\"array\" ref=\"A1:A2\">_xlfn._xlws.PY(0,0)</f><v>1</v></c>\
                <c r=\"B1\" t=\"str\"><f>_xlfn._xlws.PY(1,1,C1:C3)</f><v>DataFrame</v></c></row>\
                <row r=\"2\"><c r=\"A2\"><v>2</v></c></row></sheetData>";
    let bytes = excel_shaped(data, Some(&["[1, 2]", "df = xl(\"%P2%\", headers=True)\ndf.describe()"]));
    let mut s = s();
    open(&mut s, "excel.xlsx", &bytes);
    let a1 = code(&s, "A1").expect("an Excel Python cell is a Python cell");
    assert_eq!(a1.code, "[1, 2]");
    assert!(!a1.object);
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (Value::Number(1.0), Value::Number(2.0)), "its spilled values are kept");
    let cell = s.doc().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap().clone();
    assert!(cell.formula.is_none());
    let b1 = code(&s, "B1").unwrap();
    assert_eq!(b1.code, "df = xl(\"C1:C3\", headers=True)\ndf.describe()", "a parameter is put back as its reference");
    assert!(b1.object);
    assert_eq!(v(&s, "B1"), Value::text("DataFrame"));
    // Saved by GridCraft, the code goes back into a Python part, the reference as text.
    let again = save(&mut s, "xlsx");
    assert!(part(&again, "xl/python.xml").contains("<code>df = xl(\"C1:C3\", headers=True)\ndf.describe()</code>"));
    // The string-literal form (other applications, files GridCraft wrote before) still reads.
    let literal = "<sheetData><row r=\"1\"><c r=\"A1\"><f>_xlfn._xlws.PY(\"1 + 1\",1)</f><v>2</v></c></row></sheetData>";
    open(&mut s, "literal.xlsx", &excel_shaped(literal, None));
    assert_eq!(code(&s, "A1").map(|p| (p.code, p.object)), Some(("1 + 1".to_string(), true)));
}

#[test]
fn a_py_formula_naming_a_missing_script_keeps_only_its_value() {
    let data = "<sheetData><row r=\"1\"><c r=\"A1\"><f>_xlfn._xlws.PY(0,0)</f><v>5</v></c><c r=\"B1\"><f>_xlfn._xlws.PY(3,0)</f><v>6</v></c></row></sheetData>";
    for (name, bytes) in [("one.xlsx", excel_shaped(data, Some(&["5"]))), ("none.xlsx", excel_shaped(data, None))] {
        let (wb, warnings) = crate::io::open_bytes(name, &bytes).unwrap();
        let sh = wb.sheet(0).unwrap();
        let b1 = sh.cell(CellRef::parse("B1").unwrap()).unwrap();
        assert!(b1.py.is_none() && b1.formula.is_none(), "{name}: no code to keep, and no formula #NAME? would replace");
        assert_eq!(b1.value, Value::Number(6.0), "{name}");
        assert!(warnings.iter().any(|w| w.contains("Python")), "{name}: {warnings:?}");
    }
    let (wb, _) = crate::io::open_bytes("one.xlsx", &excel_shaped(data, Some(&["5"]))).unwrap();
    assert_eq!(wb.sheet(0).unwrap().cell(CellRef::parse("A1").unwrap()).unwrap().py.as_ref().map(|p| p.code.as_str()), Some("5"));
}

/// `bytes` with the part `name` stored uncompressed and one byte of its data changed, so the
/// archive is intact but reading the part fails its checksum.
fn with_damaged_part(bytes: &[u8], name: &str, marker: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let entry = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        let options = zip::write::SimpleFileOptions::default();
        let options = if entry == name { options.compression_method(zip::CompressionMethod::Stored) } else { options };
        out.start_file(entry, options).unwrap();
        out.write_all(&data).unwrap();
    }
    let mut zipped = out.finish().unwrap().into_inner();
    let at = zipped.windows(marker.len()).position(|w| w == marker.as_bytes()).expect("the marker is stored as it is");
    zipped[at + 1] ^= 0x20;
    zipped
}

#[test]
fn a_damaged_python_part_is_a_warning_and_the_workbook_opens_with_its_values() {
    let data = "<sheetData><row r=\"1\"><c r=\"A1\"><f>_xlfn._xlws.PY(0,0)</f><v>5</v></c><c r=\"B1\"><v>9</v></c></row></sheetData>";
    let good = excel_shaped(data, Some(&["5"]));
    // The part's data fails its checksum (a damaged archive entry).
    let damaged = with_damaged_part(&good, "xl/python/python1.xml", "<pythonScripts>");
    // The part is not well-formed XML, before its first script.
    let broken = |at: &'static str, with: &'static str| {
        with_parts(&good, move |name, text| if name == "xl/python/python1.xml" { text.replace(at, with) } else { text.to_string() }, &[])
    };
    let malformed = broken("<pythonScripts>", "<pythonScripts><!-- never closed");
    for (name, bytes) in [("damaged.xlsx", damaged), ("malformed.xlsx", malformed)] {
        let (wb, warnings) = crate::io::open_bytes(name, &bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let sh = wb.sheet(0).unwrap();
        let a1 = sh.cell(CellRef::parse("A1").unwrap()).unwrap();
        assert_eq!(a1.value, Value::Number(5.0), "{name}: the cached value is kept");
        assert!(a1.py.is_none() && a1.formula.is_none(), "{name}: as for a missing script, only the value");
        assert_eq!(sh.value(CellRef::parse("B1").unwrap()), Value::Number(9.0), "{name}: the rest of the sheet is read");
        let part = warnings.iter().filter(|w| w.contains("Python part is damaged")).count();
        assert_eq!(part, 1, "{name}: {warnings:?}");
    }
    // Damaged after its first script, the part keeps what it read before the damage.
    let (wb, warnings) = crate::io::open_bytes("late.xlsx", &broken("</pythonScripts>", "<!-- never closed")).unwrap();
    let a1 = wb.sheet(0).unwrap().cell(CellRef::parse("A1").unwrap()).unwrap().clone();
    assert_eq!(a1.py.as_ref().map(|p| p.code.as_str()), Some("5"));
    assert_eq!(warnings.iter().filter(|w| w.contains("Python part is damaged")).count(), 1, "{warnings:?}");
}

#[test]
fn a_python_cell_without_its_code_keeps_its_whole_cached_result_as_values() {
    // A1's cached result spills over A1:A3; C1 is an ordinary dynamic array with a stale cache.
    let data = "<sheetData><row r=\"1\"><c r=\"A1\" cm=\"1\"><f t=\"array\" ref=\"A1:A3\">_xlfn._xlws.PY(0,0)</f><v>1</v></c>\
                <c r=\"C1\" cm=\"1\"><f t=\"array\" ref=\"C1:C3\">SEQUENCE(3)</f><v>1</v></c></row>\
                <row r=\"2\"><c r=\"A2\"><v>2</v></c><c r=\"C2\"><v>9</v></c></row>\
                <row r=\"3\"><c r=\"A3\"><v>3</v></c><c r=\"C3\"><v>9</v></c></row></sheetData>";
    let missing = excel_shaped(data, None);
    let damaged = with_damaged_part(&excel_shaped(data, Some(&["[1, 2, 3]"])), "xl/python/python1.xml", "<pythonScripts>");
    for (name, bytes) in [("missing.xlsx", missing), ("damaged.xlsx", damaged)] {
        let mut s = s();
        open(&mut s, name, &bytes);
        assert_eq!((v(&s, "A1"), v(&s, "A2"), v(&s, "A3")), (Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)), "{name}");
        let sh = s.doc().unwrap().wb.active().unwrap();
        for a in ["A1", "A2", "A3"] {
            let cell = sh.cell(CellRef::parse(a).unwrap()).unwrap_or_else(|| panic!("{name}: {a} is stored"));
            assert!(cell.py.is_none() && cell.formula.is_none(), "{name}: {a} is a plain value");
        }
        assert!(!sh.spill_ranges.contains_key(&CellRef::new(0, 0)), "{name}: no live spill");
        // The ordinary formula fills its spill again, over the stale cached values.
        assert_eq!((v(&s, "C2"), v(&s, "C3")), (Value::Number(2.0), Value::Number(3.0)), "{name}");
        assert!(sh.cell(CellRef::parse("C2").unwrap()).is_none(), "{name}: C2 is the formula's spill, not a cell");
    }
}

#[test]
fn many_cells_naming_one_long_script_take_from_one_budget() {
    // 20,000 cells name one script of 8,000 characters: 160 MB of code if each took a copy.
    let long = "x".repeat(8_000);
    let mut data = String::from("<sheetData>");
    for r in 1..=20_000 {
        data.push_str(&format!("<row r=\"{r}\"><c r=\"A{r}\"><f>_xlfn._xlws.PY(0,0)</f><v>1</v></c></row>"));
    }
    data.push_str("</sheetData>");
    let (wb, warnings) = crate::io::open_bytes("many.xlsx", &excel_shaped(&data, Some(&[&long]))).unwrap();
    let sh = wb.sheet(0).unwrap();
    let with_code = sh.cells.iter().filter(|(_, c)| c.py.is_some()).count();
    assert_eq!(with_code, 64 * 1024 * 1024 / 8_000);
    assert!(warnings.iter().any(|w| w.contains("Python")), "{warnings:?}");
    assert_eq!(sh.value(CellRef::parse("A20000").unwrap()), Value::Number(1.0), "the others keep their values");
}

#[test]
fn a_result_cut_by_the_budget_takes_its_cached_spill_with_it() {
    // Five Python cells each with a cached 1,000 x 1,000 spill: the budget rebuilds four results;
    // the fifth cell keeps only its own value, and its spilled values no longer show.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for i in 0..5u32 {
        let anchor = CellRef::new(0, i * 1_000);
        sh.set_cell(anchor, Cell { py: Some(Arc::new(PyCode::new("x"))), ..Cell::value(Value::Number(1.0)) });
        sh.spill_ranges.insert(anchor, RangeRef::new(anchor, CellRef::new(999, i * 1_000 + 999)));
        sh.spill.insert(CellRef::new(1, i * 1_000), Value::Number(7.0));
        sh.spill.insert(CellRef::new(999, i * 1_000 + 999), Value::Number(8.0));
    }
    let warnings = crate::io::adopt_python(&mut wb);
    assert!(warnings.iter().any(|w| w.contains("Python")), "{warnings:?}");
    let sh = wb.sheet(0).unwrap();
    let mut cut = 0;
    for i in 0..5u32 {
        let anchor = CellRef::new(0, i * 1_000);
        let rebuilt = sh.cell(anchor).and_then(|c| c.py.as_ref()).is_some_and(|p| p.output.is_some());
        let (below, corner) = (sh.value(CellRef::new(1, i * 1_000)), sh.value(CellRef::new(999, i * 1_000 + 999)));
        if rebuilt {
            assert_eq!((below, corner), (Value::Number(7.0), Value::Number(8.0)), "a result within the budget keeps its spill");
            assert!(sh.spill_ranges.contains_key(&anchor));
        } else {
            cut += 1;
            assert_eq!((below, corner), (Value::Empty, Value::Empty), "no stale spilled values beyond the anchor");
            assert!(!sh.spill_ranges.contains_key(&anchor));
            assert_eq!(sh.value(anchor), Value::Number(1.0), "the cell keeps its own cached value");
        }
    }
    assert_eq!(cut, 1);
}

#[test]
fn native_json_keeps_python_cells_and_their_results() {
    let (mut s, long) = build();
    let bytes = save(&mut s, "json");
    open(&mut s, "book.json", &bytes);
    assert_eq!(s.doc().unwrap().trust, Trust::FromFile);
    assert_eq!(v(&s, "B1"), Value::Number(42.0));
    assert_eq!(v(&s, "D2"), Value::text("four"));
    assert_eq!(v(&s, "G1"), Value::Error(CellError::Python), "the native format keeps the exact error");
    assert_eq!(code(&s, "F1").unwrap().code, long);
    let e1 = code(&s, "E1").unwrap();
    assert!(e1.object);
    assert_eq!(e1.output.unwrap().type_name, "list");
}

#[test]
fn a_spill_range_claiming_the_whole_sheet_is_dropped_on_open() {
    let (mut s, _) = build();
    let bytes = save(&mut s, "xlsx");
    let evil = with_sheet_xml(&bytes, |xml| xml.replace("ref=\"C1:D2\"", "ref=\"C1:XFD1048576\""));
    assert_ne!(evil, bytes);
    // Walking the claimed range would take hours; the test finishing is the check.
    open(&mut s, "evil.xlsx", &evil);
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.spill_ranges.values().all(|r| r.count() <= gridcraft_model::python::MAX_RESULT_CELLS as u64), "{:?}", sh.spill_ranges);
    // Edits afterwards don't walk the claimed range either.
    s.execute("cell.set", json!({"cell": "H9", "input": "1"})).unwrap();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(1.0), "the anchor keeps its cached value");
}

#[test]
fn rebuilt_results_have_a_budget_across_the_workbook() {
    // Five Python cells each claiming a 1,000 x 1,000 spill: more than the budget in all.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for i in 0..5u32 {
        let anchor = CellRef::new(0, i * 1_000);
        sh.set_cell(anchor, Cell { py: Some(Arc::new(PyCode::new("x"))), ..Cell::value(Value::Number(1.0)) });
        sh.spill_ranges.insert(anchor, RangeRef::new(anchor, CellRef::new(999, i * 1_000 + 999)));
    }
    let warnings = crate::io::adopt_python(&mut wb);
    let sh = wb.sheet(0).unwrap();
    let rebuilt = sh.cells.iter().filter(|(_, c)| c.py.as_ref().is_some_and(|p| p.output.is_some())).count();
    assert_eq!(rebuilt as u64, crate::io::MAX_ADOPTED_RESULT_CELLS / 1_000_000);
    assert!(warnings.iter().any(|w| w.contains("Python")), "{warnings:?}");
}

#[test]
fn hostile_python_content_in_files_is_capped() {
    let (mut s, _) = build();
    let bytes = save(&mut s, "json");
    // Make the Python cells hostile: code over the limit, and a result whose shape claims far
    // more cells than it holds (the array fields are public, so a crafted file can say anything).
    let mut wb: Workbook = serde_json::from_slice(&bytes).unwrap();
    let sh = wb.sheet_mut(0).unwrap();
    let b1 = sh.cells.get_mut(CellRef::parse("B1").unwrap()).unwrap();
    Arc::make_mut(b1.py.as_mut().unwrap()).code = "y".repeat(9_000);
    let c1 = sh.cells.get_mut(CellRef::parse("C1").unwrap()).unwrap();
    let out = Arc::make_mut(c1.py.as_mut().unwrap()).output.as_mut().unwrap();
    out.value = Value::Array(Arc::new(Array { rows: 100_000, cols: 100_000, data: vec![Value::Number(1.0)] }));
    let bytes = serde_json::to_vec(&wb).unwrap();
    let (wb, warnings) = crate::io::open_bytes("evil.json", &bytes).unwrap();
    let sh = wb.sheet(0).unwrap();
    let b1 = sh.cell(CellRef::parse("B1").unwrap()).unwrap();
    assert!(b1.py.is_none(), "code over the formula limit is dropped");
    assert_eq!(b1.value, Value::Number(42.0), "the cached value stays");
    assert!(warnings.iter().any(|w| w.contains("Python cell")), "{warnings:?}");
    let c1 = sh.cell(CellRef::parse("C1").unwrap()).unwrap();
    assert!(c1.py.as_ref().unwrap().output.is_none(), "a malformed result is dropped");
    open(&mut s, "evil.json", &bytes);
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(1.0));

    // PY formulas from other formats become Python cells; non-literal PY calls stay formulas.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_cell(CellRef::new(0, 0), Cell::formula(gridcraft_model::Formula::new("=PY(\"1+\"&\"1\",1)")));
    sh.set_cell(CellRef::new(1, 0), Cell::formula(gridcraft_model::Formula::new("=PY(A1,0)")));
    sh.set_cell(CellRef::new(2, 0), Cell::formula(gridcraft_model::Formula::new("=_xlfn._xlws.PY(\"2\",0)")));
    crate::io::adopt_python(&mut wb);
    let sh = wb.sheet(0).unwrap();
    let a1 = sh.cell(CellRef::new(0, 0)).unwrap();
    assert_eq!(a1.py.as_ref().map(|p| (p.code.as_str(), p.object)), Some(("1+1", true)));
    assert!(a1.formula.is_none());
    assert!(sh.cell(CellRef::new(1, 0)).unwrap().py.is_none());
    assert_eq!(sh.cell(CellRef::new(2, 0)).unwrap().py.as_ref().map(|p| p.code.as_str()), Some("2"));
}

#[test]
fn a_malformed_result_never_spills() {
    let mut s = s();
    py(&mut s, "A1", "x");
    let bad = Array { rows: 4_000_000_000, cols: 4_000_000_000, data: vec![] };
    cache(&mut s, "A1", Value::Array(Arc::new(bad)), "list");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert!(s.doc().unwrap().wb.active().unwrap().spill.is_empty());
}

#[test]
fn py_formulas_over_the_size_limit_keep_only_their_value() {
    let lits = |n: usize| {
        let mut v = vec![format!("\"{}\"", "x".repeat(255)); n / 255];
        if !n.is_multiple_of(255) {
            v.push(format!("\"{}\"", "x".repeat(n % 255)));
        }
        v.join("&")
    };
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    let cell = |text: String| Cell { value: Value::text("cached"), ..Cell::formula(gridcraft_model::Formula::new(&text)) };
    sh.set_cell(CellRef::new(0, 0), cell(format!("_xlfn._xlws.PY({},0)", lits(8_080))));
    sh.set_cell(CellRef::new(1, 0), cell(format!("_xlfn._xlws.PY({},0)", lits(8_079))));
    let xlsx = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (wb, warnings) = crate::io::open_bytes("long.xlsx", &xlsx).unwrap();
    let sh = wb.sheet(0).unwrap();
    let over = sh.cell(CellRef::new(0, 0)).unwrap();
    assert!(over.py.is_none() && over.formula.is_none(), "8,080 characters of code is over the limit");
    assert_eq!(over.value, Value::text("cached"));
    let within = sh.cell(CellRef::new(1, 0)).unwrap();
    assert_eq!(within.py.as_ref().map(|p| p.code.chars().count()), Some(8_079));
    assert!(warnings.iter().any(|w| w.contains("A1")), "{warnings:?}");
}

#[test]
fn a_dropped_python_result_takes_its_spill_with_it() {
    // A1's code is over the limit and B1's result is malformed; each had spilled down to row 3,
    // and C1's well-formed result keeps its spill.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    let out = |value: Value| Some(PyOutput { value, type_name: "list".into(), preview: String::new(), convertible: true });
    let col = array(3, 1, vec![Value::Number(7.0), Value::Number(8.0), Value::Number(9.0)]);
    let bad = Value::Array(Arc::new(Array { rows: 3, cols: 1, data: vec![Value::Number(7.0)] }));
    for (at, code, value) in [("A1", "y".repeat(9_000), col.clone()), ("B1", "b".into(), bad), ("C1", "c".into(), col)] {
        let at = CellRef::parse(at).unwrap();
        let py = PyCode { output: out(value), ..PyCode::new(&code) };
        sh.set_cell(at, Cell { value: Value::Number(7.0), formula: None, style: Default::default(), py: Some(Arc::new(py)) });
        let r = RangeRef::new(at, CellRef::new(2, at.col));
        sh.spill_ranges.insert(at, r);
        for c in r.iter().skip(1) {
            sh.spill.insert(c, Value::Number(8.0));
        }
    }
    let warnings = crate::io::enforce_python_limits(&mut wb);
    assert!(warnings.iter().any(|w| w.contains("A1")), "{warnings:?}");
    let sh = wb.sheet(0).unwrap();
    for (anchor, below) in [("A1", "A2"), ("B1", "B3")] {
        assert!(!sh.spill_ranges.contains_key(&CellRef::parse(anchor).unwrap()), "{anchor}'s spill range stays");
        assert_eq!(sh.value(CellRef::parse(below).unwrap()), Value::Empty, "{below} still shows {anchor}'s old spill");
        assert_eq!(sh.value(CellRef::parse(anchor).unwrap()), Value::Number(7.0), "{anchor} keeps its value");
    }
    assert_eq!(sh.value(CellRef::parse("C2").unwrap()), Value::Number(8.0), "a sound result keeps its spill");
    // A save writes no value for the dropped spills.
    let xml = sheet_xml(&gridcraft_xlsx::write_xlsx(&wb).unwrap());
    assert!(!xml.contains("r=\"A2\"") && !xml.contains("r=\"B3\""), "{xml}");
    assert!(xml.contains("r=\"C2\""), "{xml}");
}

#[test]
fn a_formula_entered_in_a_python_spill_takes_the_area_as_a_formula_would() {
    // A1's cached array spills over A1:A3; =SEQUENCE(2) entered in A2 is not blocked by that old
    // spill (the Python cell is laid out again in the same pass), so it spills and A1 is blocked.
    let mut s = s();
    py(&mut s, "A1", "[1, 2, 3]");
    cache(&mut s, "A1", array(3, 1, vec![10.0.into(), 20.0.into(), 30.0.into()]), "list");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "A3"), Value::Number(30.0));
    s.execute("cell.set", json!({"cell": "A2", "input": "=SEQUENCE(2)"})).unwrap();
    assert_eq!((v(&s, "A2"), v(&s, "A3")), (Value::Number(1.0), Value::Number(2.0)));
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    let wb = &s.doc().unwrap().wb;
    let mut fresh = (**wb).clone();
    gridcraft_calc::Calc::new().recalc_all(&mut fresh);
    for c in ["A1", "A2", "A3"] {
        let c = CellRef::parse(c).unwrap();
        assert_eq!(wb.active().unwrap().value(c), fresh.active().unwrap().value(c), "{c}");
    }
}
