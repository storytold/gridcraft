//! Python cells end to end, through a real `Session`, the real sandbox and a real Python: what the
//! engine's tests with a fake worker can't show. `gridcraft-cli` is the launcher, as the app is in
//! production. On a machine with no Python 3.11 or newer, or that doesn't confine, the tests have
//! nothing to check and pass with a note; on CI (`CI` set) that is a failure, on the systems that
//! confine.
//!
//! The tests that need pandas take the interpreter from `GRIDCRAFT_TEST_PYTHON`, and pass with a
//! note without it.

use std::time::Duration;

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_engine::python::Limits;
use gridcraft_engine::{PythonCapability, PythonState, Session};
use serde_json::json;

/// How long a test waits for Python, in polls of 50 ms.
const PATIENCE: u32 = 2_400;

/// A session whose Python is ready, or `None` when this machine can't give one.
fn session_with(limits: Limits, interpreter: Option<std::path::PathBuf>) -> Option<Session> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        eprintln!("this system doesn't confine programs; skipping");
        return None;
    }
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        interpreter,
        launcher: Some(env!("CARGO_BIN_EXE_gridcraft-cli").into()),
        limits,
        ..PythonCapability::default()
    });
    let mut left = PATIENCE;
    while s.python_environment().state == PythonState::Running {
        s.python_poll(Duration::from_millis(50));
        left -= 1;
        assert!(left > 0, "the check of Python never answered");
    }
    let env = s.python_environment();
    if env.state != PythonState::Ready {
        let why = env.diagnostic.as_ref().map_or_else(String::new, |d| format!("{}: {} {}", d.kind.id, d.kind.message, d.detail));
        assert!(std::env::var_os("CI").is_none(), "Python isn't ready on CI: {why}");
        eprintln!("Python isn't ready on this machine; skipping: {why}");
        return None;
    }
    Some(s)
}

fn session() -> Option<Session> {
    session_with(Limits::default(), None)
}

/// A session on the interpreter in `GRIDCRAFT_TEST_PYTHON`, which must have pandas.
fn pandas_session() -> Option<Session> {
    let Some(python) = std::env::var_os("GRIDCRAFT_TEST_PYTHON") else {
        eprintln!("GRIDCRAFT_TEST_PYTHON isn't set; skipping the tests that need pandas");
        return None;
    };
    let s = session_with(Limits::default(), Some(python.into()))?;
    assert!(s.python_environment().packages.iter().any(|(name, _)| name.eq_ignore_ascii_case("pandas")), "GRIDCRAFT_TEST_PYTHON has no pandas");
    Some(s)
}

/// A session on `GRIDCRAFT_TEST_PYTHON`, when that Python has matplotlib and seaborn.
fn plots_session() -> Option<Session> {
    let s = pandas_session()?;
    let has = |name: &str| s.python_environment().packages.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    if !has("matplotlib") || !has("seaborn") {
        eprintln!("GRIDCRAFT_TEST_PYTHON has no matplotlib or seaborn; skipping the tests that draw");
        return None;
    }
    Some(s)
}

fn settle(s: &mut Session) {
    let mut left = PATIENCE;
    while s.python_busy() {
        s.python_poll(Duration::from_millis(50));
        left -= 1;
        assert!(left > 0, "Python never settled");
    }
}

fn py(s: &mut Session, cell: &str, code: &str) {
    s.execute("python.set", json!({"cell": cell, "code": code})).unwrap();
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn v(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn kind(s: &Session, cell: &str) -> Option<&'static str> {
    s.python_diagnostic(0, CellRef::parse(cell).unwrap()).map(|d| d.kind.id)
}

fn num(n: f64) -> Value {
    Value::Number(n)
}

#[test]
fn the_report_names_the_python_the_sandbox_and_what_is_installed() {
    let Some(mut s) = session() else { return };
    let env = s.python_environment();
    assert_eq!(env.state, PythonState::Ready);
    let python = env.interpreter.as_ref().expect("the report names the interpreter");
    assert!(python.path.is_absolute() && python.version >= gridcraft_python::interp::MIN_VERSION, "{python:?}");
    assert!(!env.sandbox.is_empty());
    assert!(env.diagnostic.is_none() || env.diagnostic.as_ref().is_some_and(|d| d.kind.id == "packages.noPandas"), "{:?}", env.diagnostic);
    let json = s.execute("python.environment", json!({})).unwrap();
    assert_eq!(json["state"], "ready");
    assert!(json["interpreter"]["version"].is_string());
    assert!(json["packages"].is_array());
}

#[test]
fn arithmetic_runs_and_a_formula_reads_the_result() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "21 * 2");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy), "it waits, and nothing blocked");
    set(&mut s, "B1", "=A1+1");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(42.0));
    assert_eq!(v(&s, "B1"), num(43.0));
    assert_eq!(kind(&s, "A1"), None);
    py(&mut s, "A2", "'text'");
    py(&mut s, "A3", "1 == 1");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::text("text"));
    assert_eq!(v(&s, "A3"), Value::Bool(true));
}

#[test]
fn xl_reads_a_cell_and_a_range_and_a_change_runs_the_cell_again() {
    let Some(mut s) = session() else { return };
    set(&mut s, "A1", "20");
    set(&mut s, "B1", "1");
    set(&mut s, "B2", "2");
    set(&mut s, "B3", "3");
    py(&mut s, "C1", "xl(\"A1\") + 1");
    // A list of rows without pandas, a DataFrame with it.
    py(&mut s, "C2", "v = xl(\"B1:B3\")\nint(v.iloc[:, 0].sum()) if hasattr(v, 'iloc') else sum(r[0] for r in v)");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), num(21.0));
    assert_eq!(v(&s, "C2"), num(6.0));
    set(&mut s, "A1", "30");
    set(&mut s, "B3", "30");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), num(31.0));
    assert_eq!(v(&s, "C2"), num(33.0));
}

#[test]
fn a_list_of_lists_spills_and_a_cell_in_the_way_blocks_it() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "[[1, 2], [3, 4]]");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "B1"), v(&s, "A2"), v(&s, "B2")), (num(1.0), num(2.0), num(3.0), num(4.0)));
    set(&mut s, "D1", "=SUM(A1:B2)");
    assert_eq!(v(&s, "D1"), num(10.0));
    set(&mut s, "B2", "x");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert_eq!(kind(&s, "A1"), Some("result.spill"));
    set(&mut s, "B2", "");
    settle(&mut s);
    assert_eq!(v(&s, "B2"), num(4.0));
}

#[test]
fn opening_a_file_is_refused_by_the_sandbox_and_the_cell_says_so() {
    let Some(mut s) = session() else { return };
    let secret = std::env::temp_dir().canonicalize().unwrap().join(format!("gridcraft-python-cells-{}.txt", std::process::id()));
    std::fs::write(&secret, "secret\n").unwrap();
    py(&mut s, "A2", &format!("print('before')\nopen({:?}).read()", secret.display().to_string()));
    settle(&mut s);
    std::fs::remove_file(&secret).unwrap();
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Python));
    let d = s.python_diagnostic(0, CellRef::parse("A2").unwrap()).unwrap();
    assert_eq!(d.kind.id, "denied.file", "{}", d.detail);
    assert_eq!(s.python_run_log().last().map(|r| r.stdout.as_str()), Some("before\n"), "what it printed is kept");
}

#[test]
fn a_cell_past_the_time_limit_shows_timeout_and_the_rest_goes_on() {
    let limits = Limits { timeout: Duration::from_millis(500), start_timeout: Duration::from_secs(60), ..Limits::default() };
    let Some(mut s) = session_with(limits, None) else { return };
    py(&mut s, "A1", "a = 5\na");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(5.0));
    py(&mut s, "A2", "while True:\n    pass");
    py(&mut s, "A3", "a + 1");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Timeout));
    assert_eq!(kind(&s, "A2"), Some("limit.time"));
    assert_eq!(v(&s, "A3"), num(6.0), "a fresh worker ran what comes before it, and not the cell that stopped the last one");
}

