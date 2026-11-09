//! Dates and times in Python cells, through the fake worker: what reaches `xl()`, and what a result
//! leaves in the cells.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gridcraft_core::date::DateSystem;
use gridcraft_core::{Array, CellError, CellRef, Value};
use gridcraft_python::fake::{FakeHandle, FakeTransport};
use gridcraft_python::protocol::Request;
use gridcraft_python::{Clock, Dated, Day, Input, Limits, Output, PyError, Stamp};
use serde_json::json;

use super::v;
use crate::Session;
use crate::python::PythonCapability;

const JAN15: Day = Day { year: 2024, month: 1, day: 15 };
const NOON: Clock = Clock { hour: 12, minute: 0, second: 0, micro: 0 };

/// A session whose worker answers every cell with nothing and shows what it was sent.
fn session_with_inputs() -> (Session, FakeHandle) {
    session_answering(Vec::new())
}

fn settle(s: &mut Session) {
    let mut left = 4000;
    while s.python_busy() {
        s.python_poll(Duration::from_millis(5));
        left -= 1;
        assert!(left > 0, "Python never settled");
    }
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn py(s: &mut Session, cell: &str, code: &str) {
    s.execute("python.set", json!({"cell": cell, "code": code})).unwrap();
}

fn format(s: &mut Session, range: &str, code: &str) {
    s.execute("home.numberFormat", json!({"range": range, "code": code})).unwrap();
}

fn on_1904(s: &mut Session) {
    Arc::make_mut(&mut s.doc_mut().unwrap().wb).date_system = DateSystem::D1904;
}

/// What the last run of the cells' code was sent, by reference.
fn sent(h: &FakeHandle) -> BTreeMap<String, Input> {
    h.requests()
        .into_iter()
        .rev()
        .find_map(|r| match r {
            Request::Run { inputs, .. } => Some(inputs),
            Request::Render { .. } | Request::Reset { .. } => None,
        })
        .expect("a cell ran")
}

fn runs(h: &FakeHandle) -> usize {
    h.requests().iter().filter(|r| matches!(r, Request::Run { .. })).count()
}

fn stamp_of(i: &Input) -> Option<Stamp> {
    i.stamps.first().map(|(_, s)| *s)
}

#[test]
fn a_date_format_makes_a_cell_a_date_in_xl() {
    let (mut s, h) = session_with_inputs();
    for (cell, input, code) in [
        ("A1", "45306", "m/d/yyyy"),
        ("A2", "45306.5", "m/d/yyyy h:mm"),
        ("A3", "0.5", "h:mm:ss"),
        ("A4", "45306", "0.00"),
        ("A5", "1.5", "[h]:mm:ss"),
        ("A6", "45306", "General"),
    ] {
        set(&mut s, cell, input);
        format(&mut s, cell, code);
    }
    set(&mut s, "A7", "text");
    format(&mut s, "A7", "m/d/yyyy");
    py(&mut s, "B1", r#"[xl("A1"), xl("A2"), xl("A3"), xl("A4"), xl("A5"), xl("A6"), xl("A7")]"#);
    settle(&mut s);
    let inputs = sent(&h);
    assert_eq!(stamp_of(&inputs["A1"]), Some(Stamp::Date(JAN15)));
    assert_eq!(stamp_of(&inputs["A2"]), Some(Stamp::DateTime(JAN15, NOON)));
    assert_eq!(stamp_of(&inputs["A3"]), Some(Stamp::Time(NOON)));
    for (reference, value) in
        [("A4", Value::Number(45306.0)), ("A5", Value::Number(1.5)), ("A6", Value::Number(45306.0)), ("A7", Value::text("text"))]
    {
        assert_eq!((&inputs[reference].value, stamp_of(&inputs[reference])), (&value, None), "{reference}");
    }
}

#[test]
fn a_range_says_which_of_its_cells_are_dates() {
    let (mut s, h) = session_with_inputs();
    s.execute("range.setValues", json!({"range": "A1", "values": [["when", 45306], [45307, 5], [true, "x"]]})).unwrap();
    format(&mut s, "A1:A3", "m/d/yyyy");
    format(&mut s, "B1", "h:mm");
    py(&mut s, "D1", r#"xl("A1:B3")"#);
    settle(&mut s);
    let input = &sent(&h)["A1:B3"];
    let Value::Array(a) = &input.value else { panic!("{input:?}") };
    assert_eq!(a.data, vec![Value::text("when"), Value::Empty, Value::Empty, Value::Number(5.0), Value::Bool(true), Value::text("x")]);
    // In place order, and a time format with a whole number of days keeps them.
    assert_eq!(input.stamps, vec![(1, Stamp::DateTime(JAN15, Clock::MIDNIGHT)), (2, Stamp::Date(Day { day: 16, ..JAN15 }))]);
}

#[test]
fn a_spill_and_a_whole_column_read_by_their_formats() {
    let (mut s, h) = session_with_inputs();
    set(&mut s, "A1", "=SEQUENCE(3,1,45306)");
    format(&mut s, "A2", "m/d/yyyy");
    py(&mut s, "C1", r#"[xl("A1#"), xl("A:A")]"#);
    settle(&mut s);
    let inputs = sent(&h);
    for reference in ["A1#", "A:A"] {
        assert_eq!(inputs[reference].stamps, vec![(1, Stamp::Date(Day { day: 16, ..JAN15 }))], "{reference}");
    }
}

#[test]
fn the_date_system_of_the_workbook_decides_the_day() {
    let (mut s, h) = session_with_inputs();
    on_1904(&mut s);
    set(&mut s, "A1", "43844");
    format(&mut s, "A1", "m/d/yyyy");
    set(&mut s, "A2", "0");
    format(&mut s, "A2", "m/d/yyyy");
    set(&mut s, "A3", "60");
    format(&mut s, "A3", "m/d/yyyy");
    py(&mut s, "B1", r#"[xl("A1"), xl("A2"), xl("A3")]"#);
    settle(&mut s);
    let inputs = sent(&h);
    assert_eq!(stamp_of(&inputs["A1"]), Some(Stamp::Date(JAN15)));
    assert_eq!(stamp_of(&inputs["A2"]), Some(Stamp::Date(Day { year: 1904, month: 1, day: 1 })), "a real day here");
    assert_eq!(stamp_of(&inputs["A3"]), Some(Stamp::Date(Day { year: 1904, month: 3, day: 1 })), "a real day here");
}

/// What no calendar has stays a number, and the cell runs.
#[test]
fn a_number_with_no_place_on_the_calendar_goes_as_a_number() {
    let (mut s, h) = session_with_inputs();
    for (cell, input) in [("A1", "60"), ("A2", "0"), ("A3", "0.5"), ("A4", "-1"), ("A5", "2958466"), ("A6", "1E+300"), ("A7", "60.5")] {
        set(&mut s, cell, input);
    }
    format(&mut s, "A1:A7", "m/d/yyyy");
    py(&mut s, "B1", r#"[xl("A1"), xl("A2"), xl("A3"), xl("A4"), xl("A5"), xl("A6"), xl("A7")]"#);
    settle(&mut s);
    let inputs = sent(&h);
    assert_eq!(inputs.len(), 7);
    for (reference, input) in &inputs {
        assert!(input.stamps.is_empty() && matches!(input.value, Value::Number(_)), "{reference}: {input:?}");
    }
    // The days on either side of the one that never was are real.
    set(&mut s, "A1", "59");
    set(&mut s, "A7", "61");
    settle(&mut s);
    let inputs = sent(&h);
    assert_eq!(stamp_of(&inputs["A1"]), Some(Stamp::Date(Day { year: 1900, month: 2, day: 28 })));
    assert_eq!(stamp_of(&inputs["A7"]), Some(Stamp::Date(Day { year: 1900, month: 3, day: 1 })));
}

#[test]
fn a_time_format_gives_a_time_below_one_and_a_date_above() {
    let (mut s, h) = session_with_inputs();
    for (cell, input) in [("A1", "0"), ("A2", "0.5"), ("A3", "45306.5"), ("A4", "60.5"), ("A5", "-0.5")] {
        set(&mut s, cell, input);
    }
    format(&mut s, "A1:A5", "h:mm");
    py(&mut s, "B1", r#"[xl("A1"), xl("A2"), xl("A3"), xl("A4"), xl("A5")]"#);
    settle(&mut s);
    let inputs = sent(&h);
    assert_eq!(stamp_of(&inputs["A1"]), Some(Stamp::Time(Clock::MIDNIGHT)));
    assert_eq!(stamp_of(&inputs["A2"]), Some(Stamp::Time(NOON)));
    assert_eq!(stamp_of(&inputs["A3"]), Some(Stamp::DateTime(JAN15, NOON)));
    assert_eq!(inputs["A4"].value, Value::Number(60.5), "the day that never was");
    assert_eq!(inputs["A5"].value, Value::Number(-0.5));
}

#[test]
fn a_new_format_runs_what_reads_the_cell_and_others_do_not() {
    let (mut s, h) = session_with_inputs();
    set(&mut s, "A1", "45306");
    py(&mut s, "B1", r#"xl("A1")"#);
    settle(&mut s);
    assert_eq!(runs(&h), 1);
    assert_eq!(stamp_of(&sent(&h)["A1"]), None);
    // A number to a date: Python sees something else.
    format(&mut s, "A1", "m/d/yyyy");
    settle(&mut s);
    assert_eq!(runs(&h), 2);
    assert_eq!(stamp_of(&sent(&h)["A1"]), Some(Stamp::Date(JAN15)));
    // Another date format: Python sees the same.
    format(&mut s, "A1", "yyyy-mm-dd");
    settle(&mut s);
    assert_eq!(runs(&h), 2);
    format(&mut s, "A1", "0.00");
    settle(&mut s);
    assert_eq!(runs(&h), 3);
    assert_eq!(stamp_of(&sent(&h)["A1"]), None);
    // A format on something else runs nothing.
    format(&mut s, "A1", "0.0");
    set(&mut s, "C1", "text");
    format(&mut s, "C1", "m/d/yyyy");
    settle(&mut s);
    assert_eq!(runs(&h), 3);
}

/// A spilled value has no cell to change, so the format of its column is all that tells.
#[test]
fn a_new_column_format_reaches_what_reads_a_spill() {
    let (mut s, h) = session_with_inputs();
    set(&mut s, "A1", "=SEQUENCE(2,1,45306)");
    py(&mut s, "C1", r#"xl("A2")"#);
    settle(&mut s);
    assert_eq!(runs(&h), 1);
    assert_eq!(stamp_of(&sent(&h)["A2"]), None);
    format(&mut s, "A:A", "m/d/yyyy");
    settle(&mut s);
    assert_eq!(runs(&h), 2);
    assert_eq!(stamp_of(&sent(&h)["A2"]), Some(Stamp::Date(Day { day: 16, ..JAN15 })));
    // A row's format comes before its column's.
    format(&mut s, "2:2", "0.00");
    settle(&mut s);
    assert_eq!(runs(&h), 3);
    assert_eq!(stamp_of(&sent(&h)["A2"]), None);
    // A column's width isn't its format.
    s.execute("home.columnWidth", json!({"cols": "A", "width": 30})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h), 3);
}

#[test]
fn a_constant_name_has_no_format() {
    let (mut s, h) = session_with_inputs();
    s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=45306"})).unwrap();
    py(&mut s, "B1", r#"xl("Rate")"#);
    settle(&mut s);
    assert_eq!(v(&s, "B1"), Value::Empty);
    assert_eq!(sent(&h)["Rate"].value, Value::Number(45306.0));
}

#[test]
fn an_error_in_a_date_cell_stays_an_error() {
    let (mut s, h) = session_with_inputs();
    set(&mut s, "A1", "=1/0");
    format(&mut s, "A1", "m/d/yyyy");
    py(&mut s, "B1", r#"xl("A1")"#);
    settle(&mut s);
    assert_eq!(sent(&h)["A1"].value, Value::Error(CellError::Div0));
    assert!(s.doc().unwrap().wb.active().unwrap().cell(CellRef::parse("B1").unwrap()).is_some());
}

fn scalar(stamp: Stamp) -> Output {
    Output::Dated(Dated { value: Value::Empty, stamps: vec![(0, stamp)] })
}

fn column(data: Vec<Value>, stamps: Vec<(u32, Stamp)>) -> Output {
    let rows = data.len();
    Output::Dated(Dated { value: Value::from(Array::new(rows, 1, data).unwrap()), stamps })
}

/// A session whose worker answers by the first word of the code, from what the test gives it.
fn session_answering(answers: Vec<(&'static str, Output)>) -> (Session, FakeHandle) {
    let t = FakeTransport::answering(move |code, _| {
        let word = code.split_whitespace().next().unwrap_or("");
        match word {
            "num" => Ok(Output::Value(Value::Number(5.0))),
            "text" => Ok(Output::Value(Value::text("x"))),
            "fail" => Err(PyError { message: "ValueError: boom".into(), line: None, kind: None, subject: None }),
            _ => answers.iter().find(|(w, _)| *w == word).map(|(_, o)| Ok(o.clone())).unwrap_or(Ok(Output::Value(Value::Empty))),
        }
    });
    let h = t.handle();
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        limits: Limits::default(),
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    (s, h)
}

fn format_of(s: &Session, cell: &str) -> String {
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).num_fmt.as_str().to_string()
}

fn shown(s: &Session, cell: &str) -> String {
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    crate::display::value_text(wb, sh, CellRef::parse(cell).unwrap())
}

fn diagnostic(s: &Session, cell: &str) -> Option<&'static str> {
    s.python_diagnostic(0, CellRef::parse(cell).unwrap()).map(|d| d.kind.id)
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().undo.len()
}

#[test]
fn a_date_result_is_a_serial_with_a_matching_format() {
    let (mut s, _h) = session_answering(vec![
        ("date", scalar(Stamp::Date(JAN15))),
        ("both", scalar(Stamp::DateTime(JAN15, NOON))),
        ("time", scalar(Stamp::Time(NOON))),
        ("long", scalar(Stamp::Duration(1.5))),
    ]);
    for (cell, code) in [("A1", "date"), ("A2", "both"), ("A3", "time"), ("A4", "long")] {
        py(&mut s, cell, code);
    }
    settle(&mut s);
    assert_eq!((v(&s, "A1"), format_of(&s, "A1"), shown(&s, "A1")), (Value::Number(45306.0), "m/d/yyyy".into(), "1/15/2024".into()));
    assert_eq!((v(&s, "A2"), format_of(&s, "A2"), shown(&s, "A2")), (Value::Number(45306.5), "m/d/yyyy h:mm".into(), "1/15/2024 12:00".into()));
    assert_eq!((v(&s, "A3"), format_of(&s, "A3"), shown(&s, "A3")), (Value::Number(0.5), "h:mm:ss".into(), "12:00:00".into()));
    assert_eq!((v(&s, "A4"), format_of(&s, "A4"), shown(&s, "A4")), (Value::Number(1.5), "[h]:mm:ss".into(), "36:00:00".into()));
    // The card says what it is.
    let cell = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!((cell["python"]["typeName"].as_str(), cell["python"]["preview"].as_str()), (Some("date"), Some("2024-01-15")));
    assert_eq!(diagnostic(&s, "A1"), None);
}

#[test]
fn a_result_in_a_1904_workbook_counts_from_1904() {
    let (mut s, _h) = session_answering(vec![
        ("date", scalar(Stamp::Date(JAN15))),
        ("both", scalar(Stamp::DateTime(JAN15, NOON))),
        ("time", scalar(Stamp::Time(NOON))),
    ]);
    on_1904(&mut s);
    py(&mut s, "A1", "date");
    py(&mut s, "A2", "both");
    py(&mut s, "A3", "time");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), shown(&s, "A1")), (Value::Number(43844.0), "1/15/2024".into()));
    assert_eq!((v(&s, "A2"), shown(&s, "A2")), (Value::Number(43844.5), "1/15/2024 12:00".into()));
    assert_eq!(v(&s, "A3"), Value::Number(0.5));
}

#[test]
fn a_cells_own_format_stays() {
    let (mut s, _h) = session_answering(vec![("date", scalar(Stamp::Date(JAN15)))]);
    format(&mut s, "A1", "yyyy-mm-dd");
    py(&mut s, "A1", "date");
    format(&mut s, "A2", "0.00");
    py(&mut s, "A2", "date");
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), shown(&s, "A1")), ("yyyy-mm-dd".into(), "2024-01-15".into()));
    assert_eq!((format_of(&s, "A2"), shown(&s, "A2")), ("0.00".into(), "45306.00".into()));
    // What else the style holds is kept when a format is written.
    s.execute("home.bold", json!({"range": "A3"})).unwrap();
    py(&mut s, "A3", "date");
    settle(&mut s);
    let wb = &s.doc().unwrap().wb;
    let style = wb.styles.get(wb.active().unwrap().style_id(CellRef::parse("A3").unwrap()));
    assert!(style.font.bold && style.num_fmt.as_str() == "m/d/yyyy", "{style:?}");
}

#[test]
fn a_spill_formats_its_date_cells_and_only_those() {
    let data = vec![Value::text("when"), Value::Empty, Value::Number(5.0), Value::Empty, Value::Empty];
    let (mut s, _h) = session_answering(vec![("dates", column(data, vec![(1, Stamp::Date(JAN15)), (3, Stamp::Time(NOON))]))]);
    py(&mut s, "A1", "dates");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Number(45306.0));
    assert_eq!(v(&s, "A4"), Value::Number(0.5));
    assert_eq!(
        (format_of(&s, "A1"), format_of(&s, "A2"), format_of(&s, "A3"), format_of(&s, "A4"), format_of(&s, "A5")),
        ("General".into(), "m/d/yyyy".into(), "General".into(), "h:mm:ss".into(), "General".into())
    );
    assert_eq!(shown(&s, "A2"), "1/15/2024");
}

#[test]
fn a_column_with_times_shows_them_on_all_its_dates() {
    let data = vec![Value::Empty, Value::Empty, Value::Empty, Value::Empty];
    let output = Output::Dated(Dated {
        value: Value::from(Array::new(2, 2, data).unwrap()),
        stamps: vec![(0, Stamp::Date(JAN15)), (1, Stamp::Date(JAN15)), (2, Stamp::DateTime(JAN15, NOON)), (3, Stamp::Date(JAN15))],
    });
    let (mut s, _h) = session_answering(vec![("dates", output)]);
    py(&mut s, "A1", "dates");
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), format_of(&s, "A2")), ("m/d/yyyy h:mm".into(), "m/d/yyyy h:mm".into()));
    assert_eq!((format_of(&s, "B1"), format_of(&s, "B2")), ("m/d/yyyy".into(), "m/d/yyyy".into()));
}

