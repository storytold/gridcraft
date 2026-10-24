//! Real input paths for narrow ribbon overflow and viewport-bounded dialogs.

use egui::{Event, Key, Modifiers, MouseWheelUnit, Rect, TouchPhase, accesskit::Role, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(width: f32, height: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(width, height)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.extend([
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
}

fn in_view(h: &Harness, rect: Rect) {
    let view = h.ctx.content_rect().expand(1.0);
    assert!(view.contains_rect(rect), "{rect:?} outside {view:?}");
}

fn open_overflow(h: &mut Harness) {
    let button = h.get_by_role_and_label(Role::Button, "More commands on this tab");
    in_view(h, button.rect());
    button.click();
    h.run_steps(6);
}

fn style(h: &Harness) -> gridcraft_engine::model::Style {
    let doc = h.state().session.active().unwrap();
    doc.wb.styles.get(doc.wb.active().unwrap().style_id(CellRef::new(0, 0))).clone()
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
}

#[test]
fn overflow_font_and_size_pickers_apply_parameters_once() {
    let mut h = harness(640.0, 700.0);
    let current = style(&h).font.name;
    open_overflow(&mut h);
    let font = h.get_by_role_and_label(Role::Button, &current);
    in_view(&h, font.rect());
    font.click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "Consolas").click();
    h.run_steps(4);
    assert_eq!(style(&h).font.name, "Consolas");
    assert!(h.state().message.is_none());
    // The nested picker closes without losing the overflow's controls.
    h.get_by_label("Font Size").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "18").click();
    h.run_steps(4);
    assert_eq!(style(&h).font.size, 18.0);
    assert!(h.state().message.is_none());
    for command in ["home.fontName", "home.fontSize"] {
        assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == command).count(), 1, "{command}");
    }
}

#[test]
fn overflow_palette_preserves_nested_interaction_and_parameters() {
    let mut h = harness(640.0, 700.0);
    open_overflow(&mut h);
    let arrow = h.get_by_role_and_label(Role::Button, "Font Color");
    in_view(&h, arrow.rect());
    arrow.click();
    h.run_steps(4);
    h.get_by_label("More Colors…").click();
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "More Colors belongs to the palette, not SheetApp dialogs");
    assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).as_deref(), Some("Home"));
    h.get_by_role_and_label(Role::TextInput, "Hex").click();
    h.run_steps(2);
    assert!(h.get_by_role_and_label(Role::TextInput, "Hex").is_focused());
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    // The shortcut is queued by the harness; process it before putting text directly
    // in RawInput, otherwise the text arrives before Select All at the seven-character limit.
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("#12AB34".into()));
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Hex").value().as_deref(), Some("#12AB34"));
    assert!(h.state().session.journal.iter().all(|(id, _)| id != "home.fontColor"), "typing a colour does not apply it before OK");
    h.get_by_label("  OK  ").click();
    h.run_steps(4);
    assert_eq!(h.state().grid.last_font_color, "#12AB34");
    assert_eq!(style(&h).font.color, gridcraft_engine::model::Color::Rgb(0x12AB34));
    assert!(h.state().message.is_none());
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "home.fontColor").count(), 1);
    assert!(h.query_by_label("Theme Colors").is_none(), "OK closes the child palette");
    assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).as_deref(), Some("Home"));
}

#[test]
fn overflow_data_analysis_closes_the_parent_and_the_dialog_receives_clicks() {
    let mut h = harness(640.0, 700.0);
    h.get_by_role_and_label(Role::Button, "More tabs").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Data").click();
    h.run_steps(4);
    open_overflow(&mut h);
    let analysis = h.get_by_role_and_label(Role::Button, "Data Analysis");
    in_view(&h, analysis.rect());
    analysis.click();
    h.run_steps(6);
    assert_eq!(h.state().dialog.as_ref().map(|dialog| dialog.name()), Some("analysisToolPak"));
    assert!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).is_none());
    assert!(!egui::Popup::is_any_open(&h.ctx), "no child popup remains over the dialog");
    let tool = h.get_by_label("Descriptive Statistics");
    in_view(&h, tool.rect());
    assert_eq!(h.ctx.layer_id_at(tool.rect().center()).map(|layer| layer.order), Some(egui::Order::Middle));
    tool.click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("pick"), Some(&json!("descriptive")));
    h.get_by_role_and_label(Role::Button, "   OK   ").click();
    h.run_steps(6);
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("tool"), Some(&json!("descriptive")));
    assert_eq!(h.state().dialog.as_ref().unwrap().title, "Descriptive Statistics");
    h.get_by_role_and_label(Role::Button, " Cancel ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
}

