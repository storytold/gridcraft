//! Scenario Manager's visible workflow, using a synthetic workbook.

use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"B2","input":"10"})).unwrap();
    session.execute("cell.set", json!({"cell":"B3","input":"20"})).unwrap();
    session.execute("data.scenarioAdd", json!({"name":"Base","changing":"B2:B3"})).unwrap();
    session.execute("data.scenarioAdd", json!({"name":"Best","changing":"B2:B3","values":[100,3],"comment":"optimistic"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(5);
    h
}

fn value(h: &egui_kittest::Harness<'static, SheetApp>, at: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(at).unwrap())
}

#[test]
fn manager_shows_and_deletes_selected_scenario() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(5);
    h.get_by_label("Best").click();
    h.run_steps(3);
    h.get_by_label("Show").click();
    h.run_steps(3);
    assert_eq!(value(&h, "B2"), Value::Number(100.0));
    assert_eq!(value(&h, "B3"), Value::Number(3.0));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("scenarioManager"));
    h.get_by_label("Close").click();
    h.run_steps(3);
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().list_index, 1);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    let list = h.state_mut().run("data.scenarioManager", json!({})).unwrap();
    assert_eq!(list["scenarios"].as_array().unwrap().len(), 1);
}

#[test]
fn adding_scenario_opens_values_step_and_returns_to_manager() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(4);
    h.get_by_label("Add…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("scenarioAdd"));
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.values.insert("name".into(), json!("Worst"));
        d.values.insert("changing".into(), json!("B2,$B$3"));
    }
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(3);
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!(d.tab, "values");
    assert_eq!(d.values["cells"], json!(["B2", "B3"]));
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.values.insert("draftValues".into(), json!(["1", "2"]));
    }
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("scenarioManager"));
    h.state_mut().run("data.scenarioShow", json!({"name":"Worst"})).unwrap();
    assert_eq!(value(&h, "B2"), Value::Number(1.0));
    assert_eq!(value(&h, "B3"), Value::Number(2.0));
}

#[test]
fn editing_name_keeps_values_and_summary_creates_a_report() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(4);
    h.get_by_label("Best").click();
    h.run_steps(3);
    h.get_by_label("Edit…").click();
    h.run_steps(3);
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.values.insert("name".into(), json!("Upside"));
    }
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["draftValues"], json!(["100", "3"]));
    h.get_by_label("OK").click();
    h.run_steps(3);
    let list = h.state_mut().run("data.scenarioManager", json!({})).unwrap();
    assert_eq!(list["scenarios"].as_array().unwrap().len(), 2);
    assert_eq!(list["scenarios"][1]["name"], "Upside");
    h.state_mut().run("data.scenarioShow", json!({"name":"Upside"})).unwrap();
    assert_eq!(value(&h, "B2"), Value::Number(100.0));
    h.run_steps(3);
    h.get_by_label("Summary…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("scenarioSummary"));
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Scenario Summary");
}

#[test]
fn enter_advances_add_dialog_and_confirms_values() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioAdd", json!({"name":"Keyboard", "changing":"B2"}));
    h.run_steps(3);
    let enter = |pressed| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([enter(true), enter(false)]);
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().tab, "values");
    h.input_mut().events.extend([enter(true), enter(false)]);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let list = h.state_mut().run("data.scenarioManager", json!({})).unwrap();
    assert_eq!(list["scenarios"].as_array().unwrap().len(), 3);
}

#[test]
fn editing_cell_values_uses_general_number_text() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(3);
    h.get_by_label("Base").click();
    h.run_steps(3);
    h.get_by_label("Edit…").click();
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["draftValues"], json!(["10", "20"]));
    assert!(h.query_by_label("Scenario Values").is_some());
    h.get_by_label("Back").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().tab, "");
    assert!(h.query_by_label("Edit Scenario").is_some());
}

#[test]
fn german_scenario_manager_translates_controls_without_changing_document_text() {
    let mut h = harness();
    h.state_mut().ui.language = gridcraft_ui_egui::i18n::Language::De;
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(4);
    for label in ["Szenario-Manager", "Szenarien:", "Anzeigen", "Hinzufügen…", "Bearbeiten…", "Zusammenfassung…", "Base", "Best"] {
        assert!(h.query_by_label(label).is_some(), "missing {label}");
    }
    h.get_by_label("Bearbeiten…").click();
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert!(h.query_by_label("Szenariowerte").is_some());
    assert_eq!(h.state().dialog.as_ref().unwrap().values["draftValues"], json!(["10", "20"]));
}