#[test]
fn cells_share_variables_and_run_in_order() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "base = 10\nbase");
    py(&mut s, "A2", "base * 2");
    py(&mut s, "A3", "total + 1");
    py(&mut s, "A4", "total = 1\ntotal");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), num(20.0));
    // A later cell defines it: the run order is what is wrong, and the cell says where to look.
    assert_eq!(v(&s, "A3"), Value::Error(CellError::Python));
    let d = s.python_diagnostic(0, CellRef::parse("A3").unwrap()).unwrap();
    assert_eq!(d.kind.id, "python.nameOrder");
    assert!(d.detail.contains("A4"), "{}", d.detail);
    // A cell that changes takes the ones after it with it.
    py(&mut s, "A1", "base = 100\nbase");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), num(200.0));
    // An undefined name with no such cell is just that.
    py(&mut s, "A5", "nowhere");
    settle(&mut s);
    assert_eq!(kind(&s, "A5"), Some("python.name"));
}

#[test]
fn stop_and_reset_act_on_the_real_worker() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "a = 1\na");
    settle(&mut s);
    py(&mut s, "A2", "while True:\n    pass");
    // Sent to the worker, and left running.
    s.python_poll(Duration::ZERO);
    assert!(s.python_busy());
    assert_eq!(s.execute("python.stop", json!({})).unwrap()["stopped"], true);
    assert!(!s.python_busy());
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Python));
    assert_eq!(kind(&s, "A2"), Some("state.stopped"));
    // Reset gives fresh variables and runs everything, the stopped cell too; so the cell goes first.
    py(&mut s, "A2", "b = a + 1\nb");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), num(2.0));
    s.execute("python.reset", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (num(1.0), num(2.0)));
    assert!(s.python_run_log().count() >= 5);
}

#[test]
fn an_object_shows_its_type_with_its_preview_for_the_card() {
    let Some(mut s) = session() else { return };
    s.execute("python.set", json!({"cell": "A1", "code": "{'a': 1}", "output": "object"})).unwrap();
    py(&mut s, "A2", "def f():\n    pass\nf");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("dict 1"));
    let got = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!((got["python"]["typeName"].as_str(), got["python"]["preview"].as_str()), (Some("dict"), Some("{'a': 1}")));
    assert_eq!(v(&s, "A2"), Value::text("function"), "no cell form, even as an Excel value");
}

#[test]
fn a_range_is_a_dataframe_when_python_has_pandas() {
    let Some(mut s) = pandas_session() else { return };
    s.execute("range.setValues", json!({"range": "A1", "values": [["name", "score"], ["a", 1], ["b", 2], ["c", 4]]})).unwrap();
    py(&mut s, "D1", "type(xl(\"A1:B4\")).__name__");
    py(&mut s, "D2", "xl(\"A1:B4\", headers=True)[\"score\"].sum()");
    py(&mut s, "D3", "list(xl(\"A1:B4\", headers=True).columns)[1]");
    py(&mut s, "D4", "pd.__name__");
    settle(&mut s);
    assert_eq!(v(&s, "D1"), Value::text("DataFrame"));
    assert_eq!(v(&s, "D2"), num(7.0), "a numpy number comes back as a cell's number");
    assert_eq!(v(&s, "D3"), Value::text("score"));
    assert_eq!(v(&s, "D4"), Value::text("pandas"), "Excel's default names are there");
    set(&mut s, "B2", "10");
    settle(&mut s);
    assert_eq!(v(&s, "D2"), num(16.0));
}

#[test]
fn a_dataframe_result_spills_with_its_header_row() {
    let Some(mut s) = pandas_session() else { return };
    py(&mut s, "A1", "pd.DataFrame({'x': [1, 2], 'y': [3.5, 4.5]})");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "B1")), (Value::text("x"), Value::text("y")), "its column names first");
    assert_eq!((v(&s, "A2"), v(&s, "B2")), (num(1.0), num(3.5)));
    assert_eq!((v(&s, "A3"), v(&s, "B3")), (num(2.0), num(4.5)));
    assert_eq!(v(&s, "A4"), Value::Empty);
    // As an object it stays in one cell, with its size.
    s.execute("python.output", json!({"cell": "A1", "mode": "object"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("DataFrame 2x2"));
    assert_eq!(v(&s, "A2"), Value::Empty);
}

fn object_cell(s: &mut Session, cell: &str, code: &str) {
    s.execute("python.set", json!({"cell": cell, "code": code, "output": "object"})).unwrap();
}

#[test]
fn a_python_object_says_what_it_is_and_a_list_of_rows_gives_its_table() {
    let Some(mut s) = session() else { return };
    object_cell(&mut s, "A1", "[[1, 'a'], [2, '=b']]");
    object_cell(&mut s, "A2", "list(range(12))");
    object_cell(&mut s, "A3", "{'a': 1, 'b': 2, 'c': 3}");
    object_cell(&mut s, "A4", "object()");
    object_cell(&mut s, "A5", "[[r, c] for r in range(500) for c in range(1)]");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("list 2"));
    assert_eq!(v(&s, "A2"), Value::text("list 12"));
    assert_eq!(v(&s, "A3"), Value::text("dict 3"));
    assert_eq!(v(&s, "A4"), Value::text("object"));
    assert_eq!(v(&s, "A5"), Value::text("list 500"));
    assert_eq!(kind(&s, "A1"), None);

    let t = s.execute("python.table", json!({"cell": "A1", "format": "csv"})).unwrap();
    assert_eq!(t["text"], "1,a\r\n2,\"'=b\"\r\n");
    assert_eq!(t["cut"], false);
    assert!(s.execute("python.table", json!({"cell": "A2"})).is_err(), "a flat list has no table");

    // A table bigger than the preview: the file says what the answer says.
    let dir = std::env::temp_dir().join(format!("gridcraft-python-table-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cut.csv");
    let r = s.execute("python.saveCsv", json!({"cell": "A5", "path": path})).unwrap();
    assert_eq!((r["cut"].as_bool(), r["rows"].as_u64(), r["totalRows"].as_u64()), (Some(true), Some(200), Some(500)));
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 200);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_text_with_line_breaks_stays_in_its_cell_until_it_is_asked_to_spill() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "'first\\nsecond\\r\\nthird\\n'");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("first\nsecond\r\nthird\n"));
    assert_eq!(v(&s, "A2"), Value::Empty);
    s.execute("python.output", json!({"cell": "A1", "lines": "rows"})).unwrap();
    assert_eq!(
        (v(&s, "A1"), v(&s, "A2"), v(&s, "A3"), v(&s, "A4")),
        (Value::text("first"), Value::text("second"), Value::text("third"), Value::Empty)
    );
    // It lasts through another run.
    s.execute("python.run", json!({"cell": "A1"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A3"), Value::text("third"));
}

#[test]
fn pandas_and_numpy_objects_show_their_size_and_their_table_on_the_card() {
    let Some(mut s) = pandas_session() else { return };
    s.execute("range.setValues", json!({"range": "A1", "values": [["name", "score"], ["a", 1], ["b", 2], ["c", 4]]})).unwrap();
    object_cell(&mut s, "D1", "xl(\"A1:B4\", headers=True)");
    object_cell(&mut s, "D2", "xl(\"A1:B4\", headers=True)[\"score\"]");
    object_cell(&mut s, "D3", "np.arange(6).reshape(2, 3)");
    object_cell(&mut s, "D4", "pd.DataFrame(np.zeros((1000, 50)))");
    settle(&mut s);
    assert_eq!(v(&s, "D1"), Value::text("DataFrame 3x2"));
    assert_eq!(v(&s, "D2"), Value::text("Series 3"));
    assert_eq!(v(&s, "D3"), Value::text("ndarray 2x3"));
    assert_eq!(v(&s, "D4"), Value::text("DataFrame 1000x50"));

    let t = s.execute("python.table", json!({"cell": "D1", "format": "csv"})).unwrap();
    assert_eq!(t["text"], "name,score\r\na,1\r\nb,2\r\nc,4\r\n");
    let md = s.execute("python.table", json!({"cell": "D1", "format": "markdown"})).unwrap();
    assert_eq!(md["text"], "| name | score |\n| --- | --- |\n| a | 1 |\n| b | 2 |\n| c | 4 |\n");
    let series = s.execute("python.table", json!({"cell": "D2"})).unwrap();
    assert_eq!(series["columns"], json!(["score"]));
    let big = s.execute("python.table", json!({"cell": "D4"})).unwrap();
    assert_eq!((big["rows"].as_u64(), big["cols"].as_u64(), big["cut"].as_bool()), (Some(200), Some(30), Some(true)));

    // Spill Here gives the whole table, as a cell's value is the whole of it.
    s.execute("python.output", json!({"cell": "D4", "mode": "value"})).unwrap();
    settle(&mut s);
    assert_eq!(s.execute("cell.get", json!({"cell": "D4"})).unwrap()["spill"], "D4:BA1004");
    // It doesn't change a thing about what a file keeps: only the name and its size.
    let bytes = gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "o.xlsx").unwrap();
    let (back, _) = gridcraft_engine::io::open_bytes("o.xlsx", &bytes).unwrap();
    let sheet = back.sheet(0).unwrap();
    assert_eq!(sheet.value(CellRef::parse("D1").unwrap()), Value::text("DataFrame 3x2"));
    assert_eq!(sheet.cell(CellRef::parse("D1").unwrap()).and_then(|c| c.py.as_ref()).and_then(|p| p.output.as_ref()), None);
}

#[test]
fn a_memory_spike_lands_on_its_cell_and_the_next_cell_keeps_its_variables() {
    let limits = Limits { memory_bytes: 200 * 1024 * 1024, ..Limits::default() };
    let Some(mut s) = session_with(limits, None) else { return };
    py(&mut s, "A1", "x = 40\nx + 2");
    py(&mut s, "A2", "spike = b'x' * (400 * 1024 * 1024)\ndel spike\n'done'");
    py(&mut s, "A3", "x + 1");
    py(&mut s, "A4", "x + 2");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(42.0));
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Python));
    assert_eq!(kind(&s, "A2"), Some("limit.memory"), "the cell that did it, through the sandbox");
    assert_eq!((v(&s, "A3"), kind(&s, "A3")), (num(41.0), None), "a fresh worker ran A1 first");
    assert_eq!((v(&s, "A4"), kind(&s, "A4")), (num(42.0), None));
}

