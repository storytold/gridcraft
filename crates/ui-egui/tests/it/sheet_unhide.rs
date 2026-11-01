//! The hidden-sheet picker stays readable as its list grows.
use egui::Shape;
use egui_kittest::SnapshotResults;
use gridcraft_engine::Session;
use serde_json::json;

fn picker(sheet_count: usize) -> crate::common::Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("sheet.rename", json!({"sheet": 0, "name": "Hidden 000"})).unwrap();
    for index in 1..sheet_count {
        session.execute("home.insertSheet", json!({"name": format!("Hidden {index:03}")})).unwrap();
    }
    session.execute("home.insertSheet", json!({"name": "Visible"})).unwrap();
    for index in 0..sheet_count {
        session.execute("sheet.hide", json!({"sheet": format!("Hidden {index:03}")})).unwrap();
    }
    let mut app = crate::common::app(session);
    app.run("sheet.unhide", json!({})).unwrap();
    crate::common::harness_with(crate::common::builder().with_size(egui::vec2(900.0, 700.0)), app, 8)
}

fn has_text(h: &crate::common::Harness, text: &str) -> bool {
    h.output().shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(t) if t.galley.job.text.trim() == text))
}

#[test]
fn picker_layout_snapshots_for_one_ten_and_one_hundred_hidden_sheets() {
    let mut snapshots = SnapshotResults::new();
    for (count, last) in [(1, "Hidden 000"), (10, "Hidden 009"), (100, "Hidden 099")] {
        let mut h = picker(count);
        assert!(has_text(&h, "Search sheets…"), "{count} hidden sheets: search is visible");
        assert!(has_text(&h, "Hidden 000"), "{count} hidden sheets: first item is visible");
        if count < 10 {
            assert!(has_text(&h, last), "{count} hidden sheets: all items fit in the list");
        }
        let painted_rows =
            h.output().shapes.iter().filter(|shape| matches!(&shape.shape, Shape::Text(t) if t.galley.job.text.starts_with("Hidden "))).count();
        assert!(painted_rows <= 12, "{count} hidden sheets: picker bounds rendered rows to its scroll viewport");
        assert!(has_text(&h, "OK"), "{count} hidden sheets: confirmation stays visible");
        assert!(has_text(&h, "Cancel"), "{count} hidden sheets: cancel stays visible");
        h.snapshot(format!("unhide_sheet_{count}"));
        snapshots.extend_harness(&mut h);
    }
    let errors = snapshots.into_inner();
    assert!(errors.is_empty(), "snapshot comparisons failed: {errors:?}");
}

#[test]
fn very_hidden_sheets_are_not_offered_by_the_picker() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.insertSheet", json!({"name": "Secret"})).unwrap();
    session.execute("home.insertSheet", json!({"name": "Ordinary"})).unwrap();
    let wb = std::sync::Arc::make_mut(&mut session.active_mut().unwrap().wb);
    std::sync::Arc::make_mut(wb.sheets.get_mut(1).unwrap()).visibility = gridcraft_model::Visibility::VeryHidden;
    session.execute("sheet.hide", json!({"sheet": "Ordinary"})).unwrap();
    let mut app = crate::common::app(session);
    app.run("sheet.unhide", json!({})).unwrap();
    let dialog = app.dialog.as_ref().expect("the ordinary hidden sheet opens the picker");
    assert!(
        dialog.fields.iter().any(|field| matches!(field, gridcraft_ui_egui::dialogs::Field::SheetPicker { options, .. } if options == &["Ordinary"])),
        "VeryHidden sheets are omitted from picker options"
    );
}

#[test]
fn search_and_arrow_navigation_reach_late_sheets() {
    let mut h = picker(100);
    h.input_mut().events.push(egui::Event::Text("Hidden 099".into()));
    h.step();
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("sheet")).and_then(serde_json::Value::as_str), Some("Hidden 099"));

    h.state_mut().dialog.as_mut().unwrap().search.clear();
    h.run_steps(2);
    for _ in 0..99 {
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: Some(egui::Key::ArrowDown),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("sheet")).and_then(serde_json::Value::as_str), Some("Hidden 099"));
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: Some(egui::Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    assert!(h.state().dialog.is_none(), "Enter confirms the selected sheet");
    assert_eq!(
        h.state().session.active().unwrap().wb.sheets.iter().find(|sheet| sheet.name == "Hidden 099").unwrap().visibility,
        gridcraft_model::Visibility::Visible
    );
}

#[test]
fn escape_cancels_without_unhiding_the_selected_sheet() {
    let mut h = picker(1);
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    assert!(h.state().dialog.is_none(), "Escape closes the picker");
    assert_eq!(
        h.state().session.active().unwrap().wb.sheets.iter().find(|sheet| sheet.name == "Hidden 000").unwrap().visibility,
        gridcraft_model::Visibility::Hidden
    );
}
