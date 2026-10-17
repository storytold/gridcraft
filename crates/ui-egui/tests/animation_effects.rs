//! File ▸ Options ▸ Animation effects: Use system setting, On, Off. Off (chosen, or the system's
//! own setting while the preference follows it) stops every motion in the window: wheel scrolling
//! and zooming land at once, the selection outline is on its cells, the dashes round a copied
//! range stand still, and egui's own fades are off.

use std::cell::Cell;
use std::rc::Rc;

use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::motion::Animations;
use gridcraft_ui_egui::{Services, SheetApp, UiState};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// What the system says about its animation effects, switched by the test.
type System = Rc<Cell<Option<bool>>>;

/// A window on a blank workbook whose host reports `system`. The harness has egui's own
/// animations off; `animated` puts them on, as a real window has them.
fn harness(system: &System, animated: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let asked = system.clone();
    let services = Services { animations_enabled: Some(Box::new(move || asked.get())), ..Default::default() };
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    if animated {
        h.ctx.all_styles_mut(|s| s.animation_time = 0.1);
        h.step();
    }
    h
}

fn set(h: &mut Harness, mode: Animations) {
    h.state_mut().run("view.animations", json!({"mode": mode.code()})).unwrap();
    h.step();
}

fn steps(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

/// Turns the wheel over the middle of the grid: `lines` notches (negative scrolls down).
fn wheel(h: &mut Harness, lines: f32, modifiers: Modifiers) {
    let at = h.state().grid.cells_rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.step();
    h.input_mut().events.push(Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, lines), phase: TouchPhase::Move, modifiers });
    h.step();
}

fn scroll_y(h: &Harness) -> f32 {
    h.state().view().scroll.y
}

fn zoom(h: &Harness) -> u16 {
    h.state().session.active().unwrap().wb.active().unwrap().zoom
}

/// What the Animation effects box on the Options page shows: the second combo box there, under
/// Display theme.
fn shown(h: &Harness) -> String {
    let combo = h.get_all_by_role(egui::accesskit::Role::ComboBox).nth(1).expect("animation effects combo");
    combo.accesskit_node().value().unwrap_or_default()
}

/// Moves the selection and says whether its outline glides there.
fn glides_to(h: &mut Harness, a1: &str) -> bool {
    h.state_mut().run("selection.set", json!({"cell": a1})).unwrap();
    steps(h, 2);
    let gliding = h.state().grid.glide.is_gliding();
    steps(h, 30);
    assert!(!h.state().grid.glide.is_gliding(), "an outline always comes to rest");
    gliding
}

#[test]
fn the_default_follows_the_system_and_on_and_off_overrule_it() {
    let system = System::default();
    let mut h = harness(&system, true);
    assert_eq!(h.state().ui.animations, Animations::System);
    // The system says nothing (no hook answer yet, or a platform that can't be asked): on.
    assert!(h.state().animations_on() && h.state().system_animations().is_none());
    assert!(glides_to(&mut h, "C3"));
    // Animation effects are switched off in the system's settings while the app runs.
    system.set(Some(false));
    h.step();
    assert_eq!(h.state().system_animations(), Some(false));
    assert!(!h.state().animations_on());
    assert_eq!(h.state().ui.animations, Animations::System, "the preference itself is untouched");
    assert!(!glides_to(&mut h, "E7"));
    // ...and back on.
    system.set(Some(true));
    h.step();
    assert!(h.state().animations_on());
    assert!(glides_to(&mut h, "B2"));
    // On and Off don't ask the system.
    system.set(Some(false));
    set(&mut h, Animations::On);
    assert!(h.state().animations_on());
    assert!(glides_to(&mut h, "D4"));
    system.set(Some(true));
    set(&mut h, Animations::Off);
    assert!(!h.state().animations_on());
    assert!(!glides_to(&mut h, "F9"));
    // Back to following it.
    set(&mut h, Animations::System);
    assert!(glides_to(&mut h, "A1"));
}

