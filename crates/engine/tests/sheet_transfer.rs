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
    assert_ne!(dest.sheet(0).unwrap().uid.get(), source.sheet(0).unwrap().uid.get(), "a copy has its own runtime identity");
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
    let identity = s.documents()[0].wb.sheet(0).unwrap().uid.get();
    s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
    assert_eq!(s.documents()[1].wb.sheets.len(), 2);
    assert_eq!(s.documents()[1].wb.sheet(0).unwrap().uid.get(), identity, "a move retains its identity");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 2);
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
    assert_eq!(s.documents()[0].wb.sheet(0).unwrap().uid.get(), identity, "undo restores the same sheet");
    s.set_active(0);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
    assert_eq!(s.documents()[1].wb.sheets.len(), 2);
    assert_eq!(s.documents()[1].wb.sheet(0).unwrap().uid.get(), identity, "redo moves the same sheet");
}

#[test]
fn transferred_tables_and_drawings_share_the_destinations_object_id_namespace() {
    use gridcraft_model::Image;
    let (mut s, _, destination) = books();
    s.execute("cell.set", json!({"cell":"A1","input":"Qty"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"3"})).unwrap();
    s.execute("insert.table", json!({"range":"A1:A2","header":true,"name":"Sales"})).unwrap();
    let table_id = s.active().unwrap().wb.sheet(0).unwrap().tables[0].id;
    let image = |id| Image { id, anchor: Default::default(), data: Default::default(), mime: String::new(), alt: String::new() };
    Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().images.push(image(table_id + 1));
    s.set_active(1);
    let target = Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap();
    target.images.extend([image(table_id), image(table_id + 1)]);
    s.set_active(0);
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let ids: Vec<_> =
        s.active().unwrap().wb.sheets.iter().flat_map(|sh| sh.tables.iter().map(|o| o.id).chain(sh.images.iter().map(|o| o.id))).collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), ids.len(), "all object kinds use distinct workbook-wide ids");
}

#[test]
fn uncheckable_python_and_control_bindings_refuse_transfer_without_mutation() {
    use gridcraft_model::{Cell, FormControl, PyCode};
    for python in [false, true] {
        for on_source in [false, true] {
            let (mut s, _, destination) = movable_books();
            let sh = Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(if on_source { 0 } else { 1 }).unwrap();
            if python {
                sh.cells.set(
                    CellRef::new(0, 0),
                    Cell { py: Some(Arc::new(PyCode { code: "xl('Source!A1')".into(), ..Default::default() })), ..Default::default() },
                );
            } else {
                sh.controls.push(FormControl::default());
            }
            let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
            assert!(
                s.execute("sheet.move", json!({"to":0,"copy":on_source,"targetWorkbook":destination})).is_err(),
                "python={python}, on_source={on_source}"
            );
            for (d, wb) in s.documents().iter().zip(before) {
                assert!(Arc::ptr_eq(&d.wb, &wb));
            }
        }
    }
}

#[test]
fn transfer_refuses_an_open_undo_group_in_either_participant() {
    for grouped in [0, 1] {
        let (mut s, _, destination) = movable_books();
        s.set_active(grouped);
        let token = s.group_begin("Grouped edit").unwrap();
        s.set_active(0);
        let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
        assert!(s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).is_err());
        for (d, wb) in s.documents().iter().zip(before) {
            assert!(Arc::ptr_eq(&d.wb, &wb));
        }
        s.group_end(token).unwrap();
    }
}

#[test]
fn multi_step_undo_and_redo_cross_a_linked_sheet_move() {
    let (mut s, _, destination) = movable_books();
    s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
    s.execute("cell.set", json!({"cell":"B1","input":"first"})).unwrap();
    s.execute("cell.set", json!({"cell":"C1","input":"second"})).unwrap();

    s.execute("edit.undo", json!({"steps":3})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 2);
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
    assert_eq!(s.documents()[0].wb.sheet(0).unwrap().name, "Source");

    s.execute("edit.redo", json!({"steps":3})).unwrap();
    assert_eq!(s.documents()[0].wb.sheets.len(), 1);
    assert_eq!(s.documents()[1].wb.sheets.len(), 2);
    let restored = s.documents()[1].wb.sheet(0).unwrap();
    assert_eq!(restored.value(CellRef::new(0, 1)), Value::from("first"));
    assert_eq!(restored.value(CellRef::new(0, 2)), Value::from("second"));
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
fn chart_series_referring_to_another_sheet_refuse_transfer() {
    let (mut s, _, destination) = movable_books();
    s.execute("cell.set", json!({"cell":"A2","input":"43"})).unwrap();
    s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
    let sh = Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap();
    sh.charts[0].series[0].values = "Keep!A1:A2".into();
    let source = s.active().unwrap().wb.clone();
    assert!(s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).is_err());
    assert!(Arc::ptr_eq(&s.documents()[0].wb, &source));
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
}