#[test]
fn a_blocked_spill_formats_nothing_until_it_is_free() {
    let data = vec![Value::Empty, Value::Empty];
    let dates = column(data, vec![(0, Stamp::Date(JAN15)), (1, Stamp::Date(Day { day: 16, ..JAN15 }))]);
    let (mut s, _h) = session_answering(vec![("dates", dates)]);
    set(&mut s, "A2", "7");
    py(&mut s, "A1", "dates");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert_eq!(
        (format_of(&s, "A1"), format_of(&s, "A2"), shown(&s, "A2")),
        ("General".into(), "General".into(), "7".into()),
        "what is in the way isn't a date"
    );
    // Out of the way, the dates show as dates, and that is no step of its own.
    let undo = undo_len(&s);
    set(&mut s, "A2", "");
    settle(&mut s);
    assert_eq!(undo_len(&s), undo + 1, "only the edit");
    assert_eq!((shown(&s, "A1"), shown(&s, "A2")), ("1/15/2024".into(), "1/16/2024".into()));
    // In the way again: the number typed there isn't shown as a date, and the formats go.
    set(&mut s, "A2", "7");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert_eq!((format_of(&s, "A1"), format_of(&s, "A2"), shown(&s, "A2")), ("General".into(), "General".into(), "7".into()));
    // Undo puts back what was there then.
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((shown(&s, "A1"), shown(&s, "A2")), ("1/15/2024".into(), "1/16/2024".into()));
}

