//! Manage Rules on a sheet with thousands of conditional formats (issue #3321): the list scrolls
//! inside the dialog, and only the visible rules are laid out, instead of one row per rule
//! making the window as tall as the whole list and the application window go blank.

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, TouchPhase, Vec2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const RULES: usize = 3000;
const SCREEN_H: f32 = 800.0;

fn harness_with_rules(n: usize) -> Harness {
    let ranges: Vec<String> = (0..n).map(|i| format!("A{}", i + 1)).collect();
    harness_with_ranges(&ranges)
}

/// One rule per entry of `ranges`, added in order (so the last one is listed first).
fn harness_with_ranges(ranges: &[String]) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    for range in ranges {
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

fn dialog(h: &Harness) -> Rect {
    h.get_by_label("Conditional Formatting Rules Manager").rect()
}

/// The rows of the list shown inside the dialog: (rule number, row rectangle).
fn visible_rows(h: &Harness) -> Vec<(usize, Rect)> {
    let window = dialog(h);
    let mut rows: Vec<(usize, Rect)> = h
        .query_all_by_label_contains("applies to")
        .filter(|n| window.contains(n.rect().left_center()))
        .filter_map(|n| Some((n.value()?.split('.').next()?.parse().ok()?, n.rect())))
        .collect();
    rows.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
    rows
}

/// The row of rule `n`, wherever it is laid out (its label may reach past the dialog's edges).
fn row_of(h: &Harness, n: usize) -> Rect {
    let prefix = format!("{n}. ");
    h.query_all_by_label_contains("applies to")
        .find(|r| r.value().is_some_and(|l| l.starts_with(&prefix)))
        .expect("the rule's row is laid out")
        .rect()
}

/// The `label` button on the row at `row`, if it is inside the dialog.
fn row_button(h: &Harness, row: Rect, label: &str) -> Option<Rect> {
    let window = dialog(h);
    h.query_all_by_label(label).map(|n| n.rect()).find(|r| window.contains_rect(*r) && (r.center().y - row.center().y).abs() < row.height() / 2.0)
}

fn wheel(h: &mut Harness, at: Pos2, delta: Vec2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
    h.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta,
        phase: TouchPhase::Move,
        source: MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(30);
}

fn click(h: &mut Harness, r: Rect) {
    let pos = r.center();
    for pressed in [true, false] {
        h.input_mut()
            .events
            .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        h.step();
    }
    h.run_steps(4);
}

/// The ranges of every rule on the sheet, in list order.
fn rule_ranges(h: &Harness) -> Vec<String> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    sh.cond_formats.iter().map(|cf| cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(",")).collect()
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

#[test]
fn a_rule_scrolled_into_view_is_moved_and_deleted() {
    let mut h = harness_with_rules(RULES);
    let before = visible_rows(&h);
    let last_shown = before.iter().map(|r| r.0).max().expect("the first rules are listed");
    // Scroll the list down, then act on a row in the middle of what is shown now.
    let at = pos2(dialog(&h).left() + 40.0, before[before.len() / 2].1.center().y);
    wheel(&mut h, at, vec2(0.0, -2000.0));
    let rows = visible_rows(&h);
    let (n, row) = rows[rows.len() / 2];
    assert!(n > last_shown, "rule {n} was not on screen before scrolling (rules 1 to {last_shown} were)");
    // Rule n is the n-th listed; new rules go first, so it applies to A{RULES + 1 - n}.
    let target = format!("A{}", RULES + 1 - n);
    assert_eq!(rule_ranges(&h)[n - 1], target);
    let down = row_button(&h, row, "▼").expect("the row's move-down button is in the dialog");
    click(&mut h, down);
    let ranges = rule_ranges(&h);
    assert_eq!(ranges[n], target, "rule {n} moved down one place");
    assert_eq!(ranges[n - 1], format!("A{}", RULES - n), "the rule below it moved up");
    // The moved rule is now rule n + 1, on the next row.
    let delete = row_button(&h, row_of(&h, n + 1), "Delete").expect("the row's Delete button is in the dialog");
    click(&mut h, delete);
    let ranges = rule_ranges(&h);
    assert_eq!(ranges.len(), RULES - 1);
    assert!(!ranges.contains(&target), "the rule scrolled to was deleted");
}

#[test]
fn a_rule_with_a_long_label_is_reached_by_scrolling_sideways() {
    let long: Vec<String> = (0..200).map(|i| format!("C{}", 2 * i + 1)).collect();
    let long = long.join(",");
    let mut h = harness_with_ranges(&["A1".into(), long.clone(), "B1".into()]);
    assert_eq!(rule_ranges(&h), ["B1", long.as_str(), "A1"]);
    let row = row_of(&h, 2);
    assert!(row_button(&h, row, "▼").is_none(), "the long rule's buttons start out of view");
    let window = dialog(&h);
    wheel(&mut h, pos2(window.left() + 40.0, row.center().y), vec2(-20000.0, 0.0));
    let down = row_button(&h, row_of(&h, 2), "▼").expect("scrolling sideways brings the move-down button into the dialog");
    click(&mut h, down);
    assert_eq!(rule_ranges(&h), ["B1", "A1", long.as_str()], "the long rule moved down");
    let delete = row_button(&h, row_of(&h, 3), "Delete").expect("the long rule's Delete button is in the dialog on its new row");
    click(&mut h, delete);
    assert_eq!(rule_ranges(&h), ["B1", "A1"], "the long rule was deleted");
}
