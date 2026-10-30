#![allow(clippy::unwrap_used)]
use std::io::Write as _;
use std::sync::Arc;

use gridcraft_engine::model::Formula;

use super::package::{CORE_PROPERTIES, core_properties, diff_core_properties};
use super::*;

#[test]
fn discovery_reports_a_directory_read_error() {
    let path = std::env::temp_dir().join(format!("gridcraft-check-not-a-directory-{}", std::process::id()));
    std::fs::write(&path, "not a directory").unwrap();
    let mut discovery = Discovery::default();
    let mut seen = std::collections::HashSet::new();
    assert!(walk(&path, 0, &mut discovery, &mut seen));
    assert!(discovery.files.is_empty());
    assert_eq!(discovery.failures.len(), 1);
    assert_eq!(discovery.failures[0].path, path.to_string_lossy());
    assert!(discovery.failures[0].error.as_ref().unwrap().contains("cannot read directory"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn discovery_reports_omissions_only_after_the_file_limit() {
    let mut discovery = Discovery { files: vec![PathBuf::from("book.xlsx"); MAX_FILES - 1], ..Default::default() };
    assert!(discovery.add_file(PathBuf::from("last.xlsx")));
    assert!(discovery.failures.is_empty());
    assert!(!discovery.add_file(PathBuf::from("omitted.xlsx")));
    assert_eq!(discovery.files.len(), MAX_FILES);
    assert_eq!(discovery.failures[0].path, "omitted.xlsx");
    assert!(discovery.failures[0].error.as_ref().unwrap().contains("file limit"));
}

fn at(a1: &str) -> CellRef {
    CellRef::parse(a1).unwrap()
}

/// A workbook whose cells are `(A1, input, cached value)`: an input starting with `=` is a
/// formula with that cached value, anything else a constant.
fn book(cells: &[(&str, &str, Value)]) -> Workbook {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for (a1, input, cached) in cells {
        let cell = if input.starts_with('=') {
            Cell { value: cached.clone(), formula: Some(Arc::new(Formula::new(input))), ..Default::default() }
        } else {
            Cell::value(cached.clone())
        };
        sh.set_cell(at(a1), cell);
    }
    wb
}

fn check(wb: &Workbook, opts: &Options) -> FileReport {
    let bytes = gridcraft_engine::io::save_bytes(wb, "t.xlsx").unwrap();
    check_bytes("t.xlsx", &bytes, opts)
}

fn n(x: f64) -> Value {
    Value::number(x)
}

#[test]
fn matching_cached_values_pass() {
    let wb = book(&[("A1", "2", n(2.0)), ("B1", "=A1*3", n(6.0)), ("C1", "=A1&\"x\"", Value::text("2x")), ("D1", "=A1>1", Value::Bool(true))]);
    let r = check(&wb, &Options::default());
    assert_eq!(r.error, None);
    assert_eq!((r.calc.formula_cells, r.calc.checked, r.calc.matched, r.calc.mismatched), (3, 3, 3, 0));
    assert!(r.is_clean(), "{r:?}");
}

#[test]
fn wrong_cached_value_is_a_mismatch_and_ranks_its_function() {
    // Excel "said" SUM gives 7; GridCraft computes 6. C1 reads B1 and is consistent with
    // Excel's 7, so it mismatches too, but only as a downstream effect.
    let wb = book(&[("A1", "2", n(2.0)), ("A2", "4", n(4.0)), ("B1", "=SUM(A1:A2)", n(7.0)), ("C1", "=B1+1", n(8.0))]);
    let r = check(&wb, &Options::default());
    assert_eq!((r.calc.checked, r.calc.mismatched), (2, 2));
    let first = &r.calc.mismatches[0];
    assert_eq!(first.cell, "Sheet1!B1");
    assert_eq!(first.formula, "=SUM(A1:A2)");
    assert_eq!((first.expected.clone(), first.actual.clone(), first.kind, first.primary), (n(7.0), n(6.0), "numeric", true));
    assert!(!r.calc.mismatches[1].primary);
    assert_eq!(r.calc.functions["SUM"], FnStat { checked: 1, mismatched: 1, primary: 1 });
    assert_eq!(r.calc.functions["(no function)"], FnStat { checked: 1, mismatched: 1, primary: 0 });
    assert!(!r.is_clean());
}

#[test]
fn errors_compare_by_kind() {
    let wb = book(&[("A1", "=1/0", Value::Error(CellError::Div0)), ("A2", "=1/0", Value::Error(CellError::NA)), ("A3", "=1/0", n(0.0))]);
    let r = check(&wb, &Options::default());
    assert_eq!((r.calc.matched, r.calc.mismatched), (1, 2));
    assert!(r.calc.mismatches.iter().all(|m| m.kind == "error"));
    assert_eq!(r.calc.signatures.get("#N/A → #DIV/0!"), Some(&1));
    assert_eq!(r.calc.signatures.get("number → #DIV/0!"), Some(&1));
    assert!(values_match(&Value::Error(CellError::NA), &Value::Error(CellError::NA), 0.0));
    assert!(!values_match(&Value::Error(CellError::NA), &Value::Error(CellError::Value), 0.0));
}

#[test]
fn numbers_compare_within_tolerance() {
    assert!(values_match(&n(3.0000000001), &n(3.0), 1e-9));
    assert!(!values_match(&n(3.0000000001), &n(3.0), 1e-12));
    // Absolute near zero.
    assert!(values_match(&n(1e-12), &n(0.0), 1e-9));
    assert!(values_match(&n(1e12 + 1.0), &n(1e12), 1e-9));
    assert!(!values_match(&n(0.0), &Value::Empty, 1.0));
    assert!(!values_match(&Value::text("1"), &n(1.0), 1.0));
    assert!(values_match(&Value::text("a"), &Value::text("a"), 0.0));
    assert!(!values_match(&Value::text("a"), &Value::text("A"), 0.0));
    assert!(!values_match(&Value::Bool(true), &n(1.0), 1.0));
    let wb = book(&[("A1", "=1+2", n(3.0000000001))]);
    assert_eq!(check(&wb, &Options::default()).calc.matched, 1);
    assert_eq!(check(&wb, &Options { tolerance: 1e-12, ..Default::default() }).calc.mismatched, 1);
}

/// Business Central writes TRUE and FALSE as `<c><f>TRUE()</f><v>1</v></c>`, without `t="b"`:
/// the cache holds the right value under a number type, so it matches and is counted apart.
#[test]
fn booleans_cached_as_numbers_match_and_are_counted_apart() {
    let wb = book(&[
        ("A1", "=TRUE()", n(1.0)),
        ("B1", "=FALSE()", n(0.0)),
        ("C1", "=TRUE()", Value::Bool(true)),
        ("D1", "=1=1", n(0.0)),
        ("E1", "=TRUE()+0", n(1.0)),
    ]);
    let r = check(&wb, &Options::default());
    assert_eq!((r.calc.checked, r.calc.matched, r.calc.mismatched), (5, 4, 1));
    // A cached 0 for TRUE is still a mismatch, and a number result is compared as a number.
    assert_eq!(r.calc.mismatches[0].cell, "Sheet1!D1");
    assert_eq!(r.calc.signatures.get("number → bool"), Some(&1));
    assert_eq!(r.calc.bool_as_number, 2);
    let v = report_json(std::slice::from_ref(&r), &Options::default());
    assert_eq!((v["files"][0]["calc"]["boolAsNumber"].clone(), v["totals"]["boolAsNumber"].clone()), (json!(2), json!(2)));
    assert!(bool_cached_as_number(&n(1.0), &Value::Bool(true)));
    assert!(!bool_cached_as_number(&n(1.0), &Value::Bool(false)));
    assert!(!bool_cached_as_number(&n(2.0), &Value::Bool(true)));
    assert!(!bool_cached_as_number(&Value::Bool(true), &n(1.0)));
}

#[test]
fn missing_cached_values_and_volatile_formulas_are_skipped() {
    let wb =
        book(&[("A1", "=1+1", Value::Empty), ("B1", "=NOW()", n(1.0)), ("B2", "=B1+1", n(2.0)), ("B3", "=CELL(\"filename\")", Value::text("x"))]);
    let r = check(&wb, &Options::default());
    assert_eq!((r.calc.formula_cells, r.calc.checked, r.calc.no_oracle, r.calc.volatile), (4, 0, 1, 3));
}

#[test]
fn full_calc_on_load_means_no_oracle() {
    // A formula without a cached value makes GridCraft's writer flag the file
    // `fullCalcOnLoad`, like libraries that store 0 for every formula: B1's wrong "cached"
    // value must not count as a mismatch then.
    let wb = book(&[("A1", "=1+1", Value::Empty), ("B1", "=2+2", n(0.0))]);
    let bytes = gridcraft_engine::io::save_bytes(&wb, "t.xlsx").unwrap();
    assert!(full_calc_on_load(&bytes));
    let r = check_bytes("t.xlsx", &bytes, &Options::default());
    assert!(r.calc.stale_cache);
    assert_eq!((r.calc.checked, r.calc.no_oracle, r.calc.mismatched), (0, 2, 0));
    let fresh = gridcraft_engine::io::save_bytes(&book(&[("B1", "=2+2", n(0.0))]), "t.xlsx").unwrap();
    assert!(!full_calc_on_load(&fresh));
    assert!(!full_calc_on_load(b"not a zip"));
}

#[test]
fn volatile_names_propagate() {
    let mut wb = book(&[("A1", "=Stamp+1", n(1.0))]);
    wb.names.push(gridcraft_engine::model::DefinedName {
        name: "Stamp".into(),
        scope: None,
        formula: "TODAY()".into(),
        comment: String::new(),
        hidden: false,
    });
    assert!(volatile_names(&wb).contains("STAMP"));
    assert_eq!(check(&wb, &Options::default()).calc.volatile, 1);
}

#[test]
fn round_trip_of_a_simple_book_is_clean() {
    let mut wb = book(&[("A1", "2", n(2.0)), ("B1", "=A1*3", n(6.0)), ("A2", "x", Value::text("x"))]);
    wb.sheet_mut(0).unwrap().merges.push(gridcraft_engine::core::RangeRef::parse("C1:D2").unwrap());
    let source = gridcraft_engine::io::save_bytes(&wb, "t.xlsx").unwrap();
    let rt = round_trip(&wb, &source, &Options::default());
    assert_eq!(rt.error, None);
    assert!(rt.is_clean(), "{rt:?}");
    assert!(rt.categories.iter().any(|c| c.name == "package"));
}

#[test]
fn diff_detects_lost_features() {
    let mut a = book(&[("A1", "2", n(2.0)), ("B1", "=A1*3", n(6.0))]);
    a.sheet_mut(0).unwrap().merges.push(gridcraft_engine::core::RangeRef::parse("C1:D2").unwrap());
    let mut b = a.clone();
    let sh = b.sheet_mut(0).unwrap();
    sh.merges.clear();
    sh.set_cell(at("A1"), Cell::value(n(3.0)));
    sh.set_cell(at("B1"), Cell::value(n(6.0)));
    b.sheets.push(Arc::new(Sheet::new("Extra")));
    let cats = diff_workbooks(&a, &b, &Options { max_report: 1, ..Default::default() });
    let get = |name: &str| cats.iter().find(|c| c.name == name).unwrap();
    assert_eq!(get("values").diffs, 1);
    assert_eq!(get("values").examples, ["Sheet1!A1: 2 → 3"]);
    assert_eq!(get("formulas").examples, ["Sheet1!B1: =A1*3 → "]);
    assert_eq!(get("merges").examples, ["Sheet1: lost C1:D2"]);
    assert_eq!(get("sheets").diffs, 1);
    assert_eq!(get("tables").diffs, 0);
}

#[test]
fn diff_detects_changed_cond_format_rule() {
    use gridcraft_engine::model::{CfRule, CfValueKind, Color, CondFormat};
    let bar = |gradient: bool, priority: u32| CondFormat {
        ranges: vec![gridcraft_engine::core::RangeRef::parse("A1").unwrap()],
        rule: CfRule::DataBar { min: CfValueKind::Min, max: CfValueKind::Max, color: Color::Rgb(0x638EC6), gradient, show_value: true },
        priority,
        stop_if_true: false,
    };
    let with = |cf: CondFormat| {
        let mut wb = book(&[("A1", "2", n(2.0))]);
        wb.sheet_mut(0).unwrap().cond_formats.push(cf);
        wb
    };
    let diffs = |a: &Workbook, b: &Workbook| diff_workbooks(a, b, &Options::default()).into_iter().find(|c| c.name == "condFormats").unwrap().diffs;
    assert_eq!(diffs(&with(bar(false, 1)), &with(bar(true, 1))), 1);
    assert_eq!(diffs(&with(bar(true, 1)), &with(bar(true, 1))), 0);
    assert_eq!(diffs(&with(bar(true, 1)), &with(bar(true, 5))), 0);
}

fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in parts {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}

#[test]
fn package_validation_finds_what_excel_would_repair() {
    let bytes = zip_of(&[
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="r"/><Override PartName="/xl/workbook.xml" ContentType="w"/><Override PartName="/xl/gone.xml" ContentType="g"/></Types>"#,
        ),
        ("_rels/.rels", r#"<Relationships><Relationship Id="rId1" Type="x/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
        ("xl/workbook.xml", "<workbook/>"),
        ("xl/styles.xml", "<styleSheet/>"),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="x/styles" Target="styles.xml"/><Relationship Id="rId2" Type="x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="x/hyperlink" Target="https://example.com" TargetMode="External"/><Relationship Id="rId3" Type="x/theme" Target="/xl/../xl/styles.xml"/></Relationships>"#,
        ),
    ]);
    let issues = validate_package(&bytes).unwrap();
    assert_eq!(
        issues,
        [
            "part xl/styles.xml has no content type",
            "content type override for missing part /xl/gone.xml",
            "xl/_rels/workbook.xml.rels: rId2 (worksheet) targets missing part xl/worksheets/sheet1.xml",
            "xl/_rels/workbook.xml.rels: duplicate relationship id rId2",
        ]
    );
    assert!(validate_package(b"not a zip").is_err());
    let own = gridcraft_engine::io::save_bytes(&book(&[("A1", "=1", n(1.0))]), "t.xlsx").unwrap();
    assert_eq!(validate_package(&own).unwrap(), Vec::<String>::new());
}

const WB_RELS: &str = "xl/_rels/workbook.xml.rels";
const SHEET_RELS: &str = "xl/worksheets/_rels/sheet1.xml.rels";
const MADE_UP: &str = r#"<Relationship Id="rId90" Type="http://example.com/relationships/madeUp" Target="madeUp.xml"/>"#;
/// Not empty: an empty part isn't a loss.
const MADE_UP_PART: (&str, &str) = ("xl/madeUp.xml", "<madeUp><item/></madeUp>");

/// `bytes` with relationships added to rels parts (created when missing) and `parts` added.
fn with_rels(bytes: &[u8], rels: &[(&str, &str)], parts: &[(&str, &str)]) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut pending: Vec<(&str, &str)> = rels.to_vec();
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut body = String::new();
        f.read_to_string(&mut body).unwrap();
        if let Some(at) = pending.iter().position(|(r, _)| *r == name) {
            body = body.replace("</Relationships>", &format!("{}</Relationships>", pending.remove(at).1));
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(body.as_bytes()).unwrap();
    }
    let created = pending.iter().map(|(r, extra)| {
        (*r, format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{extra}</Relationships>"#))
    });
    for (name, body) in created.collect::<Vec<_>>().iter().map(|(n, b)| (*n, b.as_str())).chain(parts.iter().copied()) {
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(body.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// The `package` examples of checking `source`.
fn package_of(source: &[u8]) -> Vec<String> {
    let r = check_bytes("t.xlsx", source, &Options::default());
    r.round_trip.categories.iter().find(|c| c.name == "package").unwrap().examples.clone()
}

fn plain() -> Vec<u8> {
    gridcraft_engine::io::save_bytes(&book(&[("A1", "1", n(1.0))]), "t.xlsx").unwrap()
}

#[test]
fn package_reports_relationship_kinds_the_output_lost() {
    // A kind GridCraft doesn't know is dropped on save; a calculation chain is never written
    // (Excel rebuilds it), so its absence isn't a loss.
    let calc_chain =
        r#"<Relationship Id="rId91" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>"#;
    let source = with_rels(&plain(), &[(WB_RELS, &format!("{MADE_UP}{calc_chain}"))], &[MADE_UP_PART, ("xl/calcChain.xml", "<calcChain/>")]);
    assert_eq!(package_of(&source), ["lost relationship madeUp"]);
    assert!(check_bytes("t.xlsx", &plain(), &Options::default()).round_trip.is_clean());
}

// In the three tests below the made-up kind stays lost, so the comparison is known to run.

#[test]
fn an_internal_link_written_as_a_relationship_is_not_a_loss() {
    // openpyxl writes `#Sheet1!B2` as an external relationship; GridCraft writes `location`.
    let link = r##"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#Sheet1!B2" TargetMode="External"/>"##;
    let source = with_rels(&plain(), &[(WB_RELS, MADE_UP), (SHEET_RELS, link)], &[MADE_UP_PART]);
    assert_eq!(package_of(&source), ["lost relationship madeUp"]);
}

#[test]
fn comments_without_a_comment_are_not_a_loss() {
    let rels = r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/>"#;
    let comments = r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>A</author></authors><commentList/></comments>"#;
    let vml = r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><o:shapelayout><o:idmap data="1"/></o:shapelayout><v:shapetype id="_x0000_t202" path="m,l,21600r21600,l21600,xe"><v:path gradientshapeok="t"/></v:shapetype></xml>"#;
    let source = with_rels(
        &plain(),
        &[(WB_RELS, MADE_UP), (SHEET_RELS, rels)],
        &[MADE_UP_PART, ("xl/comments1.xml", comments), ("xl/drawings/vmlDrawing1.vml", vml)],
    );
    assert_eq!(package_of(&source), ["lost relationship madeUp"]);
}

#[test]
fn empty_drawings_and_person_lists_are_not_a_loss() {
    let drawing = r#"<Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>"#;
    let person = r#"<Relationship Id="rId92" Type="http://schemas.microsoft.com/office/2017/10/relationships/person" Target="persons/person.xml"/>"#;
    let source = with_rels(
        &plain(),
        &[(WB_RELS, &format!("{MADE_UP}{person}")), (SHEET_RELS, drawing)],
        &[
            MADE_UP_PART,
            ("xl/drawings/drawing1.xml", r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"/>"#),
            ("xl/persons/person.xml", r#"<personList xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments"/>"#),
        ],
    );
    assert_eq!(package_of(&source), ["lost relationship madeUp"]);
}

#[test]
fn json_lists_warning_messages() {
    let with = FileReport { path: "a.ods".into(), warnings: vec!["ODS data-only import".into()], ..Default::default() };
    let without = FileReport { path: "b.xlsx".into(), ..Default::default() };
    let v = report_json(&[with, without], &Options::default());
    assert_eq!(v["files"][0]["warnings"], json!(["ODS data-only import"]));
    assert_eq!(v["files"][1]["warnings"], json!([]));
}

#[test]
fn unopenable_files_are_reported() {
    let r = check_bytes("broken.xlsx", b"PK\x03\x04 not really", &Options::default());
    assert!(r.error.is_some());
    assert!(!r.is_clean());
}

#[test]
fn json_schema_keys() {
    let wb = book(&[("A1", "=1+1", n(3.0))]);
    let reports = vec![check(&wb, &Options::default()), check_bytes("bad.xlsx", b"", &Options::default())];
    let v = report_json(&reports, &Options::default());
    let keys = |v: &Json| v.as_object().unwrap().keys().cloned().collect::<BTreeSet<String>>();
    let set = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
    assert_eq!(keys(&v), set(&["schema", "tolerance", "files", "totals", "functions", "signatures"]));
    assert_eq!(v["schema"], SCHEMA);
    let f = &v["files"][0];
    assert_eq!(keys(f), set(&["path", "opened", "error", "warnings", "calc", "roundTrip"]));
    assert_eq!(
        keys(&f["calc"]),
        set(&[
            "formulaCells",
            "checked",
            "matched",
            "mismatched",
            "noOracle",
            "staleCache",
            "volatile",
            "boolAsNumber",
            "mismatches",
            "functions",
            "signatures"
        ])
    );
    assert_eq!(f["calc"]["signatures"], json!({"number → number": 1}));
    assert_eq!(keys(&f["calc"]["mismatches"][0]), set(&["cell", "formula", "expected", "actual", "kind", "functions", "primary"]));
    assert_eq!(f["calc"]["mismatches"][0]["expected"], json!(3.0));
    assert_eq!(keys(&f["roundTrip"]), set(&["error", "differing", "categories"]));
    assert_eq!(
        keys(&f["roundTrip"]["categories"]),
        set(&[
            "sheets",
            "values",
            "formulas",
            "numFmts",
            "merges",
            "names",
            "tables",
            "condFormats",
            "validations",
            "charts",
            "comments",
            "hyperlinks",
            "pivots",
            "package",
            "properties"
        ])
    );
    assert_eq!(keys(&f["roundTrip"]["categories"]["values"]), set(&["equal", "diffs", "examples"]));
    assert_eq!(v["files"][1]["opened"], json!(false));
    assert_eq!(
        keys(&v["totals"]),
        set(&[
            "files",
            "opened",
            "failedToOpen",
            "formulaCells",
            "checked",
            "matched",
            "mismatched",
            "noOracle",
            "volatile",
            "boolAsNumber",
            "roundTripFailed",
            "roundTripDiffering",
            "categoriesDiffering"
        ])
    );
    assert_eq!(keys(&v["functions"][0]), set(&["name", "checked", "mismatched", "primary"]));
}

/// A one-sheet package whose `docProps/core.xml` holds `props` (inside `<cp:coreProperties>`).
fn book_with_core(props: &str) -> Vec<u8> {
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">{props}</cp:coreProperties>"#
    );
    zip_of(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#,
        ),
        ("docProps/core.xml", &core),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
        ),
    ])
}

const ALL_CORE_PROPERTIES: &str = r#"<dc:title>Title &amp; more</dc:title><dc:subject>Subject</dc:subject><dc:creator>Author A</dc:creator><cp:keywords>k1; k2</cp:keywords><dc:description>Description</dc:description><cp:lastModifiedBy>Author B</cp:lastModifiedBy><cp:lastPrinted>2025-06-01T08:00:00Z</cp:lastPrinted><dcterms:created xsi:type="dcterms:W3CDTF">2024-01-15T09:30:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2025-06-30T17:45:00Z</dcterms:modified><cp:category>Configuration</cp:category><cp:contentStatus>Draft</cp:contentStatus><cp:revision>7</cp:revision><cp:version>1.0</cp:version><dc:identifier>id-1</dc:identifier><dc:language>en-US</dc:language>"#;

#[test]
fn round_trip_keeps_every_core_property() {
    let r = check_bytes("props.xlsx", &book_with_core(ALL_CORE_PROPERTIES), &Options::default());
    assert_eq!(r.error, None);
    let props = r.round_trip.categories.iter().find(|c| c.name == "properties").unwrap();
    assert_eq!((props.diffs, &props.examples), (0, &vec![]), "{:?}", r.round_trip);
    assert_eq!(core_properties(&book_with_core(ALL_CORE_PROPERTIES)).unwrap().len(), CORE_PROPERTIES.len());
}

#[test]
fn a_core_property_lost_or_changed_is_reported() {
    let source = book_with_core(ALL_CORE_PROPERTIES);
    let written = book_with_core(
        &ALL_CORE_PROPERTIES
            .replace(r#"<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-15T09:30:00Z</dcterms:created>"#, "")
            .replace("<cp:lastModifiedBy>Author B</cp:lastModifiedBy>", "<cp:lastModifiedBy>Someone</cp:lastModifiedBy>")
            .replace("<cp:category>Configuration</cp:category>", "<cp:category></cp:category>"),
    );
    let c = diff_core_properties(&source, &written, 20);
    assert_eq!(
        c.examples,
        ["property cp:lastModifiedBy changed: \"Author B\" → \"Someone\"", "property dcterms:created lost", "property cp:category lost",]
    );
    // A property the source doesn't set, or a source with no core part, isn't a difference.
    assert_eq!(diff_core_properties(&book_with_core(""), &written, 20).diffs, 0);
    assert_eq!(diff_core_properties(&zip_of(&[("a.xml", "<a/>")]), &written, 20).diffs, 0);
    // The output losing its whole core part is reported property by property.
    assert_eq!(diff_core_properties(&source, &zip_of(&[("a.xml", "<a/>")]), 20).diffs, CORE_PROPERTIES.len());
}