fn on_1904(s: &mut Session) {
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).date_system = gridcraft_core::DateSystem::D1904;
}

fn format_cell(s: &mut Session, range: &str, code: &str) {
    s.execute("home.numberFormat", json!({"range": range, "code": code})).unwrap();
}

fn format_of(s: &Session, cell: &str) -> String {
    let wb = &s.doc().unwrap().wb;
    wb.styles.get(wb.active().unwrap().style_id(CellRef::parse(cell).unwrap())).num_fmt.as_str().to_string()
}

/// What `xl()` gives, and what comes back, on a real Python through the real sandbox, in one date
/// system: a date, a time and a date-time read, a day added, and returned.
fn dates_round_trip(s: &mut Session, serial: f64) {
    set(s, "A1", &serial.to_string());
    format_cell(s, "A1", "m/d/yyyy");
    set(s, "A2", &format!("{}", serial + 0.5));
    format_cell(s, "A2", "m/d/yyyy h:mm");
    set(s, "A3", "0.25");
    format_cell(s, "A3", "h:mm");
    py(s, "B1", "type(xl(\"A1\")).__name__");
    py(s, "B2", "xl(\"A1\").isoformat()");
    py(s, "B3", "import datetime\nxl(\"A1\") + datetime.timedelta(days=1)");
    py(s, "B4", "type(xl(\"A2\")).__name__");
    py(s, "B5", "xl(\"A2\").isoformat()");
    py(s, "B6", "xl(\"A2\") + datetime.timedelta(days=1)");
    py(s, "B7", "type(xl(\"A3\")).__name__");
    py(s, "B8", "xl(\"A3\").isoformat()");
    py(s, "B9", "xl(\"A3\")");
    settle(s);
    assert_eq!(v(s, "B1"), Value::text("date"));
    assert_eq!(v(s, "B2"), Value::text("2024-01-15"));
    assert_eq!((v(s, "B3"), format_of(s, "B3")), (num(serial + 1.0), "m/d/yyyy".into()), "read a date, add a day, return it");
    assert_eq!(v(s, "B4"), Value::text("datetime"));
    assert_eq!(v(s, "B5"), Value::text("2024-01-15T12:00:00"));
    assert_eq!((v(s, "B6"), format_of(s, "B6")), (num(serial + 1.5), "m/d/yyyy h:mm".into()));
    assert_eq!(v(s, "B7"), Value::text("time"));
    assert_eq!(v(s, "B8"), Value::text("06:00:00"));
    assert_eq!((v(s, "B9"), format_of(s, "B9")), (num(0.25), "h:mm:ss".into()));
}

#[test]
fn dates_round_trip_in_the_1900_system() {
    let Some(mut s) = session() else { return };
    dates_round_trip(&mut s, 45306.0);
}

#[test]
fn dates_round_trip_in_the_1904_system() {
    let Some(mut s) = session() else { return };
    on_1904(&mut s);
    dates_round_trip(&mut s, 43844.0);
}

#[test]
fn the_days_that_never_were_stay_numbers_and_their_neighbours_are_real() {
    let Some(mut s) = session() else { return };
    for (cell, input) in [("A1", "60"), ("A2", "0"), ("A3", "0.5"), ("A4", "-1"), ("A5", "59"), ("A6", "61"), ("A7", "1")] {
        set(&mut s, cell, input);
    }
    format_cell(&mut s, "A1:A7", "m/d/yyyy");
    for (cell, source) in [("B1", "A1"), ("B2", "A2"), ("B3", "A3"), ("B4", "A4")] {
        py(&mut s, cell, &format!("[[type(xl(\"{source}\")).__name__, xl(\"{source}\")]]"));
    }
    py(&mut s, "E1", "xl(\"A5\").isoformat()");
    py(&mut s, "E2", "xl(\"A6\").isoformat()");
    py(&mut s, "E3", "xl(\"A7\").isoformat()");
    // Going over the day that never was, in Python's calendar, which has none.
    py(&mut s, "E4", "import datetime\nxl(\"A5\") + datetime.timedelta(days=1)");
    settle(&mut s);
    assert_eq!(v(&s, "E1"), Value::text("1900-02-28"));
    assert_eq!(v(&s, "E2"), Value::text("1900-03-01"));
    assert_eq!(v(&s, "E3"), Value::text("1900-01-01"));
    assert_eq!(v(&s, "E4"), num(61.0));
    for (row, type_name, number) in [(1, "int", 60.0), (2, "int", 0.0), (3, "float", 0.5), (4, "int", -1.0)] {
        assert_eq!(v(&s, &format!("B{row}")), Value::text(type_name), "{row}");
        assert_eq!(v(&s, &format!("C{row}")), num(number), "{row}: it stays the number");
    }
}

