//! Text fields over workbook values (chart titles, table name) keep what the user types until they commit it.

use egui::{Event, Key, Modifiers, accesskit::Role, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
    session
}

/// Clicks the field, replaces its text with `text` and presses Enter.
fn retype(h: &mut Harness, field: egui::Rect, text: &str) {
    h.input_mut().events.push(Event::PointerMoved(field.center()));
    h.run_steps(1);
    h.get_all_by_role(Role::TextInput).find(|n| n.rect() == field).unwrap().click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    // One keystroke per frame, as a person types: each must survive the next frame.
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(2);
    }
    h.key_press(Key::Enter);
    h.run_steps(2);
}

#[test]
fn typing_a_chart_title_in_format_chart_area_sets_it() {
    let mut session = session();
    session.execute("selection.set", json!({"range": "A1:B3"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "D3"})).unwrap();
    let id = chart["chart"].as_u64().unwrap() as u32;
    let mut h = harness(session);
    h.state_mut().selected_chart = Some(id);
    h.state_mut().grid.pane = Some("formatChart".into());
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Chart Title").click();
    h.run_steps(2);
    // The pane sits on the right; the formula bar and name box are further left.
    let field = h.get_all_by_role(Role::TextInput).map(|n| n.rect()).find(|r| r.left() > 850.0).unwrap();
    retype(&mut h, field, "Sales");
    let title = h.state().session.active().unwrap().wb.active().unwrap().charts.iter().find(|c| c.id == id).unwrap().title.clone();
    assert_eq!(title.as_deref(), Some("Sales"));
}

#[test]
fn typing_a_table_name_on_the_table_design_tab_renames_the_table() {
    let mut session = session();
    session.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
    session.execute("selection.set", json!({"range": "A2"})).unwrap();
    let mut h = harness(session);
    h.state_mut().ui.ribbon_tab = "Table Design".into();
    h.run_steps(2);
    let label = h.get_by_label("Table Name:").rect();
    let below_label = |r: &egui::Rect| r.top() >= label.bottom() - 2.0 && (r.left() - label.left()).abs() < 20.0;
    let field = h.get_all_by_role(Role::TextInput).map(|n| n.rect()).find(below_label).unwrap();
    retype(&mut h, field, "Sales");
    let name = h.state().session.active().unwrap().wb.active().unwrap().tables[0].name.clone();
    assert_eq!(name, "Sales");
}
