//! Get & Transform: sources through a fake data access, every applied step, loading, refresh,
//! the trust prompt and WEBSERVICE.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gridcraft_core::{CellError, CellRef, Value};
use serde_json::{Value as Json, json};

use super::access::{DataAccess, DbTable, FileEntry, HttpRequest, HttpResponse};
use crate::Session;

/// Files and web responses held in memory.
#[derive(Default)]
struct Fake {
    files: Mutex<HashMap<String, Vec<u8>>>,
    web: Mutex<HashMap<String, (u16, String, Vec<u8>)>>,
    gets: AtomicUsize,
}

impl Fake {
    fn file(&self, path: &str, body: &[u8]) {
        self.files.lock().unwrap().insert(path.to_string(), body.to_vec());
    }
    fn page(&self, url: &str, ctype: &str, body: &str) {
        self.web.lock().unwrap().insert(url.to_string(), (200, ctype.to_string(), body.as_bytes().to_vec()));
    }
}

impl DataAccess for Fake {
    fn read_file(&self, path: &str, max: usize) -> Result<Vec<u8>, String> {
        let f = self.files.lock().unwrap().get(path).cloned().ok_or_else(|| format!("Couldn't open '{path}'"))?;
        if f.len() > max { Err("too large".into()) } else { Ok(f) }
    }
    fn list_folder(&self, path: &str, max: usize) -> Result<Vec<FileEntry>, String> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let files = self.files.lock().unwrap();
        let mut names: Vec<&String> = files.keys().filter(|k| k.starts_with(&prefix)).collect();
        names.sort();
        Ok(names
            .into_iter()
            .take(max)
            .map(|k| {
                let name = k.trim_start_matches(&prefix).to_string();
                let extension = name.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
                FileEntry {
                    size: files.get(k).map_or(0, |b| b.len() as u64),
                    name,
                    extension,
                    folder: prefix.clone(),
                    modified: Some(46000.5),
                    ..Default::default()
                }
            })
            .collect())
    }
    fn web_available(&self) -> Result<(), String> {
        Ok(())
    }
    fn odbc_available(&self) -> Result<(), String> {
        Ok(())
    }
    fn odbc_tables(&self, connection: &str) -> Result<Vec<String>, String> {
        if connection.contains("Nope") {
            Err("Couldn't connect to the data source.".into())
        } else {
            Ok(vec!["dbo.Orders".into(), "Customers".into()])
        }
    }
    fn odbc_table(&self, _connection: &str, table: &str, max_rows: usize) -> Result<DbTable, String> {
        if table != "dbo.Orders" {
            return Err(format!("unexpected table {table}"));
        }
        let rows = vec![
            vec![Value::number(1.0), Value::text("2026-01-05"), Value::number(9.5)],
            vec![Value::number(2.0), Value::text("2026-02-06"), Value::Empty],
        ];
        Ok(DbTable { columns: vec!["Id".into(), "Ordered".into(), "Amount".into()], rows: rows.into_iter().take(max_rows).collect() })
    }
    fn odbc_query(&self, _connection: &str, sql: &str, _max_rows: usize) -> Result<DbTable, String> {
        if !sql.to_ascii_uppercase().starts_with("SELECT") {
            return Err("The query failed.".into());
        }
        Ok(DbTable { columns: vec!["N".into(), "N".into()], rows: vec![vec![Value::number(7.0), Value::text("x")]] })
    }
    fn http_get(&self, req: &HttpRequest) -> Result<HttpResponse, String> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        let (status, ctype, body) = self.web.lock().unwrap().get(&req.url).cloned().ok_or_else(|| format!("Couldn't reach {}", req.url))?;
        if body.len() > req.max_bytes {
            return Err("too large".into());
        }
        Ok(HttpResponse { status, content_type: Some(ctype), body })
    }
}

const SALES: &str = "Region,Units,Price,Date\nNorth,10,2.5,2026-01-05\nSouth,7,3,2026-01-06\nNorth,3,2.5,2026-02-01\n,,,\nEast,4,1,2026-03-01\n";

fn session(fake: &Arc<Fake>) -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_data_access(fake.clone());
    s
}

fn cell(s: &Session, sheet: &str, a1: &str) -> Value {
    let d = s.doc().unwrap();
    let si = d.wb.sheet_index(sheet).unwrap_or_else(|| panic!("no sheet {sheet}"));
    d.wb.sheet(si).unwrap().value(CellRef::parse(a1).unwrap())
}

/// Defines a query over inline CSV text (no external access) and returns its preview after all
/// steps.
fn preview(s: &mut Session, text: &str, steps: Json) -> Json {
    let q = json!({"name": "T", "source": {"kind": "text", "text": text}, "steps": steps});
    s.execute("query.preview", json!({"query": q})).unwrap()
}

fn rows(p: &Json) -> Vec<Vec<Json>> {
    p["rows"].as_array().unwrap().iter().map(|r| r.as_array().unwrap().clone()).collect()
}

