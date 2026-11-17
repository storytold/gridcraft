//! Real input paths for narrow ribbon tabs and viewport-bounded dialogs.

use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Rect, TouchPhase, accesskit::Role, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(width: f32, height: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(width, height)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

fn in_view(h: &Harness, rect: Rect) {
    let view = h.ctx.content_rect().expand(1.0);
    assert!(view.contains_rect(rect), "{rect:?} outside {view:?}");
}

#[test]
fn narrow_tabs_reveal_the_active_tab_and_keep_contextual_tabs_reachable() {
    let mut h = harness(640.0, 480.0);
    h.get_by_role_and_label(Role::Button, "More tabs").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Automate").click();
    h.run_steps(6);
    assert_eq!(h.state().ui.ribbon_tab, "Automate");
    in_view(&h, h.get_by_role_and_label(Role::RadioButton, "Automate").rect());
    h.state_mut().session.execute("range.setValues", json!({"range":"A1:B2","values":[["Name","Qty"],["a",1]]})).unwrap();
    h.state_mut().session.execute("insert.table", json!({"range":"A1:B2"})).unwrap();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "More tabs").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Table Design").click();
    h.run_steps(6);
    assert_eq!(h.state().ui.ribbon_tab, "Table Design");
    in_view(&h, h.get_by_role_and_label(Role::RadioButton, "Table Design").rect());

    h.state_mut().run("app.language.set", json!({"language":"pt"})).unwrap();
    h.run_steps(4);
    in_view(&h, h.get_by_role_and_label(Role::Button, "Mais abas").rect());
}

