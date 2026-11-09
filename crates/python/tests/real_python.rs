//! The shim on a real interpreter. Without one the tests print why and pass, except on CI with a
//! `python3` on PATH, where a missing interpreter is a failure.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::disallowed_methods)] // the clock, off wasm

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gridcraft_core::{Array, CellError, Value};
use gridcraft_python::interp::{self, Detection, SearchEnv};
use gridcraft_python::process::ProcessTransport;
use gridcraft_python::protocol::{MAX_PREVIEW_CHARS, MAX_TABLE_BYTES, MAX_TABLE_COLS, MAX_TABLE_ROWS, MAX_TABLE_TEXT_BYTES, Table};
use gridcraft_python::{Client, Clock, Dated, Day, Input, Limits, Outcome, Output, PyError, RunResult, SelfTest, Stamp, Transport};

fn python() -> Option<PathBuf> {
    let env = SearchEnv::from_process();
    let found = match interp::detect(&env, None, interp::PROBE_TIMEOUT) {
        Detection::Found { path, .. } => Some(path),
        _ => None,
    };
    if found.is_none() {
        let ci = std::env::var_os("CI").is_some();
        let on_path = !interp::candidates(&SearchEnv { path: env.path.clone(), ..SearchEnv::default() }).is_empty();
        assert!(!(ci && on_path), "CI has a python on PATH, but no Python 3.11 or newer was found");
        println!("skipped: no Python 3.11 or newer found");
    }
    found
}

fn client(python: PathBuf) -> Client {
    Client::new(
        Box::new(ProcessTransport::new(python)),
        Limits { timeout: Duration::from_secs(10), start_timeout: Duration::from_secs(20), ..Limits::default() },
    )
}

fn run(c: &mut Client, code: &str) -> Outcome {
    c.run(code, BTreeMap::new()).unwrap()
}

fn value(o: Outcome) -> Value {
    match o {
        Outcome::Ok { output: Output::Value(v), .. } => v,
        other => panic!("not a value: {other:?}"),
    }
}

fn array(rows: usize, cols: usize, data: Vec<Value>) -> Value {
    Array::new(rows, cols, data).map(Value::from).unwrap()
}

