//! After a left-drag of the fill handle, the Auto Fill Options button redoes the fill as Copy Cells,
//! Fill Series, Fill Formatting Only or Fill Without Formatting (#1403), and from a date as Fill
//! Days, Fill Weekdays, Fill Months or Fill Years.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(setup: impl FnOnce(&mut Session)) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    setup(&mut session);
    crate::support::stepped_harness(session)
}

/// B1:B2 holds 1, 2 and is selected.
fn one_two(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2]]})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
}

fn corner(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.mirror_pos(geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).right_bottom() - vec2(1.0, 1.0))
}

fn center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.screen_cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
}

fn press(h: &mut Harness, pos: Pos2, button: PointerButton) {
    h.input_mut().events.extend([Event::PointerMoved(pos), Event::PointerButton { pos, button, pressed: true, modifiers: Modifiers::NONE }]);
    h.step();
}

fn move_to(h: &mut Harness, from: Pos2, to: Pos2) {
    for i in 1..=10 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
        h.step();
    }
}

fn release(h: &mut Harness, pos: Pos2, button: PointerButton) {
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Drags the handle of the current selection (whose bottom-right cell is `from`) to `to`.
fn drag_handle(h: &mut Harness, from: &str, to: &str, button: PointerButton) {
    let (start, end) = (corner(h, from), center(h, to));
    press(h, start, button);
    move_to(h, start, end);
    release(h, end, button);
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn values(h: &Harness, cells: &[&str]) -> Vec<Value> {
    cells.iter().map(|c| value(h, c)).collect()
}

fn bold(h: &Harness, cell: &str) -> bool {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).font.bold
}

fn italic(h: &Harness, cell: &str) -> bool {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).font.italic
}

fn selection(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.a1()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

#[test]
fn left_drag_shows_the_auto_fill_options_button() {
    let mut h = harness(one_two);
    assert!(h.query_by_label("Auto Fill Options").is_none());
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    assert_eq!(values(&h, &["B3", "B6"]), [n(3.0), n(6.0)]);
    assert!(h.query_by_label("Auto Fill Options").is_some());
}

#[test]
fn copy_cells_replaces_the_series_the_drag_made() {
    let mut h = harness(one_two);
    let steps_before = h.state().session.active().unwrap().undo.len();
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Copy Cells").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4", "B5", "B6"]), [n(1.0), n(2.0), n(1.0), n(2.0)]);
    assert_eq!(selection(&h), "B1:B6");
    assert_eq!(h.state().session.active().unwrap().undo.len(), steps_before + 1, "one fill step, not two");
}

#[test]
fn a_lone_number_copies_on_drag_and_counts_up_with_fill_series() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "B1", "input": "5"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    drag_handle(&mut h, "B1", "B4", PointerButton::Primary);
    assert_eq!(values(&h, &["B2", "B3", "B4"]), [n(5.0), n(5.0), n(5.0)]);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Fill Series").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B2", "B3", "B4"]), [n(6.0), n(7.0), n(8.0)]);
}

#[test]
fn fill_months_redoes_a_date_drag_by_months() {
    let mut h = harness(|s| {
        // 1/31/2026 is serial 46053.
        s.execute("cell.set", json!({"cell": "B1", "input": "1/31/2026"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    drag_handle(&mut h, "B1", "B3", PointerButton::Primary);
    assert_eq!(values(&h, &["B2", "B3"]), [n(46054.0), n(46055.0)], "a plain drag steps by days");
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Fill Months").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B2", "B3"]), [n(46081.0), n(46112.0)], "Feb 28 and Mar 31");
}

#[test]
fn a_number_drag_offers_no_date_fills() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    assert!(h.query_by_label("Copy Cells").is_some());
    assert!(h.query_by_label("Fill Days").is_none());
}

fn formatted(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2], [null], [9]]})).unwrap();
    s.execute("home.bold", json!({"range": "B1:B2", "on": true})).unwrap();
    s.execute("home.italic", json!({"range": "B4", "on": true})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
}

#[test]
fn fill_without_formatting_keeps_the_targets_formats() {
    let mut h = harness(formatted);
    drag_handle(&mut h, "B2", "B4", PointerButton::Primary);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Fill Without Formatting").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4"]), [n(3.0), n(4.0)]);
    assert!(!bold(&h, "B3") && !bold(&h, "B4"));
    assert!(italic(&h, "B4"));
}

