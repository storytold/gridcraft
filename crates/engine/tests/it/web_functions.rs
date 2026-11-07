//! Use-case workbook for the Web functions (`FILTERXML`, `WEBSERVICE`, `ENCODEURL`) and
//! `PERCENTOF`. The workbook is built in code (no binary fixture); every case row checks itself
//! with a PASS/FAIL formula, so the same sheet also works when opened in a spreadsheet app.
//!
//! Set `GRIDCRAFT_WRITE_USE_CASES=/path/to/file.xlsx` to save the workbook while running the test.

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_engine::calc::WebFetcher;
use serde_json::json;

/// Serves canned responses by exact URL; any other URL fails like an unreachable host.
#[derive(Debug)]
struct CannedWeb;

impl WebFetcher for CannedWeb {
    fn get(&self, url: &str) -> Option<String> {
        match url {
            "https://api.test/rate" => Some("<rates><rate cur='EUR'>0.92</rate><rate cur='GBP'>0.79</rate></rates>".into()),
            "https://api.test/hello" => Some("hello".into()),
            "https://api.test/search?q=a%20b%26c" => Some("found".into()),
            "https://api.test/huge" => Some("x".repeat(32_768)),
            "https://api.test/limit" => Some("y".repeat(32_767)),
            "https://api.test/empty" => Some(String::new()),
            _ => None,
        }
    }
}

/// One scalar case: a function, what it demonstrates, the formula, and the expected result.
struct Case {
    function: &'static str,
    use_case: &'static str,
    formula: &'static str,
    /// Typed into the Expected column: a number, text, or an error literal such as `#VALUE!`.
    expected: &'static str,
}

const fn case(function: &'static str, use_case: &'static str, formula: &'static str, expected: &'static str) -> Case {
    Case { function, use_case, formula, expected }
}

