//! The recorder: a slice of the command journal, as a runnable Office Script.
//!
//! Only the documented subset is written ([`SUPPORTED`]): values, bold, number formats, adding
//! and deleting worksheets, and `console.log` (which a script can use directly). A command
//! outside it becomes a comment saying what was left out, never a guess. The same pass produces
//! the bridge calls the script will make ([`Recording::calls`]), which is how tests prove a
//! recording that runs through the fake host reproduces the recorded work.

use serde_json::Value as Json;

use crate::bridge::BridgeCall;

/// The command ids the recorder turns into ExcelScript calls.
pub const SUPPORTED: &[&str] = &["cell.set", "range.setValues", "home.bold", "home.numberFormat", "home.insertSheet", "home.deleteSheet"];

/// One command from the journal: its id and parameters, without secrets.
#[derive(Clone, Debug, PartialEq)]
pub struct JournalStep {
    pub id: String,
    pub params: Json,
}

impl JournalStep {
    pub fn new(id: impl Into<String>, params: Json) -> Self {
        JournalStep { id: id.into(), params }
    }
}

/// A journal entry the recorder could not turn into a call, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Skipped {
    pub id: String,
    pub reason: String,
}

/// The result of recording: the TypeScript source, the calls it makes, and what was left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub code: String,
    pub calls: Vec<BridgeCall>,
    pub skipped: Vec<Skipped>,
}

/// Turns journal steps into a script. Entries the recorder itself made (`script.*`) are ignored
/// without a comment: they are recording controls, not workbook work.
pub fn record(steps: &[JournalStep]) -> Recording {
    let mut body = Vec::new();
    let mut calls = Vec::new();
    let mut skipped = Vec::new();
    for step in steps {
        if step.id.starts_with("script.") {
            continue;
        }
        match step_call(step) {
            Some((call, line)) => {
                body.push(format!("  {line}"));
                calls.push(call);
            }
            None => {
                let reason = skip_reason(step);
                body.push(format!("  // Skipped: {} ({reason}).", step.id));
                skipped.push(Skipped { id: step.id.clone(), reason });
            }
        }
    }
    let mut code = String::from("// Recorded in GridCraft.\nfunction main(workbook: ExcelScript.Workbook) {\n");
    for line in body {
        code.push_str(&line);
        code.push('\n');
    }
    code.push_str("}\n");
    Recording { code, calls, skipped }
}

/// Why a step cannot be recorded; also used as the reason for an unsupported command.
fn skip_reason(step: &JournalStep) -> String {
    if !SUPPORTED.contains(&step.id.as_str()) {
        return "not supported by the Office Scripts bridge".into();
    }
    match step.id.as_str() {
        "cell.set" if step.params.get("array").and_then(Json::as_bool) == Some(true) => {
            "it is an array entry, which a recording cannot replay".into()
        }
        "cell.set" if step.params.get("cell").and_then(Json::as_str).is_none() => "it used the active cell, which the recording does not know".into(),
        "range.setValues" if step.params.get("uncaptured").and_then(Json::as_bool) == Some(true) => {
            "its stored values are above the 10,000-cell capture bound".into()
        }
        "range.setValues" if step.params.get("range").and_then(Json::as_str).is_none() => "it used the selection without a range".into(),
        "range.setValues" => "its values are not rows of numbers, text or true/false".into(),
        "home.bold" if step.params.get("range").and_then(Json::as_str).is_none() => "it used the selection without a range".into(),
        "home.bold" => "it toggled the selection, which a recording cannot replay".into(),
        "home.numberFormat" if step.params.get("range").and_then(Json::as_str).is_none() => "it used the selection without a range".into(),
        "home.numberFormat" => "its format is neither a code nor a known format name".into(),
        "home.insertSheet" if step.params.get("before").is_some() || step.params.get("position").is_some() => {
            "it inserted the sheet at a position, which the bridge cannot replay".into()
        }
        "home.deleteSheet" => "it did not name the sheet".into(),
        _ if step.params.get("sheet").is_some_and(Json::is_number) => "its sheet index could not be resolved".into(),
        _ => "it could not be recorded".into(),
    }
}

