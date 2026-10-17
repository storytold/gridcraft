//! Dragging the fill handle with the right button fills nothing on release: it opens Excel's fill
//! menu (Copy Cells, Fill Series, Fill Formatting Only, Fill Without Formatting, Series…).

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Drag, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(setup: impl FnOnce(&mut Session)) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    setup(&mut session);
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

/// B1:B2 holds 1, 2 and is selected.
fn one_two(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2]]})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
}

fn corner(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).right_bottom() - vec2(1.0, 1.0)
}

fn center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
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

fn escape(h: &mut Harness) {
    h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
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

fn fills(h: &Harness) -> usize {
    h.state().session.journal.iter().filter(|(id, _)| id == "edit.autoFill").count()
}

fn selection(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.a1()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

#[test]
fn right_drag_opens_the_fill_menu_and_fills_nothing() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Secondary);

    let menu = h.state().grid.fill_menu.map(|(src, target, _)| (src, target));
    assert_eq!(menu, Some((RangeRef::parse("B1:B2").unwrap(), RangeRef::parse("B1:B6").unwrap())));
    assert_eq!(values(&h, &["B3", "B6"]), [Value::Empty, Value::Empty]);
    assert_eq!(fills(&h), 0);
    assert!(h.state().grid.context_menu.is_none(), "not the cell context menu");
    assert_eq!(selection(&h), "B1:B2");
}

#[test]
fn left_drag_still_fills_the_series() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    assert_eq!(values(&h, &["B3", "B4", "B5", "B6"]), [n(3.0), n(4.0), n(5.0), n(6.0)]);
    assert!(h.state().grid.fill_menu.is_none());
}

#[test]
fn copy_cells_repeats_the_source() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Secondary);
    h.get_by_label("Copy Cells").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4", "B5", "B6"]), [n(1.0), n(2.0), n(1.0), n(2.0)]);
    assert!(h.state().grid.fill_menu.is_none());
    assert_eq!(selection(&h), "B1:B6");
}

#[test]
fn fill_series_counts_up_from_a_lone_number() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "B1", "input": "5"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    drag_handle(&mut h, "B1", "B4", PointerButton::Secondary);
    h.get_by_label("Fill Series").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B2", "B3", "B4"]), [n(6.0), n(7.0), n(8.0)]);
}

/// B1:B2 = 1, 2 in bold, B4 = 9 in italic.
fn formatted(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2], [null], [9]]})).unwrap();
    s.execute("home.bold", json!({"range": "B1:B2", "on": true})).unwrap();
    s.execute("home.italic", json!({"range": "B4", "on": true})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
}

#[test]
fn fill_formatting_only_leaves_the_values() {
    let mut h = harness(formatted);
    drag_handle(&mut h, "B2", "B4", PointerButton::Secondary);
    h.get_by_label("Fill Formatting Only").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4"]), [Value::Empty, n(9.0)]);
    assert!(bold(&h, "B3") && bold(&h, "B4"));
}

#[test]
fn fill_without_formatting_extends_the_series_under_the_targets_formats() {
    let mut h = harness(formatted);
    drag_handle(&mut h, "B2", "B4", PointerButton::Secondary);
    h.get_by_label("Fill Without Formatting").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4"]), [n(3.0), n(4.0)]);
    assert!(!bold(&h, "B3") && !bold(&h, "B4"), "the source's bold stays behind");
    assert!(italic(&h, "B4"));
}

#[test]
fn series_opens_the_series_dialog_on_the_dragged_cells_in_their_direction() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "B1", "input": "1"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    drag_handle(&mut h, "B1", "E1", PointerButton::Secondary);
    h.get_by_label("Series…").click();
    h.run_steps(3);

    assert_eq!(selection(&h), "B1:E1");
    let dialog = h.state().dialog.as_ref().map(|d| (d.name().to_string(), d.values.get("direction").cloned()));
    assert_eq!(dialog, Some(("series".into(), Some(json!("rows")))));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(values(&h, &["C1", "D1", "E1"]), [n(2.0), n(3.0), n(4.0)]);
}

#[test]
fn escape_mid_right_drag_cancels_it() {
    let mut h = harness(|s| {
        one_two(s);
        s.execute("edit.copy", json!({"range": "D1"})).unwrap();
        s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
    });
    assert!(h.state().session.clipboard.is_some());
    let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
    press(&mut h, start, PointerButton::Secondary);
    move_to(&mut h, start, end);
    escape(&mut h);
    assert_eq!(h.state().grid.drag, Drag::None, "the dashed outline goes");
    assert!(h.query_by_label("Copy Cells").is_none(), "the menu doesn't flash for the Esc frame");
    release(&mut h, end, PointerButton::Secondary);

    assert!(h.state().grid.fill_menu.is_none());
    assert!(h.state().grid.context_menu.is_none());
    assert_eq!(values(&h, &["B3", "B6"]), [Value::Empty, Value::Empty]);
    assert!(h.state().session.clipboard.is_some(), "Esc cancels the drag, not the copy");
}

#[test]
fn arrow_keys_mid_right_drag_leave_the_source_alone() {
    let mut h = harness(one_two);
    let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
    press(&mut h, start, PointerButton::Secondary);
    move_to(&mut h, start, end);
    h.input_mut().events.push(Event::Key { key: Key::ArrowDown, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    release(&mut h, end, PointerButton::Secondary);

    assert_eq!(selection(&h), "B1:B2");
    assert_eq!(h.state().grid.fill_menu.map(|(src, ..)| src), Some(RangeRef::parse("B1:B2").unwrap()));
}

#[test]
fn escape_closes_the_menu_without_filling() {
    let mut h = harness(|s| {
        one_two(s);
        s.execute("edit.copy", json!({"range": "D1"})).unwrap();
        s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
    });
    drag_handle(&mut h, "B2", "B6", PointerButton::Secondary);
    assert!(h.state().grid.fill_menu.is_some());
    escape(&mut h);
    h.run_steps(2);

    assert!(h.state().grid.fill_menu.is_none());
    assert_eq!(values(&h, &["B3", "B6"]), [Value::Empty, Value::Empty]);
    assert!(h.state().session.clipboard.is_some(), "Esc closes the menu, not the copy");
}

#[test]
fn right_click_on_the_handle_opens_the_context_menu() {
    let mut h = harness(one_two);
    let at = corner(&h, "B2");
    press(&mut h, at, PointerButton::Secondary);
    release(&mut h, at, PointerButton::Secondary);

    assert!(h.state().grid.context_menu.is_some());
    assert!(h.state().grid.fill_menu.is_none());
}