/// Cases whose result is one value. Column layout: A function, B use case, C formula text,
/// D expected, E result, F PASS/FAIL.
const SCALAR_CASES: &[Case] = &[
    // PERCENTOF
    case("PERCENTOF", "Share of a subset in a range", "=PERCENTOF(V2:V3,V2:V6)", "0.5"),
    case("PERCENTOF", "Two scalars", "=PERCENTOF(25,200)", "0.125"),
    case("PERCENTOF", "Whole is zero", "=PERCENTOF(1,V7)", "#DIV/0!"),
    case("PERCENTOF", "Text subset", "=PERCENTOF(\"abc\",10)", "#VALUE!"),
    case("PERCENTOF", "Error propagates", "=PERCENTOF(V8,V2:V6)", "#N/A"),
    // ENCODEURL
    case("ENCODEURL", "Space and reserved characters", "=ENCODEURL(\"a b&c/d\")", "a%20b%26c%2Fd"),
    case("ENCODEURL", "Non-ASCII is UTF-8 percent-encoded", "=ENCODEURL(\"é\")", "%C3%A9"),
    case("ENCODEURL", "Unreserved characters are kept", "=ENCODEURL(\"A-z_0.9~\")", "A-z_0.9~"),
    case("ENCODEURL", "Empty text", "=ENCODEURL(\"\")", ""),
    // FILTERXML: single values
    case("FILTERXML", "Element text", "=FILTERXML(\"<r><a>hello</a></r>\",\"//a\")", "hello"),
    case("FILTERXML", "Numeric text becomes a number", "=FILTERXML(\"<r><n>42</n></r>\",\"//n\")+1", "43"),
    case("FILTERXML", "Attribute value", "=FILTERXML(\"<r><i id='7'/></r>\",\"//i/@id\")", "7"),
    case("FILTERXML", "Predicate on an attribute", "=FILTERXML(\"<r><p k='a'>1</p><p k='b'>2</p></r>\",\"//p[@k='b']\")", "2"),
    case("FILTERXML", "Position predicate", "=FILTERXML(\"<r><p>1</p><p>2</p><p>3</p></r>\",\"//p[2]\")", "2"),
    case("FILTERXML", "last()", "=FILTERXML(\"<r><p>1</p><p>2</p><p>3</p></r>\",\"//p[last()]\")", "3"),
    case("FILTERXML", "Absolute path", "=FILTERXML(\"<r><a><b>deep</b></a></r>\",\"/r/a/b\")", "deep"),
    case("FILTERXML", "Wildcard step", "=FILTERXML(\"<r><a><b>w</b></a></r>\",\"/r/*/b\")", "w"),
    case("FILTERXML", "Parent step", "=FILTERXML(\"<r><a><b>x</b></a><c>up</c></r>\",\"//b/../../c\")", "up"),
    case("FILTERXML", "Predicate on child text", "=FILTERXML(\"<r><p><n>x</n><v>1</v></p><p><n>y</n><v>2</v></p></r>\",\"//p[n='y']/v\")", "2"),
    case("FILTERXML", "contains()", "=FILTERXML(\"<r><p>apple</p><p>banana</p></r>\",\"//p[contains(.,'nan')]\")", "banana"),
    case("FILTERXML", "starts-with() with and", "=FILTERXML(\"<r><p>ab</p><p>ac</p></r>\",\"//p[starts-with(.,'a') and .='ac']\")", "ac"),
    case("FILTERXML", "count()", "=FILTERXML(\"<r><p/><p/><p/></r>\",\"count(//p)\")", "3"),
    case("FILTERXML", "sum()", "=FILTERXML(\"<r><n>1</n><n>2.5</n></r>\",\"sum(//n)\")", "3.5"),
    case("FILTERXML", "concat()", "=FILTERXML(\"<r><a>x</a><b>y</b></r>\",\"concat(//a,'-',//b)\")", "x-y"),
    case("FILTERXML", "string-length()", "=FILTERXML(\"<r><a>four</a></r>\",\"string-length(//a)\")", "4"),
    case("FILTERXML", "normalize-space()", "=FILTERXML(\"<r><a>  a   b </a></r>\",\"normalize-space(//a)\")", "a b"),
    case("FILTERXML", "Entities are decoded", "=FILTERXML(\"<r><a>x &amp; y</a></r>\",\"//a\")", "x & y"),
    case("FILTERXML", "CDATA is text", "=FILTERXML(\"<r><a><![CDATA[<b>]]></a></r>\",\"//a\")", "<b>"),
    case("FILTERXML", "text() node test", "=FILTERXML(\"<r><a>t<b/></a></r>\",\"//a/text()\")", "t"),
    case("FILTERXML", "XML read from a cell", "=FILTERXML(V10,\"/feed/entry[1]/title\")", "First"),
    case("FILTERXML", "Rows returned for several matches", "=ROWS(FILTERXML(\"<r><p>1</p><p>2</p></r>\",\"//p\"))", "2"),
    case("FILTERXML", "Sum of a spilled result", "=SUM(FILTERXML(\"<r><n>1</n><n>2</n><n>3</n></r>\",\"//n\"))", "6"),
    case("FILTERXML", "No match is #VALUE!", "=FILTERXML(\"<r><a>1</a></r>\",\"//zzz\")", "#VALUE!"),
    case("FILTERXML", "Malformed XML is #VALUE!", "=FILTERXML(\"<r><a></r>\",\"//a\")", "#VALUE!"),
    case("FILTERXML", "Malformed XPath is #VALUE!", "=FILTERXML(\"<r><a>1</a></r>\",\"//a[\")", "#VALUE!"),
    case("FILTERXML", "Unknown XPath function is #VALUE!", "=FILTERXML(\"<r><a>1</a></r>\",\"nope(//a)\")", "#VALUE!"),
    case("FILTERXML", "Empty XML is #VALUE!", "=FILTERXML(\"\",\"//a\")", "#VALUE!"),
    case("FILTERXML", "IFERROR supplies a default", "=IFERROR(FILTERXML(\"<r/>\",\"//a\"),\"none\")", "none"),
    // WEBSERVICE
    case("WEBSERVICE", "Fetches text", "=WEBSERVICE(\"https://api.test/hello\")", "hello"),
    case("WEBSERVICE", "URL built with ENCODEURL", "=WEBSERVICE(\"https://api.test/search?q=\"&ENCODEURL(\"a b&c\"))", "found"),
    case("WEBSERVICE", "Empty response is empty text", "=WEBSERVICE(\"https://api.test/empty\")", ""),
    case("WEBSERVICE", "Response of exactly 32767 characters", "=LEN(WEBSERVICE(\"https://api.test/limit\"))", "32767"),
    case("WEBSERVICE", "Response over 32767 characters is #VALUE!", "=WEBSERVICE(\"https://api.test/huge\")", "#VALUE!"),
    case("WEBSERVICE", "Unreachable host is #VALUE!", "=WEBSERVICE(\"https://nowhere.test/\")", "#VALUE!"),
    case("WEBSERVICE", "Only http and https are allowed (ftp)", "=WEBSERVICE(\"ftp://api.test/hello\")", "#VALUE!"),
    case("WEBSERVICE", "Only http and https are allowed (file)", "=WEBSERVICE(\"file:///etc/hosts\")", "#VALUE!"),
    case("WEBSERVICE", "Empty URL is #VALUE!", "=WEBSERVICE(\"\")", "#VALUE!"),
    case("WEBSERVICE", "URL over 2048 characters is #VALUE!", "=WEBSERVICE(\"https://api.test/\"&REPT(\"a\",2040))", "#VALUE!"),
    case("WEBSERVICE", "Error argument propagates", "=WEBSERVICE(V8)", "#N/A"),
    case(
        "WEBSERVICE+FILTERXML",
        "Pick a value out of a response",
        "=FILTERXML(WEBSERVICE(\"https://api.test/rate\"),\"//rate[@cur='GBP']\")",
        "0.79",
    ),
    case("WEBSERVICE+FILTERXML", "Failed fetch flows through as #VALUE!", "=FILTERXML(WEBSERVICE(\"https://nowhere.test/\"),\"//rate\")", "#VALUE!"),
];