#[test]
fn a_format_goes_with_the_date_it_was_written_for() {
    let (mut s, _h) = session_answering(vec![("date", scalar(Stamp::Date(JAN15))), ("time", scalar(Stamp::Time(NOON)))]);
    py(&mut s, "A1", "date");
    settle(&mut s);
    assert_eq!(format_of(&s, "A1"), "m/d/yyyy");
    // Another kind of date takes its own format.
    py(&mut s, "A1", "time");
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), shown(&s, "A1")), ("h:mm:ss".into(), "12:00:00".into()));
    // A number isn't a date.
    py(&mut s, "A1", "num");
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), shown(&s, "A1")), ("General".into(), "5".into()));
    // Replacing the code by Find and Replace is the same.
    py(&mut s, "A1", "date one");
    settle(&mut s);
    assert_eq!(format_of(&s, "A1"), "m/d/yyyy");
    s.execute("edit.replace", json!({"what": "date one", "with": "num one"})).unwrap();
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), shown(&s, "A1")), ("General".into(), "5".into()));
    // A failed run has no date.
    py(&mut s, "A1", "date");
    settle(&mut s);
    py(&mut s, "A1", "fail");
    settle(&mut s);
    assert_eq!(format_of(&s, "A1"), "General");
}

#[test]
fn a_format_the_person_chose_is_never_taken_back() {
    let (mut s, _h) = session_answering(vec![("date", scalar(Stamp::Date(JAN15)))]);
    py(&mut s, "A1", "date");
    settle(&mut s);
    format(&mut s, "A1", "yyyy-mm-dd");
    py(&mut s, "A1", "num");
    settle(&mut s);
    assert_eq!(format_of(&s, "A1"), "yyyy-mm-dd");
    // And it comes back to no format only when the person says so.
    py(&mut s, "A2", "num");
    format(&mut s, "A2", "m/d/yyyy");
    settle(&mut s);
    assert_eq!(format_of(&s, "A2"), "m/d/yyyy", "the same code as a result's, and no result wrote it");
}

