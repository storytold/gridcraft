//! Automate › Macros in the window: recording with the status bar's Stop, the Macros dialog
//! (macros, Automate scripts and a VBA project), macro shortcuts and the question asked when a
//! macro's output would land on data.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(tab: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.ribbon_tab = tab.into();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn value(h: &Harness, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))).unwrap_or_default()
}

fn save(h: &mut Harness, m: serde_json::Value) {
    h.state_mut().run("macro.save", json!({"macro": m})).unwrap();
    h.run_steps(2);
}

fn dialog_name(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name().to_string())
}

#[test]
fn record_from_the_ribbon_and_stop_from_the_status_bar() {
    let mut h = harness("Automate");
    h.get_by_label("Record Macro").click();
    h.run_steps(2);
    assert_eq!(dialog_name(&h).as_deref(), Some("recordMacro"), "Record Macro asks for a name first");
    assert!(!h.state().session.is_recording());
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().session.is_recording());
    assert!(h.query_by_label("Stop Recording").is_some(), "the button turns into Stop Recording");
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Recording · 1 step").is_some(), "the status bar counts the steps");
    h.get_by_label("Stop").click();
    h.run_steps(2);
    assert!(!h.state().session.is_recording());
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.macros.len(), 1);
    let name = wb.macros[0].name.clone();
    assert_eq!(name, "Macro1");
    assert_eq!(h.state().macros.recent, [name]);
    assert!(h.query_by_label_contains("Recording ·").is_none());
}

#[test]
fn use_relative_references_sets_how_the_next_recording_starts() {
    let mut h = harness("Automate");
    assert!(h.state().macros.relative, "relative is the default");
    h.get_by_label("Use Relative References").click();
    h.run_steps(2);
    assert!(!h.state().macros.relative);
    h.get_by_label("Record Macro").click();
    h.run_steps(5);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().session.macro_recording.as_ref().is_some_and(|r| !r.relative));
}

#[test]
fn the_macros_dialog_runs_the_selected_macro() {
    let mut h = harness("Automate");
    save(
        &mut h,
        json!({"name": "Fill", "output": {"onConflict": "overwrite"}, "steps": [{"command": "cell.set", "params": {"cell": "C3", "input": "done"}}]}),
    );
    save(&mut h, json!({"name": "Other", "steps": []}));
    h.get_by_label("Macros").click();
    h.run_steps(2);
    assert_eq!(dialog_name(&h).as_deref(), Some("macros"));
    h.get_by_label("Fill").click();
    h.run_steps(2);
    h.get_by_label("Run").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(value(&h, "C3"), Value::Text("done".into()));
    // One undo step undoes the whole run.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(value(&h, "C3"), Value::Empty);
}

#[test]
fn the_macros_dialog_lists_scripts_and_the_vba_project_beside_macros() {
    let mut s = Session::new();
    s.new_workbook();
    let mut wb = gridcraft_engine::model::Workbook::new();
    wb.vba_project = Some(vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    s.active_mut().unwrap().wb = std::sync::Arc::new(wb);
    s.execute("automate.newScript", json!({"name": "Mark", "commands": [{"command": "cell.set", "params": {"cell": "D4", "input": "script"}}]}))
        .unwrap();
    s.execute("macro.save", json!({"name": "Mark", "steps": []})).unwrap();
    let mut app = SheetApp::new(s, Default::default());
    app.ui.ribbon_tab = "Automate".into();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().open_dialog("macros", json!({}));
    h.run_steps(2);
    // The dialog keeps view.macros (scripts and VBA) as its result, as before.
    let items = h.state().dialog.as_ref().and_then(|d| d.result.clone()).and_then(|r| r.as_array().cloned()).unwrap_or_default();
    assert!(items.iter().any(|i| i["kind"] == "vba"), "{items:?}");
    assert!(h.query_by_label_contains("also has a VBA project").is_some());
    // A script and a macro may share a name; each has its own row.
    h.get_by_label("Mark    (script, 1 step)").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["kind"], json!("script"));
    h.get_by_label("Run").click();
    h.run_steps(3);
    assert_eq!(value(&h, "D4"), Value::Text("script".into()));
    // Deleting asks first, then removes the script and leaves the macro.
    h.state_mut().open_dialog("macros", json!({}));
    h.run_steps(2);
    h.get_by_label("Mark    (script, 1 step)").click();
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(2);
    assert!(h.state().macros.question.as_ref().is_some_and(|q| q.text.contains("script “Mark”")));
    // The question's Delete, in front of the dialog's.
    h.get_all_by_label("Delete").last().expect("the question's Delete").click();
    h.run_steps(3);
    assert!(h.query_by_label("Mark    (script, 1 step)").is_none());
    assert_eq!(h.state().session.active().unwrap().wb.macros.len(), 1);
}