/// A case whose result spills down a column.
struct ArrayCase {
    title: &'static str,
    formula: &'static str,
    expected: &'static [&'static str],
}

const ARRAY_CASES: &[ArrayCase] = &[
    ArrayCase { title: "FILTERXML: all elements", formula: "=FILTERXML(\"<r><p>a</p><p>b</p><p>c</p></r>\",\"//p\")", expected: &["a", "b", "c"] },
    ArrayCase { title: "FILTERXML: numbers", formula: "=FILTERXML(\"<r><n>1</n><n>2.5</n></r>\",\"//n\")", expected: &["1", "2.5"] },
    ArrayCase { title: "FILTERXML: attributes", formula: "=FILTERXML(\"<r><i id='x'/><i id='y'/></r>\",\"//i/@id\")", expected: &["x", "y"] },
    ArrayCase {
        title: "FILTERXML: union in document order",
        formula: "=FILTERXML(\"<r><b>1</b><a>2</a><b>3</b></r>\",\"//a|//b\")",
        expected: &["1", "2", "3"],
    },
    ArrayCase {
        title: "FILTERXML: position() filter",
        formula: "=FILTERXML(\"<r><p>1</p><p>2</p><p>3</p></r>\",\"//p[position()>1]\")",
        expected: &["2", "3"],
    },
    ArrayCase {
        title: "WEBSERVICE+FILTERXML: all rates",
        formula: "=FILTERXML(WEBSERVICE(\"https://api.test/rate\"),\"//rate\")",
        expected: &["0.92", "0.79"],
    },
];

/// First row of the scalar table; rows above hold the title, legend and summary.
const FIRST_ROW: usize = 6;
/// First data row of the array blocks (title above, status in between).
const ARRAY_ROW: usize = 6;
/// First column of the array blocks (H): two columns per case, result then expected.
const ARRAY_COL: usize = 7;

fn col_name(c: usize) -> String {
    let mut n = c + 1;
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, char::from(b'A' + (n % 26) as u8));
        n /= 26;
    }
    s
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::parse(cell).unwrap())
}

fn text_of(v: &Value) -> String {
    match v {
        Value::Text(t) => t.to_string(),
        other => format!("{other:?}"),
    }
}

fn pass_formula(row: usize) -> String {
    format!(
        "=IF(ISERROR(D{row}),IF(ISERROR(E{row}),IF(ERROR.TYPE(E{row})=ERROR.TYPE(D{row}),\"PASS\",\"FAIL\"),\"FAIL\"),IF(ISERROR(E{row}),\"FAIL\",IF(E{row}=D{row},\"PASS\",\"FAIL\")))"
    )
}

