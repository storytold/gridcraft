//! #2876: editing a stored hyperlink starts from its address, display text and ScreenTip.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Cell, Hyperlink};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Runs the same dialog controls used by the public automation interface.
fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    let Outcome::Done(result) = handle(h.state_mut(), &ctx, method, &params) else { panic!("unexpected screenshot response") };
    assert_eq!(result["ok"], true, "{method}: {result}");
    result
}

/// A non-default active cell carrying a stored hyperlink and the requested input.
fn linked_harness(input: &str, target: &str) -> Harness {
    let mut session = crate::support::blank_session();
    session.execute("cell.set", json!({"cell": "B2", "input": input})).unwrap();
    session.execute("insert.link", json!({"cell": "B2", "target": target, "tooltip": "Original tip"})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    crate::support::harness(session, 4)
}

/// Captures both content and stored link metadata so cancellation and undo are exact.
fn linked_cell(h: &Harness) -> (Cell, Hyperlink) {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let at = CellRef::parse("B2").unwrap();
    (sheet.cells.get(at).cloned().unwrap(), sheet.hyperlinks.get(&at).cloned().unwrap())
}

/// Both relationship URLs and workbook locations prefill the same form.
#[test]
fn existing_external_and_internal_links_prefill_their_fields() {
    for target in ["https://example.org/original?q=1", "Sheet1!F9"] {
        let mut h = linked_harness("Original label", target);
        control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
        h.run_steps(3);
        let dialog = h.state().dialog.as_ref().unwrap();
        assert_eq!(dialog.values["target"], target);
        assert_eq!(dialog.values["text"], "Original label");
        assert_eq!(dialog.values["tooltip"], "Original tip");
        assert!(h.query_by_label("Display:").is_some());
        assert!(h.query_by_label("ScreenTip:").is_some());
    }
}

/// The keyboard entry point shares the prefilled form rather than resetting the address.
#[test]
fn ctrl_k_opens_the_existing_link_for_editing() {
    let mut h = linked_harness("Original label", "https://example.org/original");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    h.run_steps(3);
    let dialog = h.state().dialog.as_ref().unwrap();
    assert_eq!(dialog.name, "insertLink");
    assert_eq!(dialog.values["target"], "https://example.org/original");
}

/// Both the OK button and automation confirmation update one link and undo in one step.
#[test]
fn editing_a_link_confirms_and_undo_restores_it() {
    for button in [false, true] {
        let mut h = linked_harness("Original label", "https://example.org/original");
        let before = linked_cell(&h);
        control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
        control(&mut h, "ui.dialog.set", json!({"field": "target", "value": "Sheet1!F9"}));
        control(&mut h, "ui.dialog.set", json!({"field": "text", "value": "Updated label"}));
        control(&mut h, "ui.dialog.set", json!({"field": "tooltip", "value": "Updated tip"}));
        if button {
            h.run_steps(4);
            h.get_by_label("   OK   ").click();
            h.run_steps(4);
        } else {
            control(&mut h, "ui.dialog.confirm", json!({}));
        }
        assert!(h.state().dialog.is_none());
        let (cell, link) = linked_cell(&h);
        assert_eq!(cell.value.display(), "Updated label");
        assert_eq!(link.target, "Sheet1!F9");
        assert_eq!(link.tooltip.as_deref(), Some("Updated tip"));
        assert!(!link.external);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(linked_cell(&h), before);
    }
}

/// Closing a partially edited form leaves the existing link and cell untouched.
#[test]
fn cancelling_link_edits_keeps_the_original_cell_and_link() {
    let mut h = linked_harness("Original label", "Sheet1!F9");
    let before = linked_cell(&h);
    control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
    control(&mut h, "ui.dialog.set", json!({"field": "target", "value": "https://example.org/changed"}));
    control(&mut h, "ui.dialog.set", json!({"field": "text", "value": "Changed label"}));
    control(&mut h, "ui.dialog.set", json!({"field": "tooltip", "value": "Changed tip"}));
    control(&mut h, "ui.dialog.cancel", json!({}));
    assert!(h.state().dialog.is_none());
    assert_eq!(linked_cell(&h), before);
}

/// Rendered forms keep linked numbers, formulas and number formats when their display text is unchanged.
#[test]
fn unchanged_display_text_preserves_number_and_formula_cells() {
    for (input, format, expected) in [("42", "General", "42"), ("=6*7", "General", "42"), ("42", "0.00", "42.00"), ("=6*7", "0.00", "42.00")] {
        let mut h = linked_harness(input, "https://example.org/original");
        h.state_mut().run("home.numberFormat", json!({"code": format})).unwrap();
        let (before, _) = linked_cell(&h);
        let doc = h.state().session.active().unwrap();
        let display = gridcraft_engine::display::cell_text(&doc.wb, doc.wb.active().unwrap(), CellRef::parse("B2").unwrap());
        assert_eq!(display, expected, "formatted input {input}");
        control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
        h.run_steps(3);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["text"], display);
        control(&mut h, "ui.dialog.set", json!({"field": "target", "value": "https://example.org/updated"}));
        h.run_steps(3);
        control(&mut h, "ui.dialog.confirm", json!({}));
        let (after, link) = linked_cell(&h);
        assert_eq!(after, before, "input {input}");
        assert_eq!(link.target, "https://example.org/updated");
        assert_eq!(link.tooltip.as_deref(), Some("Original tip"));
        control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
        h.run_steps(3);
        h.get_by_label("   OK   ").click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        assert_eq!(linked_cell(&h).0, before, "unedited OK for {input}");
    }
}

