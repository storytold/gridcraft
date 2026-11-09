//! What a Python Object cell keeps and what can be done with it, with a fake worker: its sized label,
//! its table, the commands that read and save the table, and a text's lines.

use std::sync::Arc;

use gridcraft_core::{Array, CellError, Value};
use gridcraft_python::fake::FakeTransport;
use gridcraft_python::protocol::{MAX_TABLE_COLS, MAX_TABLE_ROWS, Table};
use gridcraft_python::{Limits, Output};
use serde_json::{Value as Json, json};

use super::macros::put;
use super::python_run::{c, py, settle};
use super::v;
use crate::macros::policy::{Policy, macro_policy};
use crate::{PythonCapability, Session, Trust};

fn frame(rows: usize, cols: usize, total_rows: u64, total_cols: u64) -> Table {
    Table {
        columns: (0..cols).map(|c| format!("c{c}")).collect(),
        rows,
        cols,
        data: (0..rows * cols).map(|i| Value::Number(i as f64)).collect(),
        total_rows,
        total_cols,
        ..Table::default()
    }
}

/// A worker that answers each code with what it names.
fn answer(code: &str) -> Output {
    let table = |type_name: &str, size: Vec<u64>, table: Table| Output::Object {
        type_name: type_name.into(),
        preview: "repr".into(),
        size,
        table: Some(table),
        picture: None,
    };
    let sized =
        |type_name: &str, size: Vec<u64>| Output::Object { type_name: type_name.into(), preview: "repr".into(), size, table: None, picture: None };
    let words: Vec<&str> = code.split_whitespace().collect();
    match words.as_slice() {
        ["frame"] => table("DataFrame", vec![6, 4], frame(6, 4, 6, 4)),
        ["big"] => table("DataFrame", vec![1000, 50], frame(MAX_TABLE_ROWS, MAX_TABLE_COLS, 1000, 50)),
        ["series"] => {
            table("Series", vec![12], Table { rows: 1, cols: 1, data: vec![Value::Number(1.0)], total_rows: 12, total_cols: 1, ..Table::default() })
        }
        ["list"] => sized("list", vec![12]),
        ["dict"] => sized("dict", vec![3]),
        ["array"] => sized("ndarray", vec![2, 3]),
        ["cube"] => sized("ndarray", vec![2, 3, 4]),
        ["function"] => Output::object("function", "<function f>"),
        ["empty"] => table("DataFrame", vec![0, 0], Table::default()),
        ["none"] => table("DataFrame", vec![0, 2], Table { cols: 2, columns: vec!["a".into(), "b".into()], total_cols: 2, ..Table::default() }),
        ["odd"] => sized("a\nb", vec![1]),
        ["unnamed"] => sized("", vec![1]),
        ["guarded"] => table(
            "list",
            vec![2],
            Table { rows: 2, cols: 1, data: vec![Value::text("=1+1"), Value::text("x|y\nz")], total_rows: 2, total_cols: 1, ..Table::default() },
        ),
        ["text"] => Output::Value(Value::text("one\ntwo\nthree")),
        ["single"] => Output::Value(Value::text("one line")),
        ["spill"] => Output::Value(Array::new(2, 1, vec![1.0.into(), 2.0.into()]).map(Value::from).unwrap()),
        _ => Output::Value(Value::Number(1.0)),
    }
}

fn session() -> Session {
    let t = FakeTransport::answering(|code, _| Ok(answer(code)));
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        limits: Limits::default(),
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    s
}

fn object(s: &mut Session, cell: &str, code: &str) {
    s.execute("python.set", json!({"cell": cell, "code": code, "output": "object"})).unwrap();
    settle(s);
}