fn step_call(step: &JournalStep) -> Option<(BridgeCall, String)> {
    let p = &step.params;
    // A journal entry that still holds a sheet index cannot be replayed: the script would target
    // the active sheet instead of the one the command edited.
    if p.get("sheet").is_some_and(Json::is_number) {
        return None;
    }
    // A range write whose stored values were not captured has no faithful replay: the inputs may
    // have read as other values, so the step is skipped with its reason instead.
    if step.id == "range.setValues" && p.get("uncaptured").and_then(Json::as_bool) == Some(true) {
        return None;
    }
    // An array entry writes a whole range and clears its neighbours: a single `setValue`
    // cannot replay it, so the step is skipped with its reason instead.
    if step.id == "cell.set" && p.get("array").and_then(Json::as_bool) == Some(true) {
        return None;
    }
    match step.id.as_str() {
        "cell.set" => {
            let cell = p.get("cell").and_then(Json::as_str)?;
            let value = p.get("input")?;
            // The literal is what the engine resolved the cell to store: `record()` already
            // substituted the stored value for the typed text, like a range write's values. The
            // replay stores it verbatim (`parse: false`), because the shim drops `parse`.
            let literal = js_literal(value)?;
            let mut call = range_call("Range.setValue", step, cell, "value", value.clone());
            if let Json::Object(map) = &mut call.params {
                map.insert("parse".into(), Json::Bool(false));
            }
            Some((call, format!("{}.setValue({literal});", target(step, cell))))
        }
        "range.setValues" => {
            let range = p.get("range").and_then(Json::as_str)?;
            let values = p.get("values")?.as_array()?;
            let literal = js_rows(values)?;
            let mut call = range_call("Range.setValues", step, range, "values", p.get("values")?.clone());
            // `range.setValues` parses typed text by default (`parse: false` opts out); the replay
            // must do the same, or a recorded "2.500" comes back as text.
            if let Json::Object(map) = &mut call.params {
                map.insert("parse".into(), Json::Bool(p.get("parse").and_then(Json::as_bool).unwrap_or(true)));
            }
            Some((call, format!("{}.setValues({literal});", target(step, range))))
        }
        "home.bold" => {
            let range = p.get("range").and_then(Json::as_str)?;
            let on = p.get("on")?.as_bool()?;
            let call = range_call("RangeFont.setBold", step, range, "bold", Json::Bool(on));
            Some((call, format!("{}.getFormat().getFont().setBold({on});", target(step, range))))
        }
        "home.numberFormat" => {
            let range = p.get("range").and_then(Json::as_str)?;
            let code = number_format_code(p)?;
            let call = range_call("Range.setNumberFormat", step, range, "format", Json::String(code.clone()));
            Some((call, format!("{}.setNumberFormat({});", target(step, range), js_string(&code))))
        }
        "home.insertSheet" if p.get("before").is_some() || p.get("position").is_some() => None,
        "home.insertSheet" => {
            let name = p.get("name").and_then(Json::as_str);
            let params = match name {
                Some(n) => serde_json::json!({ "name": n }),
                None => serde_json::json!({}),
            };
            let call = BridgeCall::new("Workbook.addWorksheet", params);
            let line = match name {
                Some(n) => format!("workbook.addWorksheet({});", js_string(n)),
                None => "workbook.addWorksheet();".to_string(),
            };
            Some((call, line))
        }
        "home.deleteSheet" => {
            let name = p.get("sheet").and_then(Json::as_str)?;
            let call = BridgeCall::new("Worksheet.delete", serde_json::json!({ "sheet": name }));
            Some((call, format!("workbook.getWorksheet({}).delete();", js_string(name))))
        }
        _ => None,
    }
}

/// A range call: the method, the address, the sheet when the step names one, and the operation's
/// own parameter.
fn range_call(method: &str, step: &JournalStep, range: &str, key: &str, value: Json) -> BridgeCall {
    let (sheet, address) = split_target(step, range);
    let mut params = serde_json::Map::new();
    params.insert("range".into(), Json::String(address));
    params.insert(key.into(), value);
    if let Some(sheet) = sheet {
        params.insert("sheet".into(), Json::String(sheet));
    }
    BridgeCall::new(method, Json::Object(params))
}

/// The sheet and the address a range names: an explicit `sheet` parameter wins, then a `Data!A1`
/// prefix on the range itself. The address never keeps the prefix, because the bridge's `getRange`
/// takes an address alone.
fn split_target(step: &JournalStep, range: &str) -> (Option<String>, String) {
    let explicit = step.params.get("sheet").and_then(Json::as_str).map(str::to_string);
    let (prefix, address) = split_range(range);
    match explicit {
        Some(sheet) => (Some(sheet), address),
        None => (prefix, address),
    }
}