fn cols(p: &Json) -> Vec<String> {
    p["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect()
}

#[test]
fn csv_query_promotes_headers_detects_types_and_loads_a_table() {
    let fake = Arc::new(Fake::default());
    fake.file("C:/data/sales.csv", SALES.as_bytes());
    let mut s = session(&fake);
    let r = s.execute("data.fromTextCsv", json!({"path": "C:/data/sales.csv"})).unwrap();
    assert_eq!(r["query"], "sales");
    assert_eq!(r["sheet"], "sales");
    assert_eq!(r["table"], "sales");
    let d = s.doc().unwrap();
    let q = &d.wb.queries[0];
    assert_eq!(q.steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Promoted Headers", "Changed Type"]);
    assert_eq!(cell(&s, "sales", "A1"), Value::text("Region"));
    assert_eq!(cell(&s, "sales", "B2"), Value::number(10.0));
    // The blank line is a row of nulls (Remove Blank Rows would drop it).
    assert_eq!(cell(&s, "sales", "B5"), Value::Empty);
    assert_eq!(cell(&s, "sales", "B6"), Value::number(4.0));
    assert_eq!(cell(&s, "sales", "C2"), Value::number(2.5));
    // Dates are serials with a date format.
    let d = s.doc().unwrap();
    let sh = d.wb.sheet(d.wb.sheet_index("sales").unwrap()).unwrap();
    assert_eq!(sh.value(CellRef::parse("D2").unwrap()), Value::number(46027.0));
    assert!(d.wb.styles.get(sh.style_id(CellRef::parse("D2").unwrap())).num_fmt.as_str().contains('y'));
    assert_eq!(sh.tables[0].range.a1(), "A1:D6");
    assert_eq!(d.wb.queries[0].status.rows, Some(5));
    // Recent Sources remembers the file; the M text shows the steps.
    assert_eq!(s.prefs.recent_sources, vec!["C:/data/sales.csv"]);
    let m = s.execute("query.mText", json!({"name": "sales"})).unwrap();
    assert!(m["m"].as_str().unwrap().contains("Table.PromoteHeaders(Source"), "{m}");
    // One undo removes the query, the sheet and the table.
    s.execute("edit.undo", json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.wb.queries.is_empty() && d.wb.sheet_index("sales").is_none());
}

#[test]
fn every_transform_step() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    let data = "Name,City,Sales\nAnn,Berlin,10\nBob,Paris,\nAnn,Berlin,10\nCy,,5\n,,\n";
    let base = json!([{"op": "promoteHeaders"}, {"op": "changeType", "columns": [{"column": "Sales", "type": "number"}]}]);
    let with = |extra: Json| {
        let mut steps = base.as_array().unwrap().clone();
        steps.extend(extra.as_array().unwrap().clone());
        Json::Array(steps)
    };
    // Remove / choose / rename / reorder / duplicate columns.
    let p = preview(&mut s, data, with(json!([{"op": "removeColumns", "columns": ["City"]}])));
    assert_eq!(cols(&p), vec!["Name", "Sales"]);
    let p = preview(&mut s, data, with(json!([{"op": "chooseColumns", "columns": ["Sales", "Name"]}])));
    assert_eq!(cols(&p), vec!["Sales", "Name"]);
    let p = preview(&mut s, data, with(json!([{"op": "renameColumns", "renames": [{"from": "City", "to": "Town"}]}])));
    assert_eq!(cols(&p), vec!["Name", "Town", "Sales"]);
    let p = preview(&mut s, data, with(json!([{"op": "renameColumns", "renames": [{"from": "City", "to": "Name"}]}])));
    assert!(p["error"].as_str().unwrap().contains("already exists"), "{p}");
    let p = preview(&mut s, data, with(json!([{"op": "reorderColumns", "columns": ["Sales"]}])));
    assert_eq!(cols(&p), vec!["Sales", "Name", "City"]);
    let p = preview(&mut s, data, with(json!([{"op": "duplicateColumn", "column": "Name"}])));
    assert_eq!(cols(&p)[3], "Name - Copy");
    let p = preview(&mut s, data, with(json!([{"op": "removeColumns", "columns": ["Nope"]}])));
    assert_eq!(p["error"], "Removed Columns: The column 'Nope' of the table wasn't found.");
    // Keep / remove rows.
    assert_eq!(preview(&mut s, data, with(json!([{"op": "keepTopRows", "count": 2}])))["rowCount"], 2);
    assert_eq!(rows(&preview(&mut s, data, with(json!([{"op": "keepBottomRows", "count": 1}]))))[0][0], "");
    assert_eq!(rows(&preview(&mut s, data, with(json!([{"op": "keepRange", "first": 2, "count": 1}]))))[0][0], "Bob");
    assert_eq!(preview(&mut s, data, with(json!([{"op": "removeTopRows", "count": 4}])))["rowCount"], 1);
    assert_eq!(preview(&mut s, data, with(json!([{"op": "removeBottomRows", "count": 99}])))["rowCount"], 0);
    assert_eq!(preview(&mut s, data, with(json!([{"op": "removeDuplicates"}])))["rowCount"], 4);
    assert_eq!(preview(&mut s, data, with(json!([{"op": "removeDuplicates", "columns": ["Name"]}])))["rowCount"], 4);
    assert_eq!(preview(&mut s, data, with(json!([{"op": "removeBlankRows"}])))["rowCount"], 4);
    let errs = "A\nx\n1\n";
    let p = preview(
        &mut s,
        errs,
        json!([{"op": "promoteHeaders"}, {"op": "changeType", "columns": [{"column": "A", "type": "whole"}]}, {"op": "removeErrors"}]),
    );
    assert_eq!(rows(&p), vec![vec![json!(1.0)]]);
    // Filter: a value list (with null), conditions joined by and / or.
    let p = preview(&mut s, data, with(json!([{"op": "filterRows", "column": "Name", "values": ["Ann", null]}])));
    assert_eq!(p["rowCount"], 3);
    let p = preview(&mut s, data, with(json!([{"op": "filterRows", "column": "Sales", "conditions": [{"op": "greaterThan", "value": 6}]}])));
    assert_eq!(p["rowCount"], 2);
    let p = preview(
        &mut s,
        data,
        with(
            json!([{"op": "filterRows", "column": "City", "conditions": [{"op": "beginsWith", "value": "Be"}, {"op": "equals", "value": "Paris"}], "or": true}]),
        ),
    );
    assert_eq!(p["rowCount"], 3);
    let p = preview(&mut s, data, with(json!([{"op": "filterRows", "column": "Sales", "conditions": [{"op": "isNull"}]}])));
    assert_eq!(p["rowCount"], 2);
    // Sort: null first ascending; descending on a second key.
    let p = preview(&mut s, data, with(json!([{"op": "sort", "keys": [{"column": "Name"}, {"column": "Sales", "descending": true}]}])));
    assert_eq!(rows(&p).iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![json!(""), json!("Ann"), json!("Ann"), json!("Bob"), json!("Cy")]);
    // Replace text inside values, and whole values.
    let p = preview(&mut s, data, with(json!([{"op": "replaceValues", "columns": ["City"], "find": "er", "replace": "ER"}])));
    assert_eq!(rows(&p)[0][1], "BERlin");
    let p = preview(&mut s, data, with(json!([{"op": "replaceValues", "columns": ["Sales"], "find": null, "replace": 0}])));
    assert_eq!(rows(&p)[1][2], 0.0);
    // Split by delimiter (each, left-most, right-most).
    let parts = "Full\na-b-c\nd\n";
    let p = preview(&mut s, parts, json!([{"op": "promoteHeaders"}, {"op": "splitColumn", "column": "Full", "delimiter": "-"}]));
    assert_eq!(cols(&p), vec!["Full.1", "Full.2", "Full.3"]);
    assert_eq!(rows(&p)[1], vec![json!("d"), Json::Null, Json::Null]);
    let p = preview(&mut s, parts, json!([{"op": "promoteHeaders"}, {"op": "splitColumn", "column": "Full", "delimiter": "-", "mode": "rightMost"}]));
    assert_eq!(rows(&p)[0], vec![json!("a-b"), json!("c")]);
    let p = preview(&mut s, parts, json!([{"op": "promoteHeaders"}, {"op": "splitColumn", "column": "Full", "delimiter": "-", "mode": "leftMost"}]));
    assert_eq!(rows(&p)[0], vec![json!("a"), json!("b-c")]);
    // Text transforms.
    let words = "T\n  hello WORLD  \n";
    for (f, want) in [("upper", "  HELLO WORLD  "), ("lower", "  hello world  "), ("proper", "  Hello World  "), ("trim", "hello WORLD")] {
        let p = preview(&mut s, words, json!([{"op": "promoteHeaders"}, {"op": "transformText", "columns": ["T"], "function": f}]));
        assert_eq!(rows(&p)[0][0], want, "{f}");
    }
    let p = preview(&mut s, "T\n\"a\u{7}b\"\n", json!([{"op": "promoteHeaders"}, {"op": "transformText", "columns": ["T"], "function": "clean"}]));
    assert_eq!(rows(&p)[0][0], "ab");
    // Fill down / up.
    let gaps = "A\n1\n\n\n2\n";
    let p = preview(
        &mut s,
        gaps,
        json!([{"op": "promoteHeaders"}, {"op": "replaceValues", "columns": ["A"], "find": "", "replace": null, "whole": true}, {"op": "fillDown", "columns": ["A"]}]),
    );
    assert_eq!(rows(&p).iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![json!("1"), json!("1"), json!("1"), json!("2")]);
    let p = preview(
        &mut s,
        gaps,
        json!([{"op": "promoteHeaders"}, {"op": "replaceValues", "columns": ["A"], "find": "", "replace": null, "whole": true}, {"op": "fillUp", "columns": ["A"]}]),
    );
    assert_eq!(rows(&p)[1][0], "2");
    // Group By.
    let p = preview(
        &mut s,
        data,
        with(json!([{"op": "removeBlankRows"}, {"op": "groupBy", "keys": ["Name"], "aggregations": [
            {"name": "Rows", "op": "count"}, {"name": "Total", "op": "sum", "column": "Sales"},
            {"name": "Avg", "op": "average", "column": "Sales"}, {"name": "Max", "op": "max", "column": "Sales"},
            {"name": "Cities", "op": "countDistinct", "column": "City"}]}])),
    );
    assert_eq!(cols(&p), vec!["Name", "Rows", "Total", "Avg", "Max", "Cities"]);
    assert_eq!(rows(&p)[0], vec![json!("Ann"), json!(2.0), json!(20.0), json!(10.0), json!(10.0), json!(1.0)]);
    // Unpivot drops nulls; transpose; demote headers.
    let wide = "Id,Q1,Q2\n1,5,\n2,6,7\n";
    let p = preview(
        &mut s,
        wide,
        json!([{"op": "promoteHeaders"}, {"op": "changeType", "columns": [{"column": "Q1", "type": "whole"}, {"column": "Q2", "type": "whole"}]}, {"op": "unpivot", "columns": ["Q1", "Q2"]}]),
    );
    assert_eq!(cols(&p), vec!["Id", "Attribute", "Value"]);
    assert_eq!(p["rowCount"], 3);
    let p = preview(&mut s, wide, json!([{"op": "promoteHeaders"}, {"op": "transpose"}]));
    assert_eq!(cols(&p), vec!["Column1", "Column2"]);
    assert_eq!(rows(&p)[1], vec![json!("5"), json!("6")]);
    let p = preview(&mut s, wide, json!([{"op": "promoteHeaders"}, {"op": "demoteHeaders"}]));
    assert_eq!(rows(&p)[0][1], "Q1");
    // Index and conditional columns.
    let p = preview(&mut s, data, with(json!([{"op": "addIndexColumn", "start": 1}])));
    assert_eq!(rows(&p).iter().map(|r| r[3].clone()).collect::<Vec<_>>(), vec![json!(1.0), json!(2.0), json!(3.0), json!(4.0), json!(5.0)]);
    let p = preview(
        &mut s,
        data,
        with(json!([{"op": "addConditionalColumn", "newColumn": "Size", "rules": [
            {"column": "Sales", "op": "greaterOrEqual", "value": 10, "output": {"value": "big"}},
            {"column": "City", "op": "equals", "value": "Paris", "output": {"column": "Name"}}],
            "otherwise": {"value": "small"}}])),
    );
    assert_eq!(
        rows(&p).iter().map(|r| r[3].clone()).collect::<Vec<_>>(),
        vec![json!("big"), json!("Bob"), json!("big"), json!("small"), json!("small")]
    );
    // Change type with a locale.
    let de = "N;D\n1.234,5;31.12.2026\n";
    let p = preview(
        &mut s,
        de,
        json!([{"op": "promoteHeaders"}, {"op": "changeType", "locale": "de-DE", "columns": [{"column": "N", "type": "number"}, {"column": "D", "type": "date"}]}]),
    );
    assert_eq!(rows(&p)[0], vec![json!(1234.5), json!(46387.0)]);
    // Date filters take dates as typed.
    let dated = "D\n2026-01-05\n2026-02-01\n3/1/2026\n";
    let p = preview(
        &mut s,
        dated,
        json!([{"op": "promoteHeaders"}, {"op": "changeType", "columns": [{"column": "D", "type": "date"}]}, {"op": "filterRows", "column": "D", "conditions": [{"op": "greaterThan", "value": "2026-01-31"}]}]),
    );
    assert_eq!(p["rowCount"], 2);
    // Preview of an earlier step, with its formula.
    let q = json!({"name": "T", "source": {"kind": "text", "text": data}, "steps": base});
    let p = s.execute("query.preview", json!({"query": q, "step": 0})).unwrap();
    assert_eq!(cols(&p), vec!["Column1", "Column2", "Column3"]);
    assert!(p["formula"].as_str().unwrap().starts_with("= Csv.Document("));
}

#[test]
fn merge_and_append_queries() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    let left = json!({"name": "Orders", "source": {"kind": "text", "text": "Id,Cust\n1,a\n2,b\n3,z\n"}, "steps": [{"op": "promoteHeaders"}], "load": {"to": "connectionOnly"}});
    let right = json!({"name": "Customers", "source": {"kind": "text", "text": "Cust,Name\na,Ann\nb,Bob\nc,Cy\n"}, "steps": [{"op": "promoteHeaders"}], "load": {"to": "connectionOnly"}});
    s.execute("query.set", json!({"query": left})).unwrap();
    s.execute("query.set", json!({"query": right})).unwrap();
    let count = |s: &mut Session, join: &str| {
        let q = json!({"name": "M", "source": {"kind": "query", "name": "Orders"}, "steps": [{"op": "mergeQueries", "query": "Customers", "left": ["Cust"], "right": ["Cust"], "join": join, "columns": ["Name"], "prefix": "Customers"}]});
        s.execute("query.preview", json!({"query": q})).unwrap()
    };
    let p = count(&mut s, "leftOuter");
    assert_eq!(cols(&p), vec!["Id", "Cust", "Customers.Name"]);
    assert_eq!(rows(&p)[2], vec![json!("3"), json!("z"), Json::Null]);
    assert_eq!(count(&mut s, "inner")["rowCount"], 2);
    assert_eq!(count(&mut s, "rightOuter")["rowCount"], 3);
    assert_eq!(count(&mut s, "fullOuter")["rowCount"], 4);
    assert_eq!(count(&mut s, "leftAnti")["rowCount"], 1);
    assert_eq!(count(&mut s, "rightAnti")["rowCount"], 1);
    // Combine Queries › Merge and Append create new loaded queries.
    let r = s
        .execute(
            "data.mergeQueries",
            json!({"left": "Orders", "right": "Customers", "leftColumns": ["Cust"], "rightColumns": ["Cust"], "join": "inner"}),
        )
        .unwrap();
    assert_eq!(r["rows"], 2);
    let r = s.execute("data.appendQueries", json!({"queries": ["Orders", "Customers"]})).unwrap();
    assert_eq!(r["rows"], 6);
    assert_eq!(r["cols"], 3);
    // A query that refers to itself is an error, not a hang.
    s.execute("query.set", json!({"query": {"name": "Loop", "source": {"kind": "query", "name": "Loop"}}})).unwrap();
    let p = s.execute("query.preview", json!({"name": "Loop"})).unwrap();
    assert!(p["error"].as_str().unwrap().contains("cyclic"), "{p}");
    // Renaming a query renames references to it.
    s.execute("query.rename", json!({"name": "Customers", "newName": "Clients"})).unwrap();
    let d = s.doc().unwrap();
    let m = d.wb.queries.iter().find(|q| q.name == "Merge1").unwrap();
    assert!(matches!(&m.steps[0].op, gridcraft_model::StepOp::MergeQueries { query, .. } if query == "Clients"));
}

