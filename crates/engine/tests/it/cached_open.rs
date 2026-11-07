//! Synthetic, in-memory XLSX lifecycle checks: no Office-authored fixtures or filesystem I/O.
use std::io::{Cursor, Read, Write};

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::{CalcMode, Cell, Formula, Workbook};
use serde_json::{Value as Json, json};

/// Build an in-memory workbook with cached, unsupported, stale and missing results.
fn fixture() -> Json {
    let mut wb = Workbook::new();
    let sheet = wb.sheet_mut(0).unwrap();
    sheet.set_value(CellRef::parse("A1").unwrap(), Value::Number(10.0));
    for (at, text, value) in [
        ("B1", "GETPIVOTDATA(\"Suma\",A1)", Value::Number(1250.25)),
        ("C1", "B1*2", Value::Number(2500.5)),
        ("D1", "A1*3", Value::Empty),
        ("E1", "C1+D1", Value::Empty),
        ("F1", "A1+1", Value::Number(999.0)),
    ] {
        let mut cell = Cell::formula(Formula::new(text));
        cell.value = value;
        sheet.set_cell(CellRef::parse(at).unwrap(), cell);
    }
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    json!({"name": "synthetic-caches.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})
}

/// Read a result through the public workbook model.
fn value(s: &Session, at: &str) -> Value {
    s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::parse(at).unwrap())
}

/// Import preserves saved results, fills missing chains and survives save/reopen.
#[test]
fn cached_open_preserves_saved_results_and_fills_missing_chains() {
    let mut s = Session::new();
    let opened = s.execute("file.open", fixture()).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(1250.25));
    assert_eq!(value(&s, "C1"), Value::Number(2500.5));
    assert_eq!(value(&s, "D1"), Value::Number(30.0));
    assert_eq!(value(&s, "E1"), Value::Number(2530.5));
    assert_eq!(value(&s, "F1"), Value::Number(999.0));
    assert!(!s.doc().unwrap().is_dirty());
    assert!(opened["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("may be out of date")));
    s.execute("cell.set", json!({"cell": "A10", "input": "1"})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(1250.25));
    assert_eq!(value(&s, "C1"), Value::Number(2500.5));
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "roundtrip.xlsx", "base64": saved["base64"]})).unwrap();
    for at in ["B1", "C1", "D1", "E1", "F1"] {
        assert_eq!(value(&reopened, at), value(&s, at));
    }
}

/// A real input edit invalidates its dependent formulas instead of silently freezing them.
#[test]
fn cached_open_recalculates_edited_inputs_and_their_dependents() {
    let mut s = Session::new();
    s.execute("file.open", fixture()).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "20"})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Error(CellError::Name));
    assert_eq!(value(&s, "C1"), Value::Error(CellError::Name));
    assert_eq!(value(&s, "D1"), Value::Number(60.0));
    assert_eq!(value(&s, "F1"), Value::Number(21.0));
}

