//! References to another workbook typed by file name or path (#804): they go through a link,
//! `[n]` in the file, and show the linked file in the formula bar and the Name Manager.

use std::io::Read as _;
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_engine::{DocState, Session};
use gridcraft_model::{Cell, DefinedName, ExternalBook, ExternalName, ExternalSheet, Formula, Workbook};
use serde_json::json;

const SOURCE: &str = r"C:\links\Source.xlsx";

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// A workbook saved with a link to `C:\links\Source.xlsx` (`Sheet1!A1:A3` = 10, 20, 30, the name
/// `Rate` = 0.05), `A1` = `[1]Sheet1!A1` and the name `Linked` over `[1]Sheet1!$A$1:$A$3`.
fn linked() -> Session {
    let mut wb = Workbook::new();
    let mut cells: Vec<_> = (0..3).map(|r| (CellRef::new(r, 0), Value::Number(10.0 * (r + 1) as f64))).collect();
    cells.push((CellRef::new(0, 1), Value::Number(0.05)));
    cells.sort_by_key(|(c, _)| *c);
    wb.external_books.push(ExternalBook {
        path: SOURCE.into(),
        sheets: vec![ExternalSheet { name: "Sheet1".into(), cells, refresh_error: false }],
        names: vec![ExternalName { name: "Rate".into(), sheet: None, refers_to: "Sheet1!$B$1".into() }],
        ..Default::default()
    });
    Arc::make_mut(&mut wb.sheets[0]).set_cell(CellRef::new(0, 0), Cell::formula(Formula::new("[1]Sheet1!A1")));
    wb.names.push(DefinedName { name: "Linked".into(), scope: None, formula: "[1]Sheet1!$A$1:$A$3".into(), comment: String::new(), hidden: false });
    let mut s = Session::new();
    s.add_document(DocState::new(wb, None, "Dest".into()));
    s
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap_or_else(|e| panic!("{input}: {e}"));
}

fn at(cell: &str) -> CellRef {
    CellRef::parse(cell).unwrap()
}

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(at(cell))
}

/// What the file holds.
fn stored(s: &Session, cell: &str) -> String {
    s.doc().unwrap().wb.active().unwrap().cell(at(cell)).unwrap().formula.as_ref().unwrap().text.clone()
}

/// What the formula bar shows.
fn shown(s: &Session, cell: &str) -> String {
    let d = s.doc().unwrap();
    gridcraft_engine::display::edit_text(&d.wb, d.wb.active().unwrap(), at(cell))
}

/// The saved package's parts by name.
fn saved(s: &Session) -> std::collections::HashMap<String, String> {
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut t = String::new();
            f.read_to_string(&mut t).unwrap_or_default();
            (f.name().to_string(), t)
        })
        .collect()
}