#[test]
fn a_range_reads_its_dates_and_a_spill_shows_them() {
    let Some(mut s) = session() else { return };
    s.execute("range.setValues", json!({"range": "A1", "values": [[45306, "x"], [45307, 3]]})).unwrap();
    format_cell(&mut s, "A1:A2", "m/d/yyyy");
    py(
        &mut s,
        "D1",
        "import datetime\n[datetime.date(2024, 1, 15), datetime.time(12), datetime.datetime(2024, 1, 15, 6), 'text', 7, datetime.timedelta(hours=36)]",
    );
    settle(&mut s);
    assert_eq!((v(&s, "D1"), format_of(&s, "D1")), (num(45306.0), "m/d/yyyy h:mm".into()), "a column with a time shows it on all its dates");
    assert_eq!((v(&s, "D2"), format_of(&s, "D2")), (num(0.5), "h:mm:ss".into()));
    assert_eq!((v(&s, "D3"), format_of(&s, "D3")), (num(45306.25), "m/d/yyyy h:mm".into()));
    assert_eq!((v(&s, "D4"), format_of(&s, "D4")), (Value::text("text"), "General".into()));
    assert_eq!((v(&s, "D5"), format_of(&s, "D5")), (num(7.0), "General".into()));
    assert_eq!((v(&s, "D6"), format_of(&s, "D6")), (num(1.5), "[h]:mm:ss".into()));
    // Something in the way: nothing is formatted.
    py(&mut s, "F1", "[xl(\"A1\"), xl(\"A2\")]");
    set(&mut s, "F2", "5");
    settle(&mut s);
    assert_eq!(v(&s, "F1"), Value::Error(CellError::Spill));
    assert_eq!(format_of(&s, "F2"), "General");
    set(&mut s, "F2", "");
    settle(&mut s);
    assert_eq!((v(&s, "F2"), format_of(&s, "F2")), (num(45307.0), "m/d/yyyy".into()));
}

#[test]
fn what_a_serial_can_not_hold_is_num_and_the_worker_stays() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "keep = 7\nkeep");
    py(&mut s, "A2", "import datetime\ndatetime.date(1800, 1, 1)");
    py(&mut s, "A3", "datetime.datetime.min");
    py(&mut s, "A4", "datetime.timedelta.max");
    py(&mut s, "A5", "datetime.date(1899, 12, 31)");
    py(&mut s, "A6", "datetime.date.max");
    py(&mut s, "A7", "datetime.datetime.max");
    py(&mut s, "A8", "keep + 1");
    settle(&mut s);
    for cell in ["A2", "A3", "A4", "A5"] {
        assert_eq!((v(&s, cell), kind(&s, cell)), (Value::Error(CellError::Num), Some("result.date")), "{cell}");
    }
    assert_eq!(v(&s, "A6"), num(2_958_465.0));
    let Value::Number(last) = v(&s, "A7") else { panic!() };
    assert!((2_958_465.0..2_958_466.0).contains(&last), "{last}: the last moment stays in the last day");
    assert_eq!(v(&s, "A8"), num(8.0), "the same worker, with its variables");
    // And it reads back as the date it is.
    py(&mut s, "B7", "xl(\"A7\").isoformat()");
    settle(&mut s);
    assert_eq!(v(&s, "B7"), Value::text("9999-12-31T23:59:59.999000"));
}

#[test]
fn a_time_zone_is_only_where_the_wall_clock_is() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "import datetime\ndatetime.datetime(2024, 1, 15, 12, tzinfo=datetime.timezone(datetime.timedelta(hours=-5)))");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(45306.5));
}

#[test]
fn a_cells_own_format_stays_and_a_format_is_no_undo_step() {
    let Some(mut s) = session() else { return };
    format_cell(&mut s, "A1", "yyyy-mm-dd");
    py(&mut s, "A1", "import datetime\ndatetime.date(2024, 1, 15)");
    py(&mut s, "A2", "datetime.date(2024, 1, 15)");
    let undo = s.doc().unwrap().undo.len();
    settle(&mut s);
    assert_eq!(s.doc().unwrap().undo.len(), undo, "a result and its format are no step");
    assert_eq!((format_of(&s, "A1"), format_of(&s, "A2")), ("yyyy-mm-dd".into(), "m/d/yyyy".into()));
}

/// A date column in and out of a DataFrame: what the brief's round trip looks like with pandas.
fn dataframe_round_trip(s: &mut Session, serial: f64) {
    s.execute("range.setValues", json!({"range": "A1", "values": [["when", "n"], [serial, 1], [serial + 1.0, 2], [null, 3]]})).unwrap();
    format_cell(s, "A2:A4", "m/d/yyyy");
    py(s, "D1", "xl(\"A1:B4\", headers=True).dtypes.iloc[0].kind");
    py(s, "D2", "str(xl(\"A1:B4\", headers=True)[\"when\"].iloc[2])");
    py(s, "F1", "d = xl(\"A1:B4\", headers=True)\nd[\"when\"] = d[\"when\"] + pd.Timedelta(days=1)\nd");
    settle(s);
    assert_eq!(v(s, "D1"), Value::text("M"), "a datetime column");
    assert_eq!(v(s, "D2"), Value::text("NaT"), "an empty cell");
    assert_eq!((v(s, "F1"), v(s, "G1")), (Value::text("when"), Value::text("n")));
    assert_eq!((v(s, "F2"), format_of(s, "F2")), (num(serial + 1.0), "m/d/yyyy".into()));
    assert_eq!((v(s, "F3"), format_of(s, "F3")), (num(serial + 2.0), "m/d/yyyy".into()));
    assert_eq!((v(s, "F4"), format_of(s, "F4")), (Value::Empty, "General".into()), "NaT is an empty cell");
    assert_eq!((v(s, "G2"), format_of(s, "G2")), (num(1.0), "General".into()));
    assert_eq!(format_of(s, "F1"), "General", "the header isn't a date");
}

#[test]
fn a_dataframe_date_column_round_trips_in_the_1900_system() {
    let Some(mut s) = pandas_session() else { return };
    dataframe_round_trip(&mut s, 45306.0);
}

#[test]
fn a_dataframe_date_column_round_trips_in_the_1904_system() {
    let Some(mut s) = pandas_session() else { return };
    on_1904(&mut s);
    dataframe_round_trip(&mut s, 43844.0);
}

#[test]
fn numpy_and_pandas_dates_and_durations_come_back_as_serials() {
    let Some(mut s) = pandas_session() else { return };
    for (cell, code, number, format) in [
        ("A1", "np.datetime64(\"2024-01-15\")", num(45306.0), "m/d/yyyy"),
        ("A2", "np.datetime64(\"2024-01-15T12:00\", \"ns\")", num(45306.5), "m/d/yyyy h:mm"),
        ("A3", "np.timedelta64(36, \"h\")", num(1.5), "[h]:mm:ss"),
        ("A4", "pd.Timestamp(\"2024-01-15\")", num(45306.0), "m/d/yyyy"),
        ("A5", "pd.Timestamp(\"2024-01-15 12:00\", tz=\"Asia/Tokyo\")", num(45306.5), "m/d/yyyy h:mm"),
        ("A6", "pd.Timedelta(days=1, hours=12)", num(1.5), "[h]:mm:ss"),
        ("A7", "pd.NaT", Value::Empty, "General"),
        ("A8", "np.datetime64(\"NaT\")", Value::Empty, "General"),
        ("A9", "np.datetime64(10**15, \"Y\")", Value::Error(CellError::Num), "General"),
        ("A10", "pd.Timedelta(days=-1)", num(-1.0), "General"),
    ] {
        py(&mut s, cell, code);
        settle(&mut s);
        assert_eq!((v(&s, cell), format_of(&s, cell)), (number, format.to_string()), "{code}");
    }
    // Across a spill, from a Series, the values of a column, and an index of dates.
    py(&mut s, "C1", "pd.Series(pd.to_datetime([\"2024-01-15\", \"2024-01-16\"]))");
    py(&mut s, "D1", "pd.Series(pd.to_datetime([\"2024-01-15\", \"2024-01-16\"])).values");
    py(&mut s, "E1", "pd.date_range(\"2024-01-15\", periods=2)");
    py(&mut s, "F1", "pd.DataFrame({\"at\": pd.to_datetime([\"2024-01-15 12:00\", \"2024-01-15 00:00\"])})");
    settle(&mut s);
    for column in ["C", "D", "E"] {
        let (top, under) = (format!("{column}1"), format!("{column}2"));
        assert_eq!((v(&s, &top), v(&s, &under)), (num(45306.0), num(45307.0)), "{column}");
        assert_eq!(format_of(&s, &under), "m/d/yyyy", "{column}");
    }
    assert_eq!((v(&s, "F2"), v(&s, "F3")), (num(45306.5), num(45306.0)));
    assert_eq!((format_of(&s, "F2"), format_of(&s, "F3")), ("m/d/yyyy h:mm".into(), "m/d/yyyy h:mm".into()), "one format down a column");
}