/// Builds the use-case workbook in `s` (a fresh session).
fn build(s: &mut Session) {
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    s.execute("file.new", json!({})).unwrap();

    // Source data for the PERCENTOF and cell-input cases (column V).
    set(s, "U1", "Test data");
    for (i, n) in [10, 10, 5, 5, 10].iter().enumerate() {
        set(s, &format!("V{}", i + 2), &n.to_string());
    }
    set(s, "V7", "0");
    set(s, "V8", "=NA()");
    set(s, "V10", "<feed><entry><title>First</title></entry><entry><title>Second</title></entry></feed>");

    set(s, "A1", "Web functions and PERCENTOF: use cases");
    set(s, "A2", "Edit nothing: column E recalculates, column F compares it with the expected value in D.");
    set(s, "A3", "Summary");
    let last = FIRST_ROW + SCALAR_CASES.len() - 1;
    set(s, "B3", &format!("=COUNTIF(F{FIRST_ROW}:F{last},\"PASS\")"));
    set(s, "C3", &format!("=COUNTIF(F{FIRST_ROW}:F{last},\"FAIL\")"));
    for (c, h) in ["Function", "Use case", "Formula", "Expected", "Result", "Check"].iter().enumerate() {
        set(s, &format!("{}{}", col_name(c), FIRST_ROW - 1), h);
    }
    for (i, c) in SCALAR_CASES.iter().enumerate() {
        let row = FIRST_ROW + i;
        set(s, &format!("A{row}"), c.function);
        set(s, &format!("B{row}"), c.use_case);
        set(s, &format!("C{row}"), c.formula.trim_start_matches('='));
        set(s, &format!("D{row}"), c.expected);
        set(s, &format!("E{row}"), c.formula);
        set(s, &format!("F{row}"), &pass_formula(row));
    }

    for (k, c) in ARRAY_CASES.iter().enumerate() {
        let res = col_name(ARRAY_COL + 2 * k);
        let exp = col_name(ARRAY_COL + 2 * k + 1);
        let top = ARRAY_ROW;
        let bottom = top + c.expected.len() - 1;
        set(s, &format!("{res}{}", top - 2), c.title);
        set(s, &format!("{res}{top}"), c.formula);
        for (i, e) in c.expected.iter().enumerate() {
            set(s, &format!("{exp}{}", top + i), e);
        }
        set(
            s,
            &format!("{res}{}", top - 1),
            &format!(
                "=IF(ROWS({res}{top}#)=ROWS({exp}{top}:{exp}{bottom}),IF(SUMPRODUCT(--({res}{top}#={exp}{top}:{exp}{bottom}))=ROWS({exp}{top}:{exp}{bottom}),\"PASS\",\"FAIL\"),\"FAIL\")"
            ),
        );
    }
}

#[test]
fn use_case_workbook_passes() {
    let mut s = Session::new();
    build(&mut s);

    let mut failures = vec![];
    for (i, c) in SCALAR_CASES.iter().enumerate() {
        let row = FIRST_ROW + i;
        let check = text_of(&value(&s, &format!("F{row}")));
        if check != "PASS" {
            failures.push(format!("{} / {}: {} -> {:?} (expected {})", c.function, c.use_case, c.formula, value(&s, &format!("E{row}")), c.expected));
        }
    }
    for (k, c) in ARRAY_CASES.iter().enumerate() {
        let res = col_name(ARRAY_COL + 2 * k);
        let check = text_of(&value(&s, &format!("{res}{}", ARRAY_ROW - 1)));
        if check != "PASS" {
            failures.push(format!("{}: {} -> {:?}", c.title, c.formula, value(&s, &format!("{res}{ARRAY_ROW}"))));
        }
    }
    assert!(failures.is_empty(), "{} use cases failed:\n{}", failures.len(), failures.join("\n"));
    assert_eq!(value(&s, "C3"), Value::Number(0.0));

    if let Ok(path) = std::env::var("GRIDCRAFT_WRITE_USE_CASES") {
        s.execute("file.saveAs", json!({"path": path})).unwrap();
    }
}

/// Without a fetcher installed (the default build) WEBSERVICE is #VALUE! for every URL.
#[test]
fn webservice_without_fetcher_is_value_error() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    set(&mut s, "A1", "=WEBSERVICE(\"https://api.test/hello\")");
    assert_eq!(value(&s, "A1"), Value::Error(gridcraft_core::CellError::Value));
}

/// A workbook opened after the host installs a fetcher shows fetched values at once, and opens clean.
#[test]
fn opening_a_workbook_uses_the_installed_fetcher() {
    let path = std::env::temp_dir().join(format!("gridcraft-webservice-open-{}.xlsx", std::process::id()));
    let mut writer = Session::new();
    writer.execute("file.new", json!({})).unwrap();
    set(&mut writer, "A1", "=WEBSERVICE(\"https://api.test/hello\")");
    assert_eq!(value(&writer, "A1"), Value::Error(gridcraft_core::CellError::Value));
    writer.execute("file.saveAs", json!({"path": path.to_string_lossy()})).unwrap();

    let mut s = Session::new();
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    s.execute("file.open", json!({"path": path.to_string_lossy()})).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(value(&s, "A1"), Value::from("hello"));
    assert!(!s.doc().unwrap().is_dirty(), "recalculating on open must not leave the workbook modified");
}

