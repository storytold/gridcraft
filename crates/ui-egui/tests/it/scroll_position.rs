//! The saved scroll position (`sheetView topLeftCell`) is restored on open and saved back (#378).
//! Each window keeps each sheet's own scroll position while sheets are inserted, deleted or
//! moved (#3485).

use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::Workbook;
use gridcraft_engine::{DocState, Session};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(freeze: Option<(u32, u32)>) -> Harness {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for r in 0..60 {
        sh.set_value(CellRef::new(r, 0), Value::text(format!("row {}", r + 1)));
    }
    sh.view_top_left = CellRef::parse("A31").unwrap();
    sh.view_active = CellRef::parse("A31").unwrap();
    sh.freeze = freeze;
    let mut session = Session::new();
    session.add_document(DocState::new(wb, None, "Book1".into(), gridcraft_engine::Trust::Local));
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

/// The first row painted at the top of the scrolling area.
fn first_scrolled_row(h: &Harness) -> u32 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let (fr, _) = sheet.freeze.unwrap_or((0, 0));
    let top = geo.cells.top() + sheet.row_top(fr) as f32 * geo.z;
    (fr..200).find(|r| geo.y(sheet, *r) <= top + 0.5 && geo.y(sheet, *r + 1) > top + 0.5).unwrap()
}

#[test]
fn saved_scroll_position_is_restored_and_saved_back() {
    for freeze in [None, Some((1, 0))] {
        let mut h = harness(freeze);
        assert_eq!(first_scrolled_row(&h), 30, "row 31 is the first visible row ({freeze:?})");
        // Scroll to row 51 and save: the file gets the new position, the document stays clean.
        {
            let app = h.state_mut();
            let sh = app.session.active().unwrap().wb.active().unwrap().clone();
            let fr = freeze.map_or(0, |f| f.0);
            app.view_mut().unwrap().scroll.y = (sh.row_top(50) - sh.row_top(fr)) as f32;
            app.run("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        }
        let d = h.state().session.active().unwrap();
        assert_eq!(d.wb.active().unwrap().view_top_left, CellRef::parse("A51").unwrap());
        assert!(!d.is_dirty(), "scrolling and saving view state does not dirty the workbook");
    }
}

#[test]
fn a_pivottable_sheet_opens_at_the_top_and_its_source_sheet_keeps_its_place() {
    // A new PivotTable sheet goes in front of its source sheet. Scroll positions were kept by
    // sheet index, so the new sheet took over the source sheet's position and opened at row 51.
    let mut h = harness(None);
    h.state_mut().run("range.setValues", json!({"range": "C1:D3", "values": [["Region", "Sales"], ["North", 10], ["South", 20]]})).unwrap();
    {
        let app = h.state_mut();
        let sh = app.session.active().unwrap().wb.active().unwrap().clone();
        app.view_mut().unwrap().scroll.y = sh.row_top(50) as f32;
    }
    h.run_steps(2);
    assert_eq!(first_scrolled_row(&h), 50, "setup: Sheet1 is scrolled to row 51");
    h.state_mut().run("insert.pivotTable", json!({"source": "Sheet1!$C$1:$D$3", "rows": ["Region"]})).unwrap();
    h.run_steps(4);
    let wb = &h.state().session.active().unwrap().wb;
    assert!(!wb.active().unwrap().pivots.is_empty(), "setup: the PivotTable's new sheet is active");
    assert_eq!(first_scrolled_row(&h), 0, "the new sheet opens at row 1");
    h.state_mut().run("sheet.next", json!({})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Sheet1");
    assert_eq!(first_scrolled_row(&h), 50, "Sheet1 is still scrolled to row 51");
}

/// Scrolls the active sheet so `row` (0-based) is the first one in view.
fn scroll_to_row(h: &mut Harness, row: u32) {
    let app = h.state_mut();
    let sh = app.session.active().unwrap().wb.active().unwrap().clone();
    app.view_mut().unwrap().scroll.y = sh.row_top(row) as f32;
    h.run_steps(2);
}

fn activate(h: &mut Harness, sheet: &str) {
    h.state_mut().run("sheet.activate", json!({"sheet": sheet})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, sheet);
}

#[test]
fn a_sheet_keeps_its_scroll_position_when_sheets_are_inserted_deleted_or_moved() {
    let mut h = harness(None);
    scroll_to_row(&mut h, 50);
    h.state_mut().run("home.insertSheet", json!({"name": "Two", "before": 0})).unwrap();
    h.run_steps(4);
    assert_eq!(first_scrolled_row(&h), 0, "the inserted sheet opens at row 1");
    scroll_to_row(&mut h, 10);
    activate(&mut h, "Sheet1");
    assert_eq!(first_scrolled_row(&h), 50, "Sheet1 moved one place right and is still at row 51");
    h.state_mut().run("home.deleteSheet", json!({"sheet": "Two"})).unwrap();
    h.run_steps(4);
    assert_eq!(first_scrolled_row(&h), 50, "Sheet1 moved back and is still at row 51");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(4);
    activate(&mut h, "Two");
    assert_eq!(first_scrolled_row(&h), 10, "the sheet undo brings back is where it was left");
    h.state_mut().run("sheet.move", json!({"sheet": 0, "to": 1})).unwrap();
    h.run_steps(4);
    activate(&mut h, "Two");
    assert_eq!(first_scrolled_row(&h), 10, "a moved sheet keeps its place");
    activate(&mut h, "Sheet1");
    assert_eq!(first_scrolled_row(&h), 50, "and so does the sheet it moved past");
}

#[test]
fn each_window_scrolls_on_its_own_and_a_closed_windows_positions_are_dropped() {
    let mut h = harness(None);
    scroll_to_row(&mut h, 50);
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(4);
    let second = h.state().session.active_window().unwrap();
    assert_ne!(first, second);
    scroll_to_row(&mut h, 10);
    assert!(h.state().views.keys().any(|(w, _)| *w == second), "setup: the second window has its own position");
    h.state_mut().run("window.close", json!({"window": second})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().session.active_window(), Some(first));
    assert_eq!(first_scrolled_row(&h), 50, "the first window kept its own position");
    assert!(h.state().views.keys().all(|(w, _)| *w == first), "nothing is kept for the closed window: {:?}", h.state().views.keys());
}