// ---------------------------------------------------------------- initialization code

fn init(s: &mut Session, code: &str) {
    s.execute("python.initialization", json!({"code": code})).unwrap();
}

/// The run log's rows of the initialization code.
fn init_runs(s: &Session) -> Vec<Option<&'static str>> {
    s.python_run_log().filter(|r| r.initialization).map(|r| r.diagnostic).collect()
}

#[test]
fn an_import_in_initialization_is_there_for_the_cells() {
    let Some(mut s) = session() else { return };
    init(&mut s, "import math as m\nrate = 2\nprint('ready')");
    py(&mut s, "A1", "m.floor(2.5) + rate");
    py(&mut s, "A2", "rate = rate * 10\nrate");
    py(&mut s, "A3", "rate");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(4.0));
    assert_eq!(v(&s, "A2"), num(20.0));
    assert_eq!(v(&s, "A3"), num(20.0), "it shares their variables");
    let log: Vec<_> = s.python_run_log().collect();
    assert!(log[0].initialization, "it ran first");
    assert_eq!(log[0].stdout, "ready\n", "what it printed is in the log");
    // Changed, everything runs again from the top, without what the old code left.
    init(&mut s, "import math as n\nrate = 3");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), num(30.0));
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Python), "the old name went with the old worker");
    assert_eq!(kind(&s, "A1"), Some("python.name"));
    // An undo brings the old code back, and the cells follow.
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(4.0));
    // xl() has no cell to read for in it, and says so.
    init(&mut s, "x = xl('A1')");
    settle(&mut s);
    let d = s.python_diagnostic(0, CellRef::parse("A1").unwrap()).unwrap();
    assert_eq!(d.kind.id, "init.failed");
    assert!(d.detail.contains("xl() only works while a cell runs"), "{}", d.detail);
}

#[test]
fn a_failing_initialization_shows_in_every_cell_until_it_is_fixed() {
    let Some(mut s) = session() else { return };
    py(&mut s, "A1", "1 + 1");
    py(&mut s, "A2", "2 + 2");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(2.0));
    // A person's import that fails is the code's failure, with Python's message and line.
    init(&mut s, "x = 1\nimport a_package_nobody_has\ny = 2");
    settle(&mut s);
    for cell in ["A1", "A2"] {
        assert_eq!(v(&s, cell), Value::Error(CellError::Python), "{cell}");
        let d = s.python_diagnostic(0, CellRef::parse(cell).unwrap()).unwrap();
        assert_eq!((d.kind.id, d.line), ("init.failed", Some(2)));
        assert!(d.detail.contains("No module named 'a_package_nobody_has'"), "{}", d.detail);
    }
    assert_eq!(init_runs(&s).last().copied().flatten(), Some("init.failed"));
    let rows = s.execute("python.diagnostics", json!({})).unwrap();
    assert_eq!(rows.as_array().unwrap().iter().filter(|r| r["kind"] == "init.failed").count(), 1, "{rows}");
    assert_eq!(s.execute("python.environment", json!({})).unwrap()["initialization"]["kind"], "init.failed");
    // A syntax error too, and an exception, each on its line.
    for (code, line, says) in [("a = 1\nif", 2, "SyntaxError"), ("\n\n1 / 0", 3, "ZeroDivisionError")] {
        init(&mut s, code);
        settle(&mut s);
        let d = s.python_diagnostic(0, CellRef::parse("A1").unwrap()).unwrap();
        assert_eq!((d.kind.id, d.line), ("init.failed", Some(line)), "{code}");
        assert!(d.detail.contains(says), "{}", d.detail);
    }
    // Recovery: fixed, the cells run, in a worker with nothing of what failed.
    init(&mut s, "rate = 5");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (num(2.0), num(4.0)));
    assert_eq!(kind(&s, "A1"), None);
    assert!(s.execute("python.environment", json!({})).unwrap()["initialization"].is_null());
    // What failed partway left nothing: a name from before the failure isn't there.
    init(&mut s, "left = 1\n1 / 0");
    settle(&mut s);
    init(&mut s, "rate = 6");
    py(&mut s, "A3", "left");
    settle(&mut s);
    assert_eq!(kind(&s, "A3"), Some("python.name"));
    assert_eq!(v(&s, "A1"), num(2.0));
}

#[test]
fn code_from_a_file_goes_on_without_what_it_cant_import() {
    let Some(mut s) = session() else { return };
    // A workbook file whose environment holds what such code holds: imports and options, none of it
    // written by a person. The packages may or may not be here; the cells run either way.
    let code = "import numpy as np\n\
                import matplotlib.pyplot as plt\n\
                import seaborn as sns\n\
                import a_package_nobody_has as nobody\n\
                import excel\n\
                from another_package_nobody_has import thing\n\
                import warnings\n\
                warnings.simplefilter('ignore')\n\
                excel.set_some_option(excel.some_value)\n\
                nobody.configure(1)\n\
                plt.style.use('default')\n\
                ready = 41\n";
    let saved = s.execute("file.saveBytes", json!({"format": "json"})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(saved["base64"].as_str().unwrap()).unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    doc["python_env"] = json!({"code": code, "userModified": false});
    let opened = gridcraft_engine::io::base64_encode(&serde_json::to_vec(&doc).unwrap());
    s.execute("file.open", json!({"name": "from-file.json", "base64": opened})).unwrap();
    py(&mut s, "A1", "ready + 1");
    py(&mut s, "A2", "repr(plt)");
    settle(&mut s);
    assert_eq!(kind(&s, "A1"), Some("off.workbook"), "a file's workbook stays off");
    assert_eq!(s.python_run_log().count(), 0, "nothing ran");
    s.set_python_workbook_enabled(true);
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(42.0), "the rest of it ran, and so do the cells");
    assert_eq!(kind(&s, "A1"), None);
    assert_eq!(s.python_init_failure(), None);
    // What it couldn't have is reported once for each package, at the first line that wanted it, and
    // what the worker gives anyway isn't: plots and numpy wait until they are used.
    let rows = s.execute("python.diagnostics", json!({})).unwrap();
    // `plt.style.use` is the first use of `plt`, so whether it worked depends on this Python.
    let skipped: Vec<(String, u64)> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "init.skipped" && r["detail"] != "matplotlib")
        .map(|r| (r["detail"].as_str().unwrap().to_string(), r["line"].as_u64().unwrap()))
        .collect();
    assert_eq!(skipped, [("a_package_nobody_has".to_string(), 4), ("another_package_nobody_has".to_string(), 6), ("excel".to_string(), 9)], "{rows}");
    assert_eq!(init_runs(&s), [Some("init.skipped")]);
    // A line that isn't an import or an option is still the code's failure.
    for (bad, line, says) in [("import nothing_here_either\n1 / 0", 2, "ZeroDivisionError"), ("undefined_name + 1", 1, "NameError")] {
        let mut doc = doc.clone();
        doc["python_env"] = json!({"code": bad, "userModified": false});
        let opened = gridcraft_engine::io::base64_encode(&serde_json::to_vec(&doc).unwrap());
        s.execute("file.open", json!({"name": "bad.json", "base64": opened})).unwrap();
        py(&mut s, "A1", "1");
        s.set_python_workbook_enabled(true);
        settle(&mut s);
        let d = s.python_diagnostic(0, CellRef::parse("A1").unwrap()).unwrap();
        assert_eq!((d.kind.id, d.line), ("init.failed", Some(line)), "{bad}");
        assert!(d.detail.contains(says), "{}", d.detail);
    }
}