/// `Sheet!A1` → its sheet and address. A quoted name keeps its spaces and a `''` inside it is one
/// quote (`'O''Brien'!A1`), as Excel writes names; an unquoted name ends at the last `!`.
fn split_range(range: &str) -> (Option<String>, String) {
    if let Some(rest) = range.strip_prefix('\'') {
        let mut name = String::new();
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            if c == '\'' {
                if rest.get(i + 1..).is_some_and(|tail| tail.starts_with('\'')) {
                    name.push('\'');
                    chars.next();
                    continue;
                }
                return match rest.get(i + 1..).and_then(|tail| tail.strip_prefix('!')) {
                    Some(address) => (Some(name), address.to_string()),
                    None => (None, range.to_string()),
                };
            }
            name.push(c);
        }
        return (None, range.to_string());
    }
    match range.rsplit_once('!') {
        Some((sheet, address)) => (Some(sheet.to_string()), address.to_string()),
        None => (None, range.to_string()),
    }
}

/// The JavaScript expression naming the range, on the step's sheet when it names one.
fn target(step: &JournalStep, range: &str) -> String {
    match split_target(step, range) {
        (Some(sheet), address) => format!("workbook.getWorksheet({}).getRange({})", js_string(&sheet), js_string(&address)),
        (None, address) => format!("workbook.getActiveWorksheet().getRange({})", js_string(&address)),
    }
}

/// The number format as a code: `code` when given, or the en-US code for the named format the
/// Number Format command accepts.
fn number_format_code(p: &Json) -> Option<String> {
    if let Some(code) = p.get("code").and_then(Json::as_str) {
        return Some(code.to_string());
    }
    let name = p.get("format").and_then(Json::as_str)?;
    let code = match name.to_ascii_lowercase().as_str() {
        "general" => "General",
        "number" => "0.00",
        "currency" => "$#,##0.00",
        "accounting" => "$#,##0.00",
        "short date" => "m/d/yyyy",
        "long date" => "dddd, mmmm d, yyyy",
        "time" => "h:mm:ss AM/PM",
        "percentage" => "0%",
        "fraction" => "# ?/?",
        "scientific" => "0.00E+00",
        "text" => "@",
        _ => return None,
    };
    Some(code.to_string())
}

/// A JavaScript literal for a JSON value: numbers, strings, booleans, null and arrays of those.
/// `Err` means the value has no straightforward Office Scripts equivalent (an object).
pub fn js_literal(value: &Json) -> Option<String> {
    match value {
        Json::Null => Some("null".into()),
        Json::Bool(b) => Some(b.to_string()),
        Json::Number(n) => Some(n.to_string()),
        Json::String(s) => Some(js_string(s)),
        Json::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&js_literal(item)?);
            }
            out.push(']');
            Some(out)
        }
        Json::Object(_) => None,
    }
}

/// Rows of values, as `setValues` takes them: `[[1, 2], [3, 4]]`. A flat array is not rows — it
/// would record a `setValues([1, 2])` call ExcelScript refuses.
fn js_rows(rows: &[Json]) -> Option<String> {
    let mut out = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push('[');
        for (j, cell) in row.as_array()?.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str(&js_literal(cell)?);
        }
        out.push(']');
    }
    out.push(']');
    Some(out)
}

