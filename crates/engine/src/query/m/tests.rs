//! Power Query M: queries as the Power Query Editor writes them, the language's rules, sources
//! through a fake data access, and hostile formulas (which must end in errors, never panics).

use std::collections::HashMap;
use std::sync::Mutex;

use gridcraft_core::{DateSystem, Value};
use gridcraft_model::query::ColType;
use gridcraft_model::{ExcelQuery, Workbook};
use serde_json::json;

use super::eval::Ev;
use super::library::FUNCTIONS;
use super::value::{MResult, MValue};
use super::{evaluate_excel_queries, parser, to_qtable, unsupported};
use crate::Session;
use crate::query::access::{DataAccess, DbTable, FileEntry, HttpRequest, HttpResponse};
use crate::query::{EvalCx, FetchCache, QTable};

#[derive(Default)]
struct Fake {
    files: Mutex<HashMap<String, Vec<u8>>>,
    web: Mutex<HashMap<String, (String, Vec<u8>)>>,
}

impl DataAccess for Fake {
    fn read_file(&self, path: &str, _max: usize) -> Result<Vec<u8>, String> {
        self.files.lock().unwrap().get(path).cloned().ok_or_else(|| format!("Couldn't open '{path}'"))
    }
    fn list_folder(&self, path: &str, _max: usize) -> Result<Vec<FileEntry>, String> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let files = self.files.lock().unwrap();
        let mut names: Vec<&String> = files.keys().filter(|k| k.starts_with(&prefix)).collect();
        names.sort();
        Ok(names
            .into_iter()
            .map(|k| {
                let name = k.trim_start_matches(&prefix).to_string();
                let extension = name.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
                FileEntry { size: 1, name, extension, folder: prefix.clone(), ..Default::default() }
            })
            .collect())
    }
    fn web_available(&self) -> Result<(), String> {
        Ok(())
    }
    fn odbc_available(&self) -> Result<(), String> {
        Err("no ODBC here".into())
    }
    fn odbc_tables(&self, _: &str) -> Result<Vec<String>, String> {
        Err("no ODBC here".into())
    }
    fn odbc_table(&self, _: &str, _: &str, _: usize) -> Result<DbTable, String> {
        Err("no ODBC here".into())
    }
    fn odbc_query(&self, _: &str, _: &str, _: usize) -> Result<DbTable, String> {
        Err("no ODBC here".into())
    }
    fn http_get(&self, req: &HttpRequest) -> Result<HttpResponse, String> {
        let (ct, body) = self.web.lock().unwrap().get(&req.url).cloned().ok_or_else(|| format!("Couldn't reach {}", req.url))?;
        Ok(HttpResponse { status: 200, content_type: Some(ct), body })
    }
}

/// A workbook with a `Sales` table (dates formatted as dates) and a `Rates` table.
fn workbook() -> Workbook {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Region", "Units", "Price", "Date"],
            ["North", 10, 2.5, 46027],
            ["South", 7, 3, 46028],
            ["North", 3, 2.5, 46054],
            ["", "", "", ""],
            ["East", 4, 1, 46082],
        ]}),
    )
    .unwrap();
    s.execute("insert.table", json!({"range": "A1:D6", "name": "Sales"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "D2:D6", "code": "m/d/yyyy"})).unwrap();
    s.execute("range.setValues", json!({"range": "G1", "values": [["Region", "Rate"], ["North", 0.1], ["South", 0.2], ["West", 0.3]]})).unwrap();
    s.execute("insert.table", json!({"range": "G1:H4", "name": "Rates"})).unwrap();
    (*s.doc().unwrap().wb).clone()
}

fn run_with(wb: &Workbook, fake: &Fake, external: Result<(), String>, formula: &str) -> MResult<MValue> {
    let mut cache = FetchCache::default();
    let mut cx = EvalCx::new(wb, fake, external, &mut cache);
    let mut ev = Ev::new(&mut cx);
    let e = parser::parse(formula)?;
    ev.eval(&e, &None)
}

fn run(formula: &str) -> MResult<MValue> {
    run_with(&workbook(), &Fake::default(), Ok(()), formula)
}

fn loaded(v: MResult<MValue>, what: &str) -> QTable {
    match v {
        Ok(MValue::Table(t)) => to_qtable(&t, DateSystem::D1900).table,
        other => panic!("{what}: not a table: {other:?}"),
    }
}

fn table(formula: &str) -> QTable {
    loaded(run(formula), formula)
}

fn show(formula: &str) -> String {
    match run(formula) {
        Ok(v) => super::library::show(&v),
        Err(e) => format!("ERROR {e}"),
    }
}

fn n(x: f64) -> Value {
    Value::number(x)
}
fn t(s: &str) -> Value {
    Value::text(s)
}

#[test]
fn a_query_as_the_editor_writes_it() {
    let r = table(
        r#"let
    Source = Excel.CurrentWorkbook(){[Name="Sales"]}[Content],
    #"Changed Type" = Table.TransformColumnTypes(Source,{{"Region", type text}, {"Units", Int64.Type}, {"Price", type number}, {"Date", type date}}),
    #"Filtered Rows" = Table.SelectRows(#"Changed Type", each ([Region] <> null and [Region] <> "")),
    #"Added Custom" = Table.AddColumn(#"Filtered Rows", "Revenue", each [Units] * [Price], type number),
    #"Added Conditional Column" = Table.AddColumn(#"Added Custom", "Size", each if [Units] >= 10 then "Big" else if [Units] >= 5 then "Medium" else "Small"),
    #"Sorted Rows" = Table.Sort(#"Added Conditional Column",{{"Revenue", Order.Descending}}),
    #"Removed Columns" = Table.RemoveColumns(#"Sorted Rows",{"Price"}),
    #"Renamed Columns" = Table.RenameColumns(#"Removed Columns",{{"Units", "Qty"}})
in
    #"Renamed Columns""#,
    );
    assert_eq!(r.columns, vec!["Region", "Qty", "Date", "Revenue", "Size"]);
    assert_eq!(r.rows.len(), 4);
    assert_eq!(r.rows[0], vec![t("North"), n(10.0), n(46027.0), n(25.0), t("Big")]);
    assert_eq!(r.rows[1], vec![t("South"), n(7.0), n(46028.0), n(21.0), t("Medium")]);
    assert_eq!(r.rows[3][4], t("Small"));
    // The date column loads as dates.
    assert_eq!(r.types[2], ColType::Date);

    let g = table(
        r#"let
    Source = Excel.CurrentWorkbook(){[Name="Sales"]}[Content],
    Typed = Table.TransformColumnTypes(Source,{{"Units", type number}}),
    Kept = Table.SelectRows(Typed, each [Units] <> null),
    #"Grouped Rows" = Table.Group(Kept, {"Region"}, {{"Units", each List.Sum([Units]), type number}, {"Count", each Table.RowCount(_), Int64.Type}, {"All", each _, type table}})
in
    #"Grouped Rows""#,
    );
    assert_eq!(g.columns, vec!["Region", "Units", "Count", "All"]);
    assert_eq!(g.rows[0], vec![t("North"), n(13.0), n(2.0), t("Table")]);
    assert_eq!(g.rows.len(), 3);
}