/// A missing ScreenTip stays absent when only the address changes or OK is pressed unchanged.
#[test]
fn editing_a_link_without_a_tooltip_keeps_it_absent() {
    let mut h = linked_harness("Original label", "https://example.org/original");
    h.state_mut().run("insert.link", json!({"target": "https://example.org/original"})).unwrap();
    assert_eq!(linked_cell(&h).1.tooltip, None);
    control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
    h.run_steps(3);
    let dialog = h.state().dialog.as_ref().unwrap();
    assert_eq!(dialog.values.get("tooltip").and_then(Json::as_str).unwrap_or_default(), "");
    control(&mut h, "ui.dialog.set", json!({"field": "target", "value": "https://example.org/updated"}));
    h.run_steps(3);
    control(&mut h, "ui.dialog.confirm", json!({}));
    let (cell, link) = linked_cell(&h);
    assert_eq!(cell.value.display(), "Original label");
    assert_eq!(link.target, "https://example.org/updated");
    assert_eq!(link.tooltip, None);
    control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
    h.run_steps(3);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(linked_cell(&h).1.tooltip, None);
}

/// A new link still begins with the web-address default and uses its target as a blank cell's label.
#[test]
fn new_links_keep_the_existing_defaults() {
    let mut h = crate::support::blank_harness(4);
    control(&mut h, "ui.dialog", json!({"name": "insertLink"}));
    h.run_steps(3);
    let dialog = h.state().dialog.as_ref().unwrap();
    assert_eq!(dialog.values["target"], "https://");
    assert_eq!(dialog.values.get("text").and_then(Json::as_str).unwrap_or_default(), "");
    assert_eq!(dialog.values.get("tooltip").and_then(Json::as_str).unwrap_or_default(), "");
    control(&mut h, "ui.dialog.set", json!({"field": "target", "value": "https://example.org/new"}));
    h.run_steps(3);
    control(&mut h, "ui.dialog.confirm", json!({}));
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let at = CellRef::parse("A1").unwrap();
    assert_eq!(sheet.value(at).display(), "https://example.org/new");
    assert_eq!(sheet.hyperlinks[&at].target, "https://example.org/new");
}