#[test]
fn conditional_format_referring_to_another_sheet_refuses_transfer() {
    let (mut s, _, destination) = movable_books();
    s.execute("home.conditionalFormat", json!({"range":"A1","rule":{"type":"expression","formula":"=Keep!A1>0","preset":"greenFill"}})).unwrap();
    let source = s.active().unwrap().wb.clone();
    assert!(s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).is_err());
    assert!(Arc::ptr_eq(&s.documents()[0].wb, &source));
    assert_eq!(s.documents()[1].wb.sheets.len(), 1);
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
    assert_eq!(gridcraft_engine::cmd::data::list_items(&wb, 0, CellRef::new(0, 1), dv), vec!["42"]);
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
    assert_eq!(listed[1]["doc"].as_u64(), Some(destination));
    s.execute("file.new", json!({})).unwrap();
    s.close_document(0);
    let listed = s.execute("view.switchWindows", json!({})).unwrap();
    assert_eq!(listed[0]["doc"].as_u64(), Some(destination));
    assert_eq!(listed[0]["index"], json!(0));
    let result = s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":listed[0]["doc"]})).unwrap();
    assert_eq!(result["workbook"].as_u64(), Some(destination));
    assert_eq!(s.active().unwrap().wb.sheets.len(), 2);
}

#[test]
fn transfers_preserve_file_provenance_for_existing_and_new_destinations() {
    use gridcraft_engine::Trust;
    for copy in [false, true] {
        for new in [false, true] {
            for source_trust in [Trust::Local, Trust::FromFile] {
                for destination_trust in [Trust::Local, Trust::FromFile] {
                    let (mut s, _, destination) = movable_books();
                    s.active_mut().unwrap().trust = source_trust;
                    s.set_active(1);
                    s.active_mut().unwrap().trust = destination_trust;
                    s.set_active(0);
                    let target = if new { json!("new") } else { json!(destination) };
                    s.execute("sheet.move", json!({"to":0,"copy":copy,"targetWorkbook":target})).unwrap();
                    let expected = if new { Trust::Local } else { destination_trust }.receive(source_trust);
                    assert_eq!(s.active().unwrap().trust, expected, "copy={copy}, new={new}");
                    s.execute("edit.undo", json!({})).unwrap();
                    assert_eq!(s.active().unwrap().trust, expected, "undo must not upgrade trust");
                    s.execute("edit.redo", json!({})).unwrap();
                    assert_eq!(s.active().unwrap().trust, expected);
                }
            }
        }
    }
}

#[test]
fn moving_the_active_sheet_selects_a_visible_remaining_sheet() {
    use gridcraft_model::Visibility;
    for visibility in [Visibility::Hidden, Visibility::VeryHidden] {
        let (mut s, _, destination) = movable_books();
        s.execute("home.insertSheet", json!({"name":"Hidden","before":0})).unwrap();
        Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().visibility = visibility;
        s.execute("sheet.activate", json!({"sheet":"Source"})).unwrap();
        s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
        let source = &s.documents()[0].wb;
        assert_eq!(source.active().unwrap().name, "Keep");
        assert_eq!(source.active().unwrap().visibility, Visibility::Visible);
    }
}

#[test]
fn transfer_keeps_literal_chart_labels_and_rewrites_only_evaluated_names() {
    for label in ["Sales revenue (net)", "Sales!Units", "Missing!A1", "\"Keep!A1\"", "Source!A1", "=Source!A1"] {
        let (mut s, _, destination) = movable_books();
        s.execute("cell.set", json!({"cell":"A2","input":"43"})).unwrap();
        s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
        Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().charts[0].series[0].name = Some(label.into());
        s.set_active(1);
        s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
        s.set_active(0);
        s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
        let name = s.active().unwrap().wb.sheet(0).unwrap().charts[0].series[0].name.as_deref().unwrap();
        if label.contains("Source!A1") {
            assert!(name.contains("Source (2)"), "self-reference should follow renamed sheet: {name}");
        } else {
            assert_eq!(name, label);
        }
    }
}

#[test]
fn chart_name_dependencies_are_checked_in_both_transfer_directions() {
    use gridcraft_model::DefinedName;
    for (label, definition, allowed) in [("Title", "Source!A1", true), ("Title", "Keep!A1", false), ("Keep!A1", "", false), ("=Keep!A1", "", false)] {
        let (mut s, _, destination) = movable_books();
        s.execute("cell.set", json!({"cell":"A2","input":"43"})).unwrap();
        s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
        let wb = Arc::make_mut(&mut s.active_mut().unwrap().wb);
        if !definition.is_empty() {
            wb.names.push(DefinedName { name: "Title".into(), scope: None, formula: definition.into(), comment: String::new(), hidden: false });
        }
        wb.sheet_mut(0).unwrap().charts[0].series[0].name = Some(label.into());
        let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
        let result = s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination}));
        assert_eq!(result.is_ok(), allowed, "label={label}, definition={definition}");
        if allowed {
            assert!(s.active().unwrap().wb.names.iter().any(|n| n.name == "Title"));
        } else {
            for (d, wb) in s.documents().iter().zip(before) {
                assert!(Arc::ptr_eq(&d.wb, &wb));
            }
        }
    }
    let (mut s, _, destination) = movable_books();
    s.execute("sheet.activate", json!({"sheet":"Keep"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"2"})).unwrap();
    s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
    Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(1).unwrap().charts[0].series[0].name = Some("Source!Units".into());
    s.execute("sheet.activate", json!({"sheet":"Source"})).unwrap();
    s.execute("sheet.move", json!({"to":0,"targetWorkbook":destination})).unwrap();
    assert_eq!(s.documents()[0].wb.sheet(0).unwrap().charts[0].series[0].name.as_deref(), Some("Source!Units"));
}