/// A formula that reads a defined name holding WEBSERVICE is calculated with the fetcher on open too.
#[test]
fn opening_a_workbook_finds_webservice_in_defined_names() {
    let path = std::env::temp_dir().join(format!("gridcraft-webservice-name-{}.xlsx", std::process::id()));
    let mut writer = Session::new();
    writer.execute("file.new", json!({})).unwrap();
    writer.execute("formulas.defineName", json!({"name": "RemoteData", "refersTo": "=WEBSERVICE(\"https://api.test/hello\")"})).unwrap();
    set(&mut writer, "A1", "=RemoteData");
    writer.execute("file.saveAs", json!({"path": path.to_string_lossy()})).unwrap();

    let mut s = Session::new();
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    s.execute("file.open", json!({"path": path.to_string_lossy()})).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(value(&s, "A1"), Value::from("hello"));
}

/// `DocState::from_saved` keeps the file's values until an edit reaches them, fetcher or not.
#[test]
fn from_saved_documents_keep_their_values() {
    let mut writer = Session::new();
    writer.execute("file.new", json!({})).unwrap();
    set(&mut writer, "A1", "=WEBSERVICE(\"https://api.test/hello\")");
    let wb = (*writer.doc().unwrap().wb).clone();

    let mut s = Session::new();
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    s.add_document(gridcraft_engine::DocState::from_saved(wb, None, "saved".into(), gridcraft_engine::Trust::FromFile));
    assert_eq!(value(&s, "A1"), Value::Error(gridcraft_core::CellError::Value));
}

/// Installing a fetcher calculates only the formulas that reach WEBSERVICE: an unused defined name
/// that mentions it, and unrelated formulas, are left alone.
#[test]
fn opening_calculates_only_formulas_that_reach_webservice() {
    let mut writer = Session::new();
    writer.execute("file.new", json!({})).unwrap();
    writer.execute("formulas.defineName", json!({"name": "Unused", "refersTo": "=WEBSERVICE(\"https://api.test/hello\")"})).unwrap();
    set(&mut writer, "A1", "=1+1");
    let wb = (*writer.doc().unwrap().wb).clone();

    let mut s = Session::new();
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    let mut d = gridcraft_engine::DocState::new(wb, None, "unused".into(), gridcraft_engine::Trust::FromFile);
    d.calc.last_recalc_cells = 999;
    s.add_document(d);
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 999, "nothing reaches the name, so nothing is calculated");

    let mut writer = Session::new();
    writer.execute("file.new", json!({})).unwrap();
    set(&mut writer, "A1", "=WEBSERVICE(\"https://api.test/hello\")");
    set(&mut writer, "A2", "=1+1");
    let wb = (*writer.doc().unwrap().wb).clone();
    let mut s = Session::new();
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    s.add_document(gridcraft_engine::DocState::new(wb, None, "used".into(), gridcraft_engine::Trust::FromFile));
    assert_eq!(value(&s, "A1"), Value::from("hello"));
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 1, "only the WEBSERVICE cell is calculated, not A2");
}

/// Dynamic formulas (INDIRECT, OFFSET, volatile ones) that nothing ties to WEBSERVICE stay out of
/// the open-time pass too, though every ordinary edit recalculates them.
#[test]
fn opening_leaves_unrelated_dynamic_formulas_alone() {
    let mut writer = Session::new();
    writer.execute("file.new", json!({})).unwrap();
    set(&mut writer, "A1", "=WEBSERVICE(\"https://api.test/hello\")");
    set(&mut writer, "A2", "=INDIRECT(\"C1\")");
    set(&mut writer, "A3", "=RAND()");
    set(&mut writer, "A4", "=OFFSET(C1,0,0)");
    let wb = (*writer.doc().unwrap().wb).clone();

    let mut s = Session::new();
    s.set_web_fetcher(Some(Arc::new(CannedWeb)));
    let d = gridcraft_engine::DocState::new(wb, None, "dynamic".into(), gridcraft_engine::Trust::FromFile);
    let rand_before = d.wb.sheet(0).unwrap().value(CellRef::parse("A3").unwrap());
    s.add_document(d);
    assert_eq!(value(&s, "A1"), Value::from("hello"));
    assert_eq!(value(&s, "A3"), rand_before, "RAND() must not be recalculated by the open-time pass");
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 1, "only the WEBSERVICE cell is calculated");
}