#[test]
fn values_and_errors() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    assert_eq!(value(run(&mut c, "1 + 2")), Value::Number(3.0));
    assert_eq!(c.self_test(), Some(&SelfTest::NotChecked));
    assert_eq!(value(run(&mut c, "'text'")), Value::text("text"));
    assert_eq!(value(run(&mut c, "True")), Value::Bool(true));
    assert_eq!(value(run(&mut c, "x = 1")), Value::Empty, "no last expression");
    assert_eq!(value(run(&mut c, "None")), Value::Empty);
    assert_eq!(value(run(&mut c, "float('nan')")), Value::Error(CellError::Num));
    assert_eq!(value(run(&mut c, "10 ** 400")), Value::Error(CellError::Num));
    assert_eq!(
        value(run(&mut c, "[[1, 'a'], [True]]")),
        array(2, 2, vec![1.0.into(), Value::text("a"), Value::Bool(true), Value::Error(CellError::NA)])
    );
    assert_eq!(value(run(&mut c, "[1, 2, 3]")), array(3, 1, vec![1.0.into(), 2.0.into(), 3.0.into()]), "a flat list is a column");
    assert_eq!(value(run(&mut c, "[]")), Value::Error(CellError::Calc));
    match run(&mut c, "{'a': 1}") {
        Outcome::Ok { output: Output::Object { type_name, preview, .. }, .. } => {
            assert_eq!(type_name, "dict");
            assert_eq!(preview, "{'a': 1}");
        }
        other => panic!("{other:?}"),
    }
    match run(&mut c, "list(range(1000))[0:0] or set(range(10**4))") {
        Outcome::Ok { output: Output::Object { preview, .. }, .. } => assert_eq!(preview.chars().count(), 255),
        other => panic!("{other:?}"),
    }

    // An error carries its line in the cell's code, and the message.
    assert_eq!(
        run(&mut c, "a = 1\nb = a + undefined_name\nb"),
        Outcome::PythonError {
            error: PyError {
                message: "NameError: name 'undefined_name' is not defined".into(),
                line: Some(2),
                kind: Some("python.name".into()),
                subject: Some("undefined_name".into())
            },
            stdout: String::new()
        }
    );
    match run(&mut c, "x = (\n1 +\n") {
        Outcome::PythonError { error, .. } => assert!(error.message.starts_with("SyntaxError"), "{error:?}"),
        other => panic!("{other:?}"),
    }
    match run(&mut c, "def f():\n    return 1 / 0\n\nf()") {
        Outcome::PythonError { error, .. } => {
            assert!(error.message.starts_with("ZeroDivisionError"), "{error:?}");
            assert_eq!(error.line, Some(2), "the line inside the function, in the cell's code");
        }
        other => panic!("{other:?}"),
    }
    match run(&mut c, "import sys\nsys.exit(3)") {
        Outcome::PythonError { error, .. } => assert_eq!(error.message, "SystemExit: 3"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn globals_last_until_a_reset() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    // Globals persist across runs, until a reset.
    assert_eq!(value(run(&mut c, "shared = 41")), Value::Empty);
    assert_eq!(value(run(&mut c, "shared + 1")), Value::Number(42.0));
    assert_eq!(value(run(&mut c, "import __main__\n__main__.shared")), Value::Number(41.0), "the cells' globals are __main__");
    assert_eq!(value(c.reset().unwrap()), Value::Empty);
    match run(&mut c, "shared") {
        Outcome::PythonError { error, .. } => assert!(error.message.starts_with("NameError"), "{error:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(c.starts(), 1, "all on one worker");
}

#[test]
fn xl_reads_only_the_inputs_sent() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    let mut inputs = BTreeMap::new();
    inputs.insert("A1".to_string(), Input::new(Value::Number(5.0)));
    inputs.insert("B1:C2".to_string(), Input::new(array(2, 2, vec![1.5.into(), Value::text("x"), Value::Empty, Value::Error(CellError::Div0)])));
    let code = "a = xl(\"A1\")\nr = xl(\"B1:C2\")\n[[a, type(a).__name__], [r[0][0], r[0][1]], [r[1][0], r[1][1]]]";
    assert_eq!(
        value(c.run(code, inputs.clone()).unwrap()),
        array(3, 2, vec![5.0.into(), Value::text("int"), 1.5.into(), Value::text("x"), Value::Empty, Value::Error(CellError::Div0)]),
        "a cell is a scalar, a range a list of rows, and an error cell is the error again"
    );
    assert_eq!(value(c.run("repr(xl('B1:C2')[1][1])", inputs.clone()).unwrap()), Value::text("#DIV/0!"));
    for bad in ["xl('Z9')", "xl('A' + '1') if False else xl('a1')", "xl(1)"] {
        match c.run(bad, inputs.clone()).unwrap() {
            Outcome::PythonError { error, .. } => assert!(error.message.contains("xl("), "{bad}: {error:?}"),
            other => panic!("{bad}: {other:?}"),
        }
    }
    // A function kept from an earlier cell can't read inputs outside a run.
    assert_eq!(value(c.run("def later():\n    return xl('A1')", inputs.clone()).unwrap()), Value::Empty);
    assert_eq!(value(c.run("later()", inputs).unwrap()), Value::Number(5.0));
}

#[test]
fn stdout_is_captured_and_the_protocol_is_out_of_reach() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    match run(&mut c, "print('hello')\nprint('world')\n7") {
        Outcome::Ok { output: Output::Value(v), stdout } => {
            assert_eq!(v, Value::Number(7.0));
            assert_eq!(stdout, "hello\nworld\n");
        }
        other => panic!("{other:?}"),
    }
    // Printed output is cut at the cap, and the run still answers.
    match run(&mut c, "for i in range(300000): print('0123456789')\n1") {
        Outcome::Ok { stdout, .. } => {
            assert!(stdout.len() <= gridcraft_python::protocol::MAX_STDOUT_BYTES);
            assert!(stdout.ends_with("[output cut off]\n"));
        }
        other => panic!("{other:?}"),
    }
    // Writing a forged reply to standard output, at the Python or the file-descriptor level,
    // reaches standard error, not the protocol.
    let forged = r#"{"type":"done","id":99,"ok":true,"value":1}"#;
    assert_eq!(
        value(run(&mut c, &format!("import os, sys\nsys.__stdout__.write('{forged}\\n')\nsys.__stdout__.flush()\nos.write(1, b'{forged}\\n')\n5"))),
        Value::Number(5.0)
    );
    // input() finds an empty pipe, not the requests.
    match run(&mut c, "input()") {
        Outcome::PythonError { error, .. } => assert!(error.message.starts_with("EOFError"), "{error:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(value(run(&mut c, "import sys\nsys.stdin.read()")), Value::text(""));
    // The shim's names aren't in __main__ (after a reset, which drops the names set above).
    assert_eq!(value(c.reset().unwrap()), Value::Empty);
    assert_eq!(value(run(&mut c, "','.join(sorted(k for k in globals() if not k.startswith('__')))")), Value::text("excel,np,pd,plt,sm,sns,xl"));
    assert_eq!(
        value(run(&mut c, "','.join(sorted(k for k in vars(__import__('sys').modules['__main__']) if not k.startswith('__')))")),
        Value::text("excel,np,pd,plt,sm,sns,xl")
    );
    // The same worker still answers after all that.
    assert_eq!(value(run(&mut c, "2")), Value::Number(2.0));
    assert_eq!(c.starts(), 1);
}

/// The shim keeps its replies within the protocol's caps, so a big result is an error in the
/// cell, not a killed worker.
#[test]
fn big_results_stay_within_the_caps() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    match value(run(&mut c, "[[0] * 1000 for _ in range(1000)]")) {
        Value::Array(a) => assert_eq!((a.rows, a.cols), (1000, 1000)),
        other => panic!("{other:?}"),
    }
    for (code, says) in [
        ("[[0] * 1001 for _ in range(1000)]", "at most 1000000 cells"),
        ("list(range(1000001))", "at most 1000000 cells"),
        ("'x' * (200 * 1024)", "A text result can have at most"),
        // 1,000,000 texts of 100 bytes: within every field's cap, but over the line's.
        ("['y' * 100] * 1000000", "too large to send"),
    ] {
        match run(&mut c, code) {
            Outcome::PythonError { error, .. } => assert!(error.message.contains(says), "{code}: {error:?}"),
            other => panic!("{code}: {other:?}"),
        }
    }
    let long_message = run(&mut c, "raise ValueError('m' * 100000)");
    match long_message {
        Outcome::PythonError { error, .. } => assert!(error.message.len() <= gridcraft_python::protocol::MAX_MESSAGE_BYTES),
        other => panic!("{other:?}"),
    }
    assert_eq!(c.starts(), 1, "none of these killed the worker");
}

#[test]
fn the_interpreter_is_isolated() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    // Only the variable the shim sets itself, a fresh empty working directory, no site module,
    // isolated flags. Python adds LC_CTYPE when it coerces the C locale to UTF-8, and macOS adds its
    // text encoding, both inside the worker.
    let env = value(run(&mut c, "import os\n','.join(sorted(k for k in os.environ if k not in ('LC_CTYPE', '__CF_USER_TEXT_ENCODING')))"));
    assert_eq!(env, Value::text("OPENBLAS_NUM_THREADS"));
    assert_eq!(value(run(&mut c, "import os\nos.listdir('.')")), Value::Error(CellError::Calc), "an empty list");
    assert_eq!(value(run(&mut c, "import sys\n'site' in sys.modules")), Value::Bool(false));
    assert_eq!(
        value(run(&mut c, "import sys\n[bool(f) for f in (sys.flags.isolated, sys.flags.no_site, sys.flags.ignore_environment)]")),
        array(3, 1, vec![Value::Bool(true); 3])
    );
    assert_eq!(value(run(&mut c, "import sys, sysconfig\nsysconfig.get_paths()['purelib'] in sys.path")), Value::Bool(true));
}

#[test]
fn deadlines_and_kills_with_a_real_worker() {
    let Some(py) = python() else { return };
    let mut c = Client::new(
        Box::new(ProcessTransport::new(py)),
        Limits { timeout: Duration::from_millis(1_500), start_timeout: Duration::from_secs(20), ..Limits::default() },
    );
    assert_eq!(value(run(&mut c, "warm = 1")), Value::Empty);
    let t = Instant::now();
    assert_eq!(run(&mut c, "while True: pass"), Outcome::TimedOut);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // A fresh worker, with fresh globals.
    match run(&mut c, "warm") {
        Outcome::PythonError { error, .. } => assert!(error.message.starts_with("NameError")),
        other => panic!("{other:?}"),
    }
    assert_eq!(c.starts(), 2);
    // A crash (the process exits) is reported, and the next request starts again.
    assert!(matches!(run(&mut c, "import os\nos._exit(9)"), Outcome::Crashed(_)));
    assert_eq!(value(run(&mut c, "3")), Value::Number(3.0));
    assert_eq!(c.starts(), 3);
}

#[test]
fn a_short_spike_over_the_memory_limit_counts_against_the_cell_that_made_it() {
    let Some(py) = python() else { return };
    let mut c = Client::new(
        Box::new(ProcessTransport::new(py)),
        Limits { timeout: Duration::from_secs(30), start_timeout: Duration::from_secs(20), memory_bytes: 200 * 1024 * 1024, ..Limits::default() },
    );
    assert_eq!(value(run(&mut c, "1 + 1")), Value::Number(2.0));
    // Touched and gone again within a moment, between two reads of the watcher.
    match run(&mut c, "spike = b'x' * (400 * 1024 * 1024)\ndel spike\n'done'") {
        Outcome::OverMemory { rss_bytes } => assert!(rss_bytes > 200 * 1024 * 1024, "{rss_bytes}"),
        other => panic!("the spike went unnoticed: {other:?}"),
    }
    // The next cell has nothing to do with it.
    assert_eq!(value(run(&mut c, "2 + 2")), Value::Number(4.0));
}

#[test]
fn the_worker_exits_when_its_input_closes() {
    let Some(py) = python() else { return };
    let mut t = ProcessTransport::new(py);
    let pipes = t.start().unwrap();
    drop(pipes.stdin);
    let mut out = Vec::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut stdout = pipes.stdout;
    std::thread::spawn(move || {
        let r = stdout.by_ref().take(4096).read_to_end(&mut out);
        let _ = tx.send((r.is_ok(), out));
    });
    let (ok, out) = rx.recv_timeout(Duration::from_secs(20)).expect("the worker ended at EOF");
    assert!(ok);
    assert!(String::from_utf8_lossy(&out).starts_with("{\"type\":\"ready\""), "{}", String::from_utf8_lossy(&out));
    t.kill();
}

#[test]
fn a_missing_interpreter_fails_to_start() {
    let mut c = Client::new(Box::new(ProcessTransport::new(PathBuf::from("/nonexistent/python3"))), Limits::default());
    let o = run(&mut c, "1");
    assert!(matches!(&o, Outcome::FailedToStart(_)), "{o:?}");
    assert_eq!(o.cell_error(), Some(CellError::Connect));
}

/// The worker says what kind of failure an error is.
#[test]
fn errors_say_what_kind_they_are() {
    let Some(p) = python() else { return };
    let mut c = client(p);
    let kind = |c: &mut Client, code: &str| match run(c, code) {
        Outcome::PythonError { error, .. } => (error.kind, error.subject),
        other => panic!("{code}: {other:?}"),
    };
    let k = |s: &str| Some(s.to_string());
    assert_eq!(kind(&mut c, "1 +"), (k("python.syntax"), None));
    assert_eq!(kind(&mut c, "import not_a_module_anywhere"), (k("module.missing"), k("not_a_module_anywhere")));
    assert_eq!(kind(&mut c, "not_a_name"), (k("python.name"), k("not_a_name")));
    assert_eq!(kind(&mut c, "1 / 0"), (k("python.exception"), None));
    assert_eq!(kind(&mut c, "r = 'A1'\nxl(r)"), (k("xl.notLiteral"), None));
    // A default name of Excel's initialization only matters once it is used.
    assert_eq!(value(run(&mut c, "x = 1\nx")), Value::Number(1.0));
    let fake = fake_pandas("lazy");
    let mut with = Client::new(Box::new(ProcessTransport::new(python().unwrap()).with_site_dirs(vec![fake.clone()])), Limits::default());
    assert_eq!(value(run(&mut with, "pd.DataFrame([[1]]).rows[0][0]")), Value::Number(1.0));
    std::fs::remove_dir_all(fake).unwrap();
    assert_eq!(kind(&mut c, "raise PermissionError(1, 'no', '/some/file')"), (k("denied.file"), k("/some/file")));
    assert_eq!(kind(&mut c, "import os\nraise PermissionError(1, 'no') # os.fork()"), (k("denied.process"), None));
    assert_eq!(kind(&mut c, "raise PermissionError(1, 'no') # socket"), (k("denied.network"), None));
    // Wrapped, as urllib and requests do.
    assert_eq!(
        kind(&mut c, "try:\n    raise PermissionError(1, 'no') # socket\nexcept OSError as e:\n    raise ValueError('wrapped') from e"),
        (k("denied.network"), None)
    );
}

/// A folder with a stand-in for pandas: what `xl()` gives it is the test, not pandas itself.
fn fake_pandas(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-fake-pandas-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pandas")).unwrap();
    std::fs::write(
        dir.join("pandas/__init__.py"),
        "class DataFrame:\n    def __init__(self, rows, columns=None):\n        self.rows = rows\n        self.columns = columns\n",
    )
    .unwrap();
    dir
}

#[test]
fn a_range_is_a_dataframe_when_pandas_is_there() {
    let Some(p) = python() else { return };
    let site = fake_pandas("frame");
    let transport = ProcessTransport::new(p).with_site_dirs(vec![site.clone()]);
    let mut c = Client::new(Box::new(transport), Limits::default());
    let range = Value::Array(std::sync::Arc::new(Array { rows: 2, cols: 1, data: vec![Value::text("name"), Value::Number(5.0)] }));
    let inputs = || BTreeMap::from([("A1:A2".to_string(), Input::new(range.clone()))]);
    let run_xl = |c: &mut Client, code: &str| match c.run(code, inputs()).unwrap() {
        Outcome::Ok { output: Output::Value(v), .. } => v,
        other => panic!("{code}: {other:?}"),
    };
    assert_eq!(run_xl(&mut c, "type(xl('A1:A2')).__name__"), Value::text("DataFrame"));
    assert_eq!(run_xl(&mut c, "xl('A1:A2').rows[1][0]"), Value::Number(5.0));
    assert_eq!(run_xl(&mut c, "xl('A1:A2', headers=True).columns[0]"), Value::text("name"));
    assert_eq!(run_xl(&mut c, "xl('A1:A2', headers=True).rows[0][0]"), Value::Number(5.0));
    std::fs::remove_dir_all(site).unwrap();
}

/// A list spills, unless the cell's return type is a Python object.
#[test]
fn the_return_type_decides_between_cells_and_an_object() {
    let Some(p) = python() else { return };
    let mut c = client(p);
    let id = c.submit_cell("[1, 2]", BTreeMap::new(), false, None).unwrap();
    let Some(RunResult { outcome: Outcome::Ok { output: Output::Value(Value::Array(a)), .. }, .. }) = c.wait().filter(|r| r.id == id) else {
        panic!("cells")
    };
    assert_eq!((a.rows, a.cols), (2, 1));
    let id = c.submit_cell("[1, 2]", BTreeMap::new(), true, None).unwrap();
    let Some(RunResult { outcome: Outcome::Ok { output: Output::Object { type_name, preview, .. }, .. }, .. }) = c.wait().filter(|r| r.id == id)
    else {
        panic!("object")
    };
    assert_eq!((type_name.as_str(), preview.as_str()), ("list", "[1, 2]"));
    // A number stays a number either way.
    let id = c.submit_cell("3", BTreeMap::new(), true, None).unwrap();
    let Some(RunResult { outcome: Outcome::Ok { output: Output::Value(Value::Number(n)), .. }, .. }) = c.wait().filter(|r| r.id == id) else {
        panic!("number")
    };
    assert_eq!(n, 3.0);
}

const JAN15: Day = Day { year: 2024, month: 1, day: 15 };
const CLOCK: Clock = Clock { hour: 10, minute: 30, second: 5, micro: 250_000 };
const NOON: Clock = Clock { hour: 12, minute: 0, second: 0, micro: 0 };

fn one(stamp: Stamp) -> Input {
    Input { value: Value::Empty, headers: None, stamps: vec![(0, stamp)] }
}

fn day(year: i32, month: u8, day: u8) -> Stamp {
    Stamp::Date(Day { year, month, day })
}

fn dated_scalar(stamp: Stamp) -> Output {
    Output::Dated(Dated { value: Value::Empty, stamps: vec![(0, stamp)] })
}

fn output(o: Outcome) -> Output {
    match o {
        Outcome::Ok { output, .. } => output,
        other => panic!("not a result: {other:?}"),
    }
}

/// `import datetime` and the code, on a client, as its output.
fn with_datetime(c: &mut Client, code: &str) -> Output {
    output(run(c, &format!("import datetime\n{code}")))
}

/// The shim gives `xl()` real `datetime` values, with no serial number in sight.
#[test]
fn xl_gives_dates_and_times() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    let inputs = BTreeMap::from([
        ("A1".to_string(), one(Stamp::Date(JAN15))),
        ("A2".to_string(), one(Stamp::DateTime(JAN15, CLOCK))),
        ("A3".to_string(), one(Stamp::Time(CLOCK))),
        (
            "B1:C2".to_string(),
            Input {
                value: array(2, 2, vec![Value::Empty, Value::text("x"), Value::Number(60.0), Value::Empty]),
                headers: None,
                stamps: vec![(0, Stamp::Date(JAN15)), (3, Stamp::Time(CLOCK))],
            },
        ),
    ]);
    let answer = |c: &mut Client, code: &str| value(c.run(code, inputs.clone()).unwrap());
    assert_eq!(answer(&mut c, "type(xl('A1')).__name__"), Value::text("date"));
    assert_eq!(answer(&mut c, "xl('A1').isoformat()"), Value::text("2024-01-15"));
    assert_eq!(answer(&mut c, "type(xl('A2')).__name__"), Value::text("datetime"));
    assert_eq!(answer(&mut c, "xl('A2').isoformat()"), Value::text("2024-01-15T10:30:05.250000"));
    assert_eq!(answer(&mut c, "type(xl('A3')).__name__"), Value::text("time"));
    assert_eq!(answer(&mut c, "xl('A3').isoformat()"), Value::text("10:30:05.250000"));
    // In a range, without pandas: a list of rows, each cell what it is.
    assert_eq!(
        answer(&mut c, "[[type(x).__name__ for x in r] for r in xl('B1:C2')]"),
        array(2, 2, vec![Value::text("date"), Value::text("str"), Value::text("int"), Value::text("time")])
    );
    assert_eq!(answer(&mut c, "xl('B1:C2')[1][0]"), Value::Number(60.0), "a serial that names no day stays a number");
    // They are real values: arithmetic works, and the round trip gives a date back.
    assert_eq!(answer(&mut c, "(xl('A1') - __import__('datetime').date(2024, 1, 1)).days"), Value::Number(14.0));
    assert_eq!(output(c.run("xl('A1') + __import__('datetime').timedelta(days=1)", inputs.clone()).unwrap()), dated_scalar(day(2024, 1, 16)));
    assert_eq!(c.starts(), 1);
}

