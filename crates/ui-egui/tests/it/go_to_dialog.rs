//! The Go To dialog lists the named ranges of every sheet and the references entered before; a
//! double-click on an entry goes there.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.insertSheet", json!({"name": "Sheet2"})).unwrap();
    session.execute("selection.set", json!({"sheet": 0})).unwrap();
    for (name, refers, scope) in [("Price", "=Sheet1!$B$2:$B$5", "Workbook"), ("Local", "=Sheet2!$C$3", "Sheet2"), ("Rate", "=0.07", "Workbook")] {
        session.execute("formulas.defineName", json!({"name": name, "refersTo": refers, "scope": scope})).unwrap();
    }
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    let out = match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    };
    h.run_steps(2);
    out
}

fn button(h: &mut Harness, pos: Pos2, pressed: bool) {
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.step();
}

fn double_click(h: &mut Harness, label: &str) {
    let pos = h.get_by_label(label).rect().center();
    button(h, pos, true);
    button(h, pos, false);
    button(h, pos, true);
    button(h, pos, false);
    h.run_steps(2);
}

fn place(h: &Harness) -> (usize, String) {
    let d = h.state().session.active().unwrap();
    (d.wb.active_sheet, d.selection.a1())
}

#[test]
fn double_clicking_a_name_scoped_to_another_sheet_goes_there() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "goTo"}));
    assert!(h.query_by_label("Price").is_some(), "a workbook range name is listed");
    assert!(h.query_by_label("Rate").is_none(), "a constant is not a named range");
    double_click(&mut h, "Sheet2!Local");
    assert!(h.state().dialog.is_none(), "the dialog closed");
    assert_eq!(place(&h), (1, "C3".to_string()));
}

#[test]
fn a_reference_entered_is_listed_and_a_double_click_returns_to_it() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "goTo"}));
    control(&mut h, "ui.dialog.set", json!({"field": "reference", "value": "B14:C22,F19:G30"}));
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "Enter went");
    assert_eq!(place(&h), (0, "B14:C22,F19:G30".to_string()));

    control(&mut h, "selection.set", json!({"sheet": 1}));
    control(&mut h, "ui.dialog", json!({"name": "goTo"}));
    double_click(&mut h, "Sheet1!B14:C22,Sheet1!F19:G30");
    assert!(h.state().dialog.is_none(), "the dialog closed");
    assert_eq!(place(&h), (0, "B14:C22,F19:G30".to_string()));
}
