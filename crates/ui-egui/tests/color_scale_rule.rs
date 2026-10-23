//! New Formatting Rule: the 2-/3-colour scale branch and in-place editing.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::model::{CfRule, CfValueKind, Color};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.run("range.setValues", json!({"range": "A1:A3", "values": [[1], [2], [3]]})).unwrap();
    session.run("selection.set", json!({"range": "A1:A3"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn stops(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<(CfValueKind, Color)> {
    let doc = h.state().session.active().unwrap();
    let rule = &doc.wb.active().unwrap().cond_formats[0].rule;
    match rule {
        CfRule::ColorScale { stops } => stops.clone(),
        _ => Vec::new(),
    }
}

#[test]
fn creates_a_three_colour_scale_with_per_stop_thresholds() {
    let mut h = harness();
    h.state_mut().run("home.conditionalFormat", json!({})).unwrap();
    h.run_steps(6);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("newFormattingRule"));

    h.state_mut().dialog.as_mut().unwrap().values.insert(
        "stops".into(),
        json!([
            {"type": "min", "color": "#112233"},
            {"type": "percentile", "value": 90, "color": "#445566"},
            {"type": "max", "color": "#778899"},
        ]),
    );
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(
        stops(&h),
        vec![
            (CfValueKind::Min, Color::Rgb(0x112233)),
            (CfValueKind::Percentile(90.0), Color::Rgb(0x445566)),
            (CfValueKind::Max, Color::Rgb(0x778899)),
        ]
    );
}

#[test]
fn edits_an_existing_scale_in_place() {
    let mut h = harness();
    h.state_mut()
        .run(
            "home.conditionalFormat",
            json!({"rule": {"type": "colorScale", "stops": [
                {"type": "min", "color": "#112233"},
                {"type": "percentile", "value": 50, "color": "#445566"},
                {"type": "max", "color": "#778899"},
            ]}}),
        )
        .unwrap();
    let rule = serde_json::to_value(&h.state().session.active().unwrap().wb.active().unwrap().cond_formats[0].rule).unwrap();
    h.state_mut().open_dialog("newFormattingRule", json!({"edit": 0, "rule": rule}));
    h.run_steps(6);
    h.state_mut().dialog.as_mut().unwrap().values.insert("style".into(), json!("2color"));
    h.state_mut().dialog.as_mut().unwrap().values.insert(
        "stops".into(),
        json!([
            {"type": "formula", "value": "$B$1", "color": "#ABCABC"},
            {"type": "max", "color": "#DEFDEF"},
        ]),
    );
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(stops(&h), vec![(CfValueKind::Formula("$B$1".into()), Color::Rgb(0xABCABC)), (CfValueKind::Max, Color::Rgb(0xDEFDEF))]);
}

#[test]
fn new_rule_still_creates_a_formula_rule() {
    let mut h = harness();
    h.state_mut().open_dialog("newFormattingRule", json!({}));
    h.run_steps(6);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["ruleType"], json!("allCells"));

    h.get_by_label("Use a formula to determine which cells to format").click();
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["ruleType"], json!("formula"));
    h.state_mut().dialog.as_mut().unwrap().values.insert("formula".into(), json!("=A1>2"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    let rule = &h.state().session.active().unwrap().wb.active().unwrap().cond_formats[0].rule;
    let CfRule::Expression { formula, .. } = rule else { panic!("expected a formula rule, got {rule:?}") };
    assert_eq!(formula, "A1>2");
}

#[test]
fn the_editor_widgets_drive_the_scale() {
    let mut h = harness();
    h.state_mut().open_dialog("newFormattingRule", json!({}));
    h.run_steps(6);

    // Format Style: 3 -> 2 -> 3 through the combo itself.
    h.get_all_by_role(egui::accesskit::Role::ComboBox).next().expect("format style").click();
    h.run_steps(2);
    h.get_by_label("2-Color Scale").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["stops"].as_array().unwrap().len(), 2);
    h.get_all_by_role(egui::accesskit::Role::ComboBox).next().expect("format style").click();
    h.run_steps(2);
    h.get_by_label("3-Color Scale").click();
    h.run_steps(2);

    // Midpoint: Percentile -> Number, then type a new threshold.
    h.get_all_by_role(egui::accesskit::Role::ComboBox).nth(2).expect("midpoint type").click();
    h.run_steps(2);
    h.get_by_label("Number").click();
    h.run_steps(2);
    h.get_all_by_role(egui::accesskit::Role::SpinButton).next().expect("midpoint threshold").click();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.key_press(egui::Key::Backspace);
    h.get_all_by_role(egui::accesskit::Role::SpinButton).next().expect("midpoint threshold").type_text("75");
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["stops"][1]["value"], json!(75.0));

    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert_eq!(
        stops(&h),
        vec![(CfValueKind::Min, Color::Rgb(0xF8696B)), (CfValueKind::Number(75.0), Color::Rgb(0xFFEB84)), (CfValueKind::Max, Color::Rgb(0x63BE7B)),]
    );
}

#[test]
fn switching_three_to_two_to_three_uses_the_default_midpoint() {
    let mut h = harness();
    h.state_mut()
        .run(
            "home.conditionalFormat",
            json!({"rule": {"type": "colorScale", "stops": [
                {"type": "min", "color": "#112233"},
                {"type": "percentile", "value": 50, "color": "#445566"},
                {"type": "max", "color": "#778899"},
            ]}}),
        )
        .unwrap();
    let rule = serde_json::to_value(&h.state().session.active().unwrap().wb.active().unwrap().cond_formats[0].rule).unwrap();
    h.state_mut().open_dialog("newFormattingRule", json!({"edit": 0, "rule": rule}));
    h.run_steps(6);

    // The style combo drives these transitions in `new_formatting_rule`.
    h.state_mut().dialog.as_mut().unwrap().values.insert("style".into(), json!("2color"));
    h.run_steps(2);
    let two = h.state().dialog.as_ref().unwrap().values["stops"].as_array().unwrap().clone();
    assert_eq!(two.len(), 2);
    assert_eq!(two[0]["color"], json!("#112233"), "dropping the midpoint keeps the minimum's colour");
    assert_eq!(two[1]["color"], json!("#778899"), "dropping the midpoint keeps the maximum's colour");

    h.state_mut().dialog.as_mut().unwrap().values.insert("style".into(), json!("3color"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(
        stops(&h),
        vec![
            (CfValueKind::Min, Color::Rgb(0x112233)),
            (CfValueKind::Percentile(50.0), Color::Rgb(0xFFEB84)),
            (CfValueKind::Max, Color::Rgb(0x778899)),
        ]
    );
}

#[test]
fn edit_keeps_an_unchanged_theme_colour() {
    let mut h = harness();
    h.state_mut()
        .run(
            "home.conditionalFormat",
            json!({"rule": {"type": "colorScale", "stops": [
                {"type": "min", "color": {"theme": 4, "tint": 0}},
                {"type": "percentile", "value": 50, "color": {"theme": 2, "tint": 0}},
                {"type": "max", "color": {"theme": 6, "tint": 0}},
            ]}}),
        )
        .unwrap();
    let rule = serde_json::to_value(&h.state().session.active().unwrap().wb.active().unwrap().cond_formats[0].rule).unwrap();
    h.state_mut().open_dialog("newFormattingRule", json!({"edit": 0, "rule": rule}));
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(
        stops(&h),
        vec![(CfValueKind::Min, Color::Theme(4, 0)), (CfValueKind::Percentile(50.0), Color::Theme(2, 0)), (CfValueKind::Max, Color::Theme(6, 0)),]
    );
}

#[test]
fn an_empty_formula_keeps_ok_disabled() {
    let mut h = harness();
    h.state_mut().open_dialog("newFormattingRule", json!({}));
    h.run_steps(6);
    h.get_by_label("Use a formula to determine which cells to format").click();
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["ruleType"], json!("formula"));
    // No formula typed: OK (and Enter) must do nothing, or the rule never applies.
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_some(), "the dialog stays open for a formula");
    assert!(h.state().session.active().unwrap().wb.active().unwrap().cond_formats.is_empty());
}

#[test]
fn an_unparseable_formula_is_refused_with_a_message() {
    let mut h = harness();
    h.state_mut().open_dialog("newFormattingRule", json!({}));
    h.run_steps(6);
    h.get_by_label("Use a formula to determine which cells to format").click();
    h.run_steps(4);
    h.state_mut().dialog.as_mut().unwrap().values.insert("formula".into(), json!("=C1>"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_some(), "an invalid formula keeps the dialog open");
    let message = h.state().message.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
    assert!(message.contains("invalid") && message.contains("does not parse"), "{message}");
    assert!(h.state().session.active().unwrap().wb.active().unwrap().cond_formats.is_empty(), "no unopenable rule is stored");
}

#[test]
fn switching_a_numeric_stop_to_formula_needs_a_formula() {
    let mut h = harness();
    h.state_mut().open_dialog("newFormattingRule", json!({}));
    h.run_steps(6);

    // Switching a Number stop to Formula without touching the field: the editor must store the
    // field as a string rather than leave the old number in place, and OK must wait for one.
    h.state_mut().dialog.as_mut().unwrap().values.insert(
        "stops".into(),
        json!([
            {"type": "formula", "value": 7, "color": "#112233"},
            {"type": "max", "color": "#778899"},
        ]),
    );
    h.run_steps(2);
    let stops_json = h.state().dialog.as_ref().unwrap().values["stops"].as_array().unwrap().clone();
    assert_eq!(stops_json[0]["value"], json!(""), "a numeric value is not a formula");
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_some(), "a formula stop with no formula does not confirm");
    assert!(h.state().session.active().unwrap().wb.active().unwrap().cond_formats.is_empty());

    // A blank formula is refused the same way.
    h.state_mut().dialog.as_mut().unwrap().values.insert(
        "stops".into(),
        json!([
            {"type": "formula", "value": "   ", "color": "#112233"},
            {"type": "max", "color": "#778899"},
        ]),
    );
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_some(), "a blank formula does not confirm");
    assert!(h.state().session.active().unwrap().wb.active().unwrap().cond_formats.is_empty());

    // A real formula confirms.
    h.state_mut().dialog.as_mut().unwrap().values.insert("style".into(), json!("2color"));
    h.state_mut().dialog.as_mut().unwrap().values.insert(
        "stops".into(),
        json!([
            {"type": "formula", "value": "$Z$1", "color": "#112233"},
            {"type": "max", "color": "#778899"},
        ]),
    );
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(stops(&h), vec![(CfValueKind::Formula("$Z$1".into()), Color::Rgb(0x112233)), (CfValueKind::Max, Color::Rgb(0x778899))]);
}
