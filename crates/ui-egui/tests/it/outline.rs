//! The outline band of grouped rows and columns: − / + and level buttons (#373).

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::outline::{Action, layout};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn click(h: &mut Harness, pos: Pos2) {
    let press = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(pos), press(true)]);
    h.step();
    h.input_mut().events.push(press(false));
    h.run_steps(3);
}

/// Where the button that does `which` is drawn this frame (screen coordinates).
fn button(h: &Harness, which: impl Fn(&Action) -> bool) -> Pos2 {
    let app = h.state();
    let geo = app.grid.geo.clone().unwrap();
    let sh = app.session.active().unwrap().wb.active().unwrap().clone();
    let b = layout(&geo, &sh).buttons.into_iter().find(|b| which(&b.action)).expect("button on screen");
    geo.mirror_pos(b.rect.center())
}

fn hidden_rows(h: &Harness) -> Vec<u32> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    sh.rows.iter().filter(|(_, l)| l.hidden).map(|(r, _)| *r).collect()
}

fn toggle(rows: bool, level: u8) -> impl Fn(&Action) -> bool {
    move |a| matches!(a, Action::Toggle { rows: r, group } if *r == rows && group.level == level)
}

fn outline_buttons_collapse_and_expand(rtl: bool) {
    let mut h = harness();
    if rtl {
        h.state_mut().session.run("view.rightToLeft", json!({"on": true})).unwrap();
    }
    h.state_mut().session.run("data.group", json!({"rows": "5:8"})).unwrap();
    h.state_mut().session.run("data.group", json!({"cols": "C:E"})).unwrap();
    h.run_steps(2);
    let geo = h.state().grid.geo.clone().unwrap();
    assert!(geo.band_w > 0.0 && geo.band_h > 0.0);

    // − on row 9 hides rows 5:8 and turns into +.
    let minus = button(&h, toggle(true, 1));
    click(&mut h, minus);
    assert_eq!(hidden_rows(&h), [4, 5, 6, 7]);
    let plus = button(&h, toggle(true, 1));
    click(&mut h, plus);
    assert!(hidden_rows(&h).is_empty());

    // Level 1 for the columns collapses C:E; level 2 shows them again.
    let level1 = button(&h, |a| *a == Action::Level { rows: false, level: 1 });
    click(&mut h, level1);
    let col_hidden = |h: &Harness| (2..=4).all(|c| h.state().session.active().unwrap().wb.active().unwrap().col_width(c) == 0.0);
    assert!(col_hidden(&h));
    let level2 = button(&h, |a| *a == Action::Level { rows: false, level: 2 });
    click(&mut h, level2);
    assert!(!col_hidden(&h));

    // Every click was a command: undo walks them back.
    h.state_mut().session.run("edit.undo", json!({})).unwrap();
    assert!(col_hidden(&h));
}

#[test]
fn outline_buttons_collapse_expand_and_undo() {
    outline_buttons_collapse_and_expand(false);
}

#[test]
fn outline_buttons_work_on_right_to_left_sheets() {
    outline_buttons_collapse_and_expand(true);
}