/// General is all a format can say about having none, so what tells a person taking a result's
/// format off from a cell that never had one is that a result wrote one there.
#[test]
fn a_format_the_person_takes_off_stays_off() {
    let dates = column(vec![Value::Empty, Value::Empty], vec![(0, Stamp::Date(JAN15)), (1, Stamp::Date(JAN15))]);
    let (mut s, h) = session_answering(vec![("date", dates)]);
    set(&mut s, "C1", "1");
    py(&mut s, "A1", "date xl(\"C1\")");
    settle(&mut s);
    assert_eq!(shown(&s, "A2"), "1/15/2024");
    format(&mut s, "A2", "General");
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), shown(&s, "A2")), ("m/d/yyyy".into(), "45306".into()), "it isn't put back");
    // A new run gives dates again: the cell the person changed stays as they left it.
    let before = runs(&h);
    set(&mut s, "C1", "2");
    settle(&mut s);
    assert_eq!(runs(&h), before + 1);
    assert_eq!((format_of(&s, "A1"), shown(&s, "A2")), ("m/d/yyyy".into(), "45306".into()));
    // A result with no date there lets go of it, and the next date there takes a format again.
    py(&mut s, "A1", "num");
    settle(&mut s);
    py(&mut s, "A1", "date");
    settle(&mut s);
    assert_eq!(shown(&s, "A2"), "1/15/2024");
}