#[test]
fn typed_file_names_and_paths_become_a_new_link() {
    // (typed in A1, kept as, shown as, link path); Source.xlsx isn't linked yet, so every
    // reference into it is #REF! until the file is read, as in Excel when the file is missing.
    for (typed, file, bar, path) in [
        ("=[Source.xlsx]Sheet1!A1", "[1]Sheet1!A1", "=[Source.xlsx]Sheet1!A1", "Source.xlsx"),
        ("='[Source.xlsx]Sheet1'!A1", "[1]Sheet1!A1", "=[Source.xlsx]Sheet1!A1", "Source.xlsx"),
        (r"='C:\links\[Source.xlsx]Sheet1'!A1", "[1]Sheet1!A1", r"='C:\links\[Source.xlsx]Sheet1'!A1", SOURCE),
        (r"=SUM('C:\links\[Source.xlsx]Sheet1'!A1:A3)", "SUM([1]Sheet1!A1:A3)", r"=SUM('C:\links\[Source.xlsx]Sheet1'!A1:A3)", SOURCE),
        (r"='C:\links\[Source.xlsx]P&L Data'!A1", "'[1]P&L Data'!A1", r"='C:\links\[Source.xlsx]P&L Data'!A1", SOURCE),
        ("=Source.xlsx!Rate", "[1]!Rate", "=Source.xlsx!Rate", "Source.xlsx"),
        (r"='C:\links\Source.xlsx'!Rate", "[1]!Rate", r"='C:\links\Source.xlsx'!Rate", SOURCE),
    ] {
        let mut s = blank();
        set(&mut s, "A1", typed);
        assert_eq!(stored(&s, "A1"), file, "{typed}");
        assert_eq!(value(&s, "A1"), Value::Error(CellError::Ref), "{typed}");
        assert_eq!(shown(&s, "A1"), bar, "{typed}");
        let books = &s.doc().unwrap().wb.external_books;
        assert_eq!(books.len(), 1, "{typed}");
        assert_eq!(books[0].path, path, "{typed}");

        // The file holds `[1]` and a link part with the path, never the path in <f>.
        let parts = saved(&s);
        let sheet = &parts["xl/worksheets/sheet1.xml"];
        let f = file.replace('&', "&amp;");
        assert!(sheet.contains(&format!("<f>{f}</f>")), "{typed}: {sheet}");
        let wbx = &parts["xl/workbook.xml"];
        assert!(wbx.contains("<externalReferences><externalReference r:id="), "{typed}: {wbx}");
        let link = &parts["xl/externalLinks/externalLink1.xml"];
        let rels = &parts["xl/externalLinks/_rels/externalLink1.xml.rels"];
        let target = if path == SOURCE { r"file:///C:\links\Source.xlsx" } else { "Source.xlsx" };
        assert!(rels.contains(&format!("/externalLinkPath\" Target=\"{target}\" TargetMode=\"External\"")), "{typed}: {rels}");
        if file.contains("!Rate") {
            assert!(link.contains("<definedNames><definedName name=\"Rate\"/></definedNames>"), "{typed}: {link}");
        } else {
            let sheet = if typed.contains("P&L") { "P&amp;L Data" } else { "Sheet1" };
            assert!(link.contains(&format!("<sheetNames><sheetName val=\"{sheet}\"/></sheetNames>")), "{typed}: {link}");
            assert!(link.contains("<sheetData sheetId=\"0\" refreshError=\"1\"/>"), "{typed}: {link}");
        }
        assert!(parts["[Content_Types].xml"].contains("/xl/externalLinks/externalLink1.xml"), "{typed}");

        // Reopened, the link keeps its path and the formula bar still names the file.
        let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
        let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        assert_eq!(wb.external_books.first().map(|b| b.path.as_str()), Some(path), "{typed}");
        let mut again = Session::new();
        again.add_document(DocState::new(wb, None, "Again".into()));
        assert_eq!(shown(&again, "A1"), bar, "{typed} reopened");
    }
}