/// Callers can request the previous full-recalculation policy explicitly.
#[test]
fn cached_open_can_explicitly_recalculate_all_on_open() {
    let mut s = Session::new();
    let mut params = fixture();
    params["recalculate"] = json!(true);
    let opened = s.execute("file.open", params).unwrap();
    assert_eq!(value(&s, "B1"), Value::Error(CellError::Name));
    assert_eq!(value(&s, "C1"), Value::Error(CellError::Name));
    assert_eq!(value(&s, "F1"), Value::Number(11.0));
    assert!(!opened["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("may be out of date")));
}

/// Workbooks without saved formula results do not show the cache notice.
#[test]
fn cached_open_without_saved_formula_results_has_no_cache_warning() {
    let mut s = Session::new();
    let wb = Workbook::new();
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let opened = s.execute("file.open", json!({"name": "empty.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    assert!(opened["warnings"].as_array().unwrap().is_empty());
}

/// Dynamic references and workbook-defined functions remain live after cache-preserving open.
#[test]
fn cached_open_keeps_supported_dynamic_references_live() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "10"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=INDIRECT(\"A1\")"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "READVALUE", "refersTo": "=LAMBDA(x,INDIRECT(x))"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=READVALUE(\"A1\")"})).unwrap();
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut opened = Session::new();
    opened.execute("file.open", json!({"name": "dynamic.xlsx", "base64": saved["base64"]})).unwrap();
    assert_eq!(value(&opened, "B1"), Value::Number(10.0));
    assert_eq!(value(&opened, "C1"), Value::Number(10.0));
    opened.execute("cell.set", json!({"cell": "A1", "input": "20"})).unwrap();
    assert_eq!(value(&opened, "B1"), Value::Number(20.0));
    assert_eq!(value(&opened, "C1"), Value::Number(20.0));
}

/// The Calculate Now command remains an explicit full recalculation.
#[test]
fn cached_open_honors_explicit_calculate_now() {
    let mut s = Session::new();
    s.execute("file.open", fixture()).unwrap();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Error(CellError::Name));
    assert_eq!(value(&s, "F1"), Value::Number(11.0));
}

/// Saved values from registered but unimplemented functions survive an unrelated edit and save.
#[test]
fn cached_open_preserves_stub_results_through_unrelated_edits() {
    let mut wb = Workbook::new();
    for (at, formula, value) in
        [("A1", "FILTERXML(\"<x/>\",\"//x\")", Value::from("SYNTHETIC")), ("B1", "CUBEVALUE(\"synthetic-connection\")", Value::Number(75.5))]
    {
        let mut cell = Cell::formula(Formula::new(formula));
        cell.value = value;
        wb.sheet_mut(0).unwrap().set_cell(CellRef::parse(at).unwrap(), cell);
    }
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name": "synthetic-stubs.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    s.execute("cell.set", json!({"cell": "Z20", "input": "1"})).unwrap();
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "saved-stubs.xlsx", "base64": saved["base64"]})).unwrap();
    assert_eq!(value(&reopened, "A1"), Value::from("SYNTHETIC"));
    assert_eq!(value(&reopened, "B1"), Value::Number(75.5));
}

/// Manual calculation keeps saved dependents until their own formula is edited.
#[test]
fn cached_open_respects_manual_calculation_after_open() {
    let params = fixture();
    let bytes = gridcraft_engine::io::base64_decode(params["base64"].as_str().unwrap()).unwrap();
    let (mut wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    wb.calc.mode = CalcMode::Manual;
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name": "manual.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "20"})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(1250.25));
    assert_eq!(value(&s, "F1"), Value::Number(999.0));
    s.execute("cell.set", json!({"cell": "F1", "input": "=A1+1"})).unwrap();
    assert_eq!(value(&s, "F1"), Value::Number(21.0));
}