#[test]
fn a_shrinking_spill_takes_its_formats_along() {
    let long = column(vec![Value::Empty; 3], vec![(0, Stamp::Date(JAN15)), (1, Stamp::Date(JAN15)), (2, Stamp::Date(JAN15))]);
    let short = column(vec![Value::Empty; 2], vec![(0, Stamp::Date(JAN15)), (1, Stamp::Date(JAN15))]);
    let (mut s, _h) = session_answering(vec![("long", long), ("short", short)]);
    py(&mut s, "A1", "long");
    settle(&mut s);
    assert_eq!(format_of(&s, "A3"), "m/d/yyyy");
    format(&mut s, "A2", "yyyy");
    py(&mut s, "A1", "short");
    settle(&mut s);
    assert_eq!(v(&s, "A3"), Value::Empty);
    assert_eq!((format_of(&s, "A1"), format_of(&s, "A2"), format_of(&s, "A3")), ("m/d/yyyy".into(), "yyyy".into(), "General".into()));
    let wb = &s.doc().unwrap().wb;
    assert!(wb.active().unwrap().cell(CellRef::parse("A3").unwrap()).is_none(), "and leaves no empty cell behind");
}

#[test]
fn a_format_makes_no_undo_step_and_undo_goes_back_past_it() {
    let (mut s, h) = session_answering(vec![("date", scalar(Stamp::Date(JAN15)))]);
    set(&mut s, "C1", "1");
    py(&mut s, "A1", "date");
    let undo = undo_len(&s);
    settle(&mut s);
    assert_eq!(format_of(&s, "A1"), "m/d/yyyy");
    assert_eq!(undo_len(&s), undo, "the result and its format are no step");
    set(&mut s, "C2", "2");
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), v(&s, "C2")), ("m/d/yyyy".into(), Value::Empty));
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), v(&s, "A1")), ("General".into(), Value::Empty), "the cell is gone, and the format with it");
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), v(&s, "A1")), ("m/d/yyyy".into(), Value::Number(45306.0)));
    assert_eq!(h.starts(), 1);
}

