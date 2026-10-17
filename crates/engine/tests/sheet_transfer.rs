use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::json;

fn books() -> (Session, u64, u64) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1","input":"42"})).unwrap();
    let source = s.active().unwrap().uid;
    s.new_workbook();
    s.execute("sheet.rename", json!({"name":"Destination"})).unwrap();
    let destination = s.active().unwrap().uid;
    s.set_active(0);
    (s, source, destination)
}

#[test]
fn copy_targets_another_workbook_and_keeps_the_source_unchanged() {
    let (mut s, _, destination) = books();
    let source = s.active().unwrap().wb.clone();
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    assert!(Arc::ptr_eq(&s.documents()[0].wb, &source));
    let dest = &s.documents()[1].wb;
    assert_eq!(dest.sheets.len(), 2);
    assert_eq!(dest.sheet(0).unwrap().name, "Source");
    assert_eq!(dest.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(42.0));
}

#[test]
fn a_closed_or_invalid_destination_is_not_silently_treated_as_the_source() {
    let (mut s, _, _) = books();
    let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
    assert!(s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":u64::MAX})).is_err());
    for (d, wb) in s.documents().iter().zip(before) {
        assert!(Arc::ptr_eq(&d.wb, &wb));
    }
}

fn movable_books() -> (Session, u64, u64) {
    let (mut s, source, destination) = books();
    s.execute("home.insertSheet", json!({"name":"Keep","before":1})).unwrap();
    s.execute("sheet.activate", json!({"sheet":"Source"})).unwrap();
    (s, source, destination)
}

#[test]
fn move_undo_and_redo_restore_both_workbooks_from_either_side() {
    let (mut s, _, destination) = movable_books();
    s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
    assert_eq!(s.documents()[1].wb.sheets.len(), 2);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 2);
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
    s.set_active(0);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
    assert_eq!(s.documents()[1].wb.sheets.len(), 2);
}

#[test]
fn linked_undo_refuses_later_edits_without_partial_changes() {
    let (mut s, _, destination) = movable_books();
    s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
    s.execute("cell.set", json!({"cell":"B1","input":"99"})).unwrap();
    s.set_active(0);
    let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
    assert!(s.execute("edit.undo", json!({})).is_err());
    for (d, wb) in s.documents().iter().zip(before) {
        assert!(Arc::ptr_eq(&d.wb, &wb));
    }
    s.set_active(1);
    s.execute("edit.undo", json!({})).unwrap();
    s.set_active(0);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 2);
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
}

#[test]
fn linked_undo_does_not_remove_the_only_copy_after_a_workbook_closes() {
    let (mut s, _, destination) = movable_books();
    s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
    s.close_document(0);
    let before = s.active().unwrap().wb.clone();
    assert!(s.execute("edit.undo", json!({})).is_err());
    assert!(Arc::ptr_eq(&before, &s.active().unwrap().wb));
}

#[test]
fn copying_to_a_new_workbook_creates_one_sheet_and_is_undoable() {
    let (mut s, _, _) = books();
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":"new"})).unwrap();
    assert_eq!(s.documents().len(), 3);
    assert_eq!(s.active().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().name, "Source");
    assert!(s.active().unwrap().is_dirty());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().name, "Sheet1");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(42.0));
}

#[test]
fn local_formulas_follow_a_renamed_copy_instead_of_a_destination_namesake() {
    let (mut s, _, destination) = books();
    s.set_active(1);
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1","input":"999"})).unwrap();
    s.set_active(0);
    s.execute("cell.set", json!({"cell":"B1","input":"=Source!A1*2"})).unwrap();
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let sh = s.active().unwrap().wb.sheet(0).unwrap();
    assert_eq!(sh.name, "Source (2)");
    assert_eq!(sh.value(CellRef::new(0, 1)), Value::Number(84.0));
    assert!(sh.cells.get(CellRef::new(0, 1)).unwrap().formula.as_ref().unwrap().text.contains("Source (2)"));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
}