fn formula(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn cell_get(s: &mut Session, cell: &str) -> Json {
    s.execute("cell.get", json!({"cell": cell})).unwrap()
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-python-output-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_python_object_shows_its_name_and_its_size() {
    let mut s = session();
    for (cell, code, label) in [
        ("A1", "frame", "DataFrame 6x4"),
        ("A2", "series", "Series 12"),
        ("A3", "list", "list 12"),
        ("A4", "dict", "dict 3"),
        ("A5", "array", "ndarray 2x3"),
        ("A6", "cube", "ndarray 2x3x4"),
        ("A7", "function", "function"),
        ("A8", "empty", "DataFrame 0x0"),
        // A type's name can hold anything; the cell shows one line, and something.
        ("A9", "odd", "a b 1"),
        ("A10", "unnamed", "object 1"),
    ] {
        object(&mut s, cell, code);
        assert_eq!(v(&s, cell), Value::text(label), "{code}");
    }
    let got = cell_get(&mut s, "A1");
    assert_eq!(got["python"]["typeName"], "DataFrame", "the type's name stays what it is");
    assert_eq!(got["text"], "DataFrame 6x4");
    assert_eq!(got["python"]["table"]["totalRows"], 6);
    assert!(cell_get(&mut s, "A3")["python"]["table"].is_null(), "a flat list has no table");
    // What reads the cell reads its name.
    formula(&mut s, "C1", "=A1");
    assert_eq!(v(&s, "C1"), Value::text("DataFrame 6x4"));
}

#[test]
fn an_excel_value_cell_has_no_size_and_no_table_whatever_the_worker_says() {
    let mut s = session();
    py(&mut s, "A1", "frame");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("DataFrame"));
    assert!(cell_get(&mut s, "A1")["python"]["table"].is_null());
    assert!(s.execute("python.table", json!({"cell": "A1"})).unwrap_err().to_string().contains("has no table"));
}

#[test]
fn the_table_goes_when_the_result_does() {
    let mut s = session();
    object(&mut s, "A1", "frame");
    assert!(s.execute("python.table", json!({"cell": "A1"})).is_ok());
    // Another result.
    s.execute("python.set", json!({"cell": "A1", "code": "list"})).unwrap();
    settle(&mut s);
    assert!(s.execute("python.table", json!({"cell": "A1"})).is_err());
    assert_eq!(v(&s, "A1"), Value::text("list 12"));
    // A failed run, and the other return type.
    object(&mut s, "A1", "frame");
    s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    assert!(s.execute("python.table", json!({"cell": "A1"})).is_err());
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("DataFrame"));
    // Converting to a normal cell keeps what it showed.
    object(&mut s, "B1", "frame");
    s.execute("python.clear", json!({"cell": "B1"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::text("DataFrame 6x4"));
    assert!(s.execute("python.table", json!({"cell": "B1"})).is_err());
    // Undo brings it back with its table.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("python.table", json!({"cell": "B1"})).unwrap()["rows"], 6);
    // A copy of the cell has it too, until it runs its own.
    s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    s.execute("edit.copy", json!({})).unwrap();
    s.execute("selection.set", json!({"cell": "D1"})).unwrap();
    s.execute("edit.paste", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.execute("python.table", json!({"cell": "D1"})).unwrap()["rows"], 6);
}

#[test]
fn the_table_is_not_in_a_file_and_the_label_is() {
    let mut s = session();
    object(&mut s, "A1", "big");
    let wb = s.doc().unwrap().wb.clone();
    for name in ["o.xlsx", "o.json"] {
        let bytes = crate::io::save_bytes(&wb, name).unwrap();
        assert!(bytes.len() < 200_000, "{name}: {} bytes", bytes.len());
        let (back, _) = crate::io::open_bytes(name, &bytes).unwrap();
        let cell = back.sheet(0).unwrap().cell(c("A1")).unwrap();
        assert_eq!(cell.value, Value::text("DataFrame 1000x50"), "{name}");
        let output = cell.py.as_ref().and_then(|p| p.output.as_ref());
        assert!(output.is_none_or(|o| o.table.is_none() && !o.from_run), "{name}: {output:?}");
    }
}

#[test]
fn python_table_gives_the_table_as_data_csv_or_markdown() {
    let mut s = session();
    object(&mut s, "A1", "guarded");
    let t = s.execute("python.table", json!({"cell": "A1"})).unwrap();
    assert_eq!(t["cell"], "A1");
    assert_eq!((t["rows"].as_u64(), t["cols"].as_u64(), t["cut"].as_bool()), (Some(2), Some(1), Some(false)));
    assert_eq!(t["data"], json!(["=1+1", "x|y\nz"]));
    assert_eq!(t["columns"], json!([]));
    let csv = s.execute("python.table", json!({"cell": "A1", "format": "csv"})).unwrap();
    assert_eq!(csv["text"], "\"'=1+1\"\r\n\"x|y\nz\"\r\n");
    assert!(csv.get("data").is_none());
    let md = s.execute("python.table", json!({"cell": "A1", "format": "markdown"})).unwrap();
    assert_eq!(md["text"], "|  |\n| --- |\n| =1+1 |\n| x\\|y<br>z |\n");
    assert!(s.execute("python.table", json!({"cell": "A1", "format": "xml"})).is_err());
    assert!(s.execute("python.table", json!({"cell": "A1", "format": 3})).is_err());
    // Not a Python cell, no cell, a bad reference.
    assert!(s.execute("python.table", json!({"cell": "Z9"})).unwrap_err().to_string().contains("isn't a Python cell"));
    assert!(s.execute("python.table", json!({"cell": "not a cell"})).is_err());
    // The active cell by default.
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    assert_eq!(s.execute("python.table", json!({})).unwrap()["rows"], 2);
    // A big one says it was cut.
    object(&mut s, "B1", "big");
    let t = s.execute("python.table", json!({"cell": "B1"})).unwrap();
    assert_eq!(t["cut"], true);
    assert_eq!((t["totalRows"].as_u64(), t["totalCols"].as_u64()), (Some(1000), Some(50)));
}

#[test]
fn python_save_csv_writes_the_table_and_says_when_it_was_cut() {
    let dir = temp("save");
    let mut s = session();
    object(&mut s, "A1", "frame");
    object(&mut s, "B1", "big");
    let path = dir.join("frame.csv");
    let r = s.execute("python.saveCsv", json!({"cell": "A1", "path": path})).unwrap();
    assert_eq!(r["cut"], false);
    assert_eq!(r["path"], path.to_str().unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("c0,c1,c2,c3\r\n0,1,2,3\r\n"), "{text}");
    assert_eq!(r["bytes"].as_u64(), Some(text.len() as u64));
    assert_eq!(text.lines().count(), 7);
    // `.csv` is added when it is missing, and left when it is there in any case.
    let bare = dir.join("bare");
    let r = s.execute("python.saveCsv", json!({"cell": "A1", "path": bare})).unwrap();
    assert!(r["path"].as_str().unwrap().ends_with("bare.csv") && dir.join("bare.csv").exists());
    let upper = dir.join("UPPER.CSV");
    assert!(s.execute("python.saveCsv", json!({"cell": "A1", "path": upper})).unwrap()["path"].as_str().unwrap().ends_with("UPPER.CSV"));
    // A table bigger than the preview: the answer says the file is only part of it.
    let r = s.execute("python.saveCsv", json!({"cell": "B1", "path": dir.join("big.csv")})).unwrap();
    assert_eq!((r["cut"].as_bool(), r["rows"].as_u64(), r["totalRows"].as_u64()), (Some(true), Some(200), Some(1000)));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn python_save_csv_needs_a_path_and_a_table_and_opens_no_dialog() {
    let dir = temp("refusals");
    let mut s = session();
    object(&mut s, "A1", "frame");
    object(&mut s, "B1", "empty");
    object(&mut s, "C1", "none");
    object(&mut s, "D1", "list");
    for p in [
        json!({"cell": "A1"}),
        json!({"cell": "A1", "path": null}),
        json!({"cell": "A1", "path": ""}),
        json!({"cell": "A1", "path": "  "}),
        json!({"cell": "A1", "path": 7}),
        json!({}),
    ] {
        let e = s.execute("python.saveCsv", p.clone()).unwrap_err().to_string();
        assert!(e.contains("path"), "{p}: {e}");
    }
    assert!(s.ui_requests.is_empty(), "no dialog, whatever it is given");
    let to = |name: &str| dir.join(name);
    assert!(s.execute("python.saveCsv", json!({"cell": "Z1", "path": to("z.csv")})).is_err());
    assert!(s.execute("python.saveCsv", json!({"cell": "D1", "path": to("d.csv")})).unwrap_err().to_string().contains("has no table"));
    assert!(s.execute("python.saveCsv", json!({"cell": "B1", "path": to("b.csv")})).unwrap_err().to_string().contains("nothing to save"));
    assert!(!to("z.csv").exists() && !to("d.csv").exists() && !to("b.csv").exists());
    // A table with columns and no rows is its header.
    s.execute("python.saveCsv", json!({"cell": "C1", "path": to("c.csv")})).unwrap();
    assert_eq!(std::fs::read_to_string(to("c.csv")).unwrap(), "a,b\r\n");
    // A folder that isn't there is an error, and leaves nothing behind.
    let gone = dir.join("no-such-folder").join("x.csv");
    assert!(s.execute("python.saveCsv", json!({"cell": "A1", "path": gone})).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn python_save_csv_follows_the_rule_of_commands_that_write_files() {
    let path = json!({"cell": "A1", "path": "/tmp/x.csv"});
    assert!(matches!(macro_policy("python.saveCsv", &path, Trust::Local), Policy::NeedsPermission(_)));
    assert!(matches!(macro_policy("python.saveCsv", &path, Trust::FromFile), Policy::NeedsPermission(_)));
    assert!(crate::macros::policy::writes_file("python.saveCsv", &path));
    assert!(!crate::macros::policy::writes_file("python.saveCsv", &json!({"cell": "A1"})));
    // Reading the table writes nothing.
    assert_eq!(macro_policy("python.table", &json!({"cell": "A1"}), Trust::Local), Policy::Allow);

    let dir = temp("macro");
    let file = dir.join("macro.csv");
    let mut s = session();
    object(&mut s, "A1", "frame");
    let step = json!({"command": "python.saveCsv", "params": {"cell": "A1", "path": file}});
    // Without the permission it doesn't run, and nothing is written.
    put(&mut s, json!({"name": "NoPermission", "steps": [step]}));
    assert!(s.execute("macro.run", json!({"name": "NoPermission", "approve": true})).is_err());
    assert!(!file.exists());
    // With it, but before the person approves, the same.
    put(&mut s, json!({"name": "Permission", "permissions": ["python.saveCsv"], "steps": [step]}));
    assert!(s.execute("macro.run", json!({"name": "Permission"})).is_err());
    assert!(!file.exists());
    // Once they do, it writes.
    s.execute("macro.run", json!({"name": "Permission", "approve": true})).unwrap();
    assert!(file.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_script_cannot_write_the_table_to_a_file() {
    let dir = temp("script");
    let file = dir.join("script.csv");
    let mut s = session();
    object(&mut s, "A1", "frame");
    let sandbox = crate::io::ScriptSandbox::enter();
    let e = s.execute("python.saveCsv", json!({"cell": "A1", "path": file})).unwrap_err().to_string();
    drop(sandbox);
    assert!(e.contains("can't read or write files"), "{e}");
    assert!(!file.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_text_stays_in_its_cell_until_the_option_spills_its_lines() {
    let mut s = session();
    py(&mut s, "A1", "text");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("one\ntwo\nthree"), "by default");
    assert_eq!(v(&s, "A2"), Value::Empty);
    assert_eq!(cell_get(&mut s, "A1")["python"]["lines"], "cell");
    formula(&mut s, "C1", "=A3");
    let undo = s.doc().unwrap().undo.len();
    // The option needs no run: the result is kept.
    let requests = s.python_stats().clients;
    let r = s.execute("python.output", json!({"cell": "A1", "lines": "rows"})).unwrap();
    assert_eq!((r["lines"].as_str(), r["output"].as_str()), (Some("rows"), Some("value")), "and leaves the return type");
    assert_eq!(v(&s, "A1"), Value::text("one"));
    assert_eq!(v(&s, "A2"), Value::text("two"));
    assert_eq!(v(&s, "A3"), Value::text("three"));
    assert_eq!(v(&s, "C1"), Value::text("three"), "what reads a line sees it");
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1, "one undo step");
    settle(&mut s);
    assert_eq!(v(&s, "A3"), Value::text("three"));
    assert_eq!(s.python_stats().clients, requests, "nothing started");
    // Undo takes it back, and redo brings it again.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::text("one\ntwo\nthree"));
    assert_eq!(v(&s, "A3"), Value::Empty);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "A3"), Value::text("three"));
    // It stays when the code changes, and when the return type does.
    s.execute("python.set", json!({"cell": "A1", "code": "single"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("one line"), "a text of one line has nothing to spill");
    assert_eq!(v(&s, "A2"), Value::Empty);
    s.execute("python.set", json!({"cell": "A1", "code": "text"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::text("two"));
    s.execute("python.output", json!({"cell": "A1", "mode": "object"})).unwrap();
    settle(&mut s);
    assert_eq!(cell_get(&mut s, "A1")["python"]["lines"], "rows");
    assert_eq!(v(&s, "A2"), Value::text("two"), "a text is a text under either return type");
    // Back in the cell.
    let r = s.execute("python.output", json!({"cell": "A1", "lines": "cell"})).unwrap();
    assert_eq!(r["output"], "object");
    assert_eq!(v(&s, "A1"), Value::text("one\ntwo\nthree"));
    assert_eq!(v(&s, "A2"), Value::Empty);
    // Both at once are one undo step.
    let undo = s.doc().unwrap().undo.len();
    let r = s.execute("python.output", json!({"cell": "A1", "mode": "value", "lines": "rows"})).unwrap();
    assert_eq!((r["output"].as_str(), r["lines"].as_str()), (Some("value"), Some("rows")));
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::text("two"));
    // Not a text: no effect, and no harm.
    s.execute("python.set", json!({"cell": "B1", "code": "spill"})).unwrap();
    settle(&mut s);
    s.execute("python.output", json!({"cell": "B1", "lines": "rows"})).unwrap();
    assert_eq!(v(&s, "B2"), Value::Number(2.0));
    // Words it doesn't take.
    for bad in [json!("sideways"), json!(1), json!(true)] {
        assert!(s.execute("python.output", json!({"cell": "A1", "lines": bad})).is_err());
    }
    assert!(s.execute("python.output", json!({"cell": "Z9", "lines": "rows"})).is_err());
}

#[test]
fn a_text_whose_lines_are_blocked_says_so() {
    let mut s = session();
    py(&mut s, "A1", "text");
    settle(&mut s);
    formula(&mut s, "A2", "in the way");
    s.execute("python.output", json!({"cell": "A1", "lines": "rows"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert_eq!(v(&s, "A2"), Value::text("in the way"));
    // Clearing the way lets them out.
    s.execute("cell.set", json!({"cell": "A2", "input": ""})).unwrap();
    assert_eq!(v(&s, "A3"), Value::text("three"));
}

#[test]
fn the_lines_option_is_refused_on_a_locked_cell_of_a_protected_sheet() {
    let mut s = session();
    py(&mut s, "A1", "text");
    settle(&mut s);
    s.execute("review.protectSheet", json!({})).unwrap();
    assert!(s.execute("python.output", json!({"cell": "A1", "lines": "rows"})).is_err());
    assert_eq!(v(&s, "A2"), Value::Empty);
}

#[test]
fn the_lines_option_lasts_in_json_and_for_the_session_in_xlsx() {
    let mut s = session();
    py(&mut s, "A1", "text");
    settle(&mut s);
    s.execute("python.output", json!({"cell": "A1", "lines": "rows"})).unwrap();
    let wb = s.doc().unwrap().wb.clone();
    let (json_back, _) = crate::io::open_bytes("o.json", &crate::io::save_bytes(&wb, "o.json").unwrap()).unwrap();
    assert!(json_back.sheet(0).unwrap().cell(c("A1")).unwrap().py.as_ref().unwrap().spill_lines);
    let (xlsx_back, _) = crate::io::open_bytes("o.xlsx", &crate::io::save_bytes(&wb, "o.xlsx").unwrap()).unwrap();
    assert!(!xlsx_back.sheet(0).unwrap().cell(c("A1")).unwrap().py.as_ref().unwrap().spill_lines);
    // The file keeps what the cells showed.
    assert_eq!(xlsx_back.sheet(0).unwrap().value(c("A1")), Value::text("one"));
    assert_eq!(xlsx_back.sheet(0).unwrap().value(c("A2")), Value::text("two"));
}