#[test]
fn more_tabs_scrolls_to_the_last_contextual_tab_in_a_short_viewport() {
    let mut h = harness(360.0, 170.0);
    h.state_mut().session.execute("range.setValues", json!({"range":"A1:B2","values":[["Name","Qty"],["a",1]]})).unwrap();
    h.state_mut().session.execute("insert.table", json!({"range":"A1:B2"})).unwrap();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "More tabs").click();
    h.run_steps(4);
    let first = h.get_by_role_and_label(Role::Button, "Home").rect();
    in_view(&h, first);
    h.input_mut().events.extend([
        Event::PointerMoved(first.center()),
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            source: MouseWheelSource::Unknown,
            delta: vec2(0.0, -1200.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run_steps(12);
    let last = h.get_by_role_and_label(Role::Button, "Table Design").rect();
    in_view(&h, last);
    assert!(last.top() < 170.0, "the final tab scrolled into view");
    h.get_by_role_and_label(Role::Button, "Table Design").click();
    h.run_steps(6);
    assert_eq!(h.state().ui.ribbon_tab, "Table Design");
    assert!(h.query_by_role_and_label(Role::Button, "Table Design").is_none(), "selection closes the menu");
    in_view(&h, h.get_by_role_and_label(Role::RadioButton, "Table Design").rect());
}

fn toolpak_rect(h: &Harness, tool: &str) -> Rect {
    h.ctx.memory(|memory| memory.area_rect(egui::Id::unique("dialog").with("analysisToolPak").with(tool))).unwrap()
}

#[test]
fn changing_between_languages_with_the_same_more_tabs_fallback_reveals_the_active_tab() {
    let mut h = harness(640.0, 480.0);
    h.state_mut().ui.ribbon_tab = "Automate".into();
    h.state_mut().run("app.language.set", json!({"language":"de"})).unwrap();
    h.run_steps(6);
    let active = h.get_by_role_and_label(Role::RadioButton, "Automatisieren").rect();
    in_view(&h, active);
    // Browse back to Home. With no layout change, manual browsing must stay where it was left.
    h.input_mut().events.extend([
        Event::PointerMoved(active.center()),
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            source: MouseWheelSource::Unknown,
            delta: vec2(1200.0, 0.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run_steps(12);
    assert!(h.query_by_role_and_label(Role::RadioButton, "Automatisieren").is_none_or(|n| !h.ctx.content_rect().contains_rect(n.rect())));
    // Neither German nor French translates More tabs; the language code must invalidate reveal.
    h.state_mut().run("app.language.set", json!({"language":"fr"})).unwrap();
    h.run_steps(6);
    in_view(&h, h.get_by_role_and_label(Role::RadioButton, "Automatiser").rect());
}

#[test]
fn toolpak_actions_stay_fixed_while_the_body_scrolls() {
    for (tool, label) in [
        ("", "Descriptive Statistics"),
        ("regression", "Labels in first row"),
        ("descriptive", "Columns"),
        ("randomNumberGeneration", "Uniform"),
        ("sampling", "Labels in first row"),
        ("zTest", "Variable 1 Range:"),
    ] {
        let mut h = harness(640.0, 360.0);
        h.state_mut().open_dialog("analysisToolPak", json!({"tool":tool}));
        h.run_steps(6);
        let ok = h.get_by_role_and_label(Role::Button, "   OK   ").rect();
        let cancel = h.get_by_role_and_label(Role::Button, " Cancel ").rect();
        in_view(&h, ok);
        in_view(&h, cancel);
        in_view(&h, toolpak_rect(&h, tool));
        // RefEdit's label also names its text box: select the static label explicitly there.
        let role = if tool == "zTest" {
            Role::TextInput
        } else if tool.is_empty() {
            Role::Button
        } else if tool == "descriptive" || tool == "randomNumberGeneration" {
            Role::RadioButton
        } else {
            Role::CheckBox
        };
        let before = h.get_by_role_and_label(role, label).rect();
        h.input_mut().events.extend([
            Event::PointerMoved(before.center()),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                source: MouseWheelSource::Unknown,
                delta: vec2(0.0, -700.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ]);
        h.run_steps(12);
        assert_eq!(h.get_by_role_and_label(Role::Button, "   OK   ").rect(), ok, "{tool}");
        assert_eq!(h.get_by_role_and_label(Role::Button, " Cancel ").rect(), cancel, "{tool}");
        // Prove that scrolling happened, rather than only checking two stationary buttons.
        assert!(h.query_by_role_and_label(role, label).is_none_or(|node| node.rect().top() < before.top() - 1.0), "{tool}: the body did not scroll");
        h.get_by_role_and_label(Role::Button, " Cancel ").click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
    }
}

#[test]
fn toolpak_can_use_a_tall_viewport_without_a_fixed_height_cap() {
    let mut h = harness(1200.0, 1000.0);
    h.state_mut().open_dialog("analysisToolPak", json!({}));
    h.run_steps(6);
    let compact = toolpak_rect(&h, "");
    assert!(compact.height() < 700.0, "the chooser fits its list: {compact:?}");
    resize_dialog(&mut h, compact, vec2(0.0, 350.0));
    let rect = toolpak_rect(&h, "");
    assert!(rect.height() > 700.0, "user resize can use the tall viewport: {rect:?}");
    in_view(&h, rect);
    let ok = h.get_by_role_and_label(Role::Button, "   OK   ").rect();
    in_view(&h, ok);
    assert!(ok.bottom() > compact.bottom() + 100.0, "actions follow the resized body");
}

#[test]
fn every_official_tool_keeps_actions_visible_after_the_viewport_shrinks() {
    for &(tool, _) in gridcraft_engine::cmd::analysis::TOOLS {
        let mut h = harness(1200.0, 1000.0);
        h.state_mut().open_dialog("analysisToolPak", json!({"tool":tool}));
        h.run_steps(6);
        h.set_size(vec2(360.0, 260.0));
        h.run_steps(8);
        in_view(&h, toolpak_rect(&h, tool));
        in_view(&h, h.get_by_role_and_label(Role::Button, "   OK   ").rect());
        in_view(&h, h.get_by_role_and_label(Role::Button, " Cancel ").rect());
        assert_eq!(h.state().dialog.as_ref().unwrap().values["tool"], tool);
    }
}

#[test]
fn dynamic_random_and_sampling_fields_survive_the_responsive_layout() {
    for (tool, key, variants) in [
        (
            "randomNumberGeneration",
            "distribution",
            vec![("uniform", "Between:"), ("patterned", "Repeating each sequence (times):"), ("discrete", "Value and Probability Input Range:")],
        ),
        ("sampling", "method", vec![("periodic", "Period:"), ("random", "Number of Samples:")]),
    ] {
        let mut h = harness(640.0, 360.0);
        h.state_mut().open_dialog("analysisToolPak", json!({"tool":tool}));
        for (value, label) in variants {
            h.state_mut().dialog.as_mut().unwrap().values.insert(key.into(), json!(value));
            h.run_steps(6);
            let fields = &h.state().dialog.as_ref().unwrap().fields;
            assert!(
                fields.iter().any(|f| matches!(f, gridcraft_ui_egui::dialogs::Field::Text { label: l, .. } if *l == label)),
                "{tool}/{value}: {label}"
            );
            in_view(&h, h.get_by_role_and_label(Role::Button, "   OK   ").rect());
            in_view(&h, h.get_by_role_and_label(Role::Button, " Cancel ").rect());
        }
    }
}

fn press_key(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(4);
}

#[test]
fn chooser_enter_opens_the_form_without_running_an_analysis() {
    let mut h = harness(640.0, 360.0);
    h.state_mut().open_dialog("analysisToolPak", json!({}));
    h.run_steps(6);
    h.get_by_role_and_label(Role::Button, "Descriptive Statistics").click();
    h.run_steps(3);
    let sheets = h.state().session.active().unwrap().wb.sheets.len();
    let undo = h.state().session.active().unwrap().undo.len();
    press_key(&mut h, Key::Enter);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["tool"], "descriptive");
    assert!(h.state().message.is_none(), "choosing a tool must not submit an incomplete form");
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.wb.sheets.len(), sheets);
    assert_eq!(doc.undo.len(), undo);
}

#[test]
fn collapsed_toolpak_refedit_restores_the_expanded_size_and_keeps_enter_from_confirming() {
    let mut h = harness(1200.0, 1000.0);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
    h.run_steps(6);
    let full = toolpak_rect(&h, "regression");
    h.get_all_by_label("Collapse dialog").next().unwrap().click();
    h.run_steps(6);
    assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), Some("inputY"));
    let id = egui::Id::unique("dialog").with("analysisToolPak").with("regression").with("ref_edit_collapsed");
    let compact = h.ctx.memory(|m| m.area_rect(id)).unwrap();
    assert!(compact.height() < 150.0, "{compact:?}");
    // Shared RefEdit moves a compact picker that would cover the grid to the viewport's right
    // edge, then restores the expanded dialog's position on Enter.
    assert_eq!(compact.top(), full.top());
    assert!((compact.right() - (h.ctx.content_rect().right() - 8.0)).abs() < 1.0, "{compact:?}");
    in_view(&h, compact);
    assert!(h.query_by_role_and_label(Role::Button, "   OK   ").is_none());
    press_key(&mut h, Key::Enter);
    assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), None);
    assert_eq!(toolpak_rect(&h, "regression"), full);
    assert!(h.state().message.is_none());
}