#[test]
fn applied_steps_can_be_added_removed_renamed_and_moved() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    s.execute("data.fromTextCsv", json!({"text": "A,B\n1,2\n3,4\n", "name": "Q"})).unwrap();
    s.execute("query.addStep", json!({"name": "Q", "step": {"op": "removeColumns", "columns": ["B"]}})).unwrap();
    s.execute("query.addStep", json!({"name": "Q", "step": {"op": "removeColumns", "columns": ["A"]}, "at": 3})).unwrap();
    let names = |s: &Session| s.doc().unwrap().wb.queries[0].steps.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), vec!["Promoted Headers", "Changed Type", "Removed Columns1", "Removed Columns"]);
    s.execute("query.renameStep", json!({"name": "Q", "step": "Removed Columns1", "newName": "Drop A"})).unwrap();
    assert!(s.execute("query.renameStep", json!({"name": "Q", "step": 1, "newName": "Drop A"})).is_err());
    s.execute("query.moveStep", json!({"name": "Q", "step": "Drop A", "to": 4})).unwrap();
    assert_eq!(names(&s)[3], "Drop A");
    s.execute("query.removeStep", json!({"name": "Q", "step": 4})).unwrap();
    assert_eq!(names(&s).len(), 3);
    assert!(s.execute("query.removeStep", json!({"name": "Q", "step": 9})).is_err());
    // Refresh rewrites the loaded table with the new steps.
    s.execute("data.refresh", json!({"query": "Q"})).unwrap();
    assert_eq!(cell(&s, "Q", "A1"), Value::text("A"));
    assert_eq!(cell(&s, "Q", "B1"), Value::Empty);
    let g = s.execute("query.get", json!({"name": "Q"})).unwrap();
    assert_eq!(g["steps"], json!(["Source", "Promoted Headers", "Changed Type", "Removed Columns"]));
}

