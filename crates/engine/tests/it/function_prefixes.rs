//! Synthetic imported-formula namespaces survive actual command and save paths, in memory.
use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::{Cell, FileFunctionPrefix, Formula, Workbook};
use serde_json::json;
use std::io::Read;

/// Open a serialized workbook so tests exercise the real XLSX reader, not only a model flag.
fn session() -> Session {
    let mut wb = Workbook::new();
    let mut formula = Formula::new("SYNTHETICFUTURE(A1)");
    formula.file_functions.insert("SYNTHETICFUTURE".into(), vec![Some(FileFunctionPrefix::Xlfn)]);
    wb.sheet_mut(0).unwrap().set_cell(CellRef::parse("B1").unwrap(), Cell::formula(formula));
    wb.sheet_mut(0).unwrap().set_value(CellRef::parse("A1").unwrap(), Value::Number(1.0));
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name":"synthetic-prefixes.xlsx","base64":gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    s
}

/// Independently inspect the saved XML after a command sequence.
fn saved_xml(s: &mut Session) -> String {
    let encoded = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(encoded["base64"].as_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// An unrelated edit and a JSON save/reopen preserve the source namespace.
#[test]
fn imported_prefix_survives_unrelated_edit_and_json_roundtrip() {
    let mut s = session();
    s.execute("cell.set", json!({"cell":"Z20","input":"1"})).unwrap();
    let encoded = s.execute("file.saveBytes", json!({"format":"json"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name":"synthetic.json","base64":encoded["base64"]})).unwrap();
    assert!(saved_xml(&mut reopened).contains("_xlfn.SYNTHETICFUTURE(A1)"));
}

/// Copying and filling adjust references while carrying the imported function spelling.
#[test]
fn imported_prefix_survives_copy_and_fill() {
    let mut s = session();
    s.execute("edit.copy", json!({"range":"B1"})).unwrap();
    s.execute("edit.paste", json!({"at":"B2"})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(A2)"));
    s.execute("edit.fillDown", json!({"range":"B1:B3"})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(A3)"));
}

/// Row insertion and sheet renaming must not turn imported calls into unprefixed calls.
#[test]
fn imported_prefix_survives_structural_edits_and_undo() {
    let mut s = session();
    s.execute("home.insertRows", json!({"rows":"1:1"})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(A2)"));
    s.execute("edit.undo", json!({})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(A1)"));
    s.execute("sheet.rename", json!({"name":"Synthetic renamed"})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(A1)"));
}

/// Replacing a formula with a new expression discards the imported spelling of the old call.
#[test]
fn replacing_a_formula_does_not_keep_stale_namespace_metadata() {
    let mut s = session();
    s.execute("cell.set", json!({"cell":"B1","input":"=SUM(A1,1)"})).unwrap();
    let xml = saved_xml(&mut s);
    assert!(!xml.contains("SYNTHETICFUTURE"));
    assert!(xml.contains("SUM(A1,1)"));
}

/// Confirming the existing formula or changing only its arguments keeps its imported namespace.
#[test]
fn confirming_or_editing_imported_formula_arguments_keeps_the_namespace() {
    for argument in ["A1", "A2"] {
        let mut s = session();
        s.execute("cell.set", json!({"cell":"B1","input":format!("=SYNTHETICFUTURE({argument})")})).unwrap();
        assert!(saved_xml(&mut s).contains(&format!("_xlfn.SYNTHETICFUTURE({argument})")));
    }
}

/// Sorting and arithmetic paste rebuild expressions without losing their function namespaces.
#[test]
fn imported_prefix_survives_sort_and_paste_operations() {
    let mut s = session();
    s.execute("cell.set", json!({"cell":"A1","input":"2"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"1"})).unwrap();
    s.execute("edit.copy", json!({"range":"B1"})).unwrap();
    s.execute("edit.paste", json!({"at":"B2"})).unwrap();
    s.execute("data.sortAscending", json!({"range":"A1:B2","column":"A"})).unwrap();
    assert_eq!(saved_xml(&mut s).matches("_xlfn.SYNTHETICFUTURE(").count(), 2);
    s.execute("edit.copy", json!({"range":"A1"})).unwrap();
    s.execute("edit.pasteSpecial", json!({"at":"B1","operation":"multiply"})).unwrap();
    assert_eq!(saved_xml(&mut s).matches("_xlfn.SYNTHETICFUTURE(").count(), 2);
}

/// Table and header renames retain the namespace while updating structured references.
#[test]
fn imported_prefix_survives_table_and_header_rename() {
    let mut s = session();
    s.execute("cell.set", json!({"cell":"A1","input":"Amount"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"1"})).unwrap();
    s.execute("insert.table", json!({"range":"A1:A2","header":true,"name":"SyntheticTable"})).unwrap();
    s.execute("cell.set", json!({"cell":"B1","input":"=SYNTHETICFUTURE(SyntheticTable[Amount])"})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(SyntheticTable[Amount])"));
    s.execute("table.rename", json!({"table":"SyntheticTable","name":"SyntheticRenamed"})).unwrap();
    assert!(saved_xml(&mut s).contains("_xlfn.SYNTHETICFUTURE(SyntheticRenamed[Amount])"));
    s.execute("cell.set", json!({"cell":"A1","input":"Changed"})).unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("_xlfn.SYNTHETICFUTURE(SyntheticRenamed[Changed])"), "{xml}");
}

/// A replacement that binds the old function name locally must not inherit its global namespace.
#[test]
fn imported_prefix_does_not_retarget_a_new_local_lambda_call() {
    let mut s = session();
    s.execute("cell.set", json!({"cell":"B1","input":"=LET(SYNTHETICFUTURE,LAMBDA(x,x),SYNTHETICFUTURE(A1))"})).unwrap();
    let xml = saved_xml(&mut s);
    assert!(!xml.contains("_xlfn.SYNTHETICFUTURE("));
    assert!(xml.contains("_xlpm.SYNTHETICFUTURE("));
}

/// Filling same-style cells must keep each destination's imported namespace independently.
fn range_fill_keeps_destination_namespaces(prefixed_row: usize) {
    let mut wb = Workbook::new();
    for row in 1..=2 {
        let mut formula = Formula::new(&format!("SYNTHETICFUTURE(A{row})"));
        if row == prefixed_row {
            formula.file_functions.insert("SYNTHETICFUTURE".into(), vec![Some(FileFunctionPrefix::Xlfn)]);
        }
        wb.sheet_mut(0).unwrap().set_cell(CellRef::parse(&format!("B{row}")).unwrap(), Cell::formula(formula));
    }
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name":"synthetic-fill-prefixes.xlsx","base64":gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    s.execute("selection.set", json!({"range":"B1:B2","active":"B1"})).unwrap();
    s.execute("range.fill", json!({"range":"B1:B2","input":"=SYNTHETICFUTURE(A1)"})).unwrap();
    let xml = saved_xml(&mut s);
    assert_eq!(xml.matches("_xlfn.SYNTHETICFUTURE(").count(), 1);
    assert!(xml.contains(&format!(">_xlfn.SYNTHETICFUTURE(A{prefixed_row})</f>")));
    let bare_row = 3 - prefixed_row;
    assert!(xml.contains(&format!(">SYNTHETICFUTURE(A{bare_row})</f>")));
}

/// A prefixed first destination must not spread its namespace through the style cache.
#[test]
fn range_fill_prefixed_first_does_not_tag_bare_destinations() {
    range_fill_keeps_destination_namespaces(1);
}

/// A bare first destination must not strip a later destination's imported namespace.
#[test]
fn range_fill_bare_first_does_not_strip_prefixed_destinations() {
    range_fill_keeps_destination_namespaces(2);
}

/// A known function's mixed spelling survives import, argument edits, copy and a second save.
#[test]
fn mixed_known_prefixes_survive_edit_copy_and_reopen() {
    let mut wb = Workbook::new();
    let mut formula = Formula::new("XLOOKUP(D1,$A$1:$A$2,$B$1:$B$2)+XLOOKUP(D2,$A$1:$A$2,$B$1:$B$2)");
    formula.file_functions.insert("XLOOKUP".into(), vec![None, Some(FileFunctionPrefix::Xlfn)]);
    let sheet = wb.sheet_mut(0).unwrap();
    for (addr, value) in [("A1", 1.0), ("A2", 2.0), ("B1", 10.0), ("B2", 20.0), ("D1", 1.0), ("D2", 2.0), ("D3", 1.0)] {
        sheet.set_value(CellRef::parse(addr).unwrap(), Value::Number(value));
    }
    sheet.set_cell(CellRef::parse("C1").unwrap(), Cell::formula(formula));
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name":"synthetic-mixed-known.xlsx","base64":gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    let original = "XLOOKUP(D1,$A$1:$A$2,$B$1:$B$2)+_xlfn.XLOOKUP(D2,$A$1:$A$2,$B$1:$B$2)";
    assert!(saved_xml(&mut s).contains(original));
    s.execute("cell.set", json!({"cell":"C1","input":"=XLOOKUP(D2,$A$1:$A$2,$B$1:$B$2)+XLOOKUP(D1,$A$1:$A$2,$B$1:$B$2)"})).unwrap();
    let edited = "XLOOKUP(D2,$A$1:$A$2,$B$1:$B$2)+_xlfn.XLOOKUP(D1,$A$1:$A$2,$B$1:$B$2)";
    assert!(saved_xml(&mut s).contains(edited));
    s.execute("edit.undo", json!({})).unwrap();
    assert!(saved_xml(&mut s).contains(original));
    s.execute("edit.redo", json!({})).unwrap();
    assert!(saved_xml(&mut s).contains(edited));
    s.execute("edit.copy", json!({"range":"C1"})).unwrap();
    s.execute("edit.paste", json!({"at":"C2"})).unwrap();
    let shifted = "XLOOKUP(D3,$A$1:$A$2,$B$1:$B$2)+_xlfn.XLOOKUP(D2,$A$1:$A$2,$B$1:$B$2)";
    assert!(saved_xml(&mut s).contains(shifted));
    let saved = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name":"synthetic-reopened.xlsx","base64":saved["base64"]})).unwrap();
    let xml = saved_xml(&mut reopened);
    assert!(xml.contains(edited) && xml.contains(shifted), "{xml}");
}

/// Defined-name edits use rewrite_expr directly and must invalidate stale local-call prefixes.
#[test]
fn imported_prefixes_follow_binding_changes_through_name_manager() {
    let mut wb = Workbook::new();
    wb.names.push(gridcraft_model::DefinedName { name: "XLOOKUP".into(), scope: None, formula: "1".into(), comment: String::new(), hidden: false });
    let mut formula = Formula::new("XLOOKUP(1,A:A,B:B)+LET(XLOOKUP,1,XLOOKUP(2,A:A,B:B))");
    formula.file_functions.insert("XLOOKUP".into(), vec![Some(FileFunctionPrefix::Xlfn), None]);
    wb.sheet_mut(0).unwrap().set_cell(CellRef::parse("C1").unwrap(), Cell::formula(formula));
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name":"synthetic-name-bindings.xlsx","base64":gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    let before = saved_xml(&mut s);
    assert!(before.contains("_xlpm.XLOOKUP(2,A:A,B:B)"), "{before}");
    s.execute("formulas.defineName", json!({"oldName":"XLOOKUP","name":"Renamed","refersTo":"=1"})).unwrap();
    let after = saved_xml(&mut s);
    assert!(after.contains("_xlfn.XLOOKUP(2,A:A,B:B)"), "{after}");
    assert!(after.contains("_xlpm.Renamed"), "{after}");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(saved_xml(&mut s), before);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(saved_xml(&mut s), after);
    let saved = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name":"synthetic-renamed-bindings.xlsx","base64":saved["base64"]})).unwrap();
    assert!(saved_xml(&mut reopened).contains("_xlfn.XLOOKUP(2,A:A,B:B)"));
}

/// Explicit namespace edits override imported occurrence order, including known-function aliases.
#[test]
fn explicitly_edited_prefix_order_survives_save_undo_and_reopen() {
    for (name, prefix) in [("XLOOKUP", "_xlfn."), ("FILTER", "_xlfn._xlws."), ("SYNTHETICFUTURE", "_xlfn.")] {
        let mut wb = Workbook::new();
        let mut formula = Formula::new(&format!("{name}(A1,A2,A3)+{name}(B1,B2,B3)"));
        let namespace = if prefix == "_xlfn." { FileFunctionPrefix::Xlfn } else { FileFunctionPrefix::XlfnXlws };
        formula.file_functions.insert(name.into(), vec![None, Some(namespace)]);
        wb.sheet_mut(0).unwrap().set_cell(CellRef::parse("Z1").unwrap(), Cell::formula(formula));
        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let mut s = Session::new();
        s.execute("file.open", json!({"name":"synthetic-explicit-prefixes.xlsx","base64":gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
        let original = format!("{name}(A1,A2,A3)+{prefix}{name}(B1,B2,B3)");
        assert!(saved_xml(&mut s).contains(&original));
        let edited = format!("{prefix}{name}(A1,A2,A3)+{name}(B1,B2,B3)");
        s.execute("cell.set", json!({"cell":"Z1","input":format!("={edited}")})).unwrap();
        let xml = saved_xml(&mut s);
        assert!(xml.contains(&edited), "{name}: {xml}");
        assert!(!xml.contains(&original), "{name}: {xml}");
        s.execute("edit.undo", json!({})).unwrap();
        assert!(saved_xml(&mut s).contains(&original));
        s.execute("edit.redo", json!({})).unwrap();
        assert!(saved_xml(&mut s).contains(&edited));
        let saved = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("file.open", json!({"name":"synthetic-explicit-reopened.xlsx","base64":saved["base64"]})).unwrap();
        assert!(saved_xml(&mut reopened).contains(&edited));
    }
}

/// A newly typed namespace is preserved even without a prior imported formula; closing is automatic.
#[test]
fn explicit_prefix_on_new_formula_survives_automatic_parenthesis_closing() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"B1","input":"=_XlFn.SYNTHETICFUTURE(A1"})).unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("_xlfn.SYNTHETICFUTURE(A1)"), "{xml}");
}

/// Prefix-looking string contents are data, so argument edits still inherit imported metadata.
#[test]
fn namespace_text_in_a_string_does_not_override_imported_function_spelling() {
    let mut s = session();
    s.execute("cell.set", json!({"cell":"B1","input":"=SYNTHETICFUTURE(\"_xlfn.OTHER(A1)\")"})).unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("<f>_xlfn.SYNTHETICFUTURE("), "{xml}");
}

/// A prefix on one function must not suppress another function's required canonical XLSX prefix.
#[test]
fn other_functions_keep_canonical_namespaces_when_one_prefix_is_explicit() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"Z1","input":"=_xlfn.SYNTHETICFUTURE(A1)+XLOOKUP(A1,A2,A3)"})).unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("_xlfn.SYNTHETICFUTURE(A1)+_xlfn.XLOOKUP(A1,A2,A3)"), "{xml}");
}