#[test]
fn a_persons_code_can_keep_excels_own_module() {
    // Code a person changed in Excel keeps the lines around their own: Excel's own module, which no
    // other Python has, doesn't stop it, while anything else that fails does.
    let Some(mut s) = session() else { return };
    init(&mut s, "import excel\nexcel.set_some_option(excel.some_value)\nrate = 2");
    py(&mut s, "A1", "rate");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(2.0));
    let rows = s.execute("python.diagnostics", json!({})).unwrap();
    let row = rows.as_array().unwrap().iter().find(|r| r["kind"] == "init.skipped").expect("it says so");
    assert_eq!((row["detail"].as_str(), row["line"].as_u64(), row["install"].clone()), (Some("excel"), Some(2), serde_json::Value::Null));
    assert!(!rows.as_array().unwrap().iter().any(|r| r["kind"] == "packages.missing"), "nothing to install: {rows}");
    // Another module on the same line isn't excused.
    init(&mut s, "import excel, a_package_nobody_has\nrate = 3");
    settle(&mut s);
    assert_eq!(kind(&s, "A1"), Some("init.failed"));
}

/// A session without Python that has opened the file: what the file holds.
fn opened(path: &std::path::Path) -> Session {
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s
}

#[test]
fn a_save_with_wait_has_the_result_in_the_file() {
    let Some(mut s) = session() else { return };
    let path = std::env::temp_dir().join(format!("gridcraft-python-cells-wait-{}.xlsx", std::process::id()));
    let _ = std::fs::remove_file(&path);
    set(&mut s, "A1", "20");
    py(&mut s, "B1", "import time\ntime.sleep(0.3)\nxl(\"A1\") + 22");
    assert!(s.python_busy(), "the cell is still to run");
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    assert_eq!(r["unfinished"], json!([]), "{r}");
    assert_eq!(v(&opened(&path), "B1"), num(42.0), "the file has the result");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_save_at_the_bound_says_the_cell_was_not_finished_and_keeps_its_code() {
    let Some(mut s) = session() else { return };
    let path = std::env::temp_dir().join(format!("gridcraft-python-cells-bound-{}.xlsx", std::process::id()));
    let _ = std::fs::remove_file(&path);
    py(&mut s, "A1", "20 + 22");
    settle(&mut s);
    // It runs before A2's result: A2 keeps its last one while it waits for this.
    py(&mut s, "A2", "7");
    settle(&mut s);
    let code = "import time\ntime.sleep(60)\n1";
    py(&mut s, "A1", code);
    let r = s.execute("file.save", json!({"path": path, "waitSeconds": 0.5})).unwrap();
    assert_eq!(r["unfinished"], json!(["Sheet1!A1", "Sheet1!A2"]), "{r}");
    let mut o = opened(&path);
    assert_eq!(v(&o, "A2"), num(7.0), "the last result");
    assert_eq!(o.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["code"], json!(code));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn matplotlib_and_seaborn_load_in_the_sandbox_and_it_stays_read_only() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", "import matplotlib.pyplot as pyplot\npyplot.get_backend().lower()");
    py(&mut s, "A2", "plt.__name__");
    py(&mut s, "A3", "sns.__name__");
    // A file's name is refused, as for any other write, and matplotlib's way in gave nothing away.
    py(&mut s, "A4", "plt.plot([1])\nplt.savefig('/tmp/gridcraft-plot-test.png')");
    py(&mut s, "A5", "import tempfile\ntempfile.mkdtemp()");
    py(&mut s, "A6", "import tempfile\ntempfile.mkdtemp(prefix='other-')");
    py(&mut s, "A7", "open('/tmp/gridcraft-plot-test.txt', 'w')");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("agg"));
    assert_eq!(v(&s, "A2"), Value::text("matplotlib.pyplot"));
    assert_eq!(v(&s, "A3"), Value::text("seaborn"));
    for cell in ["A4", "A5", "A6", "A7"] {
        assert_eq!(kind(&s, cell), Some("denied.file"), "{cell}");
    }
    assert!(!std::path::Path::new("/tmp/gridcraft-plot-test.png").exists());
    assert!(!std::path::Path::new("/tmp/gridcraft-plot-test.txt").exists());
}

/// `(width, height)` from `python.image`, and the bytes' pixels, for a cell that gave a picture.
fn picture(s: &mut Session, cell: &str) -> (u64, u64, u64) {
    let r = s.execute("python.image", json!({"cell": cell})).unwrap_or_else(|e| panic!("{cell}: {e}: {:?}", kind(s, cell)));
    (r["width"].as_u64().unwrap(), r["height"].as_u64().unwrap(), r["density"].as_u64().unwrap())
}

#[test]
fn a_real_plot_comes_back_from_its_file_for_either_return_type() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", "import matplotlib.pyplot as plt\nplt.figure(figsize=(2, 1))\nplt.plot([1, 2])\nplt.title('<one>')");
    s.execute(
        "python.set",
        json!({"cell": "A2", "code": "import matplotlib.pyplot as plt\nplt.figure(figsize=(1, 2))\nplt.plot([2, 1])", "output": "value"}),
    )
    .unwrap();
    settle(&mut s);
    let image = |s: &mut Session, cell: &str| s.execute("python.image", json!({"cell": cell})).unwrap();
    let (object, value) = (image(&mut s, "A1"), image(&mut s, "A2"));
    assert!(object["base64"] != value["base64"]);
    let path = std::env::temp_dir().join(format!("gridcraft-python-cells-plots-{}.xlsx", std::process::id()));
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    let mut back = opened(&path);
    assert_eq!(image(&mut back, "A1"), object, "what a run gave, with no run");
    assert_eq!(image(&mut back, "A2"), value);
    let sheet = back.doc().unwrap().wb.sheet(0).unwrap().clone();
    assert!(sheet.images.is_empty() && sheet.cell_pictures.is_empty());
    assert!(sheet.picture_at(CellRef::parse("A1").unwrap()).is_none() && sheet.picture_at(CellRef::parse("A2").unwrap()).is_some());
    std::fs::remove_file(&path).ok();
}

const IRIS: &str = "import numpy as _np\n\
    _rng = _np.random.default_rng(1)\n\
    iris = pd.DataFrame(_rng.normal(size=(150, 4)), columns=['sepal_length', 'sepal_width', 'petal_length', 'petal_width'])\n";

#[test]
fn microsofts_three_samples_give_pictures() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", IRIS);
    // The pair plot ends in an assignment, and comes out the size Excel reports.
    py(&mut s, "A2", "import seaborn as sns\npairplot = sns.pairplot(iris)");
    // The scatter plot ends in a title.
    py(
        &mut s,
        "A3",
        "import matplotlib.pyplot as plt\nplt.figure()\nplt.scatter(iris['sepal_length'], iris['sepal_width'])\nplt.xlabel('sepal_length')\nplt.ylabel('sepal_width')\nplt.title('Sepal length and width analysis')",
    );
    // The non-Latin sample ends in plt.show().
    py(
        &mut s,
        "A4",
        "import matplotlib.pyplot as plt\nfrom matplotlib.font_manager import FontProperties\nfont_path = excel.FontPath.CHINESE_SIMPLIFIED\nfont_properties = FontProperties(fname=font_path)\nplt.rcParams['font.family'] = font_properties.get_name()\nmyplot = pd.DataFrame({'\u{6b27}\u{6587}': [1, 2, 3], '\u{6bd4}\u{5c14}': [1, 2, 3]}).plot(x='\u{6b27}\u{6587}')\nplt.show()",
    );
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::text("Image"), "{:?}", s.python_diagnostic(0, CellRef::parse("A2").unwrap()));
    let (w, h, density) = picture(&mut s, "A2");
    assert!((980..=992).contains(&w) && (980..=992).contains(&h), "{w} x {h}");
    assert_eq!(density, 2, "drawn sharper than it reports");
    assert_eq!(v(&s, "A3"), Value::text("Image"));
    picture(&mut s, "A3");
    if cfg!(target_os = "macos") {
        assert_eq!(v(&s, "A4"), Value::text("Image"), "{:?}", kind(&s, "A4"));
        picture(&mut s, "A4");
    }
}

