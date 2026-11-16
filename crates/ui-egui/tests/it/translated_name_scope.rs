//! Define Name's scope box shows the workbook keyword in the interface language, so a Turkish user
//! is not asked to type "Workbook"; OK still defines a workbook-level name.

use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use gridcraft_ui_egui::i18n::Language;
use serde_json::{Value as Json, json};

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
fn define_name_shows_and_accepts_the_workbook_scope_in_the_interface_language() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("defineName", json!({}));
        h.run_steps(4);
        let shown = h.state().dialog.as_ref().unwrap().values["scope"].clone();
        assert_eq!(shown, json!(lang.tr("Workbook")), "{lang:?}");
        control(&mut h, "ui.dialog.set", json!({"field": "name", "value": "Price"}));
        let r = control(&mut h, "ui.dialog.confirm", json!({}));
        assert_eq!(r["ok"], json!(true), "{lang:?}: {r}");
        let wb = &h.state().session.active().unwrap().wb;
        let name = wb.names.iter().find(|n| n.name == "Price").expect("the name is defined");
        assert_eq!(name.scope, None, "{lang:?}: a workbook-level name");
    }
}