#[test]
fn the_button_goes_away_once_the_selection_moves() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    assert!(h.query_by_label("Auto Fill Options").is_some());
    let away = center(&h, "E9");
    h.input_mut().events.push(Event::PointerMoved(away));
    h.input_mut().events.push(Event::PointerButton { pos: away, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos: away, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);

    assert!(h.query_by_label("Auto Fill Options").is_none());
}

#[test]
fn right_drag_makes_no_button() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Secondary);
    assert!(h.query_by_label("Auto Fill Options").is_none());
}

#[test]
fn the_button_goes_away_once_the_workbook_changes() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    assert!(h.query_by_label("Auto Fill Options").is_some());
    h.state_mut().session.execute("cell.set", json!({"cell": "D1", "input": "x"})).unwrap();
    h.run_steps(3);

    assert_eq!(selection(&h), "B1:B6", "the selection did not move");
    assert!(h.query_by_label("Auto Fill Options").is_none(), "its undo would take the unrelated edit with it");
}

#[test]
fn the_button_stays_inside_the_grid_on_a_right_to_left_sheet() {
    let mut h = harness(|s| {
        one_two(s);
        s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    });
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    let button = h.get_by_label("Auto Fill Options").rect();
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    assert!(geo.cells.contains_rect(button), "{button:?} outside {:?}", geo.cells);
    let filled = geo.screen_cell_rect(sheet, CellRef::parse("B6").unwrap());
    assert!(button.center().x < filled.left() && button.top() >= filled.bottom() - 1.0, "below-left of the mirrored range's corner");
}

/// Right-to-left sheet with column A made `chars` wide (so column B's right edge, where the fill
/// range B1:B6 ends, sits far enough right to be scrolled to either edge of the cells area).
fn rtl_wide_a(chars: u32) -> Harness {
    harness(|s| {
        one_two(s);
        s.execute("home.columnWidth", json!({"cols": "A:A", "chars": chars})).unwrap();
        s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    })
}

/// Logical right edge of the filled column B (scroll-dependent) and the current geometry.
fn b_right(h: &Harness) -> (f32, Geo) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    (geo.cell_rect(sheet, CellRef::parse("B6").unwrap()).right(), geo)
}

fn scroll_b_right_to(h: &mut Harness, logical_x: f32) {
    let (now, geo) = b_right(h);
    h.state_mut().view_mut().unwrap().scroll.x += (now - logical_x) / geo.z;
    h.run_steps(3);
    let got = b_right(h).0;
    assert!((got - logical_x).abs() < 1.0, "setup: column B ends at logical x {got}, wanted {logical_x}");
}

/// Logical x of the range's corner is `cells.right() - 25`: the 18 px button (at corner + 2 ..
/// corner + 20) fits in the cells area, so on screen it sits at the left edge of the mirrored cells
/// area, left of the filled range. `fill_options_tag` tested the mirrored button against the
/// unmirrored cells, i.e. against the row-heading strip it does not own, and hid it.
#[test]
fn the_button_shows_near_the_left_screen_edge_on_a_right_to_left_sheet() {
    let mut h = rtl_wide_a(220);
    let (_, geo) = b_right(&h);
    scroll_b_right_to(&mut h, geo.cells.right() - 25.0);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    let (_, geo) = b_right(&h);
    let cells = h.state().grid.cells_rect.unwrap();
    assert!(geo.header_w > 20.0 && geo.cells.right() - 25.0 + 20.0 > geo.cells.right() - geo.header_w, "setup: in the zone the old check hides");
    let button = h.query_by_label("Auto Fill Options").expect("the button fits inside the cells area, so it shows").rect();
    assert!(cells.contains_rect(button), "{button:?} outside {cells:?}");
}

/// The corner is 10 px left of `cells.left()` (scrolled partly behind the row headings, which are
/// on the right on screen). The button would start 8 px behind the headings, so it must not be
/// drawn there. The old check let it through because the mirrored button fits the unmirrored cells.
#[test]
fn the_button_stays_off_the_row_headings_on_a_right_to_left_sheet() {
    let mut h = rtl_wide_a(20);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    assert!(h.query_by_label("Auto Fill Options").is_some(), "setup: the button shows before scrolling");
    let (_, geo) = b_right(&h);
    scroll_b_right_to(&mut h, geo.cells.left() - 10.0);

    let cells = h.state().grid.cells_rect.unwrap();
    if let Some(b) = h.query_by_label("Auto Fill Options") {
        let button = b.rect();
        assert!(cells.contains_rect(button), "{button:?} overlaps the row headings, cells area is {cells:?}");
    }
}