#[test]
fn overflow_stays_open_when_a_dialog_was_already_open() {
    let mut h = harness(640.0, 700.0);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":""}));
    h.run_steps(6);

    open_overflow(&mut h);

    assert_eq!(h.state().dialog.as_ref().map(|dialog| dialog.name()), Some("analysisToolPak"));
    assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).as_deref(), Some("Home"));
}

#[test]
fn collapsed_ribbon_tracks_dialog_opening_and_dismisses_overflow() {
    let mut h = harness(640.0, 700.0);
    open_overflow(&mut h);
    assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).as_deref(), Some("Home"));

    h.state_mut().ui.ribbon_collapsed = true;
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":""}));
    h.run_steps(4);

    assert!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).is_none());
}

#[test]
fn overflow_closes_on_escape_and_translates_its_anchors() {
    let mut h = harness(640.0, 480.0);
    open_overflow(&mut h);
    assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).as_deref(), Some("Home"));
    key(&mut h, Key::Escape);
    assert!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new("ribbon_overflow_open"))).is_none());
    for (language, tabs, commands) in [("pt", "Mais abas", "Mais comandos desta aba"), ("ja", "その他のタブ", "このタブのその他のコマンド")]
    {
        h.state_mut().run("app.language.set", json!({"language":language})).unwrap();
        h.run_steps(4);
        in_view(&h, h.get_by_role_and_label(Role::Button, tabs).rect());
        in_view(&h, h.get_by_role_and_label(Role::Button, commands).rect());
    }
}

fn toolpak_rect(h: &Harness, tool: &str) -> Rect {
    h.ctx.memory(|memory| memory.area_rect(egui::Id::new("dialog").with("analysisToolPak").with(tool))).unwrap()
}

#[test]
fn toolpak_actions_stay_fixed_while_the_body_scrolls() {
    for tool in ["", "regression", "descriptive"] {
        let mut h = harness(640.0, 360.0);
        h.state_mut().open_dialog("analysisToolPak", json!({"tool":tool}));
        h.run_steps(6);
        let ok = h.get_by_role_and_label(Role::Button, "   OK   ").rect();
        let cancel = h.get_by_role_and_label(Role::Button, " Cancel ").rect();
        in_view(&h, ok);
        in_view(&h, cancel);
        in_view(&h, toolpak_rect(&h, tool));
        let label = if tool.is_empty() {
            "Descriptive Statistics"
        } else if tool == "regression" {
            "Input Y Range:"
        } else {
            "Input Range:"
        };
        let before = h.get_by_label(label).rect();
        h.input_mut().events.extend([
            Event::PointerMoved(before.center()),
            Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, -700.0), phase: TouchPhase::Move, modifiers: Modifiers::NONE },
        ]);
        h.run_steps(12);
        assert_eq!(h.get_by_role_and_label(Role::Button, "   OK   ").rect(), ok, "{tool}");
        assert_eq!(h.get_by_role_and_label(Role::Button, " Cancel ").rect(), cancel, "{tool}");
        // Prove that scrolling happened, rather than only checking two stationary buttons.
        assert!(h.query_by_label(label).is_none_or(|node| node.rect().top() < before.top() - 1.0), "{tool}: the body did not scroll");
        h.get_by_role_and_label(Role::Button, " Cancel ").click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
    }
}

#[test]
fn toolpak_can_use_a_tall_viewport_without_a_fixed_height_cap() {
    let mut h = harness(1200.0, 1000.0);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
    h.run_steps(6);
    let rect = toolpak_rect(&h, "regression");
    assert!(rect.height() > 700.0, "{rect:?}");
    in_view(&h, rect);
    in_view(&h, h.get_by_role_and_label(Role::Button, "   OK   ").rect());
}

#[test]
fn dialogs_follow_a_smaller_viewport_and_keep_compact_forms_compact() {
    let mut h = harness(1200.0, 1000.0);
    h.state_mut().open_dialog("columnWidth", json!({}));
    h.run_steps(6);
    let id = egui::Id::new("dialog").with("columnWidth");
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
