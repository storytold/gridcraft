//! The AutoFilter button is the size Excel draws it: 17 px square at 100% zoom, 1 px in from the
//! cell's bottom and right edges, the same size on screen at every zoom, and smaller only when the
//! row is too short for it (each measured in Excel for Microsoft 365).

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

/// The A1 button's rect and the A1 cell's rect, in screen points, and the screen points per sheet
/// pixel at 100% zoom.
fn button_at(zoom: u32, row_height: Option<f64>) -> (egui::Rect, egui::Rect, f32) {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Val"], ["a"], ["b"]]})).unwrap();
    session.execute("data.filter", json!({"range": "A1:A3"})).unwrap();
    if let Some(h) = row_height {
        session.execute("home.rowHeight", json!({"rows": "1:1", "height": h})).unwrap();
    }
    session.execute("view.zoom", json!({"percent": zoom})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(30);
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let a1 = CellRef::parse("A1").unwrap();
    (grid::filter_button_rect(&geo, sh, a1), geo.cell_rect(sh, a1), geo.z / (zoom as f32 / 100.0))
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn filter_button_is_excels_size_at_every_zoom() {
    let (button, cell, unit) = button_at(100, None);
    assert!(near(button.width(), 17.0 * unit) && near(button.height(), 17.0 * unit), "{button:?}");
    assert!(near(cell.right() - button.right(), unit) && near(cell.bottom() - button.bottom(), unit), "{button:?} in {cell:?}");
    for zoom in [200, 400] {
        let (button, _, unit) = button_at(zoom, None);
        assert!(near(button.width(), 17.0 * unit), "zoom {zoom}: {button:?}");
    }
    // A taller row keeps the same button at the bottom right.
    let (button, cell, unit) = button_at(100, Some(40.0));
    assert!(near(button.height(), 17.0 * unit) && near(cell.bottom() - button.bottom(), unit), "{button:?} in {cell:?}");
}

#[test]
fn filter_button_shrinks_to_fit_a_short_row() {
    for (zoom, row_height) in [(100, Some(8.0)), (50, None)] {
        let (button, cell, unit) = button_at(zoom, row_height);
        assert!(button.height() < 17.0 * unit, "zoom {zoom}: {button:?}");
        assert!(near(button.height(), cell.height() - 2.0 * unit), "zoom {zoom}: {button:?} in {cell:?}");
        assert!(cell.contains_rect(button), "zoom {zoom}: {button:?} in {cell:?}");
    }
}
