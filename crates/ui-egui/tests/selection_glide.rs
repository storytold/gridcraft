//! The selection outline glides to a new selection, and View ▸ Animations turns that off (the
//! whole Animation effects setting is in `animation_effects.rs`).

use egui::Rect;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::motion::Animations;
use gridcraft_ui_egui::{ControlRequest, SheetApp, UiState, grid::Geo};
use serde_json::{Value, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// 240 frames a second, so a glide spans many frames.
const DT: f32 = 1.0 / 240.0;

fn app() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    SheetApp::new(session, Default::default())
}

fn harness() -> Harness {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(DT).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app(),
    );
    h.run_steps(4);
    h
}

/// The harness turns egui's animations off (and the glide follows that switch); this turns them on.
fn animated() -> Harness {
    let mut h = harness();
    h.ctx.all_styles_mut(|s| s.animation_time = 0.1);
    h.step();
    h
}

/// Where a cell is on screen right now.
fn cell_rect(h: &Harness, a1: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap())
}

/// Where the outline was drawn in the last frame.
fn outline(h: &Harness) -> Rect {
    h.state().grid.glide.drawn()
}

fn gliding(h: &Harness) -> bool {
    h.state().grid.glide.is_gliding()
}

fn select(h: &mut Harness, a1: &str) {
    h.state_mut().run("selection.set", json!({"cell": a1})).unwrap();
}

fn press(h: &mut Harness, key: egui::Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
    }
}

fn steps(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn reply(outcome: Outcome) -> Value {
    match outcome {
        Outcome::Done(v) => v,
        _ => Value::Null,
    }
}

#[test]
fn an_arrow_key_glides_the_outline_to_the_next_cell() {
    let mut h = animated();
    let (a1, b1) = (cell_rect(&h, "A1"), cell_rect(&h, "B1"));
    assert_eq!(outline(&h), a1, "at rest the outline is on its cells");
    press(&mut h, egui::Key::ArrowRight);
    h.step();
    // The selection itself moves at once; only its outline takes its time.
    assert_eq!(h.state().session.active().map(|d| d.selection.active.a1()).as_deref(), Some("B1"));
    steps(&mut h, 2);
    let x = outline(&h).min.x;
    assert!(gliding(&h) && x > a1.min.x && x < b1.min.x, "on its way, drawn in between: {x}");
    steps(&mut h, 40);
    assert!(!gliding(&h), "and it comes to rest");
    assert_eq!(outline(&h), b1, "exactly on the cell");
    // Zooming moves the cell on screen but doesn't glide the outline.
    h.state_mut().run("view.zoom", json!({"percent": 150})).unwrap();
    for _ in 0..3 {
        h.step();
        assert!(!gliding(&h));
    }
    assert_ne!(outline(&h), b1);
    assert_eq!(outline(&h), cell_rect(&h, "B1"));
}

#[test]
fn with_eguis_animations_off_the_outline_is_always_on_its_cells() {
    // The headless harness (so the offscreen snapshot too) never shows an outline in flight.
    let mut h = harness();
    select(&mut h, "D6");
    let d6 = cell_rect(&h, "D6");
    for _ in 0..4 {
        h.step();
        assert_eq!(outline(&h), d6);
        assert!(!gliding(&h));
    }
}

#[test]
fn the_view_tab_checkbox_turns_the_glide_off_and_on() {
    let mut h = animated();
    h.state_mut().ui.ribbon_tab = "View".into();
    h.run_steps(2);
    h.get_by_label("Animations").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.animations, Animations::Off, "the checkbox turns the setting off");
    select(&mut h, "E9");
    let e9 = cell_rect(&h, "E9");
    for _ in 0..5 {
        h.step();
        assert_eq!(outline(&h), e9, "no animations: the outline is on its cells at once");
        assert!(!gliding(&h));
    }
    h.get_by_label("Animations").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.animations, Animations::On, "and on again");
    select(&mut h, "B1");
    steps(&mut h, 2);
    let (x, b1) = (outline(&h).min.x, cell_rect(&h, "B1"));
    assert!(gliding(&h) && x > b1.min.x && x < e9.min.x, "turned back on, it glides: {x}");
    steps(&mut h, 40);
    assert_eq!(outline(&h), b1);
}

#[test]
fn a_screenshot_never_catches_the_outline_in_flight() {
    let mut h = animated();
    let (a1, b1) = (cell_rect(&h, "A1"), cell_rect(&h, "B1"));
    let (tx, rx) = std::sync::mpsc::channel();
    h.state_mut().control_rx = Some(rx);
    let (shot, _reply) = ControlRequest::new("ui.screenshot", json!({}));
    tx.send(shot).unwrap();
    press(&mut h, egui::Key::ArrowRight);
    for _ in 0..20 {
        h.step();
        let drawn = outline(&h);
        assert!(drawn == a1 || drawn == b1, "{drawn:?}");
    }
    assert_eq!(outline(&h), b1);
}