#[test]
fn options_set_a_shortcut_that_runs_the_macro() {
    let mut h = harness("Home");
    save(
        &mut h,
        json!({"name": "Stamp", "output": {"onConflict": "overwrite"}, "steps": [{"command": "cell.set", "params": {"cell": "B2", "input": "stamped"}}]}),
    );
    assert!(gridcraft_ui_egui::macro_ui::save_options(h.state_mut(), "Stamp", Some('m'), "Stamps B2"));
    let m = h.state().session.active().unwrap().wb.macros[0].clone();
    assert_eq!((m.shortcut.as_deref(), m.description.as_str()), (Some("Ctrl+Shift+M"), "Stamps B2"));
    let mods = Modifiers::COMMAND | Modifiers::SHIFT;
    h.input_mut().events.push(Event::Key { key: Key::M, physical_key: None, pressed: true, repeat: false, modifiers: mods });
    h.input_mut().events.push(Event::Key { key: Key::M, physical_key: None, pressed: false, repeat: false, modifiers: mods });
    h.run_steps(3);
    assert_eq!(value(&h, "B2"), Value::Text("stamped".into()));
}

#[test]
fn a_shortcut_gridcraft_uses_does_not_run_a_macro() {
    let mut h = harness("Home");
    // A file can bring any letter; Ctrl+Shift+Z stays Redo.
    let m = gridcraft_engine::model::Macro { name: "Z".into(), shortcut: Some("Ctrl+Shift+Z".into()), ..Default::default() };
    std::sync::Arc::make_mut(&mut h.state_mut().session.active_mut().unwrap().wb).macros.push(m);
    assert_eq!(gridcraft_ui_egui::macro_ui::shortcut_macro(h.state(), Key::Z), None);
}

#[test]
fn a_macro_whose_output_lands_on_data_asks_and_can_shift_it_down() {
    let mut h = harness("Automate");
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "old"})).unwrap();
    save(&mut h, json!({"name": "Put", "steps": [{"command": "cell.set", "params": {"cell": "B1", "input": "new"}}]}));
    gridcraft_ui_egui::macro_ui::run_macro(h.state_mut(), "Put");
    h.run_steps(2);
    let q = h.state().macros.question.clone().expect("a question");
    assert_eq!(q.choices.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["Overwrite", "Shift Down", "Shift Right"]);
    assert!(q.text.contains("B1"), "{}", q.text);
    assert!(!q.text.contains("onConflict"), "the hint for programs is not shown: {}", q.text);
    h.get_by_label("Shift Down").click();
    h.run_steps(3);
    assert!(h.state().macros.question.is_none());
    assert_eq!(value(&h, "B1"), Value::Text("new".into()));
    assert_eq!(value(&h, "B2"), Value::Text("old".into()));
    assert_eq!(h.state().macros.recent, ["Put"]);
}

#[test]
fn the_record_macro_dialog_names_the_macro_and_checks_name_and_shortcut() {
    let mut h = harness("Automate");
    save(&mut h, json!({"name": "Taken", "shortcut": "Ctrl+Shift+P", "steps": []}));
    h.get_by_label("Record Macro").click();
    h.run_steps(2);
    let set = |h: &mut Harness, k: &str, v: &str| {
        h.state_mut().dialog.as_mut().unwrap().values.insert(k.into(), json!(v));
        h.run_steps(2);
    };
    assert_eq!(h.state().dialog.as_ref().unwrap().values["name"], json!("Macro1"), "the next free name is suggested");
    set(&mut h, "name", "taken");
    assert!(h.query_by_label_contains("already exists").is_some());
    set(&mut h, "name", "Two\nlines");
    assert!(h.query_by_label_contains("line breaks").is_some());
    set(&mut h, "name", "Tidy Up");
    set(&mut h, "shortcut", "P");
    assert!(h.query_by_label_contains("already runs macro “Taken”").is_some());
    set(&mut h, "shortcut", "S");
    assert!(h.query_by_label_contains("Save As").is_some());
    // OK stays off while something is wrong.
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(!h.state().session.is_recording());
    set(&mut h, "shortcut", "Q");
    set(&mut h, "description", "Tidies the block");
    assert!(h.query_by_label_contains("Choose another").is_none());
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().session.is_recording());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
    h.get_by_label("Stop").click();
    h.run_steps(2);
    let m = h.state().session.active().unwrap().wb.macros.iter().find(|m| m.name == "Tidy Up").cloned().expect("saved under its name");
    assert_eq!(m.shortcut.as_deref(), Some("Ctrl+Shift+Q"));
    assert_eq!(m.description, "Tidies the block");
}

#[test]
fn view_macros_record_macro_toggles_recording() {
    let mut h = harness("Home");
    h.state_mut().open_dialog("recordMacro", json!({}));
    h.run_steps(4);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().session.is_recording());
    // Opening it again while recording stops and saves, as Excel's Stop Recording does.
    h.state_mut().open_dialog("recordMacro", json!({}));
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert!(!h.state().session.is_recording());
}