#[test]
fn the_options_page_offers_use_system_setting_on_and_off() {
    let system = System::default();
    let mut h = harness(&system, false);
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Options").click();
    h.run_steps(3);
    h.get_by_label("Animation effects");
    assert_eq!(shown(&h), "Use system setting", "what it reads until something is chosen");
    for (label, mode) in [("Off", Animations::Off), ("On", Animations::On), ("Use system setting", Animations::System)] {
        h.get_all_by_role(egui::accesskit::Role::ComboBox).nth(1).expect("animation effects combo").click();
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
        h.run_steps(3);
        assert_eq!(h.state().ui.animations, mode, "{label}");
        assert_eq!(shown(&h), label);
        assert!(h.state().dialog.is_none() && h.state().message.is_none());
    }
    // The choice is what gets saved.
    h.state_mut().run("view.animations", json!({"mode": "off"})).unwrap();
    let saved = serde_json::to_string(&h.state().ui).unwrap();
    assert_eq!(serde_json::from_str::<UiState>(&saved).unwrap().animations, Animations::Off);
}

#[test]
fn the_command_takes_the_three_modes_and_refuses_others() {
    let system = System::default();
    let mut h = harness(&system, false);
    system.set(Some(false));
    let app = h.state_mut();
    // The answer says what was set, what it comes to, and what the system says.
    assert_eq!(app.run("view.animations", json!({"mode": "system"})), Ok(json!({"mode": "system", "on": false, "system": false})));
    assert_eq!(app.run("view.animations", json!({"mode": "on"})), Ok(json!({"mode": "on", "on": true, "system": false})));
    assert_eq!(app.run("view.animations", json!({"mode": " OFF "})), Ok(json!({"mode": "off", "on": false, "system": false})));
    for bad in [json!("reduced"), json!(""), json!(1), json!([]), json!({"mode": "on"})] {
        assert!(app.run("view.animations", json!({"mode": bad})).is_err(), "{bad}");
        assert_eq!(app.ui.animations, Animations::Off, "a refused mode changes nothing");
    }
    // A `mode` of null is no mode (clients that write every optional field).
    assert_eq!(app.run("view.animations", json!({"mode": null, "on": false})), Ok(json!({"mode": "off", "on": false, "system": false})));
    // Without parameters it toggles what shows: following a system that has them off, to On.
    app.run("view.animations", json!({"mode": "system"})).unwrap();
    assert_eq!(app.run("view.animations", json!({})), Ok(json!({"mode": "on", "on": true, "system": false})));
    assert!(app.dialog.is_none() && app.message.is_none());
}

#[test]
fn off_scrolls_a_wheel_notch_at_once_and_on_eases_it_in() {
    let system = System::default();
    let mut eased = harness(&system, true);
    wheel(&mut eased, -1.0, Modifiers::NONE);
    let first = scroll_y(&eased);
    steps(&mut eased, 90);
    let whole = scroll_y(&eased);
    assert!(first > 0.0 && first < whole * 0.6, "on: the notch arrives over several frames ({first} of {whole})");

    let mut h = harness(&system, true);
    set(&mut h, Animations::Off);
    wheel(&mut h, -1.0, Modifiers::NONE);
    assert!((scroll_y(&h) - whole).abs() < 0.01, "off: all of it in the frame the wheel turned ({} of {whole})", scroll_y(&h));
    steps(&mut h, 30);
    assert!((scroll_y(&h) - whole).abs() < 0.01, "and nothing trails in afterwards: {}", scroll_y(&h));
    // The same when it is the system that has animations off.
    set(&mut h, Animations::System);
    system.set(Some(false));
    wheel(&mut h, -1.0, Modifiers::NONE);
    assert!((scroll_y(&h) - 2.0 * whole).abs() < 0.01, "{}", scroll_y(&h));
    // Shift turns the wheel sideways, whole as well.
    wheel(&mut h, -1.0, Modifiers::SHIFT);
    let x = h.state().view().scroll.x;
    assert!((x - whole).abs() < 0.01, "{x}");
    steps(&mut h, 30);
    assert_eq!(h.state().view().scroll.x, x);
}

