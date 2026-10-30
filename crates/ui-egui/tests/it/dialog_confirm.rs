//! `ui.dialog.confirm` runs a dialog's command with the parameters its OK button sends (#1459).

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::model::CfRule;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(6)
}

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
fn confirming_a_highlight_cells_rule_adds_the_rule_like_ok() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [[1], [3]]})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:A2"})).unwrap();
    // Home › Conditional Formatting › Highlight Cells Rules › Greater Than…
    control(&mut h, "ui.dialog", json!({"name": "cfQuick", "type": "cellIs", "operator": "greaterThan", "title": "Greater Than"}));
    control(&mut h, "ui.dialog.set", json!({"field": "value", "value": "2"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().dialog.is_none(), "the dialog closed");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.cond_formats.len(), 1, "{:?}", sheet.cond_formats);
    let rule = &sheet.cond_formats[0].rule;
    assert!(matches!(rule, CfRule::CellIs { a, .. } if a == "2"), "{rule:?}");
}

#[test]
fn confirming_text_to_columns_splits_on_the_ticked_delimiter() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["a;b"], ["c;d"]]})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:A2"})).unwrap();
    control(&mut h, "ui.dialog", json!({"name": "textToColumns"}));
    control(&mut h, "ui.dialog.set", json!({"field": "semicolon", "value": true}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    let read = h.state_mut().run("sheet.read", json!({"range": "A1:B2"})).unwrap();
    assert_eq!(read["values"], json!([["a", "b"], ["c", "d"]]), "{read}");
}

/// Confirming the generic simulation dialog must create both charts without changing its inputs.
#[test]
fn monte_carlo_dialog_confirms_a_generic_model_and_preserves_inputs() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range":"A1","values":[[10]]})).unwrap();
    h.state_mut().run("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
    control(&mut h, "ui.dialog", json!({"name":"monteCarlo"}));
    control(&mut h, "ui.dialog.set", json!({"field":"inputs","value":[{"cell":"A1","distribution":"uniform","min":5,"max":5}]}));
    control(&mut h, "ui.dialog.set", json!({"field":"outputs","value":"B1"}));
    control(&mut h, "ui.dialog.set", json!({"field":"iterations","value":"16"}));
    let result = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(result["ok"], true, "{result}");
    assert!(h.state().dialog.is_none());
    h.run_steps(8); // cooperative preparation, trials and one report publication
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.wb.sheets.len(), 2);
    assert_eq!(d.wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).as_f64(), Some(10.0));
    assert_eq!(d.wb.sheet(1).unwrap().charts.len(), 2);
}

/// Enter and Shift+Enter belong to either multiline editor, while explicit OK still runs
/// the valid simulation. A valid fixture ensures accidental confirmation would be observable.
#[test]
fn monte_carlo_multiline_enter_inserts_a_line_without_confirming() {
    use egui::{Event, Key, Modifiers, accesskit::Role};
    for label in [
        "Correlation matrix in input order (blank = independent). Rows on separate lines, numbers separated by spaces. Gaussian latent correlations, not marginal Pearson targets.",
        "Output cells, one per line (e.g. Model!F30). Qualify references to other sheets.",
    ] {
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
            let mut h = harness();
            h.state_mut().run("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
            h.state_mut().run("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
            control(&mut h, "ui.dialog", json!({"name":"monteCarlo"}));
            control(&mut h, "ui.dialog.set", json!({"field":"outputs","value":"B1"}));
            control(&mut h, "ui.dialog.set", json!({"field":"correlation","value":"1"}));
            control(&mut h, "ui.dialog.set", json!({"field":"iterations","value":"16"}));
            h.run_steps(4);
            h.get_by_role_and_label(Role::MultilineTextInput, label).click();
            h.run_steps(2);
            for pressed in [true, false] {
                h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: Some(Key::Enter), pressed, repeat: false, modifiers });
            }
            h.run_steps(2);
            assert!(h.state().dialog.is_some(), "Enter must not close the editor: {label}");
            assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 1);
            assert!(h.get_by_role_and_label(Role::MultilineTextInput, label).value().is_some_and(|v| v.contains('\n')));
            h.get_by_label("   OK   ").click();
            h.run_steps(8);
            assert!(h.state().dialog.is_none());
            assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 2);
        }
    }
}

/// The real progress UI can cancel a running job without source edits, reports or extra undo.
#[test]
fn monte_carlo_progress_cancel_preserves_the_workbook() {
    let mut h = harness();
    h.state_mut().run("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
    h.state_mut().run("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
    let source = h.state().session.doc().unwrap().wb.clone();
    let undo = h.state().session.doc().unwrap().undo.len();
    control(&mut h, "ui.dialog", json!({"name":"monteCarlo"}));
    control(&mut h, "ui.dialog.set", json!({"field":"outputs","value":"B1"}));
    control(&mut h, "ui.dialog.set", json!({"field":"iterations","value":"50000"}));
    control(&mut h, "ui.dialog.confirm", json!({}));
    assert!(h.state().session.analysis_active());
    h.run_steps(4);
    h.get_by_label("Cancel simulation").click();
    h.run_steps(4);
    assert!(!h.state().session.analysis_active());
    let d = h.state().session.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&source, &d.wb));
    assert_eq!(d.undo.len(), undo);
    assert_eq!(d.wb.sheets.len(), 1);
}
