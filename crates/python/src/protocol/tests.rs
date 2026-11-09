use super::*;

fn array(rows: usize, cols: usize, data: Vec<Value>) -> Value {
    Array::new(rows, cols, data).map(Value::from).unwrap()
}

fn done_line(value_json: &str) -> String {
    format!(r#"{{"type":"done","id":1,"ok":true,"value":{value_json}}}"#)
}

fn value_of(line: &str) -> Result<Output, String> {
    match Reply::parse(line.as_bytes())? {
        Reply::Done { result: Ok(out), .. } => Ok(out),
        other => Err(format!("not a value: {other:?}")),
    }
}

#[test]
fn requests_and_replies_round_trip() {
    let mut inputs = BTreeMap::new();
    inputs.insert("A1".to_string(), Input::new(Value::Number(2.5)));
    inputs.insert(
        "Sheet 2!B1:C2".to_string(),
        Input {
            value: array(2, 2, vec![Value::text("é\"\n"), Value::Bool(true), Value::Empty, Value::Error(CellError::Connect)]),
            headers: Some(true),
            stamps: Vec::new(),
        },
    );
    let run = Request::Run { id: 7, object: false, code: "x = xl(\"A1\")\nx * 2".into(), inputs, init: None, at: None };
    let line = run.to_line();
    assert!(line.ends_with(b"\n") && !line[..line.len() - 1].contains(&b'\n'), "one line");
    assert_eq!(Request::parse(&line[..line.len() - 1]).unwrap(), run);
    let reset = Request::Reset { id: 8 };
    assert_eq!(Request::parse(reset.to_line().trim_ascii_end()).unwrap(), reset);
    assert_eq!(String::from_utf8(reset.to_line()).unwrap(), "{\"type\":\"reset\",\"id\":8}\n");

    for reply in [
        Reply::Ready { self_test: SelfTest::NotChecked },
        Reply::Ready { self_test: SelfTest::Failed("socket opened".into()) },
        Reply::Done {
            id: 3,
            result: Ok(Output::Value(array(1, 2, vec![Value::Number(1.0), Value::text("b")]))),
            stdout: "hi\n".into(),
            peak: None,
            touched: None,
        },
        Reply::Done { id: 4, result: Ok(Output::object("dict", "{'a': 1}")), stdout: String::new(), peak: Some(123), touched: None },
        Reply::Done {
            id: 5,
            result: Err(PyError { message: "NameError: name 'y' is not defined".into(), line: Some(2), ..PyError::default() }),
            stdout: String::new(),
            peak: Some(456),
            touched: None,
        },
        Reply::Done {
            id: 6,
            result: Err(PyError { message: "SyntaxError".into(), line: None, ..PyError::default() }),
            stdout: String::new(),
            peak: None,
            touched: None,
        },
        Reply::Done { id: 9, result: Ok(Output::Value(Value::Empty)), stdout: String::new(), peak: None, touched: None },
    ] {
        let line = reply.to_line();
        assert_eq!(Reply::parse(line.trim_ascii_end()).unwrap(), reply);
    }
}

#[test]
fn what_the_shim_writes_parses() {
    assert_eq!(Reply::parse(br#"{"type":"ready","selfTest":{"status":"notChecked"}}"#).unwrap(), Reply::Ready { self_test: SelfTest::NotChecked });
    assert_eq!(value_of(&done_line("3")).unwrap(), Output::Value(Value::Number(3.0)));
    assert_eq!(value_of(&done_line("-1.5e300")).unwrap(), Output::Value(Value::Number(-1.5e300)));
    assert_eq!(value_of(&done_line("null")).unwrap(), Output::Value(Value::Empty));
    assert_eq!(value_of(&done_line(r##"{"error":"#NUM!"}"##)).unwrap(), Output::Value(Value::Error(CellError::Num)));
    assert_eq!(
        value_of(&done_line(r##"{"array":{"rows":2,"cols":1,"data":[true,{"error":"#N/A"}]}}"##)).unwrap(),
        Output::Value(array(2, 1, vec![Value::Bool(true), Value::Error(CellError::NA)]))
    );
    // The shape may come after the data.
    assert_eq!(
        value_of(&done_line(r#"{"array":{"data":[1,2],"cols":2,"rows":1}}"#)).unwrap(),
        Output::Value(array(1, 2, vec![1.0.into(), 2.0.into()]))
    );
}

#[test]
fn array_shapes_are_checked_against_the_data() {
    for bad in [
        r#"{"array":{"rows":2,"cols":2,"data":[1,2,3]}}"#,
        r#"{"array":{"rows":1,"cols":2,"data":[1,2,3]}}"#,
        r#"{"array":{"data":[1,2,3],"rows":2,"cols":2}}"#,
        r#"{"array":{"rows":0,"cols":0,"data":[]}}"#,
        r#"{"array":{"rows":0,"cols":5,"data":[]}}"#,
        r#"{"array":{"rows":1,"cols":1}}"#,
        r#"{"array":{"rows":1,"data":[1]}}"#,
        r#"{"array":{"rows":1,"cols":1,"data":[1],"rows":1}}"#,
        r#"{"array":{"rows":1,"cols":1,"data":[1],"extra":1}}"#,
        r#"{"array":{"rows":-1,"cols":1,"data":[1]}}"#,
        r#"{"array":{"rows":1.5,"cols":1,"data":[1]}}"#,
        // A product that wraps around in 64 bits.
        r#"{"array":{"rows":4294967296,"cols":4294967296,"data":[1]}}"#,
        r#"{"array":{"rows":18446744073709551615,"cols":2,"data":[1]}}"#,
        // Nesting: arrays and objects are not elements.
        r#"{"array":{"rows":1,"cols":1,"data":[{"array":{"rows":1,"cols":1,"data":[1]}}]}}"#,
        r#"{"array":{"rows":1,"cols":1,"data":[{"object":{"typeName":"x","preview":"x"}}]}}"#,
        r#"{"array":{"rows":1,"cols":1,"data":[[1]]}}"#,
        r#"{"array":{"rows":1,"cols":1,"data":{"0":1}}}"#,
    ] {
        assert!(value_of(&done_line(bad)).is_err(), "{bad}");
    }
}

#[test]
fn arrays_over_a_million_cells_are_refused() {
    let cells = |n: usize| vec!["0"; n].join(",");
    let at_cap = done_line(&format!(r#"{{"array":{{"rows":1000,"cols":1000,"data":[{}]}}}}"#, cells(MAX_ARRAY_CELLS)));
    match value_of(&at_cap).unwrap() {
        Output::Value(Value::Array(a)) => assert_eq!(a.data.len(), MAX_ARRAY_CELLS),
        other => panic!("{other:?}"),
    }
    // One more cell, with a shape that matches it, is over the cap.
    let over = done_line(&format!(r#"{{"array":{{"rows":1000001,"cols":1,"data":[{}]}}}}"#, cells(MAX_ARRAY_CELLS + 1)));
    assert!(value_of(&over).unwrap_err().contains("1000000 cells"));
    // Data that runs past a million is stopped while it is read, whatever the shape says.
    let long = done_line(&format!(r#"{{"array":{{"data":[{}],"rows":1,"cols":1}}}}"#, cells(MAX_ARRAY_CELLS + 1)));
    assert!(value_of(&long).is_err());
}

#[test]
fn every_field_has_a_cap() {
    let text = |n: usize| format!("\"{}\"", "x".repeat(n));
    assert!(value_of(&done_line(&text(MAX_TEXT_BYTES))).is_ok());
    assert!(value_of(&done_line(&text(MAX_TEXT_BYTES + 1))).is_err());
    let in_array = format!(r#"{{"array":{{"rows":1,"cols":1,"data":[{}]}}}}"#, text(MAX_TEXT_BYTES + 1));
    assert!(value_of(&done_line(&in_array)).is_err());

    let error = |n: usize| format!(r#"{{"type":"done","id":1,"ok":false,"error":{{"message":"{}","line":1}}}}"#, "e".repeat(n));
    assert!(Reply::parse(error(MAX_MESSAGE_BYTES).as_bytes()).is_ok());
    assert!(Reply::parse(error(MAX_MESSAGE_BYTES + 1).as_bytes()).is_err());

    let object = |name: usize, preview: &str| format!(r#"{{"object":{{"typeName":"{}","preview":"{preview}"}}}}"#, "t".repeat(name));
    assert!(value_of(&done_line(&object(MAX_TYPE_NAME_BYTES, "p"))).is_ok());
    assert!(value_of(&done_line(&object(MAX_TYPE_NAME_BYTES + 1, "p"))).is_err());
    // The preview counts characters: 255 four-byte characters fit, 256 one-byte ones don't.
    assert!(value_of(&done_line(&object(1, &"\u{1F600}".repeat(MAX_PREVIEW_CHARS)))).is_ok());
    assert!(value_of(&done_line(&object(1, &"p".repeat(MAX_PREVIEW_CHARS + 1)))).is_err());

    let stdout = |n: usize| format!(r#"{{"type":"done","id":1,"ok":true,"value":null,"stdout":"{}"}}"#, "o".repeat(n));
    assert!(Reply::parse(stdout(MAX_STDOUT_BYTES).as_bytes()).is_ok());
    assert!(Reply::parse(stdout(MAX_STDOUT_BYTES + 1).as_bytes()).is_err());

    let detail = |n: usize| format!(r#"{{"type":"ready","selfTest":{{"status":"failed","detail":"{}"}}}}"#, "d".repeat(n));
    assert!(Reply::parse(detail(MAX_SELF_TEST_BYTES).as_bytes()).is_ok());
    assert!(Reply::parse(detail(MAX_SELF_TEST_BYTES + 1).as_bytes()).is_err());

    // The line itself.
    let mut long = done_line("null").into_bytes();
    long.resize(MAX_LINE_BYTES + 1, b' ');
    assert!(Reply::parse(&long).unwrap_err().contains("more than"));
}

#[test]
fn malformed_replies_are_refused() {
    for bad in [
        "",
        "not json",
        "[]",
        "{}",
        r#"{"type":"done"}"#,
        r#"{"type":"done","id":1}"#,
        r#"{"type":"done","id":1,"ok":true}"#,
        r#"{"type":"done","id":1,"ok":false}"#,
        r#"{"type":"done","id":1,"ok":true,"value":1,"error":{"message":"x"}}"#,
        r#"{"type":"done","id":1,"ok":false,"value":1,"error":{"message":"x"}}"#,
        r#"{"type":"done","id":1,"ok":true,"value":1,"id":2}"#,
        r#"{"type":"done","id":-1,"ok":true,"value":1}"#,
        r#"{"type":"done","id":1,"ok":true,"value":1,"extend":600000}"#,
        r#"{"type":"done","id":1,"ok":false,"error":{"message":"x","line":-1}}"#,
        r#"{"type":"done","id":1,"ok":false,"error":{"message":"x","line":4294967296}}"#,
        r#"{"type":"done","id":1,"ok":true,"value":1,"selfTest":{"status":"passed"}}"#,
        r#"{"type":"extend","id":1,"ms":600000}"#,
        r#"{"type":"xl","id":1,"reference":"A1"}"#,
        r#"{"type":"ready"}"#,
        r#"{"type":"ready","id":1,"selfTest":{"status":"notChecked"}}"#,
        r#"{"type":"ready","selfTest":{"status":"passed","detail":"x"}}"#,
        r#"{"type":"ready","selfTest":{"status":"maybe"}}"#,
        r#"{"type":"done","id":1,"ok":true,"value":1} trailing"#,
    ] {
        assert!(Reply::parse(bad.as_bytes()).is_err(), "{bad}");
    }
    for bad_value in [
        r##"{"error":"#NOPE!"}"##,
        r##"{"error":"#CIRC!"}"##,
        r##"{"error":1}"##,
        r##"{"error":"#N/A","array":{}}"##,
        r#"{}"#,
        r#"{"number":1}"#,
        r#"[1,2]"#,
        r#"{"object":{"typeName":"x"}}"#,
        r#"{"object":{"typeName":"x","preview":"y","extra":1}}"#,
    ] {
        assert!(value_of(&done_line(bad_value)).is_err(), "{bad_value}");
    }
    // Not UTF-8.
    assert!(Reply::parse(b"{\"type\":\"done\",\"id\":1,\"ok\":true,\"value\":\"\xff\"}").is_err());
    // A lone surrogate escape isn't text.
    assert!(value_of(&done_line(r#""\ud800""#)).is_err());
}

#[test]
fn inputs_never_carry_objects_and_odd_numbers_become_errors() {
    let bad = br#"{"type":"run","id":1,"code":"","inputs":{"A1":{"value":{"object":{"typeName":"x","preview":"y"}}}}}"#;
    assert!(Request::parse(bad).is_err());
    // A number JSON can't hold is sent as #NUM!, and an array inside an array as #VALUE!.
    let inner = array(1, 1, vec![Value::Number(1.0)]);
    let mut inputs = BTreeMap::new();
    inputs.insert("A1".to_string(), Input::new(Value::Number(f64::NAN)));
    inputs.insert("B1:B2".to_string(), Input::new(array(2, 1, vec![inner, Value::Number(f64::INFINITY)])));
    let line = Request::Run { id: 1, code: String::new(), inputs, object: true, init: None, at: None }.to_line();
    let Request::Run { inputs, object: true, .. } = Request::parse(line.trim_ascii_end()).unwrap() else { panic!() };
    assert_eq!(inputs["A1"].value, Value::Error(CellError::Num));
    assert_eq!(inputs["B1:B2"].value, array(2, 1, vec![Value::Error(CellError::Value), Value::Error(CellError::Num)]));
}

/// The shim keeps its output within the same caps, so a change on one side must be made on both.
#[test]
fn the_shim_has_the_same_caps() {
    let shim = include_str!("../shim.py");
    for (name, value) in [
        ("MAX_LINE_BYTES", MAX_LINE_BYTES),
        ("MAX_TEXT_BYTES", MAX_TEXT_BYTES),
        ("MAX_MESSAGE_BYTES", MAX_MESSAGE_BYTES),
        ("MAX_TYPE_NAME_BYTES", MAX_TYPE_NAME_BYTES),
        ("MAX_DATE_BYTES", MAX_DATE_BYTES),
        ("MAX_PREVIEW_CHARS", MAX_PREVIEW_CHARS),
        ("MAX_SIZE_DIMS", MAX_SIZE_DIMS),
        ("MAX_SIZE", MAX_SIZE as usize),
        ("MAX_TABLE_ROWS", MAX_TABLE_ROWS),
        ("MAX_TABLE_COLS", MAX_TABLE_COLS),
        ("MAX_TABLE_TEXT_BYTES", MAX_TABLE_TEXT_BYTES),
        ("MAX_TABLE_BYTES", MAX_TABLE_BYTES),
        ("MAX_STDOUT_BYTES", MAX_STDOUT_BYTES),
        ("MAX_ARRAY_CELLS", MAX_ARRAY_CELLS),
    ] {
        assert!(shim.contains(&format!("    {name} = {value}\n")), "{name} = {value} in shim.py");
    }
    // And knows every error value.
    for e in CellError::ALL {
        assert!(shim.contains(&format!("\"{}\"", e.as_str())), "{e} in shim.py");
    }
}

#[test]
fn a_peak_is_an_optional_capped_number_on_an_answer() {
    let peak = |line: &str| match Reply::parse(line.as_bytes()) {
        Ok(Reply::Done { peak, .. }) => Ok(peak),
        Ok(other) => panic!("{other:?}"),
        Err(e) => Err(e),
    };
    assert_eq!(peak(r#"{"type":"done","id":1,"ok":true,"value":1}"#), Ok(None));
    assert_eq!(peak(r#"{"type":"done","id":1,"ok":true,"value":1,"peak":1048576}"#), Ok(Some(1_048_576)));
    assert_eq!(peak(&format!(r#"{{"type":"done","id":1,"ok":true,"value":1,"peak":{MAX_PEAK_BYTES}}}"#)), Ok(Some(MAX_PEAK_BYTES)));
    for bad in [
        format!(r#"{{"type":"done","id":1,"ok":true,"value":1,"peak":{}}}"#, MAX_PEAK_BYTES + 1),
        r#"{"type":"done","id":1,"ok":true,"value":1,"peak":-1}"#.to_string(),
        r#"{"type":"done","id":1,"ok":true,"value":1,"peak":1.5}"#.to_string(),
        r#"{"type":"done","id":1,"ok":true,"value":1,"peak":"big"}"#.to_string(),
        r#"{"type":"ready","selfTest":{"status":"notChecked"},"peak":1}"#.to_string(),
    ] {
        assert!(Reply::parse(bad.as_bytes()).is_err(), "{bad}");
    }
}

const DAY: Day = Day { year: 2024, month: 1, day: 15 };
const CLOCK: Clock = Clock { hour: 10, minute: 30, second: 5, micro: 250_000 };

fn dated(stamp: Stamp) -> Output {
    Output::Dated(Dated { value: Value::Empty, stamps: vec![(0, stamp)] })
}

fn day(year: i32, month: u8, day: u8) -> Stamp {
    Stamp::Date(Day { year, month, day })
}

#[test]
fn dates_in_requests_and_replies_round_trip() {
    let mut inputs = BTreeMap::new();
    inputs.insert("A1".to_string(), Input { value: Value::Empty, headers: None, stamps: vec![(0, Stamp::Date(DAY))] });
    inputs.insert(
        "B1:B3".to_string(),
        Input {
            value: array(3, 1, vec![Value::Empty, Value::Number(5.0), Value::Empty]),
            headers: None,
            stamps: vec![(0, Stamp::DateTime(DAY, CLOCK)), (2, Stamp::Time(CLOCK))],
        },
    );
    let run = Request::Run { id: 1, code: "xl('A1')".into(), inputs, object: false, init: None, at: None };
    let line = run.to_line();
    let text = String::from_utf8(line.clone()).unwrap();
    assert!(text.contains(r#""A1":{"value":{"date":"2024-01-15"}}"#), "{text}");
    assert!(text.contains(r#""data":[{"dateTime":"2024-01-15T10:30:05.250000"},5.0,{"time":"10:30:05.250000"}]"#), "{text}");
    assert_eq!(Request::parse(line.trim_ascii_end()).unwrap(), run);

    for output in [
        dated(Stamp::Date(DAY)),
        dated(Stamp::DateTime(DAY, Clock::MIDNIGHT)),
        dated(Stamp::Time(CLOCK)),
        dated(Stamp::Duration(1.5)),
        Output::Dated(Dated {
            value: array(2, 2, vec![Value::text("when"), Value::Empty, Value::Number(1.0), Value::Empty]),
            stamps: vec![(1, Stamp::Date(DAY)), (3, Stamp::Duration(-2.0))],
        }),
    ] {
        let reply = Reply::Done { id: 2, result: Ok(output), stdout: String::new(), peak: None, touched: None };
        assert_eq!(Reply::parse(reply.to_line().trim_ascii_end()).unwrap(), reply);
    }
}

#[test]
fn what_the_shim_writes_for_dates_parses() {
    assert_eq!(value_of(&done_line(r#"{"date":"2024-01-15"}"#)).unwrap(), dated(Stamp::Date(DAY)));
    assert_eq!(value_of(&done_line(r#"{"dateTime":"2024-01-15T10:30:05.25"}"#)).unwrap(), dated(Stamp::DateTime(DAY, CLOCK)));
    assert_eq!(value_of(&done_line(r#"{"dateTime":"2024-01-15T00:00:00"}"#)).unwrap(), dated(Stamp::DateTime(DAY, Clock::MIDNIGHT)));
    assert_eq!(value_of(&done_line(r#"{"time":"10:30:05.250000"}"#)).unwrap(), dated(Stamp::Time(CLOCK)));
    assert_eq!(value_of(&done_line(r#"{"duration":1.5}"#)).unwrap(), dated(Stamp::Duration(1.5)));
    assert_eq!(value_of(&done_line(r#"{"duration":-2}"#)).unwrap(), dated(Stamp::Duration(-2.0)));
    // Nanoseconds are dropped, not rounded; a year no serial holds still reads.
    assert_eq!(value_of(&done_line(r#"{"time":"10:30:05.250000999"}"#)).unwrap(), dated(Stamp::Time(CLOCK)));
    assert_eq!(value_of(&done_line(r#"{"date":"0001-01-01"}"#)).unwrap(), dated(day(1, 1, 1)));
    assert_eq!(value_of(&done_line(r#"{"date":"12345-02-03"}"#)).unwrap(), dated(day(12345, 2, 3)));
    assert_eq!(value_of(&done_line(r#"{"date":"-0500-02-03"}"#)).unwrap(), dated(day(-500, 2, 3)));
    assert_eq!(value_of(&done_line(r#"{"date":"99999999999999999999-01-01"}"#)).unwrap(), dated(day(i32::MAX, 1, 1)));
    // An array keeps the place of each stamp, and the other values.
    assert_eq!(
        value_of(&done_line(r##"{"array":{"rows":3,"cols":1,"data":[{"date":"2024-01-15"},{"error":"#N/A"},{"time":"10:30:05.25"}]}}"##)).unwrap(),
        Output::Dated(Dated {
            value: array(3, 1, vec![Value::Empty, Value::Error(CellError::NA), Value::Empty]),
            stamps: vec![(0, Stamp::Date(DAY)), (2, Stamp::Time(CLOCK))],
        })
    );
    // An array with none is an ordinary value.
    assert_eq!(value_of(&done_line(r#"{"array":{"rows":1,"cols":1,"data":[1]}}"#)).unwrap(), Output::Value(array(1, 1, vec![1.0.into()])));
}

/// Text that keeps the shape and names no date is a bad value, and the worker stays.
#[test]
fn a_date_that_can_not_be_read_is_value_and_kills_nothing() {
    for bad in [
        r#"{"date":""}"#,
        r#"{"date":"not a date"}"#,
        r#"{"date":"2024-13-01"}"#,
        r#"{"date":"2024-00-01"}"#,
        r#"{"date":"2024-01-00"}"#,
        r#"{"date":"2023-02-29"}"#,
        r#"{"date":"1900-02-29"}"#,
        r#"{"date":"24-01-15"}"#,
        r#"{"date":"+2024-01-15"}"#,
        r#"{"date":"2024-1-15"}"#,
        r#"{"date":"2024-01-15-01"}"#,
        r#"{"date":"2024-01-15T00:00:00"}"#,
        r#"{"date":"٢٠٢٤-01-15"}"#,
        r#"{"dateTime":"2024-01-15"}"#,
        r#"{"dateTime":"2024-01-15 10:30:00"}"#,
        r#"{"dateTime":"2024-01-15T24:00:00"}"#,
        r#"{"dateTime":"2024-02-30T10:30:00"}"#,
        r#"{"time":"24:00:00"}"#,
        r#"{"time":"10:60:00"}"#,
        r#"{"time":"10:30:60"}"#,
        r#"{"time":"10:30"}"#,
        r#"{"time":"10:30:00:00"}"#,
        r#"{"time":"10:30:00."}"#,
        r#"{"time":"10:30:00.1234567890"}"#,
        r#"{"time":"10:30:00.x"}"#,
        r#"{"time":"+1:30:00"}"#,
    ] {
        assert_eq!(value_of(&done_line(bad)).unwrap(), Output::Value(Value::Error(CellError::Value)), "{bad}");
        let in_array = format!(r#"{{"array":{{"rows":1,"cols":1,"data":[{bad}]}}}}"#);
        assert_eq!(value_of(&done_line(&in_array)).unwrap(), Output::Value(array(1, 1, vec![Value::Error(CellError::Value)])), "{bad}");
    }
}

#[test]
fn a_date_keeps_the_caps_and_the_shape() {
    let text = |key: &str, n: usize| format!(r#"{{"{key}":"{}"}}"#, "1".repeat(n));
    for key in ["date", "dateTime", "time"] {
        assert_eq!(value_of(&done_line(&text(key, MAX_DATE_BYTES))).unwrap(), Output::Value(Value::Error(CellError::Value)), "{key} at the cap");
        assert!(value_of(&done_line(&text(key, MAX_DATE_BYTES + 1))).unwrap_err().contains("over the cap"), "{key} past the cap");
        let in_array = format!(r#"{{"array":{{"rows":1,"cols":1,"data":[{}]}}}}"#, text(key, MAX_DATE_BYTES + 1));
        assert!(value_of(&done_line(&in_array)).is_err(), "{key} past the cap in an array");
    }
    for bad in [
        r#"{"date":5}"#,
        r#"{"date":null}"#,
        r#"{"date":["2024-01-15"]}"#,
        r#"{"dateTime":{}}"#,
        r#"{"time":true}"#,
        r#"{"duration":"1 day"}"#,
        r#"{"duration":null}"#,
        r#"{"duration":1e999}"#,
        r#"{"date":"2024-01-15","time":"10:30:00"}"#,
        r#"{"date":"2024-01-15","extra":1}"#,
        r#"{"when":"2024-01-15"}"#,
    ] {
        assert!(value_of(&done_line(bad)).is_err(), "{bad}");
    }
}

#[test]
fn a_serial_becomes_what_its_format_shows() {
    let (date, time, both) = ((true, false), (false, true), (true, true));
    let s1900 = gridcraft_core::DateSystem::D1900;
    let s1904 = gridcraft_core::DateSystem::D1904;
    let at = |s, shape, n| Stamp::from_serial(s, shape, n);
    assert_eq!(at(s1900, date, 45306.0), Some(Stamp::Date(DAY)));
    assert_eq!(at(s1904, date, 43844.0), Some(Stamp::Date(DAY)), "the same day, 1,462 days earlier in the count");
    assert_eq!(at(s1900, both, 45306.0), Some(Stamp::DateTime(DAY, Clock::MIDNIGHT)));
    let half = Clock { hour: 12, ..Clock::MIDNIGHT };
    assert_eq!(at(s1900, both, 45306.5), Some(Stamp::DateTime(DAY, half)));
    assert_eq!(at(s1904, both, 43844.5), Some(Stamp::DateTime(DAY, half)));
    assert_eq!(at(s1900, time, 0.5), Some(Stamp::Time(half)));
    assert_eq!(at(s1904, time, 0.5), Some(Stamp::Time(half)));
    assert_eq!(at(s1900, time, 0.0), Some(Stamp::Time(Clock::MIDNIGHT)));
    // What a format doesn't show isn't lost: a date with a time of day, a time with a day.
    assert_eq!(at(s1900, date, 45306.5), Some(Stamp::DateTime(DAY, half)));
    assert_eq!(at(s1900, time, 45306.5), Some(Stamp::DateTime(DAY, half)));
    // No format parts, no date.
    assert_eq!(at(s1900, (false, false), 45306.0), None);
}

#[test]
fn a_serial_with_no_place_on_the_calendar_stays_a_number() {
    let (date, time, both) = ((true, false), (false, true), (true, true));
    let s1900 = gridcraft_core::DateSystem::D1900;
    let s1904 = gridcraft_core::DateSystem::D1904;
    for s in [s1900, s1904] {
        for shape in [date, time, both] {
            for n in [-1.0, -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN, 1e300, 2_958_466.0, 2_958_465.0 + 1e9] {
                assert_eq!(Stamp::from_serial(s, shape, n), None, "{s:?} {shape:?} {n}");
            }
        }
    }
    // The days of the 1900 system that never were.
    for shape in [date, both] {
        for n in [0.0, 0.25, 0.5, 0.75] {
            assert_eq!(Stamp::from_serial(s1900, shape, n), None, "{shape:?} {n}");
        }
    }
    for shape in [date, time, both] {
        for n in [60.0, 60.25, 60.9999, 59.999_999_999_9] {
            assert_eq!(Stamp::from_serial(s1900, shape, n), None, "{shape:?} {n}");
        }
    }
    // Around the day that never was, the calendar is the real one.
    assert_eq!(Stamp::from_serial(s1900, date, 1.0), Some(day(1900, 1, 1)));
    assert_eq!(Stamp::from_serial(s1900, date, 59.0), Some(day(1900, 2, 28)));
    assert_eq!(Stamp::from_serial(s1900, date, 61.0), Some(day(1900, 3, 1)));
    // The 1904 system has none of that: its serial 0 and 60 are real days.
    assert_eq!(Stamp::from_serial(s1904, date, 0.0), Some(day(1904, 1, 1)));
    assert_eq!(Stamp::from_serial(s1904, date, 60.0), Some(day(1904, 3, 1)));
    assert_eq!(
        Stamp::from_serial(s1904, both, 0.5),
        Some(Stamp::DateTime(Day { year: 1904, month: 1, day: 1 }, Clock { hour: 12, ..Clock::MIDNIGHT }))
    );
}

#[test]
fn the_edges_of_a_serial_stay_on_their_own_side() {
    let (date, time, both) = ((true, false), (false, true), (true, true));
    let s1900 = gridcraft_core::DateSystem::D1900;
    let s1904 = gridcraft_core::DateSystem::D1904;
    let last_clock = Clock { hour: 23, minute: 59, second: 59, micro: 999_000 };
    // The last time of a day stays in its day, rather than becoming midnight.
    assert_eq!(Stamp::from_serial(s1900, time, 0.999_999_999_99), Some(Stamp::Time(last_clock)));
    assert_eq!(Stamp::from_serial(s1904, time, 0.999_999_999_99), Some(Stamp::Time(last_clock)));
    // The last moment of the calendar reads as its last millisecond in both systems.
    let last_day = Day { year: 9999, month: 12, day: 31 };
    assert_eq!(Stamp::from_serial(s1900, both, 2_958_465.999_999_999), Some(Stamp::DateTime(last_day, last_clock)));
    assert_eq!(Stamp::from_serial(s1904, both, 2_957_003.999_999_999), Some(Stamp::DateTime(last_day, last_clock)));
    assert_eq!(Stamp::from_serial(s1900, date, 2_958_465.0), Some(Stamp::Date(last_day)));
    assert_eq!(Stamp::from_serial(s1904, date, 2_957_003.0), Some(Stamp::Date(last_day)));
    assert_eq!(Stamp::from_serial(s1904, date, 2_957_004.0), None);
}

#[test]
fn a_stamp_becomes_a_serial_in_the_workbooks_date_system() {
    let s1900 = gridcraft_core::DateSystem::D1900;
    let s1904 = gridcraft_core::DateSystem::D1904;
    assert_eq!(Stamp::Date(DAY).serial(s1900), Ok(45306.0));
    assert_eq!(Stamp::Date(DAY).serial(s1904), Ok(43844.0));
    assert_eq!(Stamp::DateTime(DAY, Clock { hour: 12, ..Clock::MIDNIGHT }).serial(s1900), Ok(45306.5));
    assert_eq!(Stamp::DateTime(DAY, Clock { hour: 12, ..Clock::MIDNIGHT }).serial(s1904), Ok(43844.5));
    assert_eq!(Stamp::Time(Clock { hour: 18, ..Clock::MIDNIGHT }).serial(s1900), Ok(0.75));
    assert_eq!(Stamp::Time(Clock { hour: 18, ..Clock::MIDNIGHT }).serial(s1904), Ok(0.75), "a time of day is the same in both");
    assert_eq!(Stamp::Duration(1.5).serial(s1904), Ok(1.5));
    assert_eq!(Stamp::Duration(-1.5).serial(s1900), Ok(-1.5));
    // Around the day that never was.
    assert_eq!(day(1900, 1, 1).serial(s1900), Ok(1.0));
    assert_eq!(day(1900, 2, 28).serial(s1900), Ok(59.0));
    assert_eq!(day(1900, 3, 1).serial(s1900), Ok(61.0));
    assert_eq!(day(1904, 1, 1).serial(s1904), Ok(0.0));
    // The last day, in both systems.
    assert_eq!(day(9999, 12, 31).serial(s1900), Ok(2_958_465.0));
    assert_eq!(day(9999, 12, 31).serial(s1904), Ok(2_957_003.0));
    let last = Stamp::DateTime(Day { year: 9999, month: 12, day: 31 }, Clock { hour: 23, minute: 59, second: 59, micro: 999_999 });
    assert!(last.serial(s1900).unwrap() < 2_958_466.0);
    // What a serial can't hold. A year below 1900 isn't read as DATE() reads it (1800 as 3700).
    let num = Err(CellError::Num);
    for (stamp, s) in [
        (day(1899, 12, 31), s1900),
        (day(1800, 1, 1), s1900),
        (day(1, 1, 1), s1900),
        (day(0, 1, 1), s1900),
        (day(-500, 1, 1), s1900),
        (day(1903, 12, 31), s1904),
        (day(1800, 1, 1), s1904),
        (day(10000, 1, 1), s1900),
        (day(10000, 1, 1), s1904),
        (day(i32::MAX, 1, 1), s1900),
        (day(i32::MIN, 1, 1), s1904),
        (Stamp::DateTime(Day { year: 1800, month: 1, day: 1 }, CLOCK), s1900),
        (Stamp::Duration(1e300), s1900),
        (Stamp::Duration(-1e300), s1904),
        (Stamp::Duration(f64::INFINITY), s1900),
        (Stamp::Duration(f64::NAN), s1900),
        (Stamp::Duration(2_958_466.0), s1900),
    ] {
        assert_eq!(stamp.serial(s), num, "{stamp:?} in {s:?}");
    }
    assert_eq!(Stamp::Duration(2_958_465.5).serial(s1900), Ok(2_958_465.5));
}

/// Every day of the calendars' awkward parts comes back as the serial it came from.
#[test]
fn a_serial_read_and_written_back_is_the_same_serial() {
    let shapes = [(true, false), (true, true)];
    let s1900 = gridcraft_core::DateSystem::D1900;
    let s1904 = gridcraft_core::DateSystem::D1904;
    let days = (1..=1500).chain([36526, 45306, 2_958_465]);
    for serial in days {
        for shape in shapes {
            let n = f64::from(serial);
            match Stamp::from_serial(s1900, shape, n) {
                Some(stamp) => assert_eq!(stamp.serial(s1900), Ok(n), "{stamp:?}"),
                None => assert_eq!(serial, 60, "only the day that never was"),
            }
            if serial <= 2_957_003 {
                let stamp = Stamp::from_serial(s1904, shape, n).unwrap();
                assert_eq!(stamp.serial(s1904), Ok(n), "{stamp:?}");
            }
        }
    }
    // A time of day survives to the millisecond.
    let stamp = Stamp::from_serial(s1900, (false, true), 0.123_456).unwrap();
    assert!((stamp.serial(s1900).unwrap() - 0.123_456).abs() < 1e-8, "{stamp:?}");
}

#[test]
fn a_stamp_says_what_it_is_for_the_card() {
    assert_eq!((Stamp::Date(DAY).type_name(), Stamp::Date(DAY).preview().as_str()), ("date", "2024-01-15"));
    let both = Stamp::DateTime(DAY, CLOCK);
    assert_eq!((both.type_name(), both.preview().as_str()), ("datetime", "2024-01-15 10:30:05.250000"));
    assert_eq!((Stamp::Time(CLOCK).type_name(), Stamp::Time(CLOCK).preview().as_str()), ("time", "10:30:05.250000"));
    assert_eq!((Stamp::Duration(1.5).type_name(), Stamp::Duration(1.5).preview().as_str()), ("timedelta", "1.5 days"));
}

#[test]
fn initialization_code_says_whose_it_is() {
    for (init, wire) in [(Init::Person, "person"), (Init::File, "file")] {
        let run = Request::Run { id: 3, code: "import x".into(), inputs: BTreeMap::new(), object: false, init: Some(init), at: None };
        let line = run.to_line();
        assert!(String::from_utf8_lossy(&line).contains(&format!("\"init\":\"{wire}\"")), "{line:?}");
        assert_eq!(Request::parse(line.trim_ascii_end()).unwrap(), run);
    }
    let cell = Request::Run { id: 3, code: "1".into(), inputs: BTreeMap::new(), object: false, init: None, at: None };
    assert!(!String::from_utf8_lossy(&cell.to_line()).contains("init"), "a cell's request doesn't mention it");
    // What this build doesn't know is no initialization.
    let odd = Request::parse(br#"{"type":"run","id":1,"code":"","init":"later"}"#).unwrap();
    assert!(matches!(odd, Request::Run { init: None, .. }));
}

fn object_with(fields: &str) -> String {
    done_line(&format!(r#"{{"object":{{"typeName":"DataFrame","preview":"p"{fields}}}}}"#))
}

fn table_of(line: &str) -> Result<Table, String> {
    match value_of(line)? {
        Output::Object { table: Some(table), .. } => Ok(table),
        other => Err(format!("no table: {other:?}")),
    }
}

/// A table's wire form with these fields, around `rows`, `cols` and `data`.
fn table_line(shape: &str, more: &str) -> String {
    object_with(&format!(r#","table":{{{shape}{more}}}"#))
}

#[test]
fn an_objects_size_is_capped() {
    let size = |json: &str| match value_of(&object_with(&format!(r#","size":{json}"#)))? {
        Output::Object { size, .. } => Ok::<_, String>(size),
        other => Err(format!("{other:?}")),
    };
    assert_eq!(size("[6,4]").unwrap(), vec![6, 4]);
    assert_eq!(size("[0]").unwrap(), vec![0]);
    assert_eq!(size(&format!("[{MAX_SIZE}]")).unwrap(), vec![MAX_SIZE]);
    assert_eq!(size(&format!("[{}]", ["1"; MAX_SIZE_DIMS].join(","))).unwrap().len(), MAX_SIZE_DIMS);
    // Past a cap, or not a list of whole numbers: the line is refused.
    for bad in [
        "[]".to_string(),
        format!("[{}]", MAX_SIZE + 1),
        format!("[{}]", ["1"; MAX_SIZE_DIMS + 1].join(",")),
        "[-1]".into(),
        "[1.5]".into(),
        "[\"6\"]".into(),
        "[[1]]".into(),
        "6".into(),
        "\"6x4\"".into(),
        "{}".into(),
        "[18446744073709551616]".into(),
    ] {
        assert!(size(&bad).is_err(), "{bad}");
    }
    // Without one, as before.
    assert_eq!(value_of(&object_with("")).unwrap(), Output::object("DataFrame", "p"));
    assert_eq!(value_of(&object_with(r#","size":null,"table":null"#)).unwrap(), Output::object("DataFrame", "p"));
}

#[test]
fn objects_with_a_size_and_a_table_round_trip() {
    let table = Table {
        columns: vec!["a".into(), "é|\"".into()],
        rows: 2,
        cols: 2,
        data: vec![Value::Number(1.0), Value::text("x\ny"), Value::Empty, Value::Error(CellError::NA)],
        row_labels: true,
        total_rows: 1_000,
        total_cols: 50,
        text_cut: true,
    };
    let empty = Table { rows: 0, cols: 2, columns: vec!["a".into(), "b".into()], total_cols: 2, ..Table::default() };
    for table in [table, empty, Table::default()] {
        let reply = Reply::Done {
            id: 1,
            result: Ok(Output::Object {
                type_name: "DataFrame".into(),
                preview: "p".into(),
                size: vec![1000, 50],
                table: Some(table),
                picture: None,
            }),
            stdout: String::new(),
            peak: None,
            touched: None,
        };
        assert_eq!(Reply::parse(reply.to_line().trim_ascii_end()).unwrap(), reply);
    }
}

#[test]
fn a_tables_shape_is_checked() {
    let good = table_of(&table_line(r#""rows":1,"cols":2,"data":[1,"a"],"totalRows":1,"totalCols":2"#, "")).unwrap();
    assert_eq!((good.rows, good.cols, good.data.len(), good.columns.len(), good.row_labels, good.text_cut), (1, 2, 2, 0, false, false));
    // No rows, and no columns at all: a table can be empty.
    assert_eq!(table_of(&table_line(r#""rows":0,"cols":2,"data":[],"totalRows":0,"totalCols":2,"columns":["a","b"]"#, "")).unwrap().rows, 0);
    assert_eq!(table_of(&table_line(r#""rows":3,"cols":0,"data":[],"totalRows":3,"totalCols":0"#, "")).unwrap().cols, 0);
    // A date in a table is its text, as the preview is only read.
    let dated = table_of(&table_line(
        r#""rows":1,"cols":2,"data":[{"date":"2024-01-15"},{"dateTime":"2024-01-15T10:30:00"}],"totalRows":1,"totalCols":2"#,
        "",
    ));
    assert_eq!(dated.unwrap().data, vec![Value::text("2024-01-15"), Value::text("2024-01-15 10:30:00")]);
    // Row labels don't count as columns of the object, however few it has.
    assert!(table_of(&table_line(r#""rows":1,"cols":2,"data":[1,2],"rowLabels":true,"totalRows":1,"totalCols":1"#, "")).is_ok());

    for bad in [
        r#""rows":2,"cols":2,"data":[1,2,3],"totalRows":2,"totalCols":2"#,
        r#""rows":1,"cols":2,"data":[1,2,3],"totalRows":1,"totalCols":2"#,
        r#""rows":4294967296,"cols":4294967296,"data":[],"totalRows":4294967296,"totalCols":4294967296"#,
        r#""rows":-1,"cols":2,"data":[],"totalRows":1,"totalCols":2"#,
        r#""rows":1,"cols":1,"totalRows":1,"totalCols":1"#,
        r#""cols":1,"data":[1],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"data":[1],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"cols":1,"data":[1],"totalCols":1"#,
        r#""rows":1,"cols":1,"data":[1],"totalRows":1"#,
        r#""rows":1,"cols":1,"data":[1],"totalRows":1,"totalCols":1,"rows":1"#,
        r#""rows":1,"cols":1,"data":[1],"totalRows":1,"totalCols":1,"extra":1"#,
        // Claiming less than it holds, or more than a size may say.
        r#""rows":2,"cols":1,"data":[1,2],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"cols":2,"data":[1,2],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"cols":1,"data":[1],"totalRows":9007199254740993,"totalCols":1"#,
        r#""rows":1,"cols":1,"data":[1],"totalRows":1,"totalCols":9007199254740993"#,
        // Names that don't match the columns, and labels with no column for them.
        r#""rows":1,"cols":2,"data":[1,2],"totalRows":1,"totalCols":2,"columns":["a"]"#,
        r#""rows":1,"cols":1,"data":[1],"totalRows":1,"totalCols":1,"columns":["a","b"]"#,
        r#""rows":0,"cols":0,"data":[],"totalRows":0,"totalCols":0,"rowLabels":true"#,
        // Nesting.
        r#""rows":1,"cols":1,"data":[{"array":{"rows":1,"cols":1,"data":[1]}}],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"cols":1,"data":[{"object":{"typeName":"x","preview":"x"}}],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"cols":1,"data":[[1]],"totalRows":1,"totalCols":1"#,
        r#""rows":1,"cols":1,"data":{"0":1},"totalRows":1,"totalCols":1"#,
    ] {
        assert!(table_of(&table_line(bad, "")).is_err(), "{bad}");
    }
}

#[test]
fn a_tables_caps_hold() {
    let numbers = |n: usize| vec!["0"; n].join(",");
    let shape =
        |rows: usize, cols: usize, data: &str| format!(r#""rows":{rows},"cols":{cols},"data":[{data}],"totalRows":{rows},"totalCols":{cols}"#);
    let at_cap = table_of(&table_line(&shape(MAX_TABLE_ROWS, MAX_TABLE_COLS, &numbers(MAX_TABLE_ROWS * MAX_TABLE_COLS)), "")).unwrap();
    assert_eq!(at_cap.data.len(), MAX_TABLE_ROWS * MAX_TABLE_COLS);
    assert!(table_of(&table_line(&shape(MAX_TABLE_ROWS + 1, 1, &numbers(MAX_TABLE_ROWS + 1)), "")).is_err());
    assert!(table_of(&table_line(&shape(1, MAX_TABLE_COLS + 1, &numbers(MAX_TABLE_COLS + 1)), "")).is_err());
    // Data that runs past the cap is stopped while it is read, whatever the shape says.
    assert!(table_of(&table_line(&shape(1, 1, &numbers(MAX_TABLE_ROWS * MAX_TABLE_COLS + 1)), "")).is_err());

    // One text, a cell's or a column's name.
    let text = |n: usize| format!("\"{}\"", "x".repeat(n));
    assert!(table_of(&table_line(&shape(1, 1, &text(MAX_TABLE_TEXT_BYTES)), "")).is_ok());
    assert!(table_of(&table_line(&shape(1, 1, &text(MAX_TABLE_TEXT_BYTES + 1)), "")).is_err());
    let named = |n: usize| format!(r#","columns":[{}]"#, text(n));
    assert!(table_of(&table_line(&shape(1, 1, "1"), &named(MAX_TABLE_TEXT_BYTES))).is_ok());
    assert!(table_of(&table_line(&shape(1, 1, "1"), &named(MAX_TABLE_TEXT_BYTES + 1))).is_err());
    let names = |n: usize| format!(r#","columns":[{}]"#, vec!["\"a\""; n].join(","));
    assert!(table_of(&table_line(&shape(1, 2, "1,2"), &names(MAX_TABLE_COLS + 1))).is_err());
    assert!(table_of(&table_line(&shape(1, 2, "1,2"), r#","columns":[1,2]"#)).is_err());

    // All its text together, the names included: whole rows of cells at the cap per cell.
    let cells = |n: usize| vec![text(MAX_TABLE_TEXT_BYTES); n].join(",");
    let rows_of = |bytes: usize| bytes / MAX_TABLE_TEXT_BYTES / MAX_TABLE_COLS;
    let fit = rows_of(MAX_TABLE_BYTES);
    assert!(table_of(&table_line(&shape(fit, MAX_TABLE_COLS, &cells(fit * MAX_TABLE_COLS)), "")).is_ok());
    assert!(table_of(&table_line(&shape(fit + 1, MAX_TABLE_COLS, &cells((fit + 1) * MAX_TABLE_COLS)), "")).is_err());
    let filled = format!(r#","columns":[{}]"#, vec![text(MAX_TABLE_TEXT_BYTES); MAX_TABLE_COLS].join(","));
    assert!(table_of(&table_line(&shape(fit - 1, MAX_TABLE_COLS, &cells((fit - 1) * MAX_TABLE_COLS)), &filled)).is_ok());
    assert!(table_of(&table_line(&shape(fit, MAX_TABLE_COLS, &cells(fit * MAX_TABLE_COLS)), &filled)).is_err());

    // A table only goes on an object, and an object only as a result.
    assert!(value_of(&done_line(r#"{"table":{"rows":0,"cols":0,"data":[],"totalRows":0,"totalCols":0}}"#)).is_err());
}

/// The first bytes of a PNG that says it is `width` x `height`. Nothing reads further than this.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(width.to_be_bytes());
    b.extend(height.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b
}

fn picture_line(mime: &str, base64: &str, density: Option<u32>, size: [u64; 2]) -> Vec<u8> {
    let density = density.map_or(String::new(), |d| format!(",\"density\":{d}"));
    format!(
        "{{\"type\":\"done\",\"id\":2,\"ok\":true,\"value\":{{\"object\":{{\"typeName\":\"Image\",\"preview\":\"Image\",\"size\":[{},{}],\"image\":{{\"mime\":\"{mime}\",\"base64\":\"{base64}\"{density}}}}}}}}}",
        size[0], size[1]
    )
    .into_bytes()
}

#[test]
fn a_picture_goes_there_and_back() {
    for density in [1, 2] {
        for bytes in [png(40, 30), png(1, 1), png(8192, 1953)] {
            let picture = Picture::from_bytes(bytes.clone(), density).unwrap();
            assert_eq!(picture.bytes, bytes);
            let reply =
                Reply::Done { id: 2, result: Ok(Output::picture(picture.clone())), stdout: String::new(), peak: None, touched: Some([1, 2, 3]) };
            assert_eq!(Reply::parse(reply.to_line().trim_ascii_end()), Ok(reply));
        }
    }
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xFF, 0xC2, 0, 8, 8, 0, 30, 0, 40, 3];
    jpeg.extend([0; 8]);
    let picture = Picture::from_bytes(jpeg, 1).unwrap();
    assert_eq!((picture.mime, picture.width, picture.height), (PictureType::Jpeg, 40, 30));
}

#[test]
fn base64_reads_what_it_writes_and_nothing_else() {
    for n in 0..64usize {
        let bytes: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
        assert_eq!(from_base64(&to_base64(&bytes)).as_ref(), Some(&bytes));
    }
    for bad in ["A", "AB", "ABC", "A===", "====", "AB=C", "A=BC", "AB\nC", "\u{e9}ABC", "ABC="[..3].repeat(5).as_str()] {
        assert_eq!(from_base64(bad), None, "{bad:?}");
    }
    assert_eq!(from_base64("QQ=="), Some(b"A".to_vec()));
}

#[test]
fn a_forged_picture_is_refused() {
    let good = to_base64(&png(40, 30));
    assert!(Reply::parse(&picture_line("image/png", &good, None, [40, 30])).is_ok());
    assert!(Reply::parse(&picture_line("image/png", &good, Some(2), [20, 15])).is_ok());
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("not base64", picture_line("image/png", "@@@@", None, [40, 30])),
        ("cut base64", picture_line("image/png", "QQ", None, [40, 30])),
        ("not a picture", picture_line("image/png", &to_base64(b"GIF89a and so on"), None, [40, 30])),
        ("empty", picture_line("image/png", "", None, [40, 30])),
        ("a header cut short", picture_line("image/png", &to_base64(&png(40, 30)[..20]), None, [40, 30])),
        ("the wrong type", picture_line("image/jpeg", &good, None, [40, 30])),
        ("the wrong size", picture_line("image/png", &good, None, [41, 30])),
        ("a size for another density", picture_line("image/png", &good, Some(2), [40, 30])),
        ("density 0", picture_line("image/png", &good, Some(0), [40, 30])),
        ("density 3", picture_line("image/png", &good, Some(3), [13, 10])),
        ("no width", picture_line("image/png", &to_base64(&png(0, 30)), None, [0, 30])),
        ("no height", picture_line("image/png", &to_base64(&png(30, 0)), None, [30, 0])),
        ("too wide", picture_line("image/png", &to_base64(&png(MAX_PICTURE_SIDE + 1, 1)), None, [u64::from(MAX_PICTURE_SIDE) + 1, 1])),
        ("too tall", picture_line("image/png", &to_base64(&png(1, MAX_PICTURE_SIDE + 1)), None, [1, u64::from(MAX_PICTURE_SIDE) + 1])),
        ("too many pixels", picture_line("image/png", &to_base64(&png(8000, 8000)), None, [8000, 8000])),
        ("a huge header", picture_line("image/png", &to_base64(&png(u32::MAX, u32::MAX)), None, [u64::from(u32::MAX), u64::from(u32::MAX)])),
    ];
    for (what, line) in cases {
        assert!(Reply::parse(&line).is_err(), "{what}");
    }
    // Over the cap in bytes, in both directions a worker could try.
    let mut big = png(40, 30);
    big.resize(MAX_PICTURE_BYTES + 1, 0);
    assert!(Picture::from_bytes(big.clone(), 1).is_err());
    assert!(Reply::parse(&picture_line("image/png", &to_base64(&big), None, [40, 30])).is_err());
    // A JPEG whose lengths lead nowhere ends, without reading past its end.
    for bytes in [
        vec![0xFF, 0xD8],
        vec![0xFF, 0xD8, 0xFF],
        vec![0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF],
        vec![0xFF, 0xD8, 0, 0],
        vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 8],
    ] {
        assert!(Picture::from_bytes(bytes.clone(), 1).is_err(), "{bytes:?}");
    }
}

#[test]
fn a_cell_value_cant_be_a_picture_and_ready_carries_no_touched() {
    let good = to_base64(&png(1, 1));
    let input = format!(
        "{{\"type\":\"run\",\"id\":1,\"code\":\"\",\"inputs\":{{\"A1\":{{\"value\":{{\"object\":{{\"typeName\":\"Image\",\"preview\":\"Image\",\"image\":{{\"mime\":\"image/png\",\"base64\":\"{good}\"}}}}}}}}}}}}"
    );
    assert!(Request::parse(input.as_bytes()).is_err());
    assert!(Reply::parse(b"{\"type\":\"ready\",\"selfTest\":{\"status\":\"notChecked\"},\"touched\":[0,0,0]}").is_err());
    let unknown = format!(
        "{{\"type\":\"done\",\"id\":2,\"ok\":true,\"value\":{{\"object\":{{\"typeName\":\"Image\",\"preview\":\"Image\",\"size\":[1,1],\"image\":{{\"mime\":\"image/png\",\"base64\":\"{good}\",\"other\":1}}}}}}}}"
    );
    assert!(Reply::parse(unknown.as_bytes()).is_err());
}

#[test]
fn a_run_says_where_its_cell_is() {
    let run = Request::Run { id: 1, code: String::new(), inputs: BTreeMap::new(), object: false, init: None, at: Some([2, 3, 4]) };
    let line = run.to_line();
    assert!(std::str::from_utf8(&line).unwrap().contains("\"at\":[2,3,4]"));
    assert_eq!(Request::parse(line.trim_ascii_end()), Ok(run));
}