#[test]
fn animations_follow_the_system_until_chosen_and_the_choice_is_saved() {
    assert_eq!(UiState::default().animations, Animations::System);
    // A `ui.json` written before the setting existed follows the system.
    let old: UiState = serde_json::from_str(r#"{"dark": true, "ribbonTab": "View"}"#).unwrap();
    assert!(old.animations == Animations::System && old.dark);
    // While it was a checkbox it was saved as a boolean: ticked was only the default, so it
    // follows the system now; unticked was a choice, and stays off.
    let ticked: UiState = serde_json::from_value(json!({"animations": true})).unwrap();
    assert_eq!(ticked.animations, Animations::System);
    let off: UiState = serde_json::from_value(json!({"animations": false})).unwrap();
    assert_eq!(off.animations, Animations::Off);
    // The three values are saved under a name of their own. An older GridCraft sharing the
    // `ui.json` skips it, and still finds no string where it expects its boolean.
    for mode in Animations::ALL {
        let ui = UiState { animations: mode, ..Default::default() };
        let saved = serde_json::to_value(&ui).unwrap();
        assert_eq!(saved["animationEffects"], json!(mode.code()));
        assert!(saved.get("animations").is_none(), "{saved}");
        assert_eq!(serde_json::from_value::<UiState>(saved).unwrap().animations, mode);
    }
    // A value that makes no sense is the default, and the other preferences are kept.
    for odd in [json!("sometimes"), json!(7), json!(null), json!([true])] {
        for name in ["animationEffects", "animations"] {
            let ui: UiState = serde_json::from_value(json!({name: odd, "dark": true})).unwrap();
            assert!(ui.animations == Animations::System && ui.dark, "{name}: {odd}");
        }
    }
}

#[test]
fn view_animations_sets_and_toggles_without_a_dialog() {
    let mut a = app();
    let (on, off) = (json!({"mode": "on", "on": true, "system": null}), json!({"mode": "off", "on": false, "system": null}));
    assert_eq!(a.run("view.animations", json!({"on": false})), Ok(off.clone()));
    assert_eq!(a.run("view.animations", json!({"on": false})), Ok(off.clone()), "setting it twice changes nothing");
    // No parameters: it toggles, and never opens a dialog.
    assert_eq!(a.run("view.animations", json!({})), Ok(on.clone()));
    assert_eq!(a.run("view.animations", json!({})), Ok(off.clone()));
    assert!(a.dialog.is_none() && a.message.is_none());
    // Parameters that make no sense toggle too, rather than failing or panicking.
    assert_eq!(a.run("view.animations", json!({"on": "yes"})), Ok(on));
    assert_eq!(a.run("view.animations", Value::Null), Ok(off));
}

#[test]
fn agents_find_run_and_read_the_setting_over_the_control_channel() {
    let mut a = app();
    let ctx = egui::Context::default();
    let found = reply(control::handle(&mut a, &ctx, "engine.commands", &json!({"search": "animations"})));
    let entry = found["result"].as_array().and_then(|all| all.iter().find(|c| c["id"] == "view.animations")).cloned().unwrap();
    assert_eq!((&entry["label"], &entry["ui"]), (&json!("Animation Effects"), &json!(true)));
    assert!(entry["params"].as_str().is_some_and(|doc| doc.contains("mode") && doc.contains("on")), "{entry}");
    let ran = reply(control::handle(&mut a, &ctx, "engine.execute", &json!({"command": "view.animations", "params": {"on": false}})));
    assert_eq!(ran, json!({"ok": true, "result": {"mode": "off", "on": false, "system": null}}));
    let seen = reply(control::handle(&mut a, &ctx, "ui.inspect", &json!({})));
    assert_eq!((&seen["result"]["ui"]["animationEffects"], &seen["result"]["animationsOn"]), (&json!("off"), &json!(false)));
    // `ui.set` takes the setting's three values, and the checkbox's true and false.
    let set = reply(control::handle(&mut a, &ctx, "ui.set", &json!({"animations": true})));
    assert_eq!(set["result"]["animationEffects"], json!("on"));
    assert_eq!(a.ui.animations, Animations::On);
    let set = reply(control::handle(&mut a, &ctx, "ui.set", &json!({"animations": "system"})));
    assert_eq!(set["result"]["animationEffects"], json!("system"));
    let set = reply(control::handle(&mut a, &ctx, "ui.set", &json!({"animations": "sideways"})));
    assert_eq!(set["ok"], json!(false), "{set}");
    assert_eq!(a.ui.animations, Animations::System, "a value that isn't one of the three changes nothing");
}
