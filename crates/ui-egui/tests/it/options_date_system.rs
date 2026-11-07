//! File › Options › Formulas: "Use 1904 date system" switches the active workbook's date system
//! through `formulas.calculationOptions` (#3538).

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, DateSystem};
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

fn options_page() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "2026-01-01"})).unwrap();
    session.execute("cell.set", json!({"cell": "A2", "input": "=YEAR(A1)"})).unwrap();
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
    h
}

fn system(h: &egui_kittest::Harness<'static, SheetApp>) -> DateSystem {
    h.state().session.active().unwrap().wb.date_system
}

fn value(h: &egui_kittest::Harness<'static, SheetApp>, a: &str) -> f64 {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap()).as_f64().unwrap_or(f64::NAN)
}

fn checked(h: &egui_kittest::Harness<'static, SheetApp>, label: &str) -> bool {
    h.get_by_role_and_label(Role::CheckBox, label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_checkbox_switches_the_date_system_and_follows_undo() {
    let mut h = options_page();
    assert_eq!(system(&h), DateSystem::D1900);
    assert!(!checked(&h, "Use 1904 date system"));
    h.get_by_role_and_label(Role::CheckBox, "Use 1904 date system").click();
    h.run_steps(3);
    assert_eq!(system(&h), DateSystem::D1904);
    // The typed date keeps its number; the formula is recalculated in the new system.
    assert_eq!((value(&h, "A1"), value(&h, "A2")), (46023.0, 2030.0));
    assert!(checked(&h, "Use 1904 date system"));
    // Undo switches back, and the checkbox follows.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(system(&h), DateSystem::D1900);
    assert_eq!(value(&h, "A2"), 2026.0);
    assert!(!checked(&h, "Use 1904 date system"));
}

#[test]
fn the_checkbox_is_translated() {
    let mut h = options_page();
    h.state_mut().run("app.language.set", json!({"language": Language::De.code()})).unwrap();
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    let label = Language::De.tr("Use 1904 date system");
    assert_ne!(label, "Use 1904 date system");
    h.get_by_role_and_label(Role::CheckBox, label).click();
    h.run_steps(3);
    assert_eq!(system(&h), DateSystem::D1904);
}