#[test]
fn what_a_cell_ends_with_decides_whether_it_is_a_picture() {
    let Some(mut s) = plots_session() else { return };
    let pictures = [
        "plt.figure()\nplt.plot([1, 2])",
        "plt.figure().add_subplot()",
        "plt.subplots()",
        "plt.subplots(2, 2)[1]",
        "plt.figure()\nplt.bar([1], [2])",
        "plt.figure()\nplt.hist([1, 2, 3])",
        "plt.figure()\nplt.boxplot([1, 2, 3])",
        "plt.figure()\nplt.pie([1, 2])",
        "plt.figure()\nplt.plot([1])\nplt.show()",
        "plt.figure()\nplt.plot([1])\nz = 1",
        "plt.figure()\nplt.plot([1])\nNone",
        "pd.DataFrame({'a': [1, 2]}).plot()",
        "pd.DataFrame({'a': [1, 2]}).hist()",
        "pd.DataFrame({'a': [1, 2]})['a'].plot.bar()",
        "sns.relplot(pd.DataFrame({'a': [1, 2], 'b': [3, 4]}), x='a', y='b')",
        "sns.lineplot(pd.DataFrame({'a': [1, 2], 'b': [3, 4]}), x='a', y='b')",
        "import seaborn.objects as so\nso.Plot(pd.DataFrame({'a': [1, 2], 'b': [3, 4]}), x='a', y='b').add(so.Line())",
        "from PIL import Image\nImage.new('RGB', (40, 30), (10, 120, 200))",
        "from PIL import Image\nImage.new('L', (5, 6))",
        "from matplotlib.figure import Figure\nf = Figure()\nf.add_subplot().plot([1])\nf",
        "class P:\n    def _repr_png_(self):\n        from PIL import Image\n        import io\n        b = io.BytesIO()\n        Image.new('RGB', (9, 7)).save(b, 'PNG')\n        return b.getvalue()\nP()",
        "class J:\n    def _repr_jpeg_(self):\n        from PIL import Image\n        import io\n        b = io.BytesIO()\n        Image.new('RGB', (9, 7)).save(b, 'JPEG')\n        return b.getvalue(), {}\nJ()",
    ];
    for code in pictures {
        py(&mut s, "A1", code);
        settle(&mut s);
        assert_eq!(v(&s, "A1"), Value::text("Image"), "{code}: {:?}", s.python_diagnostic(0, CellRef::parse("A1").unwrap()));
    }
    // A number or a table comes first, an open figure doesn't matter, and an object that can't draw
    // itself stays an object.
    for (code, want) in [
        ("plt.figure()\nplt.plot([1])\n5", num(5.0)),
        ("plt.figure()\nplt.plot([1])\n'text'", Value::text("text")),
        ("x = 5\nx", num(5.0)),
        ("[1.0]", num(1.0)),
    ] {
        py(&mut s, "A1", code);
        settle(&mut s);
        assert_eq!(v(&s, "A1"), want, "{code}");
    }
    for (code, label) in [
        ("class Bad:\n    def _repr_png_(self):\n        raise ValueError()\nBad()", "Bad"),
        ("class Empty:\n    def _repr_png_(self):\n        return None\nEmpty()", "Empty"),
        ("class NotOne:\n    def _repr_png_(self):\n        return b'nothing like a picture'\nNotOne()", "NotOne"),
        ("class Svg:\n    def _repr_svg_(self):\n        return '<svg/>'\nSvg()", "Svg"),
    ] {
        py(&mut s, "A1", code);
        settle(&mut s);
        assert_eq!(v(&s, "A1"), Value::text(label), "{code}");
    }
}

#[test]
fn a_figure_nobody_changed_is_no_result() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", "plt.figure()\nplt.plot([1, 2])");
    py(&mut s, "A2", "y = 2");
    py(&mut s, "A3", "print('hello')");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("Image"));
    assert_eq!(v(&s, "A2"), Value::Empty);
    assert_eq!(v(&s, "A3"), Value::Empty);
}

#[test]
fn lines_dont_pile_up_when_cells_run_again() {
    let Some(mut s) = plots_session() else { return };
    let lines = "len(plt.gcf().axes[0].lines)";
    set(&mut s, "C1", "1");
    py(&mut s, "A1", "plt.plot([xl('C1'), 2])\nplt.gcf()");
    py(&mut s, "A2", "plt.plot([2, 1])");
    py(&mut s, "A3", lines);
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("Image"));
    assert_eq!(v(&s, "A2"), Value::text("Image"));
    assert_eq!(v(&s, "A3"), num(2.0), "a later cell draws on the same figure");
    for n in 2..5 {
        set(&mut s, "C1", &n.to_string());
        settle(&mut s);
        assert_eq!(v(&s, "A3"), num(2.0), "after run {n}");
    }
    // A2 changed A1's figure, so running A2 on its own would add a line each time: it starts from A1.
    py(&mut s, "A2", "plt.plot([2, 2])");
    settle(&mut s);
    assert_eq!(v(&s, "A3"), num(2.0));
    py(&mut s, "A2", "plt.plot([2, 3])");
    settle(&mut s);
    assert_eq!(v(&s, "A3"), num(2.0));
    // Run All, and Reset, start clean.
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A3"), num(2.0));
    s.execute("python.reset", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A3"), num(2.0));
}

#[test]
fn a_later_cell_can_finish_an_earlier_cells_plot() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", "plt.plot([1, 2])");
    py(&mut s, "A2", "plt.title('Late')");
    py(&mut s, "A3", "plt.gca().get_title()");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::text("Image"));
    assert_eq!(v(&s, "A3"), Value::text("Late"));
}

#[test]
fn a_figure_too_large_to_draw_says_so_and_costs_no_memory() {
    let Some(mut s) = plots_session() else { return };
    for code in [
        "plt.figure(figsize=(200, 200))\nplt.plot([1])",
        "plt.figure(figsize=(90, 1), dpi=100)\nplt.plot([1])",
        "plt.figure(dpi=5000)\nplt.plot([1])",
        "plt.figure()\nplt.text(1e7, 1e7, 'far away')\nplt.gcf()",
    ] {
        py(&mut s, "A1", code);
        settle(&mut s);
        assert_eq!(kind(&s, "A1"), Some("plot.tooLarge"), "{code}");
    }
    // A picture at the limit comes back at its own resolution, not twice it.
    py(&mut s, "A1", "plt.figure(figsize=(80, 5), dpi=100)\nplt.plot([1])\nplt.gca().set_title('wide')");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("Image"), "{:?}", kind(&s, "A1"));
    assert_eq!(picture(&mut s, "A1").2, 1);
    // A figure that breaks while it draws is the code's own error.
    py(&mut s, "A1", "plt.figure()\nplt.title('$\\\\frac{$')\nplt.gcf()");
    settle(&mut s);
    assert_eq!(kind(&s, "A1"), Some("python.exception"));
    // The worker is still there.
    py(&mut s, "A1", "1 + 1");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(2.0));
}