#[test]
fn one_file_is_one_link() {
    // By file name or by path: the same file is linked once; another folder is another file.
    let mut s = blank();
    set(&mut s, "A1", "=[Source.xlsx]Sheet1!A1");
    set(&mut s, "A2", "='[source.XLSX]Sheet2'!B2");
    set(&mut s, "A3", r"='C:\other\[Source.xlsx]Sheet1'!A1");
    set(&mut s, "A4", r"='C:/other/[Source.xlsx]Sheet1'!A2");
    assert_eq!(stored(&s, "A2"), "[1]Sheet2!B2");
    assert_eq!(stored(&s, "A3"), "[2]Sheet1!A1");
    assert_eq!(stored(&s, "A4"), "[2]Sheet1!A2");
    let books = &s.doc().unwrap().wb.external_books;
    assert_eq!(books.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(), ["Source.xlsx", r"C:\other\Source.xlsx"]);
    assert_eq!(books[0].sheets.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Sheet1", "Sheet2"]);
    let parts = saved(&s);
    assert_eq!(parts["xl/workbook.xml"].matches("<externalReference ").count(), 2);
    assert!(parts["xl/externalLinks/_rels/externalLink2.xml.rels"].contains(r#"Target="file:///C:\other\Source.xlsx""#));
}

#[test]
fn a_workbook_without_a_sheet_is_refused() {
    let mut s = blank();
    for typed in ["=[Sheet1]!A1", "='[Source.xlsx]'!A1", "='Q1 [draft]'!A1"] {
        assert!(s.execute("cell.set", json!({"cell": "A1", "input": typed})).is_err(), "{typed}");
    }
    assert!(s.doc().unwrap().wb.external_books.is_empty());
}

#[test]
fn a_folder_with_a_space_is_written_escaped_and_read_back() {
    // Excel writes `My Files` as `My%20Files` in the link's target.
    let mut s = blank();
    set(&mut s, "A1", r"='C:\Users\me\My Files\[Source.xlsx]Sheet1'!A1");
    let parts = saved(&s);
    let rels = &parts["xl/externalLinks/_rels/externalLink1.xml.rels"];
    assert!(rels.contains(r#"Target="file:///C:\Users\me\My%20Files\Source.xlsx""#), "{rels}");
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    assert_eq!(wb.external_books[0].path, r"C:\Users\me\My Files\Source.xlsx");
    let mut s = Session::new();
    s.add_document(DocState::new(wb, None, "Again".into()));
    assert_eq!(shown(&s, "A1"), r"='C:\Users\me\My Files\[Source.xlsx]Sheet1'!A1");
    // Typed as Excel shows it, the reference goes through the same link.
    set(&mut s, "A2", r"='C:\Users\me\My Files\[Source.xlsx]Sheet1'!A2");
    assert_eq!(stored(&s, "A2"), "[1]Sheet1!A2");
    assert_eq!(s.doc().unwrap().wb.external_books.len(), 1);
}

#[test]
fn a_local_sheet_is_not_another_workbook() {
    let mut s = blank();
    s.execute("sheet.rename", json!({"sheet": "Sheet1", "name": "Data.v2"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=0.5", "scope": "Data.v2"})).unwrap();
    set(&mut s, "A1", "=Data.v2!Rate");
    set(&mut s, "A2", "=Plain!Rate");
    assert_eq!(stored(&s, "A1"), "Data.v2!Rate");
    assert_eq!(value(&s, "A1"), Value::Number(0.5));
    assert_eq!(stored(&s, "A2"), "Plain!Rate");
    assert!(s.doc().unwrap().wb.external_books.is_empty());
}

#[test]
fn an_existing_link_shows_its_file_and_takes_typed_references() {
    let mut s = linked();
    assert_eq!(value(&s, "A1"), Value::Number(10.0));
    assert_eq!(shown(&s, "A1"), r"='C:\links\[Source.xlsx]Sheet1'!A1");

    // Typed by file name or by path, references go through the existing link and read its values.
    for (cell, typed, file, v) in [
        ("B1", "='[Source.xlsx]Sheet1'!A1", "[1]Sheet1!A1", 10.0),
        ("B2", "=[Source.xlsx]Sheet1!A2", "[1]Sheet1!A2", 20.0),
        ("B3", r"='C:\links\[Source.xlsx]Sheet1'!A3", "[1]Sheet1!A3", 30.0),
        ("B4", r"=SUM('C:\links\[Source.xlsx]Sheet1'!A1:A3)", "SUM([1]Sheet1!A1:A3)", 60.0),
        ("B5", "=Source.xlsx!Rate", "[1]!Rate", 0.05),
        ("B6", r"='C:\links\Source.xlsx'!Rate", "[1]!Rate", 0.05),
    ] {
        set(&mut s, cell, typed);
        assert_eq!(stored(&s, cell), file, "{typed}");
        assert_eq!(value(&s, cell), Value::Number(v), "{typed}");
    }
    assert_eq!(shown(&s, "B5"), r"='C:\links\Source.xlsx'!Rate");
    assert_eq!(s.doc().unwrap().wb.external_books.len(), 1);

    // What the formula bar shows goes back in unchanged.
    let bar = shown(&s, "B4");
    set(&mut s, "C1", &bar);
    assert_eq!(stored(&s, "C1"), "SUM([1]Sheet1!A1:A3)");

    // Names: the Name Manager and `document.inspect` show the file; a typed path is kept as `[1]`.
    let listed = s.execute("formulas.nameManager", json!({})).unwrap();
    assert_eq!(listed[0]["refersTo"], r"='C:\links\[Source.xlsx]Sheet1'!$A$1:$A$3");
    let info = s.execute("document.inspect", json!({})).unwrap();
    assert_eq!(info["names"][0]["refersTo"], r"='C:\links\[Source.xlsx]Sheet1'!$A$1:$A$3");
    let r = s.execute("formulas.defineName", json!({"name": "Typed", "refersTo": r"='C:\links\[Source.xlsx]Sheet1'!$A$2"})).unwrap();
    assert_eq!(r["refersTo"], r"='C:\links\[Source.xlsx]Sheet1'!$A$2");
    let wb = &s.doc().unwrap().wb;
    assert_eq!(wb.names.iter().find(|n| n.name == "Typed").map(|n| n.formula.as_str()), Some("[1]Sheet1!$A$2"));
    set(&mut s, "C2", "=Typed*2");
    assert_eq!(value(&s, "C2"), Value::Number(40.0));
}