/// A double-quoted JavaScript string, with the escapes a script needs.
pub fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32 & 0xFFFF)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(id: &str, params: Json) -> JournalStep {
        JournalStep::new(id, params)
    }

    #[test]
    fn records_values_with_each_type() {
        let journal = vec![
            step("cell.set", json!({"cell": "A1", "input": 42})),
            step("cell.set", json!({"cell": "B1", "input": "Sales"})),
            step("cell.set", json!({"cell": "C1", "input": true})),
            step("range.setValues", json!({"range": "A2:B3", "values": [[1, "two"], [false, null]]})),
        ];
        let r = record(&journal);
        assert_eq!(
            r.code,
            "// Recorded in GridCraft.\nfunction main(workbook: ExcelScript.Workbook) {\n  \
             workbook.getActiveWorksheet().getRange(\"A1\").setValue(42);\n  \
             workbook.getActiveWorksheet().getRange(\"B1\").setValue(\"Sales\");\n  \
             workbook.getActiveWorksheet().getRange(\"C1\").setValue(true);\n  \
             workbook.getActiveWorksheet().getRange(\"A2:B3\").setValues([[1, \"two\"], [false, null]]);\n}\n"
        );
        assert_eq!(r.calls.len(), 4);
        assert_eq!(r.calls[3].method, "Range.setValues");
        assert_eq!(r.calls[0].params["value"], json!(42));
        assert_eq!(r.calls[0].params["parse"], json!(false), "the stored value replays verbatim");
        assert_eq!(r.calls[3].params["parse"], json!(true), "a range write parses as the command did");
        assert_eq!(r.calls[1].params["value"], json!("Sales"));
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn records_styles_sheets_and_named_formats() {
        let journal = vec![
            step("home.bold", json!({"range": "A1:B1", "on": true})),
            step("home.numberFormat", json!({"range": "C1:C9", "code": "0.00"})),
            step("home.numberFormat", json!({"range": "D1", "format": "Percentage"})),
            step("home.insertSheet", json!({"name": "Data"})),
            step("home.deleteSheet", json!({"sheet": "Old"})),
        ];
        let r = record(&journal);
        assert!(r.code.contains("getRange(\"A1:B1\").getFormat().getFont().setBold(true);"), "{}", r.code);
        assert!(r.code.contains("getRange(\"C1:C9\").setNumberFormat(\"0.00\");"), "{}", r.code);
        assert!(r.code.contains("getRange(\"D1\").setNumberFormat(\"0%\");"), "{}", r.code);
        assert!(r.code.contains("workbook.addWorksheet(\"Data\");"), "{}", r.code);
        assert!(r.code.contains("workbook.getWorksheet(\"Old\").delete();"), "{}", r.code);
        assert_eq!(r.calls[4].method, "Worksheet.delete");
    }

    #[test]
    fn a_named_sheet_is_scoped_in_the_script_and_the_call() {
        let journal = vec![step("cell.set", json!({"sheet": "Data", "cell": "A1", "input": "x"}))];
        let r = record(&journal);
        assert!(r.code.contains("workbook.getWorksheet(\"Data\").getRange(\"A1\").setValue(\"x\");"), "{}", r.code);
        assert_eq!(r.calls[0].params["sheet"], json!("Data"));
    }

    #[test]
    fn unsupported_steps_are_skipped_with_a_comment() {
        let journal = vec![
            step("cell.set", json!({"cell": "A1", "input": 1})),
            step("data.sortAscending", json!({"range": "A1:B2"})),
            step("home.bold", json!({"range": "A1"})),
        ];
        let r = record(&journal);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.skipped.len(), 2);
        assert!(r.code.contains("// Skipped: data.sortAscending (not supported by the Office Scripts bridge)."), "{}", r.code);
        assert!(r.code.contains("// Skipped: home.bold (it toggled the selection, which a recording cannot replay)."), "{}", r.code);
    }

    #[test]
    fn a_flat_set_values_step_is_skipped_as_not_rows() {
        // `range.setValues` takes rows: recording `[1, 2]` as `setValues([1, 2])` would produce a
        // script that fails at run time (and in Excel).
        let r = record(&[step("range.setValues", json!({"range": "A1:B1", "values": [1, 2]}))]);
        assert!(r.calls.is_empty(), "{:?}", r.calls);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.code.contains("its values are not rows of numbers, text or true/false"), "{}", r.code);
        // A row of rows still records.
        let rows = record(&[step("range.setValues", json!({"range": "A1:B1", "values": [[1, 2]]}))]);
        assert!(rows.code.contains("setValues([[1, 2]]);"), "{}", rows.code);
        assert!(rows.skipped.is_empty());
    }

    #[test]
    fn a_qualified_range_names_its_sheet_in_the_script_and_the_call() {
        let r = record(&[step("cell.set", json!({"cell": "Data!A1", "input": 1}))]);
        assert!(r.code.contains("workbook.getWorksheet(\"Data\").getRange(\"A1\").setValue(1);"), "{}", r.code);
        assert_eq!(r.calls[0].params["sheet"], json!("Data"));
        assert_eq!(r.calls[0].params["range"], json!("A1"));
        // A quoted name keeps its spaces; an explicit `sheet` still wins.
        let quoted = record(&[step("cell.set", json!({"cell": "'My Data'!B2", "input": 1}))]);
        assert_eq!(quoted.calls[0].params["sheet"], json!("My Data"));
        let explicit = record(&[step("cell.set", json!({"sheet": "Other", "cell": "Data!A1", "input": 1}))]);
        assert_eq!(explicit.calls[0].params["sheet"], json!("Other"));
        assert_eq!(explicit.calls[0].params["range"], json!("A1"));
        // Excel's quoting: `''` is one quote, and a quoted name may hold a `!`.
        let apostrophe = record(&[step("cell.set", json!({"cell": "'O''Brien'!A1", "input": 1}))]);
        assert_eq!(apostrophe.calls[0].params["sheet"], json!("O'Brien"));
        let bang = record(&[step("cell.set", json!({"cell": "'Q1!Totals'!B2", "input": 1}))]);
        assert_eq!(bang.calls[0].params["sheet"], json!("Q1!Totals"));
        assert_eq!(bang.calls[0].params["range"], json!("B2"));
    }

    #[test]
    fn a_step_with_a_sheet_index_is_skipped() {
        // A journal step that still holds a sheet index cannot be replayed: the script would
        // target the active sheet instead of the one the command edited.
        let r = record(&[step("cell.set", json!({"sheet": 1, "cell": "A1", "input": 1}))]);
        assert!(r.calls.is_empty(), "{:?}", r.calls);
        assert!(r.code.contains("its sheet index could not be resolved"), "{}", r.code);
    }

    #[test]
    fn an_array_entry_is_skipped() {
        // An array entry writes a whole range and clears its neighbours: a single `setValue`
        // with the anchor's value cannot replay it.
        let r = record(&[step("cell.set", json!({"cell": "A1", "input": 1, "array": true}))]);
        assert!(r.calls.is_empty(), "{:?}", r.calls);
        assert!(r.code.contains("it is an array entry, which a recording cannot replay"), "{}", r.code);
        // Without the flag the same write records as before.
        let plain = record(&[step("cell.set", json!({"cell": "A1", "input": 1}))]);
        assert!(plain.code.contains("setValue(1);"), "{}", plain.code);
    }

    #[test]
    fn a_positioned_sheet_insert_is_skipped_and_reported() {
        let r = record(&[step("home.insertSheet", json!({"name": "Data", "before": 0}))]);
        assert!(r.calls.is_empty(), "{:?}", r.calls);
        assert!(r.code.contains("it inserted the sheet at a position, which the bridge cannot replay"), "{}", r.code);
        // An insert without a position still records.
        let plain = record(&[step("home.insertSheet", json!({"name": "Data"}))]);
        assert!(plain.code.contains("workbook.addWorksheet(\"Data\");"), "{}", plain.code);
        assert!(plain.skipped.is_empty());
    }

    #[test]
    fn recording_controls_are_not_commented() {
        let r = record(&[step("script.record", json!({"on": false})), step("cell.set", json!({"cell": "A1", "input": 1}))]);
        assert!(!r.code.contains("script.record"));
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(js_string("a\"b\\c\nd\te"), "\"a\\\"b\\\\c\\nd\\te\"");
        assert_eq!(js_string("x\u{1}y"), "\"x\\u0001y\"");
    }

    #[test]
    fn objects_have_no_literal() {
        assert!(js_literal(&json!({"a": 1})).is_none());
        assert_eq!(js_literal(&json!([1, [2, 3]])).unwrap(), "[1, [2, 3]]");
    }

    #[test]
    fn cell_writes_keep_the_stored_value_verbatim() {
        // `record()` substitutes what the cell stored (a number stays a number, formula-like
        // text keeps its marker); the recorder emits that literal verbatim, because the shim
        // drops `parse`.
        let number = record(&[step("cell.set", json!({"cell": "A1", "input": 42}))]);
        assert!(number.code.contains("setValue(42);"), "{}", number.code);
        assert_eq!(number.calls[0].params["parse"], json!(false));
        // Formula-like text the cell kept as text carries its marker; a literal leading
        // apostrophe is itself text, so the engine doubles it when substituting the stored
        // value, and the recorder emits that verbatim.
        let text = record(&[step("cell.set", json!({"cell": "B1", "input": "''=1+1"}))]);
        assert!(text.code.contains("setValue(\"''=1+1\");"), "{}", text.code);
        assert_eq!(text.calls[0].params["parse"], json!(false));
    }

    #[test]
    fn an_empty_recording_is_still_a_script() {
        let r = record(&[]);
        assert_eq!(r.code, "// Recorded in GridCraft.\nfunction main(workbook: ExcelScript.Workbook) {\n}\n");
    }
}