#[test]
fn bubble_sizes_follow_a_renamed_copy_and_survive_xlsx() {
    let (mut s, _, destination) = movable_books();
    s.execute("cell.set", json!({"cell":"A2","input":"43"})).unwrap();
    s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
    let chart = &mut Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().charts[0];
    chart.kind = gridcraft_model::ChartKind::Bubble;
    chart.series[0].bubble_sizes = Some("Source!A1:A2".into());
    s.set_active(1);
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1","input":"999"})).unwrap();
    s.set_active(0);
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let wb = &s.active().unwrap().wb;
    let check = |wb: &gridcraft_model::Workbook| {
        let chart = &wb.sheet(0).unwrap().charts[0];
        let data = gridcraft_chart::resolve(wb, 0, chart);
        assert_eq!(data.series[0].sizes, Some(vec![Some(42.0), Some(43.0)]));
    };
    check(wb);
    let bytes = gridcraft_engine::io::save_bytes(wb, "Bubble.xlsx").unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("Bubble.xlsx", &bytes).unwrap();
    check(&reopened);
}

#[test]
fn bubble_size_dependencies_refuse_transfer_without_mutation() {
    for inbound in [false, true] {
        let (mut s, _, destination) = movable_books();
        let sheet = if inbound { "Keep" } else { "Source" };
        s.execute("sheet.activate", json!({"sheet":sheet})).unwrap();
        s.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
        s.execute("cell.set", json!({"cell":"A2","input":"2"})).unwrap();
        s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
        let chart = &mut Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(usize::from(inbound)).unwrap().charts[0];
        chart.kind = gridcraft_model::ChartKind::Bubble;
        chart.series[0].bubble_sizes = Some(if inbound { "Source!A1:A2" } else { "Keep!A1:A2" }.into());
        s.execute("sheet.activate", json!({"sheet":"Source"})).unwrap();
        let before: Vec<_> = s.documents().iter().map(|d| d.wb.clone()).collect();
        assert!(s.execute("sheet.move", json!({"to":0,"copy":!inbound,"targetWorkbook":destination})).is_err());
        for (d, wb) in s.documents().iter().zip(before) {
            assert!(Arc::ptr_eq(&d.wb, &wb));
        }
    }
}

#[test]
fn transferred_literal_labels_resist_destination_collisions_and_xlsx_roundtrip() {
    use gridcraft_model::DefinedName;
    for copy in [false, true] {
        for label in ["Missing!A1", "Title", "Missing!Title"] {
            let (mut s, _, destination) = movable_books();
            s.execute("cell.set", json!({"cell":"A2","input":"43"})).unwrap();
            s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
            Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().charts[0].series[0].name = Some(label.into());
            s.set_active(1);
            s.execute("sheet.rename", json!({"name":"Missing"})).unwrap();
            s.execute("cell.set", json!({"cell":"A1","input":"Different label"})).unwrap();
            let wb = Arc::make_mut(&mut s.active_mut().unwrap().wb);
            for scope in [None, Some(0)] {
                wb.names.push(DefinedName { name: "Title".into(), scope, formula: "Missing!A1".into(), comment: String::new(), hidden: false });
            }
            s.set_active(0);
            s.execute("sheet.move", json!({"to":0,"copy":copy,"targetWorkbook":destination})).unwrap();
            let wb = &s.active().unwrap().wb;
            let check = |wb: &gridcraft_model::Workbook| {
                let data = gridcraft_chart::resolve(wb, 0, &wb.sheet(0).unwrap().charts[0]);
                assert_eq!(data.series[0].name, label);
            };
            check(wb);
            let bytes = gridcraft_engine::io::save_bytes(wb, "Labels.xlsx").unwrap();
            let (reopened, _) = gridcraft_engine::io::open_bytes("Labels.xlsx", &bytes).unwrap();
            check(&reopened);
        }
    }
}

#[test]
fn adding_a_sheet_does_not_activate_existing_destination_chart_labels() {
    let (mut s, _, destination) = books();
    s.set_active(1);
    s.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2","input":"2"})).unwrap();
    s.execute("insert.chart", json!({"range":"A1:A2","type":"line"})).unwrap();
    Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().charts[0].series[0].name = Some("Source!A1".into());
    s.set_active(0);
    s.execute("sheet.move", json!({"to":0,"copy":true,"targetWorkbook":destination})).unwrap();
    let wb = &s.active().unwrap().wb;
    let data = gridcraft_chart::resolve(wb, 1, &wb.sheet(1).unwrap().charts[0]);
    assert_eq!(data.series[0].name, "Source!A1");
}
