//! File › Options › Formulas: Recalculate workbook before saving, under the calculation modes and
//! live only while Manual is chosen (#3539).

use egui::accesskit::{Role, Toggled};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const LABEL: &str = "Recalculate workbook before saving";

/// (checked, disabled) of the checkbox labelled `label`.
fn checkbox(h: &Harness, label: &str) -> (bool, bool) {
    let node = h.get_by_role_and_label(Role::CheckBox, label);
    let node = node.accesskit_node();
    (node.toggled() == Some(Toggled::True), node.is_disabled())
}

fn setting(h: &Harness) -> bool {
    h.state().session.active().unwrap().wb.calc.recalculate_before_save
}

#[test]
fn the_checkbox_is_live_only_in_manual_mode_and_sets_the_workbook_setting() {
    let mut session = Session::new();
    session.new_workbook();
    // Tall enough that the Formulas section is on screen without scrolling.
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, Services::default())),
    );
    h.run_steps(4);
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    // On by default; greyed out while the workbook calculates automatically.
    assert_eq!(checkbox(&h, LABEL), (true, true));
    h.state_mut().run("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    h.run_steps(3);
    assert_eq!(checkbox(&h, LABEL), (true, false));
    h.get_by_role_and_label(Role::CheckBox, LABEL).click();
    h.run_steps(3);
    assert!(!setting(&h));
    assert_eq!(checkbox(&h, LABEL), (false, false));
    // An undo step of its own, as the other calculation settings are.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert!(setting(&h));
    assert_eq!(checkbox(&h, LABEL), (true, false));
    // Back in automatic mode the checkbox keeps its state, greyed out.
    h.state_mut().run("formulas.calculationOptions", json!({"mode": "automatic"})).unwrap();
    h.run_steps(3);
    assert_eq!(checkbox(&h, LABEL), (true, true));
    // German names it as Excel's German interface does.
    h.state_mut().run("app.language.set", json!({"language": Language::De.code()})).unwrap();
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(checkbox(&h, "Arbeitsmappe vor dem Speichern neu berechnen"), (true, true));
}
