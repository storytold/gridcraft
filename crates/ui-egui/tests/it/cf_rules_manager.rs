//! Conditional Formatting Rules Manager ▸ Applies to: a range the rule can't take is reported and
//! stays in the box to be corrected, and OK doesn't close the manager over it.

use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.conditionalFormat", json!({"range": "A1:A3", "rule": {"type": "blanks", "fill": "#FFC7CE"}})).unwrap();
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().open_dialog("manageRules", json!({}));
    h.run_steps(6);
    h
}

/// Types `text` as the pending Applies to edit of the selected rule.
fn edit_applies_to(h: &mut Harness, text: &str) {
    h.state_mut().dialog.as_mut().unwrap().values.insert("appliesTo".into(), json!(text));
    h.run_steps(2);
}

fn pending(h: &Harness) -> Option<Json> {
    h.state().dialog.as_ref().and_then(|d| d.values.get("appliesTo").cloned())
}

fn ranges(h: &Harness) -> Vec<String> {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    sheet.cond_formats[0].ranges.iter().map(|r| r.a1()).collect()
}

#[test]
fn a_refused_range_stays_in_the_box() {
    let mut h = harness();
    edit_applies_to(&mut h, "=nope!");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert!(h.state().message.is_some(), "the refusal is reported");
    assert_eq!(pending(&h), Some(json!("=nope!")), "the typed text is kept to be corrected");
    assert_eq!(ranges(&h), ["A1:A3"]);
}

#[test]
fn ok_over_a_refused_range_keeps_the_manager_open() {
    let mut h = harness();
    edit_applies_to(&mut h, "=nope!");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.as_ref().is_some_and(|d| d.name == "manageRules"), "the manager stays open");
    assert!(h.state().message.is_some(), "the refusal is reported");
    assert_eq!(pending(&h), Some(json!("=nope!")));
}

#[test]
fn ok_applies_a_pending_range_and_closes() {
    let mut h = harness();
    edit_applies_to(&mut h, "=$A$1:$A$5");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "OK closed the manager");
    assert_eq!(ranges(&h), ["A1:A5"], "the edit was kept, not reverted");
}
