//! Function Arguments (Shift+F3, fx): one box per argument, live values, repeating arguments,
//! the formula built from the boxes, and editing the call at the caret or in the active cell.

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [[1, "one"], [2, "two"], [3, "three"]]})).unwrap();
    session.execute("selection.set", json!({"cell": "D1"})).unwrap();
    let app = crate::common::app(session);
    crate::common::harness_with(crate::common::builder().with_os(OperatingSystem::Nix).with_size(vec2(1280.0, 800.0)), app, 4)
}

fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn input(h: &Harness, cell: &str) -> String {
    h.state().session.active().unwrap().wb.active().unwrap().input_text(CellRef::parse(cell).unwrap())
}

fn dialog_name(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name().to_string())
}

fn value(h: &Harness, key: &str) -> Json {
    h.state().dialog.as_ref().and_then(|d| d.values.get(key).cloned()).unwrap_or(Json::Null)
}

fn set_args(h: &mut Harness, args: &[&str]) {
    h.state_mut().dialog.as_mut().unwrap().values.insert("args".into(), json!(args));
    h.run_steps(3);
}

fn ok(h: &mut Harness) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, "   OK   ").click();
    h.run_steps(3);
}

/// Opens Function Arguments for a new call of `name`, as Insert Function's OK does.
fn new_call(h: &mut Harness, name: &str) {
    h.state_mut().open_dialog("functionArguments", json!({"name": name}));
    h.run_steps(4);
    assert_eq!(dialog_name(h).as_deref(), Some("functionArguments"));
}

fn painted(h: &Harness) -> Vec<String> {
    h.output().shapes.iter().filter_map(|c| if let egui::Shape::Text(t) = &c.shape { Some(t.galley.text().to_string()) } else { None }).collect()
}

#[test]
fn insert_function_ok_goes_on_to_the_arguments_and_sum_grows_a_box_per_number() {
    let mut h = harness();
    h.state_mut().open_dialog("insertFunction", json!({}));
    h.run_steps(3);
    assert_eq!(dialog_name(&h).as_deref(), Some("insertFunction"), "a blank cell has no call to edit");
    h.state_mut().dialog.as_mut().unwrap().search = "sum".into();
    h.run_steps(2);
    h.get_by_label("SUM (Math & Trig)").click();
    h.run_steps(2);
    ok(&mut h);
    assert_eq!(dialog_name(&h).as_deref(), Some("functionArguments"));
    assert_eq!(editor_text(&h).as_deref(), Some("=SUM()"));
    assert!(h.query_by_label("Number1").is_some() && h.query_by_label("Number2").is_some());
    assert!(h.query_by_label("Number3").is_none());
    // The first box has the keyboard: typing fills it, and the cell shows the formula as built.
    h.input_mut().events.push(Event::Text("A1:A3".into()));
    h.run_steps(3);
    assert_eq!(editor_text(&h).as_deref(), Some("=SUM(A1:A3)"));
    set_args(&mut h, &["A1:A3", "10"]);
    assert!(h.query_by_label("Number3").is_some(), "a filled Number2 offers Number3");
    let shown = painted(&h);
    for want in ["= {1;2;3}", "= 10", "= 16", "Formula result = 16"] {
        assert!(shown.iter().any(|t| t == want), "{want} in {shown:?}");
    }
    assert!(shown.iter().any(|t| t.contains("expression to add")), "the focused argument is described: {shown:?}");
    ok(&mut h);
    assert_eq!(dialog_name(&h), None);
    assert_eq!(editor_text(&h), None, "OK enters the formula");
    assert_eq!(input(&h, "D1"), "=SUM(A1:A3,10)");
}