#[test]
fn date_results_go_out_as_dates() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    assert_eq!(with_datetime(&mut c, "datetime.date(2024, 1, 15)"), dated_scalar(Stamp::Date(JAN15)));
    assert_eq!(with_datetime(&mut c, "datetime.datetime(2024, 1, 15, 10, 30, 5, 250000)"), dated_scalar(Stamp::DateTime(JAN15, CLOCK)));
    // A datetime is a datetime, even at midnight.
    assert_eq!(with_datetime(&mut c, "datetime.datetime(2024, 1, 15)"), dated_scalar(Stamp::DateTime(JAN15, Clock::MIDNIGHT)));
    assert_eq!(with_datetime(&mut c, "datetime.time(10, 30, 5, 250000)"), dated_scalar(Stamp::Time(CLOCK)));
    assert_eq!(with_datetime(&mut c, "datetime.timedelta(days=1, hours=12)"), dated_scalar(Stamp::Duration(1.5)));
    assert_eq!(with_datetime(&mut c, "-datetime.timedelta(days=2)"), dated_scalar(Stamp::Duration(-2.0)));
    // A time zone only says where the wall clock is.
    assert_eq!(
        with_datetime(&mut c, "datetime.datetime(2024, 1, 15, 12, tzinfo=datetime.timezone(datetime.timedelta(hours=9)))"),
        dated_scalar(Stamp::DateTime(JAN15, NOON))
    );
    assert_eq!(with_datetime(&mut c, "datetime.time(12, tzinfo=datetime.timezone.utc)"), dated_scalar(Stamp::Time(NOON)));
    // Python's own extremes go out as they are; the parent says what a serial holds.
    assert_eq!(with_datetime(&mut c, "datetime.date.min"), dated_scalar(day(1, 1, 1)));
    assert_eq!(with_datetime(&mut c, "datetime.date.max"), dated_scalar(day(9999, 12, 31)));
    assert_eq!(
        with_datetime(&mut c, "datetime.datetime.max"),
        dated_scalar(Stamp::DateTime(Day { year: 9999, month: 12, day: 31 }, Clock { hour: 23, minute: 59, second: 59, micro: 999_999 }))
    );
    assert_eq!(with_datetime(&mut c, "datetime.timedelta.max"), dated_scalar(Stamp::Duration(999_999_999.0 + 86_399.999_999 / 86_400.0)));
    // Across a spill, among other values, and in a list as in a list of lists.
    assert_eq!(
        with_datetime(&mut c, "[[datetime.date(2024, 1, 15), 'x'], [3, datetime.time(12)]]"),
        Output::Dated(Dated {
            value: array(2, 2, vec![Value::Empty, Value::text("x"), Value::Number(3.0), Value::Empty]),
            stamps: vec![(0, Stamp::Date(JAN15)), (3, Stamp::Time(NOON))],
        })
    );
    assert_eq!(
        with_datetime(&mut c, "[datetime.date(2024, 1, 15), None]"),
        Output::Dated(Dated { value: array(2, 1, vec![Value::Empty, Value::Empty]), stamps: vec![(0, Stamp::Date(JAN15))] })
    );
    assert_eq!(c.starts(), 1, "none of these cost a worker");
}

