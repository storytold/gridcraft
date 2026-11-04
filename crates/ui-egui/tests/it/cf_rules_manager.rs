//! Manage Rules on a sheet with thousands of conditional formats (issue #3321): the list scrolls
//! inside the dialog, and only the visible rules are laid out, instead of one row per rule
//! making the window as tall as the whole list and the application window go blank.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const RULES: usize = 3000;
const SCREEN_H: f32 = 800.0;

fn harness_with_rules(n: usize) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    for i in 0..n {
        let range = format!("A{}", i + 1);
        session
            .execute(
                "home.conditionalFormat",
                json!({"range": range, "rule": {"type": "cellIs", "operator": "greaterThan", "value": "5", "preset": "yellowFill"}}),
            )
            .unwrap();
    }
    let mut h = crate::common::builder().with_size(vec2(1200.0, SCREEN_H)).build_ui_state(
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

#[test]
fn many_rules_scroll_inside_a_dialog_that_fits_the_window() {
    let mut h = harness_with_rules(RULES);
    let deletes: Vec<_> = h.query_all_by_label("Delete").collect();
    assert!(!deletes.is_empty(), "the first rules are listed");
    // One more Delete sits in the ribbon.
    assert!(deletes.len() < 100, "only the visible rules are laid out, got {}", deletes.len());
    let lowest = deletes.iter().map(|d| d.rect().bottom()).fold(0.0, f32::max);
    assert!(lowest <= SCREEN_H, "the list stays inside the window, lowest button at {lowest}");
    drop(deletes);
    let window = h.get_by_label("Conditional Formatting Rules Manager").rect();
    assert!(window.bottom() <= SCREEN_H, "the dialog fits the window, bottom at {}", window.bottom());
    // Deleting through the dialog removes the first rule and the dialog stays usable.
    // (The ribbon has a Delete button of its own, outside the dialog's window.)
    h.query_all_by_label("Delete").find(|d| window.contains_rect(d.rect())).unwrap().click();
    h.run_steps(4);
    let left = h.state().session.active().unwrap().wb.active().unwrap().cond_formats.len();
    assert_eq!(left, RULES - 1);
    let first = h.state().session.active().unwrap().wb.active().unwrap().cond_formats[0].ranges[0].a1();
    // New rules go first, so the one deleted was A3000.
    assert_eq!(first, format!("A{}", RULES - 1), "the first rule was the one removed");
}

#[test]
fn an_empty_sheet_says_so() {
    let h = harness_with_rules(0);
    h.get_by_label("There are no conditional formatting rules on this sheet.");
}