#[test]
fn if_and_vlookup_are_built_from_their_boxes_and_preview_what_the_engine_computes() {
    let mut h = harness();
    new_call(&mut h, "IF");
    assert!(h.query_by_label("Logical_test").is_some() && h.query_by_label("Value_if_false").is_some());
    set_args(&mut h, &["A2>1", "", "\"small\""]);
    assert_eq!(editor_text(&h).as_deref(), Some("=IF(A2>1,,\"small\")"), "a blank box between filled ones stays an empty argument");
    set_args(&mut h, &["A2>1", "\"big\"", "\"small\""]);
    ok(&mut h);
    assert_eq!(input(&h, "D1"), "=IF(A2>1,\"big\",\"small\")");

    h.state_mut().run("selection.set", json!({"cell": "D2"})).unwrap();
    new_call(&mut h, "VLOOKUP");
    set_args(&mut h, &["2", "A1:B3", "2", "FALSE"]);
    let engine = h.state_mut().run("formulas.evaluate", json!({"formula": "=VLOOKUP(2,A1:B3,2,FALSE)", "cell": "D2"})).unwrap();
    assert_eq!(engine, json!("two"));
    let shown = painted(&h);
    assert!(shown.iter().any(|t| t == "= \"two\""), "{shown:?}");
    assert!(shown.iter().any(|t| t == "Formula result = \"two\""), "{shown:?}");
    // A box left out at the end is not written.
    set_args(&mut h, &["2", "A1:B3", "2", ""]);
    assert_eq!(editor_text(&h).as_deref(), Some("=VLOOKUP(2,A1:B3,2)"));
    set_args(&mut h, &["2", "A1:B3", "2", "FALSE"]);
    ok(&mut h);
    assert_eq!(input(&h, "D2"), "=VLOOKUP(2,A1:B3,2,FALSE)");
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.wb.active().unwrap().value(CellRef::parse("D2").unwrap()).display(), "two");
}

#[test]
fn shift_f3_in_an_edit_opens_the_call_at_the_caret_with_its_arguments() {
    let mut h = harness();
    h.state_mut().begin_edit(Some("=1+SUM(A1,A2)*2".into()), false);
    h.run_steps(3);
    {
        let ed = h.state_mut().editor.as_mut().unwrap();
        ed.caret = 8;
        ed.anchor = 8;
        ed.sync_caret = true;
    }
    h.run_steps(3);
    let shift_f3 = |pressed| Event::Key { key: Key::F3, physical_key: None, pressed, repeat: false, modifiers: Modifiers::SHIFT };
    h.input_mut().events.extend([shift_f3(true), shift_f3(false)]);
    h.run_steps(4);
    assert_eq!(dialog_name(&h).as_deref(), Some("functionArguments"));
    assert_eq!(value(&h, "function"), json!("SUM"));
    assert_eq!(value(&h, "before"), json!("=1+"));
    assert_eq!(value(&h, "after"), json!("*2"));
    assert_eq!(value(&h, "args").as_array().unwrap()[..2], [json!("A1"), json!("A2")]);
    set_args(&mut h, &["A1", "A3"]);
    assert_eq!(editor_text(&h).as_deref(), Some("=1+SUM(A1,A3)*2"));
    // Cancel puts the edit back as it was.
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(dialog_name(&h), None);
    assert_eq!(editor_text(&h).as_deref(), Some("=1+SUM(A1,A2)*2"));
}

#[test]
fn fx_on_a_formula_cell_edits_its_first_call_and_cancel_leaves_the_cell() {
    let mut h = harness();
    h.state_mut().run("cell.set", json!({"cell": "D1", "input": "=ROUND(SUM(A1:A3)/7,2)"})).unwrap();
    h.run_steps(2);
    h.state_mut().open_dialog("insertFunction", json!({}));
    h.run_steps(4);
    assert_eq!(dialog_name(&h).as_deref(), Some("functionArguments"));
    assert_eq!(value(&h, "function"), json!("ROUND"));
    assert_eq!(value(&h, "args"), json!(["SUM(A1:A3)/7", "2"]));
    assert!(painted(&h).iter().any(|t| t == "= 0.86"), "{:?}", painted(&h));
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(editor_text(&h), None, "Cancel ends the edit the dialog started");
    assert_eq!(input(&h, "D1"), "=ROUND(SUM(A1:A3)/7,2)");
    // A Function Library menu's Insert Function… still lists the functions.
    h.state_mut().open_dialog("insertFunction", json!({"category": "Math & Trig"}));
    h.run_steps(3);
    assert_eq!(dialog_name(&h).as_deref(), Some("insertFunction"));
}