#[test]
fn off_steps_a_ctrl_wheel_notch_at_once_and_on_glides_there() {
    // A notch goes to the next step of the zoom ladder: 100% → 120% (`wheel_zoom.rs`).
    let system = System::default();
    let mut eased = harness(&system, true);
    assert_eq!(zoom(&eased), 100);
    wheel(&mut eased, 1.0, Modifiers::COMMAND);
    let first = zoom(&eased);
    steps(&mut eased, 90);
    assert!(first < 120, "on: the notch eases in and the zoom glides, so it isn't there in the frame the wheel turned ({first})");
    assert_eq!(zoom(&eased), 120);

    let mut h = harness(&system, true);
    set(&mut h, Animations::Off);
    wheel(&mut h, 1.0, Modifiers::COMMAND);
    assert_eq!(zoom(&h), 120, "off: on the step in the frame the wheel turned");
    steps(&mut h, 30);
    assert_eq!(zoom(&h), 120, "and nothing trails in afterwards");
    assert_eq!(scroll_y(&h), 0.0, "Ctrl+wheel zooms; it doesn't scroll");
    wheel(&mut h, -1.0, Modifiers::COMMAND);
    assert_eq!(zoom(&h), 100, "a notch back undoes it");
    // The same when it is the system that has animations off: two notches, two steps, at once.
    set(&mut h, Animations::System);
    system.set(Some(false));
    wheel(&mut h, 2.0, Modifiers::COMMAND);
    assert_eq!(zoom(&h), 140);
    steps(&mut h, 30);
    assert_eq!(zoom(&h), 140);
}

#[test]
fn off_holds_the_copy_outline_still_and_eguis_own_animations_off() {
    let system = System::default();
    let mut h = harness(&system, true);
    h.state_mut().run("selection.set", json!({"range": "B2:C4"})).unwrap();
    h.state_mut().run("edit.copy", json!({})).unwrap();
    let phases = |h: &mut Harness| -> Vec<f32> {
        (0..12)
            .map(|_| {
                h.step();
                h.state().grid.marching_phase
            })
            .collect()
    };
    let marching = phases(&mut h);
    assert!(marching.windows(2).any(|w| w[0] != w[1]), "on: the dashes march {marching:?}");
    assert!(h.ctx.global_style().animation_time > 0.0);

    set(&mut h, Animations::Off);
    h.step();
    assert!(phases(&mut h).iter().all(|phase| *phase == 0.0), "off: they stand still");
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let style = h.ctx.style_of(theme);
        assert_eq!(style.animation_time, 0.0, "off: menus and tooltips don't fade in");
        assert_eq!(style.scroll_animation, egui::style::ScrollAnimation::none());
    }

    // On again: egui's animations are back as they were.
    set(&mut h, Animations::On);
    h.step();
    assert_eq!(h.ctx.global_style().animation_time, 0.1);
    let marching = phases(&mut h);
    assert!(marching.windows(2).any(|w| w[0] != w[1]), "{marching:?}");
}

#[test]
fn a_headless_window_never_gains_animations_from_the_setting() {
    // The harness (and the offscreen snapshot) has egui's animations off; On leaves that alone.
    let system = System::default();
    let mut h = harness(&system, false);
    for mode in [Animations::On, Animations::Off, Animations::On, Animations::System] {
        set(&mut h, mode);
        h.step();
        assert_eq!(h.ctx.global_style().animation_time, 0.0, "{mode:?}");
        assert!(!glides_to(&mut h, if mode == Animations::Off { "C5" } else { "D8" }));
    }
}