/// The stale-result notice stays visible when an XLSX also produces many import warnings.
#[test]
fn cached_open_shows_cache_notice_before_import_warning_overflow() {
    let params = fixture();
    let bytes = gridcraft_engine::io::base64_decode(params["base64"].as_str().unwrap()).unwrap();
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut part = source.by_index(index).unwrap();
        let mut data = Vec::new();
        part.read_to_end(&mut data).unwrap();
        if part.name() == "xl/worksheets/sheet1.xml" {
            let xml = String::from_utf8(data).unwrap();
            let rules = (0..6)
                .map(|n| format!(r#"<conditionalFormatting sqref="A1"><cfRule type="synthetic{n}" priority="{}"/></conditionalFormatting>"#, n + 1))
                .collect::<String>();
            data = xml.replace("</worksheet>", &format!("{rules}</worksheet>")).into_bytes();
        }
        output.start_file(part.name(), zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(&data).unwrap();
    }
    let bytes = output.finish().unwrap().into_inner();
    let mut s = Session::new();
    let opened = s.execute("file.open", json!({"name": "synthetic-warnings.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    let warnings = opened["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 7);
    for n in 0..6 {
        assert!(warnings.iter().any(|w| w.as_str().unwrap().contains(&format!("synthetic{n}"))));
    }
    let messages = s
        .take_ui_requests()
        .into_iter()
        .filter_map(|request| match request {
            gridcraft_engine::UiRequest::Message(message) => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Saved formula results were kept and may be out of date"));
    assert!(messages[0].contains("2 additional import warnings."));
}

/// Filename-dependent caches are retained by default; an explicit full recalculation refreshes them.
#[test]
fn cached_open_filename_results_follow_explicit_recalculation_policy() {
    let mut wb = Workbook::new();
    let sheet_name = wb.sheet(0).unwrap().name.clone();
    for (at, formula, cached) in [
        ("A1", "CELL(\"filename\",C1)", Value::from("[saved-synthetic.xlsx]Previous")),
        ("A2", "MID(CELL(\"filename\",C1),FIND(\"]\",CELL(\"filename\",C1))+1,31)", Value::Error(CellError::Value)),
        ("B1", "GETPIVOTDATA(\"Suma\",C1)", Value::Number(1250.25)),
    ] {
        let mut cell = Cell::formula(Formula::new(formula));
        cell.value = cached;
        wb.sheet_mut(0).unwrap().set_cell(CellRef::parse(at).unwrap(), cell);
    }
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let params = json!({"name": "opened-synthetic.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)});
    let mut cached = Session::new();
    let opened = cached.execute("file.open", params.clone()).unwrap();
    assert_eq!(value(&cached, "A1"), Value::from("[saved-synthetic.xlsx]Previous"));
    assert_eq!(value(&cached, "A2"), Value::Error(CellError::Value));
    assert_eq!(value(&cached, "B1"), Value::Number(1250.25));
    assert!(opened["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("may be out of date")));
    let mut params = params;
    params["recalculate"] = json!(true);
    let mut recalculated = Session::new();
    recalculated.execute("file.open", params).unwrap();
    assert_eq!(value(&recalculated, "A1"), Value::from(format!("[opened-synthetic.xlsx]{sheet_name}").as_str()));
    assert_eq!(value(&recalculated, "A2"), Value::from(sheet_name.as_str()));
    assert_eq!(value(&recalculated, "B1"), Value::Error(CellError::Name));
}

/// Legacy and dynamic array follower caches survive open, unrelated edits and save/reopen.
#[test]
fn cached_open_retains_array_followers_without_recalculating_the_anchor() {
    for legacy in [false, true] {
        let mut wb = Workbook::new();
        let mut formula = Formula::new("SYNTHETIC_UNSUPPORTED_ARRAY()");
        let range = gridcraft_core::RangeRef::parse("A1:A5").unwrap();
        if legacy {
            formula.array = Some(range);
        } else {
            formula.dynamic = true;
        }
        let mut anchor = Cell::formula(formula);
        anchor.value = Value::Number(17.0);
        let sheet = wb.sheet_mut(0).unwrap();
        sheet.set_cell(CellRef::parse("A1").unwrap(), anchor);
        sheet.spill_ranges.insert(range.start, range);
        let followers = [("A2", Value::Number(0.0)), ("A3", Value::from("")), ("A4", Value::Bool(false)), ("A5", Value::Error(CellError::Div0))];
        for (at, value) in &followers {
            sheet.spill.insert(CellRef::parse(at).unwrap(), value.clone());
        }
        sheet.set_cell(CellRef::parse("B1").unwrap(), Cell::formula(Formula::new("A2+1")));
        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let mut opened = Session::new();
        opened.execute("file.open", json!({"name": "synthetic-array.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
        for round in 0..2 {
            assert_eq!(value(&opened, "A1"), Value::Number(17.0), "{legacy}: do not evaluate the unsupported cached anchor");
            assert_eq!(value(&opened, "B1"), Value::Number(1.0), "missing formulas read the retained follower cache");
            let sheet = opened.doc().unwrap().wb.sheet(0).unwrap();
            assert_eq!(sheet.spill_ranges.get(&range.start), Some(&range));
            assert_eq!(sheet.cell(range.start).unwrap().formula.as_ref().unwrap().array, legacy.then_some(range));
            for (at, expected) in &followers {
                let at = CellRef::parse(at).unwrap();
                assert_eq!(sheet.value(at), *expected, "legacy={legacy}, round={round}, {at:?}");
                assert!(sheet.cell(at).is_none(), "cached followers must not block a later spill");
            }
            if round == 0 {
                opened.execute("cell.set", json!({"cell": "Z20", "input": "1"})).unwrap();
                let saved = opened.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
                let mut reopened = Session::new();
                reopened.execute("file.open", json!({"name": "synthetic-array-roundtrip.xlsx", "base64": saved["base64"]})).unwrap();
                opened = reopened;
            }
        }
    }
}

/// Legacy-array metadata protects missing followers whether the cache is entirely empty or sparse.
#[test]
fn cached_open_protects_legacy_array_followers_without_stored_values() {
    for cached_tail in [false, true] {
        let mut wb = Workbook::new();
        let area = gridcraft_core::RangeRef::parse("A1:A3").unwrap();
        let mut f = Formula::new("SYNTHETIC_UNSUPPORTED_ARRAY()");
        f.array = Some(area);
        let mut anchor = Cell::formula(f);
        anchor.value = Value::Number(17.0);
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_cell(area.start, anchor);
        sh.spill_ranges.insert(area.start, area);
        if cached_tail {
            sh.spill.insert(CellRef::parse("A3").unwrap(), Value::Number(31.0));
        }
        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let mut s = Session::new();
        s.execute("file.open", json!({"name": "synthetic-sparse-legacy.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().spill.is_empty(), !cached_tail);
        s.execute("home.bold", json!({"range": "A2"})).unwrap();
        s.execute("cell.set", json!({"cell": "Z20", "input": "1"})).unwrap();
        for (command, params) in [("cell.set", json!({"cell": "A2", "input": "9"})), ("range.setValues", json!({"range": "A2", "values": [[9]]}))] {
            let before = s.doc().unwrap().wb.clone();
            let error = s.execute(command, params).unwrap_err();
            assert!(error.to_string().contains("part of an array"), "{error}");
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb), "a refused edit leaves the document intact");
        }
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
        assert_eq!(value(&s, "A2"), Value::Number(2.0), "replacing the whole array remains allowed");
    }
}

/// A legacy array whose follower content lives only in the saved spill cache.
fn legacy_array_clear_fixture(cached: bool, filtered: bool) -> Json {
    let mut wb = Workbook::new();
    let area = gridcraft_core::RangeRef::parse("A1:B3").unwrap();
    let mut f = Formula::new("SYNTHETIC_UNSUPPORTED_ARRAY()");
    f.array = Some(area);
    let mut anchor = Cell::formula(f);
    anchor.value = Value::Number(17.0);
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_cell(area.start, anchor);
    sh.spill_ranges.insert(area.start, area);
    if cached {
        sh.spill.insert(CellRef::parse("B2").unwrap(), Value::Number(42.0));
        sh.spill.insert(CellRef::parse("A3").unwrap(), Value::Number(31.0));
    }
    sh.set_value(CellRef::parse("Z20").unwrap(), Value::Number(99.0));
    if filtered {
        for (at, text) in [("C1", "Flag"), ("C2", "keep"), ("C3", "hide")] {
            sh.set_value(CellRef::parse(at).unwrap(), Value::from(text));
        }
        sh.autofilter = Some(gridcraft_model::AutoFilter {
            range: gridcraft_core::RangeRef::parse("C1:C3").unwrap(),
            criteria: vec![(0, gridcraft_model::FilterCriterion::Values { values: vec!["keep".into()], blanks: false })],
        });
        sh.rows.entry(2).or_default().hidden = true;
    }
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    json!({"name": "synthetic-array-clear.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})
}

/// Delete/Clear All refuse partial arrays, including missing followers and gapped selections.
#[test]
fn cached_open_clear_legacy_arrays_checks_the_selection() {
    for command in ["edit.clearContents", "edit.clearAll"] {
        for cached in [false, true] {
            let mut s = Session::new();
            s.execute("file.open", legacy_array_clear_fixture(cached, false)).unwrap();
            for range in ["B2", "A1", "A1:B2", "A1:A3,B1,B3", "Z20,B2"] {
                let before = s.doc().unwrap().wb.clone();
                let error = s.execute(command, json!({"range": range})).unwrap_err();
                assert!(error.to_string().contains("part of an array"), "{error}");
                assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
                assert_eq!(value(&s, "Z20"), Value::Number(99.0));
            }
            s.execute("edit.clearFormats", json!({"range": "B2"})).unwrap();
            assert_eq!(value(&s, "B2"), if cached { Value::Number(42.0) } else { Value::Empty });
            for range in ["A1:B3", "A1:A3,B1:B3", "A1:B2,A2:B3"] {
                s.execute(command, json!({"range": range})).unwrap();
                for at in ["A1", "B2", "A3"] {
                    assert_eq!(value(&s, at), Value::Empty, "{command}: {range}: {at}");
                }
                let sh = s.doc().unwrap().wb.sheet(0).unwrap();
                assert!(sh.spill.is_empty() && sh.spill_ranges.is_empty());
                s.execute("edit.undo", json!({})).unwrap();
                assert_eq!(value(&s, "A1"), Value::Number(17.0));
                assert_eq!(value(&s, "B2"), if cached { Value::Number(42.0) } else { Value::Empty });
            }
            s.execute(command, json!({"range": "Z20"})).unwrap();
            assert_eq!(value(&s, "Z20"), Value::Empty);
            assert_eq!(value(&s, "A1"), Value::Number(17.0));
        }
    }
}

/// Filter-hidden rows are skipped, so a nominal whole-array clear may still be partial.
#[test]
fn cached_open_clear_legacy_arrays_respects_filtered_rows() {
    for command in ["edit.clearContents", "edit.clearAll"] {
        let mut s = Session::new();
        // Load the saved filter without issuing a structural command that fully recalculates.
        s.execute("file.open", legacy_array_clear_fixture(true, true)).unwrap();
        assert_eq!(value(&s, "A3"), Value::Number(31.0));
        assert!(s.doc().unwrap().wb.sheet(0).unwrap().is_row_filtered(2));
        let before = s.doc().unwrap().wb.clone();
        let error = s.execute(command, json!({"range": "A1:B3"})).unwrap_err();
        assert!(error.to_string().contains("part of an array"), "{error}");
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
        s.execute(command, json!({"range": "A3:B3"})).unwrap();
        assert_eq!(value(&s, "A3"), Value::Number(31.0));
        s.execute("data.clearFilter", json!({})).unwrap();
        s.execute("home.hideRows", json!({"rows": "3:3"})).unwrap();
        s.execute(command, json!({"range": "A1:B3"})).unwrap();
        assert_eq!(value(&s, "A1"), Value::Empty);
        assert_eq!(value(&s, "A3"), Value::Empty);
    }
}

/// A missing unsupported array anchor must not erase saved followers through open/save/reopen.
#[test]
fn cached_open_keeps_followers_of_missing_failed_array_anchors() {
    for dynamic in [false, true] {
        for text in ["SYNTHETIC_UNSUPPORTED_ARRAY()", "1+"] {
            let mut wb = Workbook::new();
            let area = gridcraft_core::RangeRef::parse("A1:A3").unwrap();
            let mut f = Formula::new(text);
            f.dynamic = dynamic;
            f.array = (!dynamic).then_some(area);
            let sh = wb.sheet_mut(0).unwrap();
            sh.set_cell(area.start, Cell::formula(f));
            sh.spill_ranges.insert(area.start, area);
            sh.spill.insert(CellRef::parse("A2").unwrap(), Value::Number(31.0));
            sh.spill.insert(CellRef::parse("A3").unwrap(), Value::Number(42.0));
            sh.set_cell(CellRef::parse("C1").unwrap(), Cell::formula(Formula::new("A2+1")));
            let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
            let mut s = Session::new();
            let opened = s
                .execute("file.open", json!({"name": "synthetic-missing-array-anchor.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)}))
                .unwrap();
            assert_eq!(value(&s, "A1"), Value::Error(CellError::Name));
            assert_eq!(value(&s, "A2"), Value::Number(31.0));
            assert_eq!(value(&s, "A3"), Value::Number(42.0));
            assert_eq!(value(&s, "C1"), Value::Number(32.0));
            assert!(
                opened["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("may be out of date")),
                "saved follower caches also require the freshness notice"
            );
            let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
            let mut back = Session::new();
            back.execute("file.open", json!({"name": "synthetic-missing-array-reopened.xlsx", "base64": saved["base64"]})).unwrap();
            assert_eq!(value(&back, "A1"), Value::Error(CellError::Name));
            assert_eq!(value(&back, "A2"), Value::Number(31.0));
            assert_eq!(value(&back, "A3"), Value::Number(42.0));
            assert_eq!(back.doc().unwrap().wb.sheet(0).unwrap().spill_ranges.get(&area.start), Some(&area));
            if !dynamic {
                assert!(back.execute("edit.clearContents", json!({"range": "A2"})).is_err());
            }
        }
    }
}
