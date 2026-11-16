//! Right-clicking the status bar opens a menu that turns its selection statistics on and off.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::{Session, StatusBarStats};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    for (cell, input) in [("A1", "4"), ("A2", "-1"), ("A3", "9")] {
        session.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    session.execute("selection.set", json!({"range": "A1:A3"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn right_click(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}

#[test]
fn right_clicking_the_status_bar_turns_on_minimum() {
    let mut h = harness();
    assert!(h.query_by_label("Sum: 12").is_some(), "Sum is on by default");
    assert!(h.query_by_label("Minimum: -1").is_none(), "Minimum is off by default");
    // An empty stretch of the 26-point status bar, between the mode text and the statistics.
    right_click(&mut h, pos2(300.0, 787.0));
    h.get_by_label("Minimum").click();
    h.run_steps(4);
    assert!(h.state().session.prefs.status_bar.minimum);
    assert!(h.query_by_label("Minimum: -1").is_some(), "the status bar shows the minimum of the selection");
}

#[test]
fn right_clicking_the_status_bar_text_opens_the_menu() {
    for text in ["Sum: 12", "Ready"] {
        let mut h = harness();
        let pos = h.get_by_label(text).rect().center();
        right_click(&mut h, pos);
        assert!(h.query_by_label("Numerical Count").is_some(), "a right-click on {text:?} opens the statistics menu");
    }
}

/// On a phone-width window the statistics are drawn from the right end; anything that does not
/// fit is left out instead of painting over the mode text at the left end (#4187).
#[test]
fn statistics_that_do_not_fit_stay_clear_of_the_mode_text() {
    let mut session = Session::new();
    session.new_workbook();
    for (cell, input) in [("A1", "4"), ("A2", "-1"), ("A3", "9")] {
        session.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    session.execute("selection.set", json!({"range": "A1:A3"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(320.0, 640.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    let ready = h.get_by_label("Ready").rect();
    for text in ["Average: 4", "Count: 3", "Sum: 12"] {
        if let Some(label) = h.query_by_label(text) {
            assert!(!label.rect().intersects(ready), "{text:?} overlaps the mode text: {:?} vs {ready:?}", label.rect());
        }
    }
}

/// With every statistic on, the fitting budget counts egui's automatic item spacing as well as
/// the explicit 10-point gap, so the leftmost label still clears the mode text at the widths where
/// one more label would only just fit (#4187).
#[test]
fn all_six_statistics_clear_the_mode_text_at_boundary_widths() {
    let all = ["Average: 4", "Count: 3", "Numerical Count: 3", "Minimum: -1", "Maximum: 9", "Sum: 12"];
    let mut drawn_total = 0;
    for width in (800..=920).step_by(4) {
        let mut session = Session::new();
        session.new_workbook();
        for (cell, input) in [("A1", "4"), ("A2", "-1"), ("A3", "9")] {
            session.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
        }
        session.execute("selection.set", json!({"range": "A1:A3"})).unwrap();
        session.prefs.status_bar =
            StatusBarStats { average: true, count: true, numerical_count: true, minimum: true, maximum: true, sum: true };
        let mut h = crate::common::builder().with_size(vec2(width as f32, 640.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            crate::common::app(session),
        );
        h.run_steps(4);
        let ready = h.get_by_label("Ready").rect();
        let mut drawn = 0;
        for text in all {
            if let Some(label) = h.query_by_label(text) {
                drawn += 1;
                assert!(!label.rect().intersects(ready), "at {width} points {text:?} overlaps the mode text: {:?} vs {ready:?}", label.rect());
            }
        }
        drawn_total += drawn;
    }
    assert!(drawn_total > 0, "the sweep drew statistics at some width");
}