/// What reads a result goes by the format that came with it.
#[test]
fn what_reads_a_date_result_gets_a_date() {
    let t = FakeTransport::answering(|code, inputs| {
        Ok(match code {
            "date" => scalar(Stamp::Date(JAN15)),
            _ => Output::Value(Value::Number(inputs.values().next().map_or(-1.0, |i| i.stamps.len() as f64))),
        })
    });
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    py(&mut s, "A1", "date");
    py(&mut s, "A2", r#"xl("A1")"#);
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Number(1.0), "in the same go");
}

#[test]
fn a_date_a_serial_can_not_hold_is_num_and_says_why() {
    let far = Stamp::Date(Day { year: 1800, month: 1, day: 1 });
    let (mut s, h) = session_answering(vec![
        ("far", scalar(far)),
        ("long", scalar(Stamp::Duration(1e12))),
        ("some", column(vec![Value::Empty, Value::Empty], vec![(0, far), (1, Stamp::Date(JAN15))])),
    ]);
    py(&mut s, "A1", "far");
    py(&mut s, "A2", "long");
    py(&mut s, "C1", "some");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), format_of(&s, "A1"), diagnostic(&s, "A1")), (Value::Error(CellError::Num), "General".into(), Some("result.date")));
    assert_eq!((v(&s, "A2"), diagnostic(&s, "A2")), (Value::Error(CellError::Num), Some("result.date")));
    assert!(s.python_diagnostic(0, CellRef::parse("A1").unwrap()).unwrap().detail.contains("1800-01-01"));
    // In a spill it is that one cell.
    assert_eq!((v(&s, "C1"), format_of(&s, "C1")), (Value::Error(CellError::Num), "General".into()));
    assert_eq!((v(&s, "C2"), format_of(&s, "C2")), (Value::Number(45306.0), "m/d/yyyy".into()));
    assert_eq!(h.starts(), 1, "none of it cost the worker");
}