#[test]
fn merge_expand_append_and_navigation() {
    let r = table(
        r#"let
    Sales = Excel.CurrentWorkbook(){[Name="Sales"]}[Content],
    Rates = Excel.CurrentWorkbook(){[Name="Rates"]}[Content],
    Merged = Table.NestedJoin(Sales, {"Region"}, Rates, {"Region"}, "Rates", JoinKind.LeftOuter),
    Expanded = Table.ExpandTableColumn(Merged, "Rates", {"Rate"}, {"Rates.Rate"}),
    Kept = Table.SelectColumns(Expanded, {"Region", "Rates.Rate"})
in
    Kept"#,
    );
    assert_eq!(r.columns, vec!["Region", "Rates.Rate"]);
    assert_eq!(r.rows.len(), 5);
    assert_eq!(r.rows[0], vec![t("North"), n(0.1)]);
    assert_eq!(r.rows[4], vec![t("East"), Value::Empty]);
    // Inner, anti and full outer joins.
    let count = |kind: &str| {
        table(&format!(
            r#"Table.NestedJoin(Excel.CurrentWorkbook(){{[Name="Sales"]}}[Content], "Region", Excel.CurrentWorkbook(){{[Name="Rates"]}}[Content], "Region", "R", JoinKind.{kind})"#
        ))
        .rows
        .len()
    };
    assert_eq!((count("Inner"), count("LeftAnti"), count("RightAnti"), count("FullOuter")), (3, 2, 1, 6));
    // Append matches columns by name.
    let a = table(r#"Table.Combine({#table({"A","B"}, {{1, 2}}), #table({"B","C"}, {{3, 4}})})"#);
    assert_eq!(a.columns, vec!["A", "B", "C"]);
    assert_eq!(a.rows, vec![vec![n(1.0), n(2.0), Value::Empty], vec![Value::Empty, n(3.0), n(4.0)]]);
    // A key that matches nothing, or more than one row, is an error.
    assert!(show(r#"Excel.CurrentWorkbook(){[Name="Nope"]}[Content]"#).contains("didn't match any rows"));
    assert!(show(r#"#table({"A"}, {{1}, {1}}){[A=1]}"#).contains("more than one row"));
    assert_eq!(show(r#"#table({"A"}, {{1}}){[A=2]}?"#), "null");
}

#[test]
fn reshaping_steps() {
    let base = r#"#table({"Id", "Q1", "Q2"}, {{"a", 1, 2}, {"b", 3, null}})"#;
    let u = table(&format!(r#"Table.UnpivotOtherColumns({base}, {{"Id"}}, "Attribute", "Value")"#));
    assert_eq!(u.columns, vec!["Id", "Attribute", "Value"]);
    // Nulls aren't unpivoted.
    assert_eq!(u.rows.len(), 3);
    assert_eq!(u.rows[1], vec![t("a"), t("Q2"), n(2.0)]);
    let p = table(&format!(
        r#"let u = Table.UnpivotOtherColumns({base}, {{"Id"}}, "Attribute", "Value") in Table.Pivot(u, List.Distinct(u[Attribute]), "Attribute", "Value", List.Sum)"#
    ));
    assert_eq!(p.columns, vec!["Id", "Q1", "Q2"]);
    assert_eq!(p.rows[1], vec![t("b"), n(3.0), Value::Empty]);
    let tr = table(&format!("Table.Transpose({base})"));
    assert_eq!(tr.columns, vec!["Column1", "Column2"]);
    assert_eq!(tr.rows[0], vec![t("a"), t("b")]);
    let s = table(
        r#"Table.SplitColumn(#table({"Name"}, {{"Ann Lee"}, {"Bo"}, {null}}), "Name", Splitter.SplitTextByDelimiter(" ", QuoteStyle.Csv), {"Name.1", "Name.2"})"#,
    );
    assert_eq!(s.columns, vec!["Name.1", "Name.2"]);
    assert_eq!(s.rows, vec![vec![t("Ann"), t("Lee")], vec![t("Bo"), Value::Empty], vec![Value::Empty, Value::Empty]]);
    let f = table(r#"Table.FillDown(#table({"A","B"}, {{"x", 1}, {null, 2}, {"y", null}}), {"A", "B"})"#);
    assert_eq!(f.rows[1], vec![t("x"), n(2.0)]);
    assert_eq!(f.rows[2], vec![t("y"), n(2.0)]);
    let c = table(
        r#"Table.CombineColumns(#table({"First","Last","Age"}, {{"Ann", "Lee", 3}, {"Bo", null, 4}}), {"First", "Last"}, Combiner.CombineTextByDelimiter(" ", QuoteStyle.None), "Full")"#,
    );
    assert_eq!(c.columns, vec!["Full", "Age"]);
    assert_eq!(c.rows[0], vec![t("Ann Lee"), n(3.0)]);
    let x = table(r#"Table.TransformColumns(#table({"A","B"}, {{"ab", 1}}), {{"A", Text.Upper, type text}, {"B", each _ * 10, type number}})"#);
    assert_eq!(x.rows[0], vec![t("AB"), n(10.0)]);
    let i = table(r#"Table.AddIndexColumn(#table({"A"}, {{"x"}, {"y"}}), "Index", 1, 1, Int64.Type)"#);
    assert_eq!(i.rows[1], vec![t("y"), n(2.0)]);
    let r = table(r#"Table.ReplaceValue(#table({"A","B"}, {{"a-b", 1}, {"c", 1}}), "-", "+", Replacer.ReplaceText, {"A"})"#);
    assert_eq!(r.rows[0][0], t("a+b"));
    let r = table(r#"Table.ReplaceValue(#table({"A"}, {{1}, {2}}), 1, 9, Replacer.ReplaceValue, {"A"})"#);
    assert_eq!(r.rows[0][0], n(9.0));
    let alt = table(r#"Table.AlternateRows(#table({"A"}, {{1},{2},{3},{4},{5}}), 1, 1, 1)"#);
    assert_eq!(alt.rows, vec![vec![n(1.0)], vec![n(3.0)], vec![n(5.0)]]);
    let ro = table(r#"Table.ReorderColumns(#table({"a","b","c","d"}, {}), {"d","b"})"#);
    assert_eq!(ro.columns, vec!["a", "d", "c", "b"]);
    let d = table(r#"Table.Distinct(#table({"A"}, {{"x"}, {"X"}, {"x"}}), {"A", Comparer.OrdinalIgnoreCase})"#);
    assert_eq!(d.rows.len(), 1);
    let p = table(r#"Table.PromoteHeaders(#table(3, {{"a", 2026, null}, {1, 2, 3}}), [PromoteAllScalars=true])"#);
    assert_eq!(p.columns, vec!["a", "2026", "Column3"]);
}

#[test]
fn power_query_naming_and_ordering() {
    // Repeated headers are numbered with one counter over all repeats, skipping taken names.
    let p = table(r#"Table.PromoteHeaders(#table(6, {{"A", "B", "A", "B", "A_1", "A"}, {1, 2, 3, 4, 5, 6}}))"#);
    assert_eq!(p.columns, vec!["A", "B", "A_2", "B_3", "A_1", "A_4"]);
    // Pivoted rows come out in the order of the other columns' values.
    let v = table(r#"Table.Pivot(#table({"K", "Att", "V"}, {{"b", "x", 1}, {"a", "x", 2}, {"b", "y", 3}}), {"x", "y"}, "Att", "V")"#);
    assert_eq!(v.rows, vec![vec![t("a"), n(2.0), Value::Empty], vec![t("b"), n(1.0), n(3.0)]]);
}

#[test]
fn enter_data_tables_decompress() {
    let json = br#"[["x","1"],["y","2"]]"#;
    let b64 = crate::io::base64_encode(&miniz_oxide::deflate::compress_to_vec(json, 6));
    let r = table(&format!(
        r#"let
    Source = Table.FromRows(Json.Document(Binary.Decompress(Binary.FromText("{b64}", BinaryEncoding.Base64), Compression.Deflate)), let _t = ((type nullable text) meta [Serialized.Text = true]) in type table [Name = _t, #"Count" = _t]),
    #"Changed Type" = Table.TransformColumnTypes(Source,{{{{"Name", type text}}, {{"Count", Int64.Type}}}})
in
    #"Changed Type""#
    ));
    assert_eq!(r.columns, vec!["Name", "Count"]);
    assert_eq!(r.rows, vec![vec![t("x"), n(1.0)], vec![t("y"), n(2.0)]]);
}

#[test]
fn files_web_and_json_through_data_access() {
    let fake = Fake::default();
    fake.files.lock().unwrap().insert("C:/d/s.csv".into(), b"Name,Qty\n\"Lee, Ann\",3\nBo,4\n".to_vec());
    fake.files.lock().unwrap().insert("C:/d/f/a.csv".into(), b"x\n1\n".to_vec());
    fake.files.lock().unwrap().insert("C:/d/f/b.csv".into(), b"x\n2\n".to_vec());
    fake.web.lock().unwrap().insert("https://x.test/api?q=1".into(), ("application/json".into(), br#"{"items":[{"a":1,"b":"z"},{"a":2}]}"#.to_vec()));
    fake.web.lock().unwrap().insert(
        "https://x.test/page".into(),
        ("text/html".into(), b"<table><tr><th>K</th><th>V</th></tr><tr><td>a</td><td>1</td></tr></table>".to_vec()),
    );
    let wb = workbook();
    let get = |f: &str| loaded(run_with(&wb, &fake, Ok(()), f), f);
    let csv = get(r#"let
    Source = Csv.Document(File.Contents("C:/d/s.csv"),[Delimiter=",", Columns=2, Encoding=65001, QuoteStyle=QuoteStyle.None]),
    #"Promoted Headers" = Table.PromoteHeaders(Source, [PromoteAllScalars=true]),
    #"Changed Type" = Table.TransformColumnTypes(#"Promoted Headers",{{"Name", type text}, {"Qty", Int64.Type}})
in
    #"Changed Type""#);
    assert_eq!(csv.columns, vec!["Name", "Qty"]);
    assert_eq!(csv.rows[0], vec![t("Lee, Ann"), n(3.0)]);
    let json = get(r#"let
    Source = Json.Document(Web.Contents("https://x.test", [RelativePath="api", Query=[q="1"]])),
    items = Source[items],
    #"Converted to Table" = Table.FromList(items, Splitter.SplitByNothing(), null, null, ExtraValues.Error),
    #"Expanded Column1" = Table.ExpandRecordColumn(#"Converted to Table", "Column1", {"a", "b"}, {"a", "b"})
in
    #"Expanded Column1""#);
    assert_eq!(json.rows, vec![vec![n(1.0), t("z")], vec![n(2.0), Value::Empty]]);
    let page = get(r#"Web.Page(Web.Contents("https://x.test/page")){0}[Data]"#);
    assert_eq!(page.columns, vec!["K", "V"]);
    let folder = get(r#"let
    Source = Folder.Files("C:/d/f"),
    Visible = Table.SelectRows(Source, each [Attributes]?[Hidden]? <> true),
    Parsed = Table.AddColumn(Visible, "Data", each Table.PromoteHeaders(Csv.Document([Content]))),
    Kept = Table.SelectColumns(Parsed, {"Name", "Data"}),
    Expanded = Table.ExpandTableColumn(Kept, "Data", {"x"})
in
    Expanded"#);
    assert_eq!(folder.rows, vec![vec![t("a.csv"), t("1")], vec![t("b.csv"), t("2")]]);
    // With external content disabled, sources are refused (with the reason).
    let e = run_with(&wb, &fake, Err("External content is disabled.".into()), r#"File.Contents("C:/d/s.csv")"#).unwrap_err();
    assert!(e.message.contains("External content is disabled"), "{e}");
    // Another workbook's sheets, tables and names.
    let mut other = Session::new();
    other.execute("file.new", json!({})).unwrap();
    other.execute("range.setValues", json!({"range": "A1", "values": [["P", "N"], ["x", 1]]})).unwrap();
    other.execute("insert.table", json!({"range": "A1:B2", "name": "Stock"})).unwrap();
    let bytes = crate::io::save_bytes(&other.doc().unwrap().wb, "o.xlsx").unwrap();
    fake.files.lock().unwrap().insert("C:/d/o.xlsx".into(), bytes);
    let stock = get(r#"Excel.Workbook(File.Contents("C:/d/o.xlsx"), null, true){[Item="Stock",Kind="Table"]}[Data]"#);
    assert_eq!(stock.rows, vec![vec![t("x"), n(1.0)]]);
    let sheet = get(r#"Excel.Workbook(File.Contents("C:/d/o.xlsx"), true){[Item="Sheet1",Kind="Sheet"]}[Data]"#);
    assert_eq!(sheet.columns, vec!["P", "N"]);
}

#[test]
fn the_language() {
    // let is lazy: an unused failing step doesn't fail the query; steps may come in any order.
    assert_eq!(show("let a = error \"no\", b = c + 1, c = 2 in b"), "3");
    assert!(show("let x = x + 1 in x").contains("cyclic"));
    assert_eq!(show("try error \"bad\" otherwise 5"), "5");
    assert_eq!(show("(try error \"bad\")[Error][Message]"), "\"bad\"");
    assert_eq!(show("(try 1)[Value]"), "1");
    assert_eq!(show("try (1/0) otherwise 2"), "Infinity");
    assert_eq!(show("null ?? 4"), "4");
    assert_eq!(show("null + 1"), "null");
    assert_eq!(show("null & \"a\""), "null");
    assert!(show("\"a\" & 1").contains("operator & to types Text and Number"));
    assert_eq!(show("1 = \"1\""), "false");
    assert_eq!(show("null < 1"), "null");
    assert!(show("1 < \"a\"").starts_with("ERROR"));
    assert_eq!(show("[a = 1, b = a + 1][b]"), "2");
    assert_eq!(show("[a = 1][c]?"), "null");
    assert_eq!(show("List.Sum({1..4, 10})"), "20");
    assert_eq!(show("{1, 2, 3}{1}"), "2");
    assert!(show("{1}{5}").contains("enough elements"));
    assert_eq!(show("((x, optional y) => if y = null then x else x + y)(1)"), "1");
    assert!(show("((x) => x)(1, 2)").contains("2 arguments"));
    assert_eq!(show("List.Transform({1, 2}, each _ * 2){1}"), "4");
    assert_eq!(show("let f = (n) => if n = 0 then 1 else n * @f(n - 1) in f(5)"), "120");
    assert_eq!(show("1 is number and \"a\" is text and null is nullable text and not (null is text)"), "true");
    assert!(show("\"a\" as number").starts_with("ERROR"));
    assert_eq!(show("#date(2026, 1, 31) + #duration(1, 0, 0, 0)"), "#date(2026, 2, 1)");
    assert_eq!(show("Table.Profile"), "ERROR Expression.Error: The M function 'Table.Profile' isn't supported by GridCraft yet.");
    assert_eq!(show("#date(2026, 3, 1) - #date(2026, 2, 1)"), "28.00:00:00");
    assert_eq!(show("Date.Year(#datetime(2026, 5, 6, 7, 8, 9))"), "2026");
    assert_eq!(show("Text.From(#date(2026, 1, 31))"), "\"1/31/2026\"");
    assert_eq!(show("Number.Round(2.5) + Number.Round(1.005, 2)"), "3");
    assert_eq!(show("Text.Combine({\"a\", null, \"b\"}, \"-\")"), "\"a-b\"");
    assert_eq!(show("Text.BeforeDelimiter(\"a-b-c\", \"-\", 1) & Text.AfterDelimiter(\"a-b-c\", \"-\", {0, RelativePosition.FromEnd})"), "\"a-bc\"");
    assert_eq!(show("Text.Length(\"😀\")"), "2");
    assert_eq!(show("\"tab#(tab)end\""), "\"tab\tend\"");
    assert_eq!(show("unknownThing"), "ERROR Expression.Error: The name 'unknownThing' wasn't recognized. Make sure it's spelled correctly.");
    assert_eq!(show("Record.FieldNames([A=1] & [B=2, A=3]){1}"), "\"B\"");
    assert_eq!(show("([A=1] & [B=2, A=3])[A]"), "3");
    assert_eq!(show("Table.RowCount(#table({\"A\"}, {{1}}) & #table({\"A\"}, {{2}}))"), "2");
}

/// A list as text (`1,2,null`), to compare list results.
fn lst(list: &str) -> String {
    show(&format!(
        "Text.Combine(List.Transform({list}, each if _ = null then \"null\" else if _ is list then \"[\" & Text.Combine(List.Transform(_, each if _ = null then \"null\" else Text.From(_)), \",\") & \"]\" else Text.From(_)), \",\")"
    ))
}

#[test]
fn hand_written_text_functions() {
    let cases = [
        ("Text.PadStart(\"Name\", 10)", "\"      Name\""),
        ("Text.PadStart(\"Name\", 10, \"|\")", "\"||||||Name\""),
        ("Text.PadEnd(\"Name\", 10, \"|\")", "\"Name||||||\""),
        ("Text.PadStart(\"Long name\", 4)", "\"Long name\""),
        ("Text.Range(\"Hello World\", 6)", "\"World\""),
        ("Text.Range(\"Hello World Hello\", 6, 5)", "\"World\""),
        ("Text.Remove(\"a,b;c\", {\",\", \";\"})", "\"abc\""),
        ("Text.Select(\"a1b2c3\", {\"0\"..\"9\"})", "\"123\""),
        ("Text.PositionOf(\"Hello, World! Hello, World!\", \"World\")", "7"),
        ("Text.PositionOf(\"Hello, World! Hello, World!\", \"World\", Occurrence.Last)", "21"),
        ("Text.PositionOf(\"Hello\", \"x\")", "-1"),
        ("Text.PositionOf(\"Hello\", \"L\", Occurrence.First, Comparer.OrdinalIgnoreCase)", "2"),
        ("Text.PositionOfAny(\"Hello, World!\", {\"W\", \"!\"})", "7"),
        ("Text.Insert(\"ABD\", 2, \"C\")", "\"ABCD\""),
        ("Text.RemoveRange(\"ABEFC\", 2)", "\"ABFC\""),
        ("Text.ReplaceRange(\"ABGF\", 2, 1, \"CDE\")", "\"ABCDEF\""),
        ("Text.Repeat(\"ab\", 3)", "\"ababab\""),
        ("Text.Reverse(\"123\")", "\"321\""),
        ("Text.At(\"Hello\", 4)", "\"o\""),
        ("Text.TrimStart(\"  a  \")", "\"a  \""),
        ("Text.TrimEnd(\"xxaxx\", \"x\")", "\"xxa\""),
        ("Text.Format(\"#{0}, #{1}!\", {\"a\", 2})", "\"a, 2!\""),
        ("Text.Format(\"Hi #[name]\", [name = \"Ann\"])", "\"Hi Ann\""),
        ("Character.FromNumber(65) & Text.From(Character.ToNumber(\"B\"))", "\"A66\""),
    ];
    for (f, want) in cases {
        assert_eq!(show(f), want, "{f}");
    }
    assert_eq!(lst("Text.PositionOf(\"abab\", \"b\", Occurrence.All)"), "\"1,3\"");
    assert_eq!(lst("Text.SplitAny(\"a,b;c\", \",;\")"), "\"a,b,c\"");
    assert_eq!(lst("Text.ToList(\"ab\")"), "\"a,b\"");
    assert_eq!(lst("{\"a\"..\"c\"}"), "\"a,b,c\"");
    for bad in [
        "Text.Range(\"abc\", 4)",
        "Text.Range(\"abc\", 1, 5)",
        "Text.At(\"abc\", 3)",
        "Text.Insert(\"abc\", 9, \"x\")",
        "Text.PadStart(\"a\", 3, \"xy\")",
        "Text.Format(\"#{3}\", {1})",
    ] {
        assert!(show(bad).starts_with("ERROR"), "{bad}");
    }
}

#[test]
fn hand_written_date_functions() {
    let cases = [
        ("Date.AddMonths(#date(2026, 1, 31), 1)", "#date(2026, 2, 28)"),
        ("Date.AddYears(#date(2024, 2, 29), 1)", "#date(2025, 2, 28)"),
        ("Date.AddQuarters(#date(2026, 11, 30), 1)", "#date(2027, 2, 28)"),
        ("Date.AddWeeks(#date(2026, 5, 17), -2)", "#date(2026, 5, 3)"),
        ("Date.StartOfMonth(#date(2026, 5, 17))", "#date(2026, 5, 1)"),
        ("Date.EndOfMonth(#date(2026, 5, 17))", "#date(2026, 5, 31)"),
        ("Date.StartOfQuarter(#date(2026, 5, 17))", "#date(2026, 4, 1)"),
        ("Date.EndOfQuarter(#date(2026, 5, 17))", "#date(2026, 6, 30)"),
        ("Date.EndOfYear(#date(2026, 5, 17))", "#date(2026, 12, 31)"),
        ("Date.StartOfWeek(#date(2026, 5, 17))", "#date(2026, 5, 17)"),
        ("Date.StartOfWeek(#date(2026, 5, 17), Day.Monday)", "#date(2026, 5, 11)"),
        ("Date.EndOfWeek(#date(2026, 5, 13), Day.Monday)", "#date(2026, 5, 17)"),
        ("Date.EndOfMonth(#datetime(2026, 5, 17, 10, 0, 0))", "5/31/2026 11:59:59 PM"),
        ("Date.DayOfWeek(#date(2026, 5, 17))", "0"),
        ("Date.DayOfWeek(#date(2026, 5, 17), Day.Monday)", "6"),
        ("Date.DayOfWeekName(#date(2026, 5, 17))", "\"Sunday\""),
        ("Date.DayOfWeekName(#date(2026, 5, 17), \"nl-NL\")", "\"zondag\""),
        ("Date.DayOfWeekName(#date(2026, 5, 17), \"pt-BR\")", "\"domingo\""),
        ("Date.MonthName(#date(2026, 3, 1), \"it-IT\")", "\"marzo\""),
        ("Date.ToText(#date(2026, 5, 17), [Format=\"d\", Culture=\"pt-BR\"])", "\"17/05/2026\""),
        ("Date.MonthName(#date(2026, 5, 17), \"de-DE\")", "\"Mai\""),
        ("Date.QuarterOfYear(#date(2026, 5, 17))", "2"),
        ("Date.DayOfYear(#date(2026, 5, 17))", "137"),
        ("Date.DaysInMonth(#date(2024, 2, 1))", "29"),
        ("Date.IsLeapYear(2024) and not Date.IsLeapYear(#date(2026, 1, 1))", "true"),
        ("Date.WeekOfYear(#date(2026, 1, 3))", "1"),
        ("Date.WeekOfYear(#date(2026, 1, 4))", "2"),
        ("Date.ToText(#date(2026, 5, 17), \"yyyy-MM-dd\")", "\"2026-05-17\""),
        ("Date.ToText(#date(2026, 5, 17), [Format = \"dd MMM yyyy\", Culture = \"en-US\"])", "\"17 May 2026\""),
        ("Date.ToText(#date(2026, 5, 17))", "\"5/17/2026\""),
        ("Date.ToText(#date(2026, 5, 17), null, \"de-DE\")", "\"17.05.2026\""),
        ("Date.ToText(#date(2026, 5, 17), \"dddd, d MMMM\", \"nl-NL\")", "\"zondag, 17 mei\""),
        ("DateTime.ToText(#datetime(2026, 5, 17, 13, 5, 9), \"h:mm tt\")", "\"1:05 PM\""),
        ("DateTime.ToText(#datetime(2026, 5, 17, 13, 5, 9), \"s\")", "\"2026-05-17T13:05:09\""),
        ("Text.From(#date(2026, 5, 17), \"de-DE\")", "\"17.05.2026\""),
        ("Date.FromText(\"2026-05-17\")", "#date(2026, 5, 17)"),
        ("Time.Hour(#time(13, 5, 9)) * 10000 + Time.Minute(#time(13, 5, 9)) * 100 + Time.Second(#time(13, 5, 9))", "130509"),
        (
            "Duration.Days(#duration(2, 3, 4, 5)) * 1000 + Duration.Hours(#duration(2, 3, 4, 5)) * 100 + Duration.Minutes(#duration(2, 3, 4, 5)) * 10",
            "2340",
        ),
        ("Duration.TotalHours(#duration(1, 12, 0, 0))", "36"),
        ("Date.IsInCurrentYear(DateTime.LocalNow())", "true"),
        ("Date.IsInPreviousNDays(Date.AddDays(DateTime.Date(DateTime.LocalNow()), -1), 2)", "true"),
        ("Date.IsInPreviousNDays(DateTime.Date(DateTime.LocalNow()), 2)", "false"),
        ("Date.IsInNextYear(Date.AddYears(DateTime.Date(DateTime.LocalNow()), 1))", "true"),
    ];
    for (f, want) in cases {
        assert_eq!(show(f), want, "{f}");
    }
    assert_eq!(lst("List.Dates(#date(2026, 1, 30), 3, #duration(1, 0, 0, 0))"), "\"1/30/2026,1/31/2026,2/1/2026\"");
}

#[test]
fn hand_written_number_list_and_value_functions() {
    let cases = [
        ("Number.ToText(1234.567, \"N2\")", "\"1,234.57\""),
        ("Number.ToText(1234.567, \"F1\")", "\"1234.6\""),
        ("Number.ToText(0.1234, \"P1\")", "\"12.3 %\""),
        ("Number.ToText(5, \"D3\")", "\"005\""),
        ("Number.ToText(1234.5, \"#,##0.00\")", "\"1,234.50\""),
        ("Number.ToText(1234.5, \"N2\", \"de-DE\")", "\"1.234,50\""),
        ("Number.ToText(-3, \"0;(0)\")", "\"(3)\""),
        ("Number.ToText(0.5, \"0%\")", "\"50%\""),
        ("Number.ToText(7, \"000\")", "\"007\""),
        ("Number.ToText(0.25, \"#.##\")", "\".25\""),
        ("Number.ToText(255, \"X\")", "\"FF\""),
        ("Number.ToText(1.5)", "\"1.5\""),
        ("Number.IsEven(4) and Number.IsOdd(3) and not Number.IsEven(2.5)", "true"),
        ("Number.Sign(-3)", "-1"),
        ("Number.Log(8, 2)", "3"),
        ("Number.Factorial(5)", "120"),
        ("Number.RoundAwayFromZero(-2.5)", "-3"),
        ("Number.RoundTowardZero(2.79, 1)", "2.7"),
        ("Int64.From(\"12.5\")", "12"),
        ("Int64.From(12.5, null, RoundingMode.AwayFromZero)", "13"),
        ("Currency.From(1.23456)", "1.2346"),
        ("Logical.ToText(true) & Logical.ToText(Logical.FromText(\"False\"))", "\"truefalse\""),
        ("Value.Is(1, type number) and not Value.Is(\"a\", type number)", "true"),
        ("Value.Type(\"a\") = type text", "true"),
        ("Number.Round(Number.PI, 4)", "3.1416"),
        ("List.Product({2, 3, 4})", "24"),
        ("Number.Round(List.StandardDeviation({1, 2, 3, 4}), 6)", "1.290994"),
        ("List.Mode({1, 2, 2, 3})", "2"),
        ("List.ContainsAny({1, 2}, {3, 2}) and not List.ContainsAll({1, 2}, {1, 3})", "true"),
        ("List.Single({7})", "7"),
        ("List.Accumulate({1, 2, 3}, 0, (state, current) => state + current)", "6"),
    ];
    for (f, want) in cases {
        assert_eq!(show(f), want, "{f}");
    }
    let lists = [
        ("List.FirstN({1, 2, 3}, 2)", "1,2"),
        ("List.FirstN({1, 2, 5, 1}, each _ < 3)", "1,2"),
        ("List.LastN({1, 2, 3}, 2)", "2,3"),
        ("List.Skip({1, 2, 3})", "2,3"),
        ("List.RemoveLastN({1, 2, 3})", "1,2"),
        ("List.Range({1..5}, 1, 2)", "2,3"),
        ("List.RemoveItems({1, 2, 3, 2}, {2})", "1,3"),
        ("List.Union({{1, 2}, {2, 3}})", "1,2,3"),
        ("List.Intersect({{1, 2, 2}, {2, 2, 3}})", "2,2"),
        ("List.Difference({1, 2, 2, 3}, {2})", "1,2,3"),
        ("List.Zip({{1, 2}, {\"a\"}})", "[1,a],[2,null]"),
        ("List.Repeat({1, 2}, 2)", "1,2,1,2"),
        ("List.Split({1..5}, 2)", "[1,2],[3,4],[5]"),
        ("List.Positions({\"a\", \"b\"})", "0,1"),
        ("List.Generate(() => 1, each _ <= 3, each _ + 1)", "1,2,3"),
        ("List.Generate(() => 1, each _ <= 3, each _ + 1, each _ * 10)", "10,20,30"),
        ("List.ReplaceValue({1, 2, 1}, 1, 9, Replacer.ReplaceValue)", "9,2,9"),
    ];
    for (f, want) in lists {
        assert_eq!(lst(f), format!("\"{want}\""), "{f}");
    }
    for bad in ["Number.ToText(1.5, \"D\")", "Number.Factorial(171)", "List.Single({1, 2})", "List.Range({1}, 3)", "List.StandardDeviation({1})"] {
        assert!(show(bad).starts_with("ERROR"), "{bad}");
    }
    // Runaway generators and repeats end in errors, not hangs.
    assert!(run("List.Generate(() => 0, each true, each _ + 1)").is_err());
    assert!(run("List.Repeat({1..1000}, 100000000)").is_err());
    assert!(run("Text.Repeat(\"x\", 1e15)").is_err());
}

#[test]
fn errors_stay_in_their_cells() {
    let r = run(r#"Table.TransformColumnTypes(#table({"A"}, {{"1"}, {"x"}, {"3"}}), {{"A", type number}})"#).unwrap();
    let MValue::Table(tb) = r else { panic!() };
    let out = to_qtable(&tb, DateSystem::D1900);
    assert_eq!(out.errors, 1);
    assert_eq!(out.table.rows, vec![vec![n(1.0)], vec![Value::Empty], vec![n(3.0)]]);
    let clean = table(r#"Table.RemoveRowsWithErrors(Table.TransformColumnTypes(#table({"A"}, {{"1"}, {"x"}}), {{"A", type number}}))"#);
    assert_eq!(clean.rows, vec![vec![n(1.0)]]);
    let fixed = table(r#"Table.ReplaceErrorValues(Table.TransformColumnTypes(#table({"A"}, {{"x"}}), {{"A", type number}}), {{"A", 0}})"#);
    assert_eq!(fixed.rows, vec![vec![n(0.0)]]);
    // An error in an added column stays in its row.
    let c = table(r#"Table.AddColumn(#table({"A"}, {{1}, {0}}), "B", each if [A] = 0 then error "zero" else 1 / [A])"#);
    assert_eq!(c.rows, vec![vec![n(1.0), n(1.0)], vec![n(0.0), Value::Empty]]);
    // German number text with the culture argument.
    let de = table(r#"Table.TransformColumnTypes(#table({"A"}, {{"1.234,5"}}), {{"A", type number}}, "de-DE")"#);
    assert_eq!(de.rows, vec![vec![n(1234.5)]]);
}

#[test]
fn queries_refer_to_each_other() {
    let mut wb = workbook();
    let q = |name: &str, formula: &str| ExcelQuery { name: name.into(), formula: formula.into(), ..Default::default() };
    wb.excel_queries = vec![
        q("Base", "let Source = Excel.CurrentWorkbook(){[Name=\"Rates\"]}[Content] in Source"),
        q("Top", "Table.FirstN(Table.Sort(Base, {{\"Rate\", Order.Descending}}), 1)"),
        q("Loop", "Loop2"),
        q("Loop2", "Loop"),
        q("Broken", "let a = in a"),
    ];
    let fake = Fake::default();
    let mut cache = FetchCache::default();
    let mut cx = EvalCx::new(&wb, &fake, Ok(()), &mut cache);
    let names: Vec<String> = ["Top", "Base", "Loop", "Broken"].iter().map(|s| s.to_string()).collect();
    let r = evaluate_excel_queries(&mut cx, &names);
    assert_eq!(r[0].as_ref().unwrap().table.rows, vec![vec![t("West"), n(0.3)]]);
    assert_eq!(r[1].as_ref().unwrap().table.rows.len(), 3);
    assert!(r[2].as_ref().unwrap_err().contains("cyclic"), "{:?}", r[2]);
    assert!(r[3].as_ref().unwrap_err().contains("SyntaxError"), "{:?}", r[3]);
}

#[test]
fn unsupported_names_are_listed_without_running() {
    let f = r#"let
    Source = Excel.CurrentWorkbook(){[Name="Sales"]}[Content],
    a = Table.AddColumn(Source, "P", each Table.Profile([Region]) & Text.Upper([Region])),
    b = Table.Buffer(Other),
    c = (x) => x + y,
    d = Sql.Database("srv", "db")
in
    d"#;
    let found = unsupported(f, &|n| n == "Other").unwrap();
    assert_eq!(found, vec!["Sql.Database", "Table.Profile", "y"]);
    assert!(unsupported("let a = in a", &|_| false).is_err());
}

#[test]
fn every_library_function_dispatches() {
    let wb = workbook();
    let fake = Fake::default();
    for (name, min, max) in FUNCTIONS {
        for count in [*min, *max] {
            let args = vec!["null"; count].join(", ");
            let f = format!("{name}({args})");
            if let Err(e) = run_with(&wb, &fake, Ok(()), &f) {
                assert!(!e.message.contains("isn't supported"), "{f}: {e}");
            }
        }
        // One argument too many is refused politely.
        let args = vec!["null"; max + 1].join(", ");
        let e = run_with(&wb, &fake, Ok(()), &format!("{name}({args})")).unwrap_err();
        assert!(e.message.contains("arguments were passed"), "{name}: {e}");
    }
}

#[test]
fn hostile_formulas_end_in_errors() {
    let cases = [
        "let f = (n) => @f(n + 1) in f(0)",
        "{1..1000000000000}",
        "List.Numbers(0, 1e15)",
        "let x = {1..100000} in Table.Transpose(Table.FromColumns({x}))",
        "#date(99999, 1, 1)",
        "#date(2026, 2, 28) + #duration(1e300, 0, 0, 0)",
        "Number.Round(1e308, 15) + Number.Round(1, -400)",
        "Table.FromRows({{1, 2}}, {\"A\"})",
        "Table.Group(#table({\"A\"}, {{1}}), \"A\", {\"x\"})",
        "Table.SplitColumn(#table({\"A\"}, {{\"a\"}}), \"A\", Splitter.SplitTextByDelimiter(\"\"))",
        "Text.Middle(\"abc\", 1e300)",
        "Json.Document(Text.Repeat(\"[\", 300))",
        "Json.Document(Text.Combine(List.Transform({1..300}, each \"[\")) & Text.Combine(List.Transform({1..300}, each \"]\")))",
        "Binary.Decompress(Binary.FromText(\"AAAA\"), Compression.Deflate)",
        "Binary.Decompress(Binary.FromText(\"H4sIAAAAAAAA\"), Compression.GZip)",
        "Excel.Workbook(#binary({1, 2, 3}))",
        "Table.ExpandTableColumn(#table({\"A\"}, {{1}}), \"A\", {\"x\"})",
        "Table.Sort(#table({\"A\"}, {{1}}), (a, b) => 0)",
        "[a = b, b = a][a]",
        "Record.Field([], \"x\")",
        "List.Max({1, \"a\"})",
        "Table.NestedJoin(#table({\"A\"}, {}), {\"A\", \"A\"}, #table({\"A\"}, {}), {\"A\"}, \"N\")",
        "Table.AddColumn(#table({\"A\"}, {}), \"A\", each 1)",
        "Table.RenameColumns(#table({\"A\", \"B\"}, {}), {\"A\", \"B\"})",
    ];
    for f in cases {
        // Any outcome but a panic; the ones that must fail do.
        let _ = run(f);
    }
    assert!(run("let f = (n) => @f(n + 1) in f(0)").unwrap_err().message.contains("too deeply"));
    assert!(run("{1..1000000000000}").is_err());
    assert!(run("Table.Group(#table({\"A\"}, {{1}}), \"A\", {\"x\"})").is_err());
    assert!(run("Table.RenameColumns(#table({\"A\", \"B\"}, {}), {\"A\", \"B\"})").unwrap_err().message.contains("already exists"));
    // A million rows of 16,000 columns is refused before it takes the machine's memory, and
    // `try` doesn't catch the limit.
    let wide = "Table.FromColumns(List.Repeat({{1..1000000}}, 16000))";
    assert_eq!(run(wide).unwrap_err().reason, super::eval::LIMIT);
    assert!(run(&format!("try Table.RowCount({wide}) otherwise 0")).is_err());
    let split = "Table.SplitColumn(Table.FromColumns({{1..1000000}}), \"Column1\", each {}, 16000)";
    assert_eq!(run(split).unwrap_err().reason, super::eval::LIMIT);
    // A pivot is refused while its groups are counted, before each one's cells are allocated.
    let pivot = "Table.Pivot(Table.FromColumns({{1..5000}, {1..5000}, {1..5000}}), List.Transform({1..16000}, Text.From), \"Column2\", \"Column3\")";
    assert_eq!(run(pivot).unwrap_err().reason, super::eval::LIMIT);
    // Deep nesting is refused by the parser (no stack overflow).
    assert!(parser::parse(&format!("{}1{}", "(".repeat(5000), ")".repeat(5000))).is_err());
    let _ = run(&format!("{}1{}", "{".repeat(150), "}".repeat(150)));
    let _ = run(&format!("{}1", "-".repeat(150)));
}

#[test]
fn random_formulas_never_panic() {
    let pieces = [
        "let",
        "in",
        "each",
        "if",
        "then",
        "else",
        "try",
        "otherwise",
        "error",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        ",",
        "=",
        "<>",
        "+",
        "-",
        "*",
        "/",
        "&",
        "??",
        "..",
        "=>",
        "and",
        "or",
        "not",
        "null",
        "true",
        "1",
        "0",
        "-1",
        "2.5",
        "\"a\"",
        "\"\"",
        "_",
        "x",
        "#\"y z\"",
        "Table.FromRows",
        "Table.AddColumn",
        "Table.SelectRows",
        "List.Sum",
        "Text.Upper",
        "#table",
        "#date",
        "Table.Group",
        "type",
        "text",
        "number",
        "is",
        "as",
        "meta",
        "@",
        "?",
        "Excel.CurrentWorkbook()",
        "{\"A\"}",
        "[A]",
        "[A=1]",
        "Table.Pivot",
        "Table.Transpose",
        "Table.ExpandTableColumn",
    ];
    let mut seed: u64 = std::env::var("GRIDCRAFT_FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0x9e37_79b9_7f4a_7c15);
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let wb = workbook();
    let fake = Fake::default();
    for _ in 0..3000 {
        let len = 1 + (next() % 24) as usize;
        let f: Vec<&str> = (0..len).map(|_| pieces[(next() % pieces.len() as u64) as usize]).collect();
        let f = f.join(" ");
        let _ = run_with(&wb, &fake, Ok(()), &f);
        let _ = unsupported(&f, &|_| false);
    }
}

#[test]
fn loaded_values_are_worksheet_values() {
    let r = table(
        r#"#table({"D", "DT", "T", "L", "N", "X"}, {{#date(2026, 1, 31), #datetime(2026, 1, 31, 12, 0, 0), #time(6, 0, 0), true, -0.5, {1}}})"#,
    );
    assert_eq!(r.rows[0], vec![n(46053.0), n(46053.5), n(0.25), Value::Bool(true), n(-0.5), t("List")]);
    assert_eq!(&r.types[..3], &[ColType::Date, ColType::DateTime, ColType::Time]);
}