#[test]
fn refresh_is_deterministic_and_picks_up_changes() {
    let fake = Arc::new(Fake::default());
    fake.file("/d/a.csv", b"K,V\nx,1\ny,2\n");
    let mut s = session(&fake);
    s.execute("data.fromTextCsv", json!({"path": "/d/a.csv"})).unwrap();
    s.execute("query.addStep", json!({"name": "a", "step": {"op": "sort", "keys": [{"column": "V", "descending": true}]}})).unwrap();
    s.execute("data.refreshAll", json!({})).unwrap();
    let first = s.doc().unwrap().wb.clone();
    s.execute("data.refreshAll", json!({})).unwrap();
    let second = s.doc().unwrap().wb.clone();
    assert_eq!(first.sheets, second.sheets);
    assert_eq!(first.queries, second.queries);
    // The source grows: refresh reads it again and the table grows with it.
    fake.file("/d/a.csv", b"K,V\nx,1\ny,2\nz,3\n");
    let r = s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(r["queries"][0]["rows"], 3);
    assert_eq!(cell(&s, "a", "A2"), Value::text("z"));
    assert_eq!(s.doc().unwrap().wb.table("a").map(|(si, ti)| s.doc().unwrap().wb.sheets[si].tables[ti].range.a1()), Some("A1:B4".into()));
    // A broken source keeps the old table and records the error.
    fake.files.lock().unwrap().clear();
    let r = s.execute("data.refresh", json!({"query": "a"})).unwrap();
    assert!(r["queries"][0]["error"].as_str().unwrap().contains("Couldn't open"), "{r}");
    assert_eq!(cell(&s, "a", "A2"), Value::text("z"));
    assert!(s.doc().unwrap().wb.queries[0].status.error.is_some());
    // Refresh is undoable.
    let before = s.doc().unwrap().undo.len();
    fake.file("/d/a.csv", b"K,V\nq,9\n");
    s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(cell(&s, "a", "A2"), Value::text("q"));
    assert_eq!(s.doc().unwrap().undo.len(), before + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(cell(&s, "a", "A2"), Value::text("z"));
}

#[test]
fn load_targets() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    // An existing sheet at a cell.
    let r = s.execute("data.fromTextCsv", json!({"text": "A\n1\n2\n", "name": "Here", "sheet": "Sheet1", "cell": "C3"})).unwrap();
    assert_eq!(r["range"], "C3:C5");
    assert_eq!(cell(&s, "Sheet1", "C4"), Value::number(1.0));
    // Only Create Connection loads nothing.
    let r = s.execute("data.fromTextCsv", json!({"text": "A\n1\n", "name": "Conn", "load": "connectionOnly"})).unwrap();
    assert!(r.get("table").is_none());
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    // Load To a new sheet later; then back to connection only removes the table.
    s.execute("query.load", json!({"name": "Conn", "to": "table", "newSheet": true})).unwrap();
    assert_eq!(cell(&s, "Conn", "A2"), Value::number(1.0));
    s.execute("query.load", json!({"name": "Conn", "to": "connectionOnly"})).unwrap();
    assert!(s.doc().unwrap().wb.table("Conn").is_none());
    assert_eq!(cell(&s, "Conn", "A2"), Value::Empty);
    // Data in the way is never overwritten.
    s.execute("cell.set", json!({"cell": "Sheet1!E1", "input": "keep"})).unwrap();
    let r = s.execute("data.fromTextCsv", json!({"text": "A\n1\n", "name": "Blocked", "sheet": "Sheet1", "cell": "E1"}));
    assert!(r.unwrap_err().to_string().contains("overwrite"));
    assert_eq!(cell(&s, "Sheet1", "E1"), Value::text("keep"));
    // Data › Properties works on the query table at the active cell.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("selection.set", json!({"range": "C4"})).unwrap();
    let p = s.execute("data.properties", json!({"refreshOnOpen": true, "everyMinutes": 5})).unwrap();
    assert_eq!(p["query"], "Here");
    assert_eq!(p["refreshOnOpen"], true);
    assert_eq!(s.doc().unwrap().wb.queries[0].refresh.every_minutes, 5);
    // A query not refreshed in this session starts its interval; it's due once it has passed.
    assert!(
        crate::cmd::query::due_refreshes(&mut s, 1e12).is_empty()
            || crate::cmd::query::due_refreshes(&mut s, 1e12 + 300_001.0) == vec!["Here".to_string()]
    );
}

#[test]
fn a_failed_load_keeps_the_old_table_and_table_renames_follow() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    s.execute("data.fromTextCsv", json!({"text": "A\n1\n", "name": "Q", "sheet": "Sheet1", "cell": "A1"})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!A4", "input": "keep"})).unwrap();
    // The source grows past the cell that holds data: the refresh fails, nothing is overwritten
    // and the old table stays.
    s.execute(
        "query.set",
        json!({"query": {"name": "Q", "source": {"kind": "text", "text": "A\n1\n2\n3\n4\n"}, "steps": [{"op": "promoteHeaders"}]}}),
    )
    .unwrap();
    let r = s.execute("data.refresh", json!({"query": "Q"})).unwrap();
    assert!(r["queries"][0]["error"].as_str().unwrap().contains("overwrite"), "{r}");
    assert_eq!(cell(&s, "Sheet1", "A2"), Value::number(1.0));
    assert_eq!(cell(&s, "Sheet1", "A4"), Value::text("keep"));
    assert!(s.doc().unwrap().wb.table("Q").is_some());
    // Renaming the table keeps it the query's table.
    s.execute("table.rename", json!({"table": "Q", "name": "Renamed"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.queries[0].load.table.as_deref(), Some("Renamed"));
    s.execute("cell.set", json!({"cell": "Sheet1!A4", "input": ""})).unwrap();
    s.execute("data.refresh", json!({"query": "Q"})).unwrap();
    assert_eq!(cell(&s, "Sheet1", "A5"), Value::text("4"));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1, "refreshed in place, no new sheet");
}

#[test]
fn table_range_and_blank_query() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    s.execute("range.setValues", json!({"range": "A1", "values": [["Fruit", "Qty"], ["Apple", 3], ["Pear", 5]]})).unwrap();
    s.execute("selection.set", json!({"range": "A2"})).unwrap();
    let r = s.execute("data.fromTableRange", json!({})).unwrap();
    // The range became Table1 and the query reads it.
    assert_eq!(r["query"], "Table1");
    assert_eq!(r["rows"], 2);
    let d = s.doc().unwrap();
    assert!(matches!(&d.wb.queries[0].source, gridcraft_model::QuerySource::CurrentWorkbook { name } if name == "Table1"));
    assert_eq!(cell(&s, "Table1", "B3"), Value::number(5.0));
    let r = s.execute("data.blankQuery", json!({})).unwrap();
    assert_eq!(r["saved"], false);
    assert_eq!(r["query"]["source"]["kind"], "blank");
}