/// Whatever the worker claims, a date it can't be asked to explain is a bad value in one cell.
#[test]
fn a_date_the_parent_can_not_convert_is_value_and_kills_nothing() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let id = req.id();
            let line = format!(r##"{{"type":"done","id":{id},"ok":true,"value":{{"date":"2023-02-30"}}}}"##);
            let spill = format!(
                r##"{{"type":"done","id":{id},"ok":true,"value":{{"array":{{"rows":2,"cols":1,"data":[{{"time":"25:00:00"}},{{"date":"2024-01-15"}}]}}}}}}"##
            );
            let Request::Run { code, .. } = &req else { continue };
            if !w.write_raw(if code.contains("spill") { spill.as_bytes() } else { line.as_bytes() }) || !w.write_raw(b"\n") {
                return;
            }
        }
    });
    let h = t.handle();
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    py(&mut s, "A1", "scalar");
    py(&mut s, "C1", "spill");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), format_of(&s, "A1")), (Value::Error(CellError::Value), "General".into()));
    assert_eq!((v(&s, "C1"), v(&s, "C2")), (Value::Error(CellError::Value), Value::Number(45306.0)));
    assert_eq!(format_of(&s, "C2"), "m/d/yyyy");
    assert_eq!(h.starts(), 1, "the worker stayed");
}