#[test]
fn toolpak_can_be_manually_resized_and_its_actions_follow_the_new_size() {
    let mut h = harness(1200.0, 1000.0);
    h.state_mut().open_dialog("analysisToolPak", json!({}));
    h.run_steps(6);
    let before = toolpak_rect(&h, "");
    resize_dialog(&mut h, before, vec2(-80.0, -200.0));
    let after = toolpak_rect(&h, "");
    assert!(after.height() < before.height() - 150.0, "{before:?} -> {after:?}");
    assert!(after.width() < before.width() - 40.0, "{before:?} -> {after:?}");
    in_view(&h, after);
    let ok = h.get_by_role_and_label(Role::Button, "   OK   ").rect();
    in_view(&h, ok);
    assert!(after.contains_rect(ok));
    in_view(&h, h.get_by_role_and_label(Role::Button, " Cancel ").rect());
}

fn resize_dialog(h: &mut Harness, before: Rect, delta: egui::Vec2) {
    let handle = h.get_by_role_and_label(Role::Splitter, "Resize window bottom-right corner").rect();
    let start = handle.center();
    assert!(before.expand(8.0).contains(start), "resize handle {handle:?} belongs to {before:?}");
    h.input_mut().events.push(Event::PointerMoved(start));
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let end = start + delta;
    for step in 1..=4 {
        h.input_mut().events.push(Event::PointerMoved(start + (end - start) * (step as f32 / 4.0)));
        h.run_steps(2);
    }
    h.input_mut().events.push(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(6);
}

#[test]
fn short_dialogs_fit_content_and_remember_a_user_resize_when_reopened() {
    for (name, params, max_height) in [
        ("rowHeight", json!({}), 250.0),
        ("columnWidth", json!({}), 250.0),
        ("cfQuick", json!({"type":"cellIs","operator":"greaterThan","title":"Greater Than…"}), 400.0),
        ("advancedFilter", json!({}), 400.0),
    ] {
        let mut h = harness(1200.0, 1000.0);
        h.state_mut().open_dialog(name, params.clone());
        h.run_steps(6);
        let id = egui::Id::unique("dialog").with(name);
        let before = h.ctx.memory(|m| m.area_rect(id)).unwrap();
        assert!(before.height() < max_height, "{name}: {before:?}");
        let ok = h.get_by_role_and_label(Role::Button, "   OK   ").rect();
        assert!(before.bottom() - ok.bottom() < 40.0, "{name}: no empty window below the actions");
        resize_dialog(&mut h, before, vec2(0.0, 150.0));
        let resized = h.ctx.memory(|m| m.area_rect(id)).unwrap();
        assert!(resized.height() > before.height() + 100.0, "{name}: {before:?} -> {resized:?}");
        h.get_by_role_and_label(Role::Button, " Cancel ").click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        h.state_mut().open_dialog(name, params);
        h.run_steps(6);
        let reopened = h.ctx.memory(|m| m.area_rect(id)).unwrap();
        assert!((reopened.height() - resized.height()).abs() < 1.0, "{name}: {resized:?} -> {reopened:?}");
    }
}

#[test]
fn dialogs_follow_a_smaller_viewport_and_keep_compact_forms_compact() {
    let mut h = harness(1200.0, 1000.0);
    h.state_mut().open_dialog("columnWidth", json!({}));
    h.run_steps(6);
    let id = egui::Id::unique("dialog").with("columnWidth");
    let compact = h.ctx.memory(|memory| memory.area_rect(id)).unwrap();
    assert!(compact.height() < 250.0, "{compact:?}");
    h.set_size(vec2(320.0, 320.0));
    h.run_steps(8);
    in_view(&h, h.ctx.memory(|memory| memory.area_rect(id)).unwrap());
    in_view(&h, h.get_by_role_and_label(Role::Button, "   OK   ").rect());
    in_view(&h, h.get_by_role_and_label(Role::Button, " Cancel ").rect());
}

#[test]
fn scaled_portuguese_toolpak_keeps_its_actions_in_the_viewport() {
    let mut h = harness(640.0, 480.0);
    h.state_mut().run("app.language.set", json!({"language":"pt"})).unwrap();
    h.state_mut().run("view.interfaceScale", json!({"percent":150})).unwrap();
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
    h.run_steps(8);
    in_view(&h, h.get_by_role_and_label(Role::Button, "   OK   ").rect());
    in_view(&h, h.get_by_role_and_label(Role::Button, " Cancelar ").rect());
    in_view(&h, toolpak_rect(&h, "regression"));
}

#[test]
fn toolpak_overwrite_actions_stay_visible_in_a_short_viewport() {
    let mut h = harness(360.0, 170.0);
    h.state_mut().open_dialog("analysisToolPak", json!({}));
    let dialog = h.state_mut().dialog.as_mut();
    assert!(dialog.is_some(), "Analysis ToolPak dialog did not open");
    if let Some(dialog) = dialog {
        dialog.values.insert("askOverwrite".into(), json!(true));
    }
    h.run_steps(6);

    in_view(&h, h.get_by_role_and_label(Role::Button, "Replace").rect());
    in_view(&h, h.get_by_role_and_label(Role::Button, "Cancel").rect());
}