#[test]
fn web_sources_html_json_csv_and_errors() {
    let fake = Arc::new(Fake::default());
    fake.page(
        "https://example.com/stats",
        "text/html; charset=utf-8",
        "<html><body><table><tr><td>menu</td></tr></table><table><caption>Prices</caption><tr><th>Item</th><th>Price</th></tr><tr><td>Tea</td><td>2.50</td></tr></table></body></html>",
    );
    fake.page("https://api.example.com/items", "application/json", r#"{"items":[{"id":1,"name":"A"},{"id":2,"name":"B"}]}"#);
    fake.page("https://example.com/data.csv", "text/csv", "a,b\n1,2\n");
    let mut s = session(&fake);
    let nav = s.execute("query.navigator", json!({"source": {"kind": "web", "url": "https://example.com/stats"}})).unwrap();
    assert_eq!(nav["items"][1]["name"], "Table 2");
    assert_eq!(nav["items"][1]["detail"], "Prices");
    let r = s.execute("data.fromWeb", json!({"url": "https://example.com/stats", "item": "Table 2"})).unwrap();
    assert_eq!(r["query"], "Table 2");
    assert_eq!(cell(&s, "Table 2", "B2"), Value::number(2.5));
    // The navigator and the load read the page once (cached).
    assert_eq!(fake.gets.load(Ordering::SeqCst), 1);
    let r = s.execute("data.fromWeb", json!({"url": "https://api.example.com/items"})).unwrap();
    assert_eq!(r["rows"], 2);
    let r = s.execute("data.getData", json!({"url": "https://example.com/data.csv"})).unwrap();
    assert_eq!(r["cols"], 2);
    // Errors: not http(s), unreachable, an HTTP error status.
    assert!(s.execute("data.fromWeb", json!({"url": "file:///etc/passwd"})).is_err());
    assert!(s.execute("data.fromWeb", json!({"url": "https://nowhere.example/"})).unwrap_err().to_string().contains("Couldn't reach"));
    fake.web.lock().unwrap().insert("https://example.com/gone".into(), (404, "text/html".into(), b"nope".to_vec()));
    assert!(s.execute("data.fromWeb", json!({"url": "https://example.com/gone"})).unwrap_err().to_string().contains("404"));
    // Programmatic calls never open dialogs.
    assert!(s.execute_headless("data.fromWeb", json!({})).unwrap_err().to_string().contains("missing `url`"));
    // Without web access (the default), the commands say why.
    let mut plain = Session::new();
    plain.execute("file.new", json!({})).unwrap();
    let e = plain.execute("data.fromWeb", json!({"url": "https://example.com/stats"})).unwrap_err().to_string();
    assert!(e.contains("desktop app"), "{e}");
}

#[test]
fn odata_feed_service_root_and_paging() {
    let fake = Arc::new(Fake::default());
    fake.page(
        "https://svc.example/odata",
        "application/json",
        r#"{"@odata.context":"https://svc.example/odata/$metadata","value":[{"name":"Products","kind":"EntitySet","url":"Products"}]}"#,
    );
    fake.page(
        "https://svc.example/odata/Products",
        "application/json",
        r#"{"@odata.context":"x","value":[{"@odata.etag":"1","ID":1,"Name":"Bread"}],"@odata.nextLink":"https://svc.example/odata/Products?$skip=1"}"#,
    );
    fake.page("https://svc.example/odata/Products?$skip=1", "application/json", r#"{"value":[{"ID":2,"Name":"Milk"}]}"#);
    let mut s = session(&fake);
    let nav = s.execute("query.navigator", json!({"source": {"kind": "odata", "url": "https://svc.example/odata"}})).unwrap();
    assert_eq!(nav["items"][0]["name"], "Products");
    assert!(s.execute("data.fromODataFeed", json!({"url": "https://svc.example/odata"})).is_err());
    let r = s.execute("data.fromODataFeed", json!({"url": "https://svc.example/odata", "item": "Products"})).unwrap();
    assert_eq!(r["rows"], 2);
    assert_eq!(r["cols"], 2);
    assert_eq!(cell(&s, "Products", "B3"), Value::text("Milk"));
}

#[test]
fn file_sources_folder_json_xml_and_excel() {
    let fake = Arc::new(Fake::default());
    fake.file("/in/jan.csv", b"Item,Qty\nA,1\n");
    fake.file("/in/feb.csv", b"Item,Qty\nB,2\nC,3\n");
    fake.file("/in/notes.txt", b"ignore me");
    fake.file("/x/data.json", br#"{"rows":[{"a":1,"b":{"c":"deep"}}]}"#);
    fake.file("/x/books.xml", br#"<catalog><book id="1"><title>T1</title></book><book id="2"><title>T2</title></book></catalog>"#);
    let mut s = session(&fake);
    let r = s.execute("data.fromFolder", json!({"path": "/in"})).unwrap();
    assert_eq!(r["rows"], 3);
    assert_eq!(cell(&s, "in", "A1"), Value::text("Name"));
    assert_eq!(cell(&s, "in", "F1"), Value::text("Attributes.Size"));
    assert_eq!(cell(&s, "in", "F2"), Value::number(17.0), "feb.csv's size");
    let r = s.execute("data.fromFolder", json!({"path": "/in", "combine": true, "extension": "csv", "name": "Combined"})).unwrap();
    assert_eq!(r["rows"], 3);
    assert_eq!(cell(&s, "Combined", "A1"), Value::text("Source.Name"));
    assert_eq!(cell(&s, "Combined", "A2"), Value::text("feb.csv"));
    let r = s.execute("data.fromJson", json!({"path": "/x/data.json"})).unwrap();
    assert_eq!(r["cols"], 2);
    assert_eq!(cell(&s, "data", "B1"), Value::text("b.c"));
    let r = s.execute("data.fromXml", json!({"path": "/x/books.xml"})).unwrap();
    assert_eq!(r["query"], "books");
    assert_eq!(cell(&s, "books", "B3"), Value::text("T2"));
    // An Excel workbook built in code: a sheet and a table.
    let mut other = Session::new();
    other.execute("file.new", json!({})).unwrap();
    other.execute("range.setValues", json!({"range": "A1", "values": [["P", "N"], ["x", 1], ["y", 2]]})).unwrap();
    other.execute("insert.table", json!({"range": "A1:B3", "name": "Stock"})).unwrap();
    let bytes = crate::io::save_bytes(&other.doc().unwrap().wb, "o.xlsx").unwrap();
    fake.file("/x/other.xlsx", &bytes);
    let e = s.execute("data.fromExcelWorkbook", json!({"path": "/x/other.xlsx"})).unwrap_err().to_string();
    assert!(e.contains("Stock (table)") && e.contains("Sheet1 (sheet)"), "{e}");
    let r = s.execute("data.fromExcelWorkbook", json!({"path": "/x/other.xlsx", "item": "Stock", "itemKind": "table"})).unwrap();
    assert_eq!(r["rows"], 2);
    let r = s.execute("data.fromExcelWorkbook", json!({"path": "/x/other.xlsx", "item": "Sheet1"})).unwrap();
    // The sheet's first row is promoted to headers.
    assert_eq!(r["rows"], 2);
    let d = s.doc().unwrap();
    assert_eq!(d.wb.queries.last().unwrap().steps[0].op, gridcraft_model::StepOp::PromoteHeaders);
}

#[test]
fn odbc_sql_server_and_access_sources() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    // The Navigator lists the tables; a table is read only when the connection lists it.
    let nav = s.execute("query.navigator", json!({"source": {"kind": "odbc", "connection": "DSN=Shop"}})).unwrap();
    assert_eq!(nav["items"][0]["name"], "dbo.Orders");
    let e = s.execute("data.fromOdbc", json!({"connection": "DSN=Shop"})).unwrap_err().to_string();
    assert!(e.contains("dbo.Orders"), "{e}");
    assert!(s.execute("data.fromOdbc", json!({"connection": "DSN=Shop", "table": "x; DROP TABLE y"})).is_err());
    let r = s.execute("data.fromOdbc", json!({"connection": "DSN=Shop", "table": "dbo.Orders"})).unwrap();
    assert_eq!(r["query"], "dbo.Orders");
    // Dates the driver gives as text are dates, typed like Power Query types them.
    let d = s.doc().unwrap();
    let q = d.wb.queries.last().unwrap();
    assert!(q.steps.is_empty(), "typed sources get no Changed Type step");
    let sheet = r["sheet"].as_str().unwrap().to_string();
    assert_eq!(cell(&s, &sheet, "B2"), Value::number(46027.0));
    assert_eq!(cell(&s, &sheet, "C3"), Value::Empty);
    // A SQL statement; duplicate column names become unique.
    let r = s.execute("data.fromOdbc", json!({"connection": "DSN=Shop", "sql": "SELECT 7, 'x'", "name": "Sql"})).unwrap();
    assert_eq!(r["cols"], 2);
    assert_eq!(cell(&s, "Sql", "B1"), Value::text("N_1"));
    // SQL Server and Access build their ODBC connection strings.
    assert_eq!(
        crate::cmd::query::sql_server_connection("db01;x", Some("Sales")),
        "Driver={SQL Server};Server={db01;x};Trusted_Connection=Yes;Database=Sales;"
    );
    assert_eq!(
        crate::cmd::query::access_connection("C:\\data\\shop.accdb"),
        "Driver={Microsoft Access Driver (*.mdb, *.accdb)};Dbq=C:\\data\\shop.accdb;"
    );
    let r = s.execute("data.fromSqlServer", json!({"server": "db01", "database": "Sales", "table": "dbo.Orders"})).unwrap();
    assert_eq!(r["rows"], 2);
    let r = s.execute("data.fromAccess", json!({"path": "C:/data/shop.accdb", "sql": "SELECT 1"})).unwrap();
    assert_eq!(r["query"], "shop");
    assert!(s.execute("data.fromOdbc", json!({"connection": "DSN=Nope", "table": "t"})).unwrap_err().to_string().contains("connect"));
    // Connection strings may hold credentials: they aren't kept in Recent Sources.
    assert!(!s.prefs.recent_sources.iter().any(|r| r.contains("DSN")));
}

/// A workbook saved with an external query and a WEBSERVICE formula, as bytes.
fn saved_external_workbook(fake: &Arc<Fake>) -> Vec<u8> {
    let mut s = session(fake);
    s.execute("data.fromTextCsv", json!({"path": "/d/a.csv"})).unwrap();
    s.execute("query.set", json!({"query": {"name": "a", "source": {"kind": "csv", "path": "/d/a.csv"}, "steps": [{"op": "promoteHeaders"}], "refresh": {"onOpen": true}}})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!A1", "input": "=WEBSERVICE(\"https://api.example.com/v\")"})).unwrap();
    crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap()
}

#[test]
fn trust_prompt_blocks_external_content_until_enabled() {
    let fake = Arc::new(Fake::default());
    fake.file("/d/a.csv", b"K\nold\n");
    fake.page("https://api.example.com/v", "text/plain", "42");
    let bytes = saved_external_workbook(&fake);
    fake.file("/d/a.csv", b"K\nnew\n");
    let mut s = Session::new();
    s.set_data_access(fake.clone());
    let r = s.execute("file.open", json!({"name": "book.xlsx", "base64": crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(r["externalBlocked"], true);
    // The query survived the save; refresh is refused while blocked; WEBSERVICE is #VALUE!.
    assert_eq!(s.doc().unwrap().wb.queries.len(), 1);
    let gets = fake.gets.load(Ordering::SeqCst);
    let e = s.execute("data.refreshAll", json!({})).unwrap_err().to_string();
    assert!(e.contains("Security Warning"), "{e}");
    assert_eq!(cell(&s, "Sheet1", "A1"), Value::Error(CellError::Value));
    assert_eq!(fake.gets.load(Ordering::SeqCst), gets, "nothing was fetched while blocked");
    // A second window on the workbook doesn't get around the warning.
    s.execute("view.newWindow", json!({})).unwrap();
    assert!(s.doc().unwrap().external_blocked);
    s.execute("file.close", json!({"force": true})).unwrap();
    // Enable Content: WEBSERVICE fetches, refresh-on-open queries refresh.
    s.execute("data.enableContent", json!({})).unwrap();
    assert_eq!(cell(&s, "Sheet1", "A1"), Value::text("42"));
    assert_eq!(cell(&s, "a", "A2"), Value::text("new"));
    assert!(s.execute("data.enableContent", json!({})).is_err());
    // Disable all data connections: refused even after enabling.
    s.execute("file.trustCenter", json!({"externalContent": "disableAll"})).unwrap();
    assert!(s.execute("data.refreshAll", json!({})).unwrap_err().to_string().contains("Trust Center"));
    // Enable all: a workbook opens unblocked and refreshes on open.
    s.execute("file.trustCenter", json!({"externalContent": "enableAll"})).unwrap();
    fake.file("/d/a.csv", b"K\nnewest\n");
    let r = s.execute("file.open", json!({"name": "book2.xlsx", "base64": crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(r["externalBlocked"], false);
    assert_eq!(cell(&s, "a", "A2"), Value::text("newest"));
    assert!(s.execute("file.trustCenter", json!({"externalContent": "sometimes"})).is_err());
}

#[test]
fn webservice_and_filterxml_through_the_fetcher() {
    let fake = Arc::new(Fake::default());
    fake.page("https://api.example.com/xml", "application/xml", "<r><v>1</v><v>2</v><v>three</v></r>");
    let mut s = session(&fake);
    s.execute("cell.set", json!({"cell": "A1", "input": "=WEBSERVICE(\"https://api.example.com/xml\")"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=FILTERXML(A1,\"//v\")"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=FILTERXML(WEBSERVICE(\"https://api.example.com/xml\"),\"//v[2]\")"})).unwrap();
    assert_eq!(cell(&s, "Sheet1", "B1"), Value::number(1.0));
    assert_eq!(cell(&s, "Sheet1", "B3"), Value::text("three"));
    assert_eq!(cell(&s, "Sheet1", "C1"), Value::number(2.0));
    // Each URL is fetched once per recalculation.
    let before = fake.gets.load(Ordering::SeqCst);
    s.execute("formulas.calculateNow", json!({})).unwrap();
    let after = fake.gets.load(Ordering::SeqCst);
    assert!(after - before <= 1, "{before} → {after}");
    // Errors and refusals are #VALUE!.
    for f in ["=WEBSERVICE(\"ftp://x\")", "=WEBSERVICE(\"https://api.example.com/missing\")", "=WEBSERVICE(\"\")"] {
        s.execute("cell.set", json!({"cell": "D1", "input": f})).unwrap();
        assert_eq!(cell(&s, "Sheet1", "D1"), Value::Error(CellError::Value), "{f}");
    }
    let long = "x".repeat(40_000);
    fake.page("https://api.example.com/long", "text/plain", &long);
    s.execute("cell.set", json!({"cell": "D1", "input": "=WEBSERVICE(\"https://api.example.com/long\")"})).unwrap();
    assert_eq!(cell(&s, "Sheet1", "D1"), Value::Error(CellError::Value));
}

#[test]
fn hostile_params_and_sources_never_panic() {
    let fake = Arc::new(Fake::default());
    fake.file("/h/bad.json", b"[[[[[");
    fake.file("/h/bad.xml", b"<a><b></a>");
    fake.file("/h/empty.csv", b"");
    fake.page("https://h.example/", "text/html", &"<table>".repeat(5000));
    let mut s = session(&fake);
    for (id, p) in [
        ("data.fromJson", json!({"path": "/h/bad.json"})),
        ("data.fromXml", json!({"path": "/h/bad.xml"})),
        ("data.fromTextCsv", json!({"path": "/h/empty.csv"})),
        ("data.fromWeb", json!({"url": "https://h.example/"})),
        ("data.fromTextCsv", json!({"text": "a", "delimiter": "toolong"})),
        ("query.set", json!({"query": {"name": "", "source": {"kind": "blank"}}})),
        ("query.set", json!({"query": {"name": "x", "source": {"kind": "nope"}}})),
        ("query.set", json!({"query": "text"})),
        ("query.addStep", json!({"name": "none", "step": {"op": "sort"}})),
        (
            "query.preview",
            json!({"query": {"name": "x", "source": {"kind": "text", "text": "a\n1"}, "steps": [{"op": "keepTopRows", "count": 18446744073709551615u64}, {"op": "splitColumn", "column": "Column1", "delimiter": ",", "count": 4000000000u64}]}}),
        ),
        ("query.navigator", json!({"source": "A1:B2"})),
        ("data.mergeQueries", json!({"left": 1})),
        ("data.dataSourceSettings", json!({"from": "/nope", "to": "/x"})),
        ("data.connectionProperties", json!({"query": "nope"})),
    ] {
        let r = s.execute(id, p.clone());
        assert!(!matches!(r, Err(crate::EngineError::Internal(..))), "{id} {p}: {r:?}");
    }
}

#[test]
fn excel_queries_refresh_from_their_m() {
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    s.execute("range.setValues", json!({"range": "A1:B2", "values": [["Region", "Amount"], ["North", 5]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
    s.execute("range.setValues", json!({"range": "D1:E4", "values": [["Region", "Amount"], ["North", 10], ["South", 20], ["North", 1]]})).unwrap();
    s.execute("insert.table", json!({"range": "D1:E4", "name": "Raw"})).unwrap();
    let table = s.doc().unwrap().wb.active().unwrap().tables[0].name.clone();
    let wb = Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    let excel = |name: &str, formula: &str, table: Option<String>| gridcraft_model::ExcelQuery {
        name: name.into(),
        formula: formula.into(),
        sheet: table.as_ref().map(|_| "Sheet1".into()),
        table,
        rows: Some(1),
        ..Default::default()
    };
    wb.excel_queries = vec![
        excel(
            "Sales",
            "let\n    Source = Excel.CurrentWorkbook(){[Name=\"Raw\"]}[Content],\n    Grouped = Table.Group(Source, {\"Region\"}, {{\"Amount\", each List.Sum([Amount]), type number}})\nin\n    Grouped",
            Some(table.clone()),
        ),
        excel("Padded", "Table.AddColumn(Sales, \"P\", each Table.Profile([Region]))", None),
        excel("Remote", "Sql.Database(\"srv\", \"db\")", None),
    ];
    let list = s.execute("query.listExcel", json!({})).unwrap();
    assert_eq!((list[0]["name"].as_str(), list[0]["load"].as_str(), list[0]["refreshable"].as_bool()), (Some("Sales"), Some("table"), Some(true)));
    // What GridCraft can't run yet is named, without running anything.
    assert_eq!((list[1]["refreshable"].as_bool(), &list[1]["unsupported"]), (Some(false), &json!(["Table.Profile"])));
    // GridCraft's own query list is unchanged; query.get shows the M text.
    assert_eq!(s.execute("query.list", json!({})).unwrap(), json!([]));
    let got = s.execute("query.get", json!({"name": "sales"})).unwrap();
    assert_eq!((got["excel"].as_bool(), got["refreshable"].as_bool()), (Some(true), Some(true)));
    assert_eq!(got["m"], list[0]["formula"]);
    // Refresh by name runs the M and rewrites the query's table in place.
    let r = s.execute("data.refresh", json!({"query": "Sales"})).unwrap();
    assert_eq!(r["excelQueries"][0]["rows"], 2, "{r}");
    assert_eq!(
        (cell(&s, "Sheet1", "A3"), cell(&s, "Sheet1", "B2"), cell(&s, "Sheet1", "B3")),
        (Value::text("South"), Value::number(11.0), Value::number(20.0))
    );
    assert!(s.doc().unwrap().wb.active().unwrap().tables.iter().any(|t| t.name == table && t.range.a1() == "A1:B3"));
    assert_eq!(s.doc().unwrap().wb.excel_queries[0].rows, Some(2));
    // One undo puts the old values back.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!((cell(&s, "Sheet1", "B2"), cell(&s, "Sheet1", "A3")), (Value::number(5.0), Value::Empty));
    // From a cell of its table, and in Refresh All (an unsupported function is that query's error).
    s.execute("selection.set", json!({"cell": "A2"})).unwrap();
    s.execute("data.refresh", json!({})).unwrap();
    assert_eq!(cell(&s, "Sheet1", "B2"), Value::number(11.0));
    let r = s.execute("data.refreshAll", json!({})).unwrap();
    // As in M, a function that fails inside `each` leaves errors in its cells; at the top it fails
    // the query.
    assert_eq!((r["excelQueries"][1]["rows"].as_u64(), r["excelQueries"][1]["errors"].as_u64()), (Some(2), Some(2)), "{r}");
    let e = r["excelQueries"][2]["error"].as_str().unwrap();
    assert!(e.contains("Sql.Database") && e.contains("isn't supported"), "{e}");
    assert_eq!(s.doc().unwrap().wb.excel_queries[2].error.as_deref(), Some(e));
    // Editing or deleting one says why it can't; a new query can't take its name.
    assert!(s.execute("query.delete", json!({"name": "Sales"})).unwrap_err().to_string().contains("editing M isn't supported"));
    assert_eq!(s.doc().unwrap().wb.excel_queries.len(), 3);
    let q = json!({"query": {"name": "Sales", "source": {"kind": "text", "text": "a\n1"}, "load": {"to": "connectionOnly"}}});
    assert!(s.execute("query.set", q).is_err());
}

#[test]
fn refresh_keeps_calculated_columns_and_the_tables_filter() {
    use gridcraft_model::{Cell, FilterCriterion, Formula, TableColumn, TotalsFn};
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    s.execute("range.setValues", json!({"range": "A1:B3", "values": [["Region", "Amount"], ["North", 5], ["South", 7]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "name": "Sales"})).unwrap();
    s.execute(
        "range.setValues",
        json!({"range": "E1:F5", "values": [["Region", "Amount"], ["North", 10], ["South", 20], ["North", 1], ["East", 2]]}),
    )
    .unwrap();
    s.execute("insert.table", json!({"range": "E1:F5", "name": "Raw"})).unwrap();
    // A calculated column added in Excel beside the query's columns, and a filter on Region.
    let wb = Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    let sh = Arc::make_mut(&mut wb.sheets[0]);
    sh.set_value(CellRef::parse("C1").unwrap(), Value::text("Twice"));
    for r in [2u32, 3] {
        sh.set_cell(
            CellRef::new(r - 1, 2),
            Cell { value: Value::Empty, formula: Some(Arc::new(Formula::new("=Sales[[#This Row],[Amount]]*2"))), style: Default::default() },
        );
    }
    let t = sh.tables.iter_mut().find(|t| t.name == "Sales").unwrap();
    t.range = gridcraft_core::RangeRef::parse("A1:C3").unwrap();
    t.columns.push(TableColumn {
        name: "Twice".into(),
        totals: TotalsFn::None,
        totals_label: None,
        formula: Some("Sales[[#This Row],[Amount]]*2".into()),
        unique_name: None,
        xml: None,
    });
    t.filter = vec![(0, FilterCriterion::Values { values: vec!["North".into()], blanks: false })];
    sh.rows.entry(2).or_default().hidden = true;
    wb.excel_queries = vec![gridcraft_model::ExcelQuery {
        name: "Sales".into(),
        formula: "Table.Sort(Excel.CurrentWorkbook(){[Name=\"Raw\"]}[Content], {{\"Amount\", Order.Descending}})".into(),
        table: Some("Sales".into()),
        sheet: Some("Sheet1".into()),
        ..Default::default()
    }];
    s.execute("data.refresh", json!({"query": "Sales"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.sheet(0).unwrap();
    let t = sh.tables.iter().find(|t| t.name == "Sales").unwrap();
    assert_eq!(t.range.a1(), "A1:C5");
    assert_eq!(t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Region", "Amount", "Twice"]);
    assert!(t.columns[2].formula.is_some());
    // The formula fills every new row and calculates.
    assert_eq!(
        (cell(&s, "Sheet1", "B2"), cell(&s, "Sheet1", "C2"), cell(&s, "Sheet1", "C5")),
        (Value::number(20.0), Value::number(40.0), Value::number(2.0))
    );
    assert!(sh.cell(CellRef::parse("C5").unwrap()).is_some_and(|c| c.formula.is_some()));
    // The filter is kept and filters the new rows: South (row 2) and East (row 4) are hidden.
    assert_eq!(t.filter.len(), 1);
    let hidden: Vec<u32> = (1..=4).filter(|r| sh.is_row_hidden(*r)).collect();
    assert_eq!(hidden, vec![1, 3]);
}

#[test]
fn an_excel_workbook_refreshes_and_keeps_its_power_query_parts() {
    use std::io::Read;
    let fake = Arc::new(Fake::default());
    let mut s = session(&fake);
    let formula = "let\r\n    Source = Excel.CurrentWorkbook(){[Name=\"Raw\"]}[Content],\r\n    #\"Changed Type\" = Table.TransformColumnTypes(Source,{{\"Region\", type text}, {\"Amount\", Int64.Type}}),\r\n    #\"Sorted Rows\" = Table.Sort(#\"Changed Type\",{{\"Amount\", Order.Descending}})\r\nin\r\n    #\"Sorted Rows\"";
    let bytes = gridcraft_xlsx::testing::power_query_workbook(formula);
    let (wb, _) = crate::io::open_bytes("pq.xlsx", &bytes).unwrap();
    assert_eq!(wb.excel_queries[0].table.as_deref(), Some("Sales"));
    *Arc::make_mut(&mut s.doc_mut().unwrap().wb) = wb;
    let r = s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(r["excelQueries"][0]["rows"], 3, "{r}");
    assert_eq!(
        (cell(&s, "Data", "A2"), cell(&s, "Data", "B2"), cell(&s, "Data", "A4")),
        (Value::text("East"), Value::number(30.0), Value::text("North"))
    );
    let t = s.doc().unwrap().wb.sheet(0).unwrap().tables.iter().find(|t| t.name == "Sales").unwrap().clone();
    assert_eq!(t.range.a1(), "A1:B4");
    assert_eq!(t.columns.iter().map(|c| c.unique_name.as_deref()).collect::<Vec<_>>(), vec![Some("1"), Some("2")]);
    // Saved: Excel's query definitions and connection are kept unchanged.
    let out = crate::io::save_bytes(&s.doc().unwrap().wb, "pq.xlsx").unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(out.clone())).unwrap();
    let mut part = |name: &str| {
        let mut f = z.by_name(name).unwrap_or_else(|_| panic!("no {name}"));
        let mut v = Vec::new();
        f.read_to_end(&mut v).unwrap();
        v
    };
    let original = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .unwrap()
        .by_name("customXml/item1.xml")
        .map(|mut f| {
            let mut v = Vec::new();
            f.read_to_end(&mut v).unwrap();
            v
        })
        .ok();
    let names: Vec<String> = zip::ZipArchive::new(std::io::Cursor::new(out.clone())).unwrap().file_names().map(str::to_string).collect();
    let mashup =
        names.iter().filter(|n| n.starts_with("customXml/item") && !n.contains("Props")).map(|n| part(n)).find(|v| Some(v) == original.as_ref());
    assert!(mashup.is_some(), "the DataMashup part is kept byte for byte: {names:?}");
    assert!(String::from_utf8_lossy(&part("xl/connections.xml")).contains("Location=Sales"));
    let tables: Vec<String> =
        names.iter().filter(|n| n.starts_with("xl/tables/table")).map(|n| String::from_utf8_lossy(&part(n)).to_string()).collect();
    assert!(tables.iter().any(|t| t.contains(r#"name="Sales""#) && t.contains(r#"ref="A1:B4""#)), "{tables:?}");
    let (again, _) = crate::io::open_bytes("pq.xlsx", &out).unwrap();
    assert_eq!(again.excel_queries.len(), 1);
    assert_eq!(again.excel_queries[0].table.as_deref(), Some("Sales"));
}