#[test]
fn text_in_other_scripts_draws_without_choosing_a_font() {
    let Some(mut s) = plots_session() else { return };
    if !cfg!(target_os = "macos") {
        return;
    }
    py(
        &mut s,
        "A1",
        "import warnings\nwith warnings.catch_warnings(record=True) as w:\n    warnings.simplefilter('always')\n    plt.figure()\n    plt.title('\u{6b27}\u{6587} \u{d55c}\u{ae00}')\n    plt.gcf().canvas.draw()\nlen(w)",
    );
    settle(&mut s);
    assert_eq!(v(&s, "A1"), num(0.0), "no glyph was missing");
    py(&mut s, "A2", "excel.FontPath.KOREAN.endswith(('.ttf', '.ttc', '.otf'))");
    py(&mut s, "A3", "excel.FontPath.NOT_A_LANGUAGE");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Bool(true));
    assert_eq!(kind(&s, "A3"), Some("python.exception"));
}

/// The pixels of the first floating picture, by its PNG header.
fn drawn(s: &Session) -> (u32, u32) {
    let wb = &s.doc().unwrap().wb;
    let data = &wb.active().unwrap().images[0].data;
    let be = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap());
    (be(16), be(20))
}

fn place(s: &mut Session, cell: &str) -> u32 {
    let r = s.execute("python.displayOverCells", json!({"cell": cell, "at": "H1"})).unwrap();
    r["image"].as_u64().unwrap() as u32
}

fn resize_to(s: &mut Session, id: u32, width: f64, height: f64) {
    s.execute("object.move", json!({"kind": "image", "id": id, "width": width, "height": height})).unwrap();
    settle(s);
}

#[test]
fn a_free_figure_is_drawn_again_at_the_size_it_is_given() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", IRIS);
    py(&mut s, "A2", "plt.plot([1, 2])\nplt.title('A title')\nplt.xlabel('x')");
    py(&mut s, "A3", "[len(plt.gca().lines), list(plt.gcf().get_size_inches())]");
    settle(&mut s);
    let id = place(&mut s, "A2");
    let log = s.python_run_log().count();
    resize_to(&mut s, id, 900.0, 300.0);
    assert_eq!(drawn(&s), (1800, 600), "twice the pixels of the size it is shown at");
    resize_to(&mut s, id, 500.0, 700.0);
    assert_eq!(drawn(&s), (1000, 1400));
    assert_eq!(s.python_run_log().count(), log, "no cell ran");
    // The next cell finds the figure as it was.
    py(&mut s, "B1", "list(plt.gcf().get_size_inches()) + [plt.gca().get_position().width > 0.7]");
    settle(&mut s);
    assert_eq!((v(&s, "B1"), v(&s, "B2"), v(&s, "B3")), (num(6.4), num(4.8), Value::Bool(true)));
    // Below the floor the figure is drawn at the floor, and the picture is scaled.
    resize_to(&mut s, id, 100.0, 100.0);
    assert_eq!(drawn(&s), (640, 480), "half its natural size, twice over");
}

#[test]
fn every_kind_of_plot_can_be_drawn_again() {
    let Some(mut s) = plots_session() else { return };
    py(&mut s, "A1", IRIS);
    settle(&mut s);
    let kinds = [
        "plt.figure()\nplt.plot([1, 2])\nplt.title('t')\nplt.xlabel('x')\nplt.ylabel('y')\nplt.legend(['a'])",
        "f, a = plt.subplots(2, 2)\nf.tight_layout()\nf",
        "f, a = plt.subplots()\nf.colorbar(a.scatter(iris['sepal_length'], iris['sepal_width'], c=iris['petal_length']))\nf",
        "iris.plot()",
        "f, a = plt.subplots(1, 2, layout='constrained')\na[0].set_title('left')\nf",
        "f, a = plt.subplots()\na.inset_axes([.5, .5, .4, .4]).plot([2, 1])\nf",
        "f, a = plt.subplots()\na.imshow(np.arange(100).reshape(10, 10))\nf",
        "sns.pairplot(iris)",
        "sns.jointplot(iris, x='sepal_length', y='sepal_width')",
        "sns.relplot(iris, x='sepal_length', y='sepal_width')",
        "plt.figure()\nsns.boxplot(iris)",
    ];
    for code in kinds {
        py(&mut s, "B1", code);
        settle(&mut s);
        assert_eq!(v(&s, "B1"), Value::text("Image"), "{code}: {:?}", kind(&s, "B1"));
        let id = place(&mut s, "B1");
        let (w, h, _) = picture(&mut s, "B1");
        let big = (w.max(640) as f64 * 1.2, h as f64 * 1.3);
        resize_to(&mut s, id, big.0.min(2000.0), big.1.min(2000.0));
        assert_eq!(kind(&s, "B1"), None, "{code}");
        let sized = s.doc().unwrap().wb.active().unwrap().images[0].rendered;
        assert!(sized.is_some(), "{code}");
        s.execute("object.delete", json!({"kind": "image", "id": id})).unwrap();
    }
}

#[test]
fn a_figure_whose_size_the_code_chose_isnt_drawn_again() {
    let Some(mut s) = plots_session() else { return };
    py(
        &mut s,
        "A1",
        "from seaborn import pairplot as _pp\ndf = pd.DataFrame(np.arange(12.0).reshape(4, 3), columns=list('abc'))\nFIXED = dict(figsize=(3, 3))\ndef helper():\n    return plt.subplots(figsize=(3, 3))[0]\ndef seaborn_helper():\n    return sns.pairplot(df, height=2)",
    );
    settle(&mut s);
    assert_eq!(kind(&s, "A1"), None);
    let fixed = [
        "plt.figure(figsize=(3, 3))\nplt.plot([1])",
        "plt.subplots(figsize=(3, 3))[0]",
        "df.plot(figsize=(3, 3))",
        "sns.pairplot(df, height=2)",
        "sns.relplot(df, x='a', y='b', aspect=2)",
        "plt.figure()\nplt.gcf().set_size_inches(3, 3)\nplt.plot([1])",
        "plt.figure()\nplt.gcf().set_figwidth(3)\nplt.plot([1])",
        "plt.figure(**FIXED)\nplt.plot([1])",
        "helper()",
        "seaborn_helper()",
    ];
    let free = [
        "plt.figure()\nplt.plot([1])",
        "plt.subplots()[0]",
        "df.plot()",
        "df.hist()",
        "sns.pairplot(df)",
        "sns.relplot(df, x='a', y='b')",
        "plt.figure()\nplt.bar([1, 2], height=[3, 4])",
    ];
    let can_be_redrawn = |s: &Session, cell: &str| {
        let wb = &s.doc().unwrap().wb;
        wb.active().unwrap().cell(CellRef::parse(cell).unwrap()).unwrap().py.as_deref().unwrap().image().unwrap().floor.is_some()
    };
    for code in fixed {
        py(&mut s, "B1", code);
        settle(&mut s);
        assert_eq!(v(&s, "B1"), Value::text("Image"), "{code}: {:?}", kind(&s, "B1"));
        assert!(!can_be_redrawn(&s, "B1"), "{code}");
    }
    for code in free {
        py(&mut s, "B1", code);
        settle(&mut s);
        assert_eq!(v(&s, "B1"), Value::text("Image"), "{code}: {:?}", kind(&s, "B1"));
        assert!(can_be_redrawn(&s, "B1"), "{code}");
    }
    // Settings that choose a size count, from then on.
    py(&mut s, "A2", "plt.rcParams['figure.figsize'] = (3, 3)");
    py(&mut s, "B2", "plt.figure()\nplt.plot([1])");
    settle(&mut s);
    assert!(!can_be_redrawn(&s, "B2"));
    // A figure saved to a buffer by the code hasn't had its size chosen.
    py(&mut s, "A2", "plt.rcdefaults()");
    py(&mut s, "B2", "import io\nplt.figure()\nplt.plot([1])\nplt.gcf().savefig(io.BytesIO(), dpi=200)\nplt.gcf()");
    settle(&mut s);
    assert!(can_be_redrawn(&s, "B2"));
}
