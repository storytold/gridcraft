//! The saved scroll position (`sheetView topLeftCell`) is restored on open and saved back (#378).

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
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
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