/// A date past the cap, on the other hand, is a broken line like any other.
#[test]
fn a_date_over_the_cap_stops_the_worker_like_any_broken_line() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let line = format!("{{\"type\":\"done\",\"id\":{},\"ok\":true,\"value\":{{\"date\":\"{}\"}}}}\n", req.id(), "1".repeat(65));
            if !w.write_raw(line.as_bytes()) {
                return;
            }
        }
    });
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    py(&mut s, "A1", "x");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Python));
}

/// A result reaches a locked cell on a protected sheet, and its format with it: neither is an edit.
#[test]
fn a_protected_sheet_shows_a_results_dates() {
    let (mut s, _h) =
        session_answering(vec![("dates", column(vec![Value::Empty, Value::Empty], vec![(0, Stamp::Date(JAN15)), (1, Stamp::Date(JAN15))]))]);
    py(&mut s, "A1", "dates");
    s.execute("review.protectSheet", json!({})).unwrap();
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((shown(&s, "A1"), shown(&s, "A2")), ("1/15/2024".into(), "1/15/2024".into()));
    let wb = &s.doc().unwrap().wb;
    assert!(wb.styles.get(wb.active().unwrap().style_id(CellRef::parse("A2").unwrap())).protection.locked, "and they stay locked");
}

#[test]
fn a_negative_length_of_time_stays_a_number() {
    let (mut s, _h) = session_answering(vec![("back", scalar(Stamp::Duration(-1.5)))]);
    py(&mut s, "A1", "back");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), format_of(&s, "A1"), shown(&s, "A1")), (Value::Number(-1.5), "General".into(), "-1.5".into()));
}

#[test]
fn a_python_object_with_a_date_keeps_its_date() {
    let (mut s, _h) = session_answering(vec![("date", scalar(Stamp::Date(JAN15)))]);
    s.execute("python.set", json!({"cell": "A1", "code": "date", "output": "object"})).unwrap();
    settle(&mut s);
    assert_eq!((v(&s, "A1"), format_of(&s, "A1")), (Value::Number(45306.0), "m/d/yyyy".into()), "a date stays a date, as a number stays a number");
    s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    settle(&mut s);
    assert_eq!(format_of(&s, "A1"), "m/d/yyyy");
}

#[test]
fn a_saved_result_keeps_its_format_and_values() {
    let (mut s, _h) =
        session_answering(vec![("dates", column(vec![Value::Empty, Value::Empty], vec![(0, Stamp::Date(JAN15)), (1, Stamp::Time(NOON))]))]);
    py(&mut s, "A1", "dates");
    settle(&mut s);
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sh = reopened.active().unwrap();
    let code = |c: &str| reopened.styles.get(sh.style_id(CellRef::parse(c).unwrap())).num_fmt.as_str().to_string();
    assert_eq!((code("A1"), code("A2")), ("m/d/yyyy".into(), "h:mm:ss".into()));
    assert_eq!(sh.value(CellRef::parse("A1").unwrap()), Value::Number(45306.0));
}

/// Many formats share one style, and go again with the dates.
#[test]
fn a_spill_of_many_dates_formats_them_all() {
    let n = 100_000;
    let stamps = (0..n).map(|i| (i, Stamp::Date(JAN15))).collect();
    let (mut s, _h) = session_answering(vec![("dates", column(vec![Value::Empty; n as usize], stamps))]);
    py(&mut s, "A1", "dates");
    settle(&mut s);
    assert_eq!((format_of(&s, "A1"), format_of(&s, &format!("A{n}"))), ("m/d/yyyy".into(), "m/d/yyyy".into()));
    assert_eq!(v(&s, &format!("A{n}")), Value::Number(45306.0));
    py(&mut s, "A1", "num");
    settle(&mut s);
    assert_eq!(format_of(&s, &format!("A{n}")), "General");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.len(), 1, "no empty cells are left behind");
}