/// A million of the longest dates fit in a line, so a big result of them reaches the cells.
#[test]
fn a_million_dates_stay_within_the_caps() {
    let Some(py) = python() else { return };
    let mut c = Client::new(
        Box::new(ProcessTransport::new(py)),
        Limits { timeout: Duration::from_secs(120), start_timeout: Duration::from_secs(20), ..Limits::default() },
    );
    match with_datetime(&mut c, "[[datetime.datetime(2024, 1, 15, 10, 30, 5, 250000)] * 1000] * 1000") {
        Output::Dated(Dated { value: Value::Array(a), stamps }) => {
            assert_eq!(((a.rows, a.cols), stamps.len()), ((1000, 1000), 1_000_000));
            assert_eq!(stamps.last(), Some(&(999_999, Stamp::DateTime(JAN15, CLOCK))));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(c.starts(), 1);
}

/// A class of the cell's own that claims to be a date can't break the protocol or the run.
#[test]
fn a_date_that_lies_about_itself_costs_nothing() {
    let Some(py) = python() else { return };
    let mut c = client(py);
    let class = "class D(datetime.date):\n    __slots__ = ()\n    year = property(lambda self: {year})\n";
    let d = "D(2024, 1, 15)";
    assert_eq!(with_datetime(&mut c, &format!("{}{d}", class.replace("{year}", "1/0"))), Output::Value(Value::Error(CellError::Value)));
    assert_eq!(with_datetime(&mut c, &format!("{}{d}", class.replace("{year}", "'x'"))), Output::Value(Value::Error(CellError::Value)));
    // A year too long for the protocol's cap is no date any serial holds.
    assert_eq!(with_datetime(&mut c, &format!("{}{d}", class.replace("{year}", "10 ** 100"))), Output::Value(Value::Error(CellError::Num)));
    assert_eq!(value(run(&mut c, "2 + 2")), Value::Number(4.0));
    assert_eq!(c.starts(), 1);
}

/// A client on the interpreter in `GRIDCRAFT_TEST_PYTHON`, which has numpy and pandas.
fn client_with_pandas() -> Option<Client> {
    let Some(python) = std::env::var_os("GRIDCRAFT_TEST_PYTHON") else {
        println!("skipped: GRIDCRAFT_TEST_PYTHON isn't set, so there is no pandas");
        return None;
    };
    let found = gridcraft_python::describe::describe(&PathBuf::from(python), Duration::from_secs(30)).unwrap();
    let transport = ProcessTransport::new(found.run_as).with_site_dirs(found.site_dirs);
    Some(Client::new(Box::new(transport), Limits { timeout: Duration::from_secs(120), start_timeout: Duration::from_secs(60), ..Limits::default() }))
}

fn numpy_code(c: &mut Client, code: &str) -> Output {
    output(run(c, &format!("import numpy, datetime\n{code}")))
}

/// numpy's own conversions give bare integers in nanoseconds and past Python's years, and overflow
/// without a word.
#[test]
fn numpy_dates_and_durations_keep_their_units() {
    let Some(mut c) = client_with_pandas() else { return };
    let moment = |unit: &str, text: &str| format!("numpy.datetime64('{text}', '{unit}')");
    for (unit, text, want) in [
        ("Y", "2024", day(2024, 1, 1)),
        ("M", "2024-03", day(2024, 3, 1)),
        ("M", "1969-12", day(1969, 12, 1)),
        // numpy counts whole weeks from a Thursday, 1 January 1970.
        ("W", "2024-01-15", day(2024, 1, 11)),
        ("D", "2024-01-15", Stamp::Date(JAN15)),
        ("D", "1900-01-01", day(1900, 1, 1)),
        ("h", "2024-01-15T12", Stamp::DateTime(JAN15, NOON)),
        ("h", "2024-01-15T00", Stamp::Date(JAN15)),
        ("m", "2024-01-15T12:00", Stamp::DateTime(JAN15, NOON)),
        ("s", "2024-01-15T12:00:00", Stamp::DateTime(JAN15, NOON)),
        ("ms", "2024-01-15T10:30:05.250", Stamp::DateTime(JAN15, CLOCK)),
        ("us", "2024-01-15T10:30:05.250000", Stamp::DateTime(JAN15, CLOCK)),
        ("ns", "2024-01-15T10:30:05.250000000", Stamp::DateTime(JAN15, CLOCK)),
        ("ns", "2024-01-15", Stamp::Date(JAN15)),
        // Picoseconds only reach some weeks from 1970.
        ("ps", "1970-01-15T10:30:05.250000000000", Stamp::DateTime(Day { year: 1970, month: 1, day: 15 }, CLOCK)),
    ] {
        assert_eq!(numpy_code(&mut c, &moment(unit, text)), dated_scalar(want), "{text} in {unit}");
    }
    // Before the epoch, and with a count in the unit.
    assert_eq!(
        numpy_code(&mut c, "numpy.datetime64('1900-03-01T06:00:00', 's')"),
        dated_scalar(Stamp::DateTime(Day { year: 1900, month: 3, day: 1 }, Clock { hour: 6, ..NOON }))
    );
    assert_eq!(numpy_code(&mut c, "numpy.datetime64(3, '10D')"), dated_scalar(day(1970, 1, 31)));
    // A zero-dimensional array is its element.
    assert_eq!(numpy_code(&mut c, "numpy.array(numpy.datetime64('2024-01-15'))"), dated_scalar(Stamp::Date(JAN15)));
    // NaT, in every form, is an empty cell.
    for nat in ["numpy.datetime64('NaT')", "numpy.datetime64('NaT', 'ns')", "numpy.timedelta64('NaT')", "numpy.timedelta64('NaT', 's')"] {
        assert_eq!(numpy_code(&mut c, nat), Output::Value(Value::Empty), "{nat}");
    }
    // Past Python's years, in units where numpy would wrap around: no serial holds them.
    for far in [
        "numpy.datetime64(10**15, 'Y')",
        "numpy.datetime64(-(10**15), 'Y')",
        "numpy.datetime64(10**15, 'D')",
        "numpy.datetime64('12345-01-01', 'D')",
        "numpy.datetime64(9 * 10**18, 'W')",
    ] {
        assert_eq!(numpy_code(&mut c, far), Output::Value(Value::Error(CellError::Num)), "{far}");
    }
    // Durations.
    for (code, days) in [
        ("numpy.timedelta64(36, 'h')", 1.5),
        ("numpy.timedelta64(36 * 3600 * 10**9, 'ns')", 1.5),
        ("numpy.timedelta64(-2, 'D')", -2.0),
        ("numpy.timedelta64(1, 'W')", 7.0),
        ("numpy.timedelta64(86400, 's')", 1.0),
        ("numpy.array(numpy.timedelta64(1, 'D'))", 1.0),
    ] {
        assert_eq!(numpy_code(&mut c, code), dated_scalar(Stamp::Duration(days)), "{code}");
    }
    assert_eq!(numpy_code(&mut c, "numpy.timedelta64(9 * 10**18, 'W')"), dated_scalar(Stamp::Duration(9e18 * 7.0)), "the parent says it is too long");
    for no_length in ["numpy.timedelta64(1, 'M')", "numpy.timedelta64(1, 'Y')"] {
        assert_eq!(numpy_code(&mut c, no_length), Output::Value(Value::Error(CellError::Num)), "{no_length}: no length in days");
    }
    // Arrays, whose tolist() gives bare integers in nanoseconds.
    assert_eq!(
        numpy_code(&mut c, "numpy.array(['2024-01-15', 'NaT'], dtype='datetime64[ns]')"),
        Output::Dated(Dated { value: array(2, 1, vec![Value::Empty, Value::Empty]), stamps: vec![(0, Stamp::Date(JAN15))] })
    );
    assert_eq!(
        numpy_code(&mut c, "numpy.array([['2024-01-15T12:00', '2024-01-15']], dtype='datetime64[ns]')"),
        Output::Dated(Dated {
            value: array(1, 2, vec![Value::Empty, Value::Empty]),
            stamps: vec![(0, Stamp::DateTime(JAN15, NOON)), (1, Stamp::Date(JAN15))]
        })
    );
    assert_eq!(
        numpy_code(&mut c, "numpy.array([86400 * 10**9], dtype='timedelta64[ns]')"),
        Output::Dated(Dated { value: array(1, 1, vec![Value::Empty]), stamps: vec![(0, Stamp::Duration(1.0))] })
    );
    assert_eq!(c.starts(), 1, "none of these cost a worker");
}

fn pandas_code(c: &mut Client, code: &str) -> Output {
    output(run(c, &format!("import pandas, numpy, datetime\n{code}")))
}

#[test]
fn pandas_results_come_back_as_dates() {
    let Some(mut c) = client_with_pandas() else { return };
    // pandas has no date without a time: midnight means the date.
    assert_eq!(pandas_code(&mut c, "pandas.Timestamp('2024-01-15')"), dated_scalar(Stamp::Date(JAN15)));
    assert_eq!(pandas_code(&mut c, "pandas.Timestamp('2024-01-15 10:30:05.25')"), dated_scalar(Stamp::DateTime(JAN15, CLOCK)));
    // Its wall clock, in whatever time zone.
    assert_eq!(pandas_code(&mut c, "pandas.Timestamp('2024-01-15 12:00', tz='Asia/Tokyo')"), dated_scalar(Stamp::DateTime(JAN15, NOON)));
    assert_eq!(pandas_code(&mut c, "pandas.Timedelta(hours=36)"), dated_scalar(Stamp::Duration(1.5)));
    assert_eq!(pandas_code(&mut c, "pandas.NaT"), Output::Value(Value::Empty));
    assert_eq!(pandas_code(&mut c, "[pandas.NaT, 1]"), Output::Value(array(2, 1, vec![Value::Empty, Value::Number(1.0)])));
    // A DataFrame's date column under its header, with a missing value.
    assert_eq!(
        pandas_code(&mut c, "pandas.DataFrame({'when': pandas.to_datetime(['2024-01-15', None]), 'n': [1, 2]})"),
        Output::Dated(Dated {
            value: array(3, 2, vec![Value::text("when"), Value::text("n"), Value::Empty, Value::Number(1.0), Value::Empty, Value::Number(2.0)]),
            stamps: vec![(2, Stamp::Date(JAN15))],
        })
    );
    // Time zones and durations in a column, and a date in the row labels.
    assert_eq!(
        pandas_code(
            &mut c,
            "pandas.DataFrame({'at': pandas.to_datetime(['2024-01-15 12:00']).tz_localize('Asia/Tokyo'), 'for': [pandas.Timedelta(days=1)]}, index=pandas.to_datetime(['2024-01-15']))"
        ),
        Output::Dated(Dated {
            value: array(2, 3, vec![Value::text(""), Value::text("at"), Value::text("for"), Value::Empty, Value::Empty, Value::Empty]),
            stamps: vec![(3, Stamp::Date(JAN15)), (4, Stamp::DateTime(JAN15, NOON)), (5, Stamp::Duration(1.0))],
        })
    );
    // A Series, the values of a column, and an index of dates.
    let column = Output::Dated(Dated { value: array(1, 1, vec![Value::Empty]), stamps: vec![(0, Stamp::Date(JAN15))] });
    assert_eq!(pandas_code(&mut c, "pandas.Series(pandas.to_datetime(['2024-01-15']))"), column);
    assert_eq!(pandas_code(&mut c, "pandas.Series(pandas.to_datetime(['2024-01-15'])).values"), column);
    assert_eq!(pandas_code(&mut c, "pandas.to_datetime(['2024-01-15'])"), column);
    assert_eq!(pandas_code(&mut c, "pandas.Series([pandas.Timestamp('2024-01-15')], dtype=object)"), column);
    assert_eq!(
        pandas_code(&mut c, "pandas.to_timedelta(['1 days'])"),
        Output::Dated(Dated { value: array(1, 1, vec![Value::Empty]), stamps: vec![(0, Stamp::Duration(1.0))] })
    );
    // Dates pandas only keeps as objects.
    assert_eq!(
        pandas_code(&mut c, "pandas.DataFrame({'d': [datetime.date(9000, 1, 15)]})"),
        Output::Dated(Dated { value: array(2, 1, vec![Value::text("d"), Value::Empty]), stamps: vec![(1, day(9000, 1, 15))] })
    );
    assert_eq!(c.starts(), 1, "none of these cost a worker");
}

#[test]
fn a_range_of_dates_is_a_datetime_column() {
    let Some(mut c) = client_with_pandas() else { return };
    let stamps = vec![(1, Stamp::Date(JAN15)), (3, Stamp::Date(Day { year: 2024, month: 1, day: 16 }))];
    let column = Input { value: array(4, 1, vec![Value::text("when"), Value::Empty, Value::Empty, Value::Empty]), headers: None, stamps };
    let inputs = BTreeMap::from([
        ("A1:A4".to_string(), column),
        (
            // Dates with times, and one past what nanoseconds hold.
            "B1:B2".to_string(),
            Input {
                value: array(2, 1, vec![Value::Empty, Value::Empty]),
                headers: None,
                stamps: vec![(0, Stamp::DateTime(JAN15, NOON)), (1, day(9000, 1, 1))],
            },
        ),
        (
            // A date among other things isn't a date column.
            "C1:C2".to_string(),
            Input { value: array(2, 1, vec![Value::Empty, Value::Number(60.0)]), headers: None, stamps: vec![(0, Stamp::Date(JAN15))] },
        ),
        (
            // Times have no column type in pandas.
            "D1:D1".to_string(),
            Input { value: array(1, 1, vec![Value::Empty]), headers: None, stamps: vec![(0, Stamp::Time(NOON))] },
        ),
    ]);
    let answer = |c: &mut Client, code: &str| match c.run(code, inputs.clone()).unwrap() {
        Outcome::Ok { output, .. } => output,
        other => panic!("{code}: {other:?}"),
    };
    let text = |s: &str| Output::Value(Value::text(s));
    assert_eq!(answer(&mut c, "xl('A1:A4', headers=True).dtypes.iloc[0].kind"), text("M"));
    assert_eq!(answer(&mut c, "str(xl('A1:A4', headers=True)['when'].iloc[0])[:10]"), text("2024-01-15"));
    assert_eq!(answer(&mut c, "str(xl('A1:A4', headers=True)['when'].iloc[1])"), text("NaT"), "an empty cell");
    assert_eq!(answer(&mut c, "xl('B1:B2').dtypes.iloc[0].kind"), text("M"), "the units follow what it holds");
    assert_eq!(answer(&mut c, "str(xl('B1:B2').iloc[1, 0])[:10]"), text("9000-01-01"));
    assert_eq!(answer(&mut c, "xl('C1:C2').dtypes.iloc[0].kind"), text("O"));
    assert_eq!(answer(&mut c, "type(xl('C1:C2').iloc[0, 0]).__name__"), text("date"));
    assert_eq!(answer(&mut c, "type(xl('D1:D1').iloc[0, 0]).__name__"), text("time"));
    // Add a day to the column and return it: the round trip of a DataFrame.
    assert_eq!(
        answer(&mut c, "d = xl('A1:A4', headers=True)\nd['when'] = d['when'] + pd.Timedelta(days=1)\nd"),
        Output::Dated(Dated {
            value: array(4, 1, vec![Value::text("when"), Value::Empty, Value::Empty, Value::Empty]),
            stamps: vec![(1, day(2024, 1, 16)), (3, day(2024, 1, 17))],
        })
    );
}

/// The stand-in for pandas isn't pandas: looking for date columns in it must not raise.
#[test]
fn a_range_of_dates_survives_a_pandas_it_cannot_ask() {
    let Some(p) = python() else { return };
    let site = fake_pandas("dates");
    let mut c = Client::new(Box::new(ProcessTransport::new(p).with_site_dirs(vec![site.clone()])), Limits::default());
    let inputs = BTreeMap::from([(
        "A1:A1".to_string(),
        Input { value: array(1, 1, vec![Value::Empty]), headers: None, stamps: vec![(0, Stamp::Date(JAN15))] },
    )]);
    assert_eq!(value(c.run("xl('A1:A1').rows[0][0].isoformat()", inputs).unwrap()), Value::text("2024-01-15"));
    std::fs::remove_dir_all(site).unwrap();
}

/// What a Python Object cell gets: the object's name, its size and its table.
fn object(c: &mut Client, code: &str) -> (String, Vec<u64>, Option<Table>) {
    let id = c.submit_cell(code, BTreeMap::new(), true, None).unwrap();
    match c.wait().filter(|r| r.id == id).map(|r| r.outcome) {
        Some(Outcome::Ok { output: Output::Object { type_name, size, table, .. }, .. }) => (type_name, size, table),
        other => panic!("{code}: {other:?}"),
    }
}

#[test]
fn a_python_object_says_its_size() {
    let Some(p) = python() else { return };
    let mut c = client(p);
    for (code, name, size) in [
        ("list(range(12))", "list", vec![12]),
        ("[]", "list", vec![0]),
        ("(1, 2, 3)", "tuple", vec![3]),
        ("{'a': 1, 'b': 2, 'c': 3}", "dict", vec![3]),
        ("{1, 2}", "set", vec![2]),
        ("frozenset()", "frozenset", vec![0]),
        // A subclass names itself.
        ("class Rows(list): pass\nRows([1, 2])", "Rows", vec![2]),
        // Other objects show their type's name alone.
        ("object()", "object", vec![]),
        ("range(5)", "range", vec![]),
        ("def f(): pass\nf", "function", vec![]),
        // An object that lies, or raises, about its length still gets a result.
        ("class Rows(list):\n    def __len__(self): raise RuntimeError('no')\nRows([1])", "Rows", vec![]),
        ("class Rows(list):\n    def __len__(self): return 2 ** 60\nRows([1])", "Rows", vec![]),
        ("class Rows(list):\n    def __len__(self): raise SystemExit(3)\nRows([1])", "Rows", vec![]),
    ] {
        let got = object(&mut c, code);
        assert_eq!((got.0.as_str(), &got.1), (name, &size), "{code}");
    }
    // The cell still gets a worker after all that.
    assert_eq!(value(run(&mut c, "1 + 1")), Value::Number(2.0));
}

#[test]
fn a_list_of_rows_gives_a_table_and_a_flat_list_none() {
    let Some(p) = python() else { return };
    let mut c = client(p);
    assert_eq!(object(&mut c, "[1, 2, 3]").2, None, "a flat list");
    assert_eq!(object(&mut c, "[]").2, None);
    assert_eq!(object(&mut c, "[[1], 2]").2, None, "not all rows");
    assert_eq!(object(&mut c, "{'a': [1]}").2, None, "a dict isn't one");

    let table = object(&mut c, "[[1, 'a', True], [None, 2.5]]").2.unwrap();
    assert_eq!((table.rows, table.cols, table.total_rows, table.total_cols), (2, 3, 2, 3));
    assert!(table.columns.is_empty() && !table.row_labels && !table.text_cut);
    assert_eq!(
        table.data,
        vec![1.0.into(), Value::text("a"), Value::Bool(true), Value::Empty, 2.5.into(), Value::Error(CellError::NA)],
        "a short row is padded as a spill is"
    );
    // No columns at all.
    let empty = object(&mut c, "[[], []]").2.unwrap();
    assert_eq!((empty.cols, empty.data.len()), (0, 0));

    // A list whose methods fail has no table, and still its result.
    assert_eq!(object(&mut c, "class Rows(list):\n    def __getitem__(self, i): raise SystemExit(4)\nRows([[1]])").2, None);
    // In an Excel value cell a dict has neither size nor table.
    let id = c.submit_cell("{'a': 1}", BTreeMap::new(), false, None).unwrap();
    match c.wait().filter(|r| r.id == id).map(|r| r.outcome) {
        Some(Outcome::Ok { output: Output::Object { size, table, .. }, .. }) => assert!(size.is_empty() && table.is_none()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_table_past_a_cap_is_cut_and_says_so() {
    let Some(p) = python() else { return };
    let mut c = client(p);
    // Rows and columns alike.
    let big = object(&mut c, "[[r * 1000 + c for c in range(40)] for r in range(500)]").2.unwrap();
    assert_eq!((big.rows, big.cols, big.total_rows, big.total_cols), (MAX_TABLE_ROWS, MAX_TABLE_COLS, 500, 40));
    assert_eq!(big.data.len(), MAX_TABLE_ROWS * MAX_TABLE_COLS);
    assert_eq!(big.data.last(), Some(&Value::Number(199_029.0)));
    assert!(!big.text_cut);

    // A long text is cut in its cell, on a character's edge.
    let long = object(&mut c, "[['é' * 100000, 'short']]").2.unwrap();
    assert!(long.text_cut);
    let Value::Text(first) = &long.data[0] else { panic!("{:?}", long.data[0]) };
    assert_eq!(first.len(), MAX_TABLE_TEXT_BYTES);
    assert_eq!(long.data[1], Value::text("short"));

    // The budget in bytes stops the rows, however many fit by count, and the line stays good.
    let wide = object(&mut c, "[['x' * 4000 for _ in range(30)] for _ in range(200)]").2.unwrap();
    assert!(wide.rows < MAX_TABLE_ROWS && wide.rows > 0, "{}", wide.rows);
    assert_eq!((wide.total_rows, wide.cols), (200, 30));
    assert!(wide.data.len() * 4000 <= MAX_TABLE_BYTES);
    // JSON escapes count against it too: a text of quotes takes twice the room on the wire.
    let quoted = object(&mut c, "[['\"' * 4000 for _ in range(30)] for _ in range(200)]").2.unwrap();
    assert!(quoted.rows < wide.rows, "{} {}", quoted.rows, wide.rows);
}

#[test]
fn a_cell_that_isnt_a_scalar_is_its_repr_whatever_that_does() {
    let Some(p) = python() else { return };
    let mut c = client(p);
    let odd =
        object(&mut c, "class Bad:\n    def __repr__(self): raise SystemExit(1)\n[[list(range(10000)), {'k': 1}, Bad(), float('nan'), 10 ** 400]]")
            .2
            .unwrap();
    assert!(odd.text_cut);
    assert!(matches!(&odd.data[0], Value::Text(t) if t.starts_with("[0, 1, 2") && t.chars().count() == MAX_PREVIEW_CHARS));
    assert_eq!(odd.data[1], Value::text("{'k': 1}"));
    assert_eq!(odd.data[2], Value::text("<Bad object>"));
    assert_eq!(odd.data[3], Value::Error(CellError::Num));
    assert_eq!(odd.data[4], Value::Error(CellError::Num));
}

/// A client on `GRIDCRAFT_TEST_PYTHON` with pandas and numpy imported.
fn client_with_imports() -> Option<Client> {
    let mut c = client_with_pandas()?;
    assert_eq!(value(run(&mut c, "import numpy, pandas\n1")), Value::Number(1.0));
    Some(c)
}

#[test]
fn a_dataframe_has_its_shape_and_its_table() {
    let Some(mut c) = client_with_imports() else { return };
    let (name, size, table) = object(&mut c, "pandas.DataFrame({'x': [1, 2, None], 'y': ['a', 'b', 'c'], 'z': [True, False, True]})");
    assert_eq!((name.as_str(), size), ("DataFrame", vec![3, 3]));
    let table = table.unwrap();
    assert_eq!(table.columns, ["x", "y", "z"]);
    assert!(!table.row_labels);
    assert_eq!(table.data[6], Value::Empty, "a missing value is an empty cell");
    assert_eq!(table.data[1], Value::text("a"));

    // Row labels unless they are 0, 1, 2..., in the first column, and not counted as a column.
    let (_, size, table) = object(&mut c, "pandas.DataFrame({'x': [1, 2]}, index=pandas.Index(['p', 'q'], name='key'))");
    assert_eq!(size, vec![2, 1]);
    let table = table.unwrap();
    assert!(table.row_labels);
    assert_eq!((table.columns.as_slice(), table.cols, table.total_cols), (["key".to_string(), "x".to_string()].as_slice(), 2, 1));
    assert_eq!(table.data, vec![Value::text("p"), 1.0.into(), Value::text("q"), 2.0.into()]);

    // Big ones are cut, with their label column inside the 30, without being read whole.
    let (_, size, table) = object(&mut c, "pandas.DataFrame(numpy.zeros((300000, 50))).set_index(pandas.RangeIndex(1, 300001))");
    assert_eq!(size, vec![300_000, 50]);
    let table = table.unwrap();
    assert_eq!((table.rows, table.cols, table.total_rows, table.total_cols), (MAX_TABLE_ROWS, MAX_TABLE_COLS, 300_000, 50));
    assert!(table.row_labels);
}

#[test]
fn a_dataframe_that_is_empty_or_odd_has_its_table_too() {
    let Some(mut c) = client_with_imports() else { return };
    let (_, size, table) = object(&mut c, "pandas.DataFrame()");
    assert_eq!(size, vec![0, 0]);
    assert_eq!(table.unwrap().cols, 0);
    let (_, size, table) = object(&mut c, "pandas.DataFrame(columns=['a', 'b'])");
    assert_eq!(size, vec![0, 2]);
    let table = table.unwrap();
    assert_eq!((table.rows, table.columns.len()), (0, 2));

    // Dates in a table are their text, and missing ones empty.
    let (_, _, table) = object(&mut c, "pandas.DataFrame({'when': pandas.to_datetime(['2024-01-15', None, '2024-01-16 10:30'], format='ISO8601')})");
    assert_eq!(table.unwrap().data, vec![Value::text("2024-01-15"), Value::Empty, Value::text("2024-01-16 10:30:00")]);

    // A column of objects, a MultiIndex, and duplicate names.
    let (_, _, table) = object(&mut c, "pandas.DataFrame({'x': [[1, 2]]})");
    assert_eq!(table.unwrap().data, vec![Value::text("[1, 2]")]);
    let (_, size, table) = object(&mut c, "pandas.DataFrame([[1, 2]], columns=pandas.MultiIndex.from_tuples([('a', 'b'), ('a', 'c')]))");
    assert_eq!(size, vec![1, 2]);
    assert_eq!(table.unwrap().columns, ["('a', 'b')", "('a', 'c')"]);
    let (_, _, table) = object(&mut c, "pandas.DataFrame([[1, 2]], columns=['a', 'a'])");
    assert_eq!(table.unwrap().columns, ["a", "a"]);
    // A subclass of a table is a table.
    let (name, size, table) = object(&mut c, "class Mine(pandas.DataFrame): pass\nMine({'x': [1]})");
    assert_eq!((name.as_str(), size), ("Mine", vec![1, 1]));
    assert!(table.is_some());
}

#[test]
fn a_series_has_its_length_and_its_table() {
    let Some(mut c) = client_with_imports() else { return };
    let (name, size, table) = object(&mut c, "pandas.Series(range(12))");
    assert_eq!((name.as_str(), size), ("Series", vec![12]));
    let table = table.unwrap();
    assert!(table.columns.is_empty() && !table.row_labels);
    assert_eq!((table.rows, table.cols), (12, 1));
    let (_, _, table) = object(&mut c, "pandas.Series([1.5, None], index=['p', 'q'], name='v')");
    let table = table.unwrap();
    assert!(table.row_labels);
    assert_eq!(table.columns, ["", "v"]);
    assert_eq!(table.data, vec![Value::text("p"), 1.5.into(), Value::text("q"), Value::Empty]);
    assert_eq!((table.cols, table.total_cols), (2, 1));
}

#[test]
fn an_array_has_its_shape_and_at_two_dimensions_its_table() {
    let Some(mut c) = client_with_imports() else { return };
    let (name, size, table) = object(&mut c, "numpy.arange(6).reshape(2, 3)");
    assert_eq!((name.as_str(), size), ("ndarray", vec![2, 3]));
    assert_eq!(table.unwrap().data, vec![0.0.into(), 1.0.into(), 2.0.into(), 3.0.into(), 4.0.into(), 5.0.into()]);
    let (_, size, table) = object(&mut c, "numpy.arange(12)");
    assert_eq!((size, table), (vec![12], None));
    let (_, size, table) = object(&mut c, "numpy.zeros((2, 3, 4))");
    assert_eq!((size, table), (vec![2, 3, 4], None));
    let (_, size, _) = object(&mut c, "numpy.zeros((1,) * 9)");
    assert!(size.is_empty(), "more dimensions than a size holds");
    let (_, _, table) = object(&mut c, "numpy.array([['2024-01-15']], dtype='datetime64[D]')");
    assert_eq!(table.unwrap().data, vec![Value::text("2024-01-15")]);
    let (_, _, table) = object(&mut c, "numpy.array([[float('nan')]])");
    assert_eq!(table.unwrap().data, vec![Value::Error(CellError::Num)], "as a spill has it");
    let (name, size, table) = object(&mut c, "numpy.matrix([[1, 2]])");
    assert_eq!((name.as_str(), size), ("matrix", vec![1, 2]));
    assert!(table.is_some());
}