#[test]
fn cross_sheet_formulas_are_refused_instead_of_binding_to_destination_sheets() {
    let (mut s, _, destination) = movable_books();
    s.execute("cell.set", json!({"cell":"B1","input":"=Keep!A1"})).unwrap();
    let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
    assert!(s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).is_err());
    for (d, wb) in s.documents().iter().zip(before) {
        assert!(Arc::ptr_eq(&d.wb, &wb));
    }
}

#[test]
fn a_move_refuses_inbound_dependencies_but_a_copy_keeps_them_valid() {
    let (mut s, _, destination) = movable_books();
    s.execute("cell.set", json!({"cell":"Keep!A1","input":"=Source!A1"})).unwrap();
    assert!(s.execute("sheet.move", json!({"sheet":"Source","to":0,"targetWorkbook":destination})).is_err());
    s.execute("sheet.move", json!({"sheet":"Source","to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    assert_eq!(s.documents()[0].wb.sheet(1).unwrap().value(CellRef::new(0, 0)), Value::Number(42.0));
}

#[test]
fn styles_and_implicit_normal_formatting_are_remapped_between_workbooks() {
    use gridcraft_model::{Style, StyleId, StyleTable};
    let (mut s, _, destination) = books();
    let wb = Arc::make_mut(&mut s.active_mut().unwrap().wb);
    let mut normal = Style::default();
    normal.font.size = 17.0;
    wb.styles = StyleTable::with_default(normal.clone());
    let bold = wb.styles.derive(StyleId::DEFAULT, |st| st.font.bold = true);
    wb.sheet_mut(0).unwrap().set_style(CellRef::new(0, 0), bold);
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let wb = &s.active().unwrap().wb;
    let sh = wb.sheet(0).unwrap();
    assert!(wb.styles.get(sh.style_id(CellRef::new(0, 0))).font.bold);
    assert_eq!(wb.styles.get(sh.style_id(CellRef::new(900, 12000))), &normal);
    assert_eq!(sh.cells.len(), 1, "empty cells must not be materialised");
}

#[test]
fn scoped_names_and_global_dependencies_are_copied_and_scopes_remain_correct() {
    use gridcraft_model::DefinedName;
    let (mut s, _, destination) = books();
    let wb = Arc::make_mut(&mut s.active_mut().unwrap().wb);
    wb.names.push(DefinedName { name: "Local".into(), scope: Some(0), formula: "Source!$A$1".into(), comment: String::new(), hidden: false });
    wb.names.push(DefinedName { name: "Rate".into(), scope: None, formula: "0.5".into(), comment: String::new(), hidden: false });
    s.execute("cell.set", json!({"cell":"B1","input":"=Local*Rate"})).unwrap();
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let wb = &s.active().unwrap().wb;
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 1)), Value::Number(21.0));
    assert!(wb.names.iter().any(|n| n.name == "Local" && n.scope == Some(0)));
    assert!(wb.names.iter().any(|n| n.name == "Rate" && n.scope.is_none()));
}

#[test]
fn unsafe_structure_or_format_changes_are_refused_before_mutation() {
    let (mut s, _, destination) = books();
    assert!(s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).is_err(), "do not remove the last visible sheet");
    Arc::make_mut(&mut s.active_mut().unwrap().wb).protected_structure = true;
    assert!(s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).is_err());
    Arc::make_mut(&mut s.active_mut().unwrap().wb).protected_structure = false;
    s.set_active(1);
    Arc::make_mut(&mut s.active_mut().unwrap().wb).date_system = gridcraft_core::DateSystem::D1904;
    s.set_active(0);
    assert!(s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).is_err());
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
}

#[test]
fn a_reference_based_validation_list_without_equals_survives_rename_and_xlsx() {
    use gridcraft_model::{Validation, ValidationKind};
    let (mut s, _, destination) = books();
    s.set_active(1);
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1","input":"999"})).unwrap();
    s.set_active(0);
    let sh = Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap();
    sh.validations.push(Validation {
        ranges: vec![gridcraft_core::RangeRef::cell(CellRef::new(0, 1))],
        kind: ValidationKind::List,
        f1: "Source!$A$1".into(),
        ..Default::default()
    });
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let bytes = gridcraft_engine::io::save_bytes(&s.active().unwrap().wb, "Copy.xlsx").unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("Copy.xlsx", &bytes).unwrap();
    let dv = &wb.sheet(0).unwrap().validations[0];
    assert_eq!(gridcraft_engine::cmd::data::list_items(&wb, 0, dv), vec!["42"]);
}

#[test]
fn ordinary_tables_receive_new_ids_and_survive_xlsx_roundtrip() {
    let (mut s, _, destination) = books();
    s.execute("cell.set", json!({"cell":"A1","input":"Qty"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"3"})).unwrap();
    s.execute("insert.table", json!({"range":"A1:A2","header":true,"name":"Sales"})).unwrap();
    s.set_active(1);
    s.execute("cell.set", json!({"cell":"A1","input":"Other"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"7"})).unwrap();
    s.execute("insert.table", json!({"range":"A1:A2","header":true,"name":"OtherSales"})).unwrap();
    let old_id = s.active().unwrap().wb.sheet(0).unwrap().tables[0].id;
    s.set_active(0);
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let wb = &s.active().unwrap().wb;
    assert_ne!(wb.sheet(0).unwrap().tables[0].id, old_id);
    let bytes = gridcraft_engine::io::save_bytes(wb, "Copy.xlsx").unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("Copy.xlsx", &bytes).unwrap();
    assert_eq!(wb.sheet(0).unwrap().tables[0].name, "Sales");
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Number(3.0));
    assert_eq!(wb.sheet(1).unwrap().tables[0].name, "OtherSales");
}

#[test]
fn a_move_refuses_other_reports_that_use_the_source_data() {
    let (mut s, _, destination) = movable_books();
    s.execute("cell.set", json!({"cell":"A1","input":"Qty"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"3"})).unwrap();
    s.execute("insert.pivotTable", json!({"source":"Source!A1:A2","destination":"Keep!D1","rows":["Qty"],"values":["Qty"]})).unwrap();
    s.execute("sheet.activate", json!({"sheet":"Source"})).unwrap();
    let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
    assert!(s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).is_err());
    for (d, wb) in s.documents().iter().zip(before) {
        assert!(Arc::ptr_eq(&d.wb, &wb));
    }
}

#[test]
fn hidden_sheets_do_not_become_the_destinations_active_sheet() {
    use gridcraft_model::Visibility;
    let (mut s, _, destination) = movable_books();
    Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().visibility = Visibility::Hidden;
    s.execute("sheet.move", json!({"sheet":"Source","to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let wb = &s.active().unwrap().wb;
    assert_eq!(wb.sheet(0).unwrap().visibility, Visibility::Hidden);
    assert_eq!(wb.active().unwrap().name, "Destination");
}

#[test]
fn commands_expose_stable_destination_ids_after_document_indices_shift() {
    let (mut s, _, destination) = books();
    let listed = s.execute("view.switchWindows", json!({})).unwrap();
    assert_eq!(listed[1]["uid"].as_u64(), Some(destination));
    s.execute("file.new", json!({})).unwrap();
    s.close_document(0);
    let listed = s.execute("view.switchWindows", json!({})).unwrap();
    assert_eq!(listed[0]["uid"].as_u64(), Some(destination));
    assert_eq!(listed[0]["index"], json!(0));
    let result = s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":listed[0]["uid"]})).unwrap();
    assert_eq!(result["workbook"].as_u64(), Some(destination));
    assert_eq!(s.active().unwrap().wb.sheets.len(), 2);
}
